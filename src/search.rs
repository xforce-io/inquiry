//! Bounded transport recovery. Workflow and business waits remain in TaskLoop.
use async_trait::async_trait;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};
use taskloop::{DomainTool, Error, Result, ToolContext, ToolOutput, ToolSpec};
use tokio::{sync::Mutex, time::Instant};

pub const PROVIDER: &str = "duckduckgo-html";

#[async_trait]
trait Backend: Send + Sync {
    async fn fetch(
        &self,
        url: &str,
        context: &ToolContext,
    ) -> std::result::Result<crate::net::Page, crate::net::FetchError>;
}
struct PublicNetwork;
#[async_trait]
impl Backend for PublicNetwork {
    async fn fetch(
        &self,
        url: &str,
        c: &ToolContext,
    ) -> std::result::Result<crate::net::Page, crate::net::FetchError> {
        crate::net::fetch(url, c).await
    }
}

pub struct Search {
    backend: Arc<dyn Backend>,
    // One shared provider cooldown, serialized across research instances.
    cooldown: Mutex<u128>,
    origin: Instant,
}
impl Default for Search {
    fn default() -> Self {
        Self {
            backend: Arc::new(PublicNetwork),
            cooldown: Mutex::new(0),
            origin: Instant::now(),
        }
    }
}
fn retry_millis(header: Option<&str>, now: chrono::DateTime<chrono::Utc>) -> u128 {
    let Some(raw) = header.map(str::trim) else {
        return 60_000;
    };
    if !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit()) {
        return raw
            .parse::<u128>()
            .unwrap_or(u128::MAX)
            .saturating_mul(1000);
    }
    chrono::DateTime::parse_from_rfc2822(raw)
        .ok()
        .map(|date| {
            (date.with_timezone(&chrono::Utc) - now)
                .num_milliseconds()
                .max(0) as u128
        })
        .unwrap_or(60_000)
}
fn unavailable(
    query: &str,
    attempts: u32,
    reason: &str,
    retryable: bool,
    wait: Option<u128>,
) -> ToolOutput {
    ToolOutput::json(
        json!({"query":query,"results":[],"status":"unavailable","reason":reason,
        "retryable":retryable,"fetch_attempts":attempts,"retry_after_ms":wait.and_then(|n|u64::try_from(n).ok()),
        "upstream_requests":null,"provider":PROVIDER,"searched_at":chrono::Utc::now().to_rfc3339()}),
    )
}
impl Search {
    async fn recover(
        &self,
        query: &str,
        c: &ToolContext,
        deadline: Instant,
        started: &AtomicU32,
    ) -> ToolOutput {
        let mut cooldown = self.cooldown.lock().await;
        let mut url = url::Url::parse("https://html.duckduckgo.com/html/").expect("constant URL");
        url.query_pairs_mut().append_pair("q", query);
        for attempts in 0..3 {
            let wait = cooldown.saturating_sub(self.origin.elapsed().as_millis());
            let remaining = deadline.saturating_duration_since(Instant::now());
            if wait >= remaining.as_millis() {
                return unavailable(
                    query,
                    attempts,
                    "搜索服务处于冷却期，稍后自动重试",
                    true,
                    Some(wait),
                );
            }
            if wait > 0 {
                tokio::time::sleep(Duration::from_millis(wait as u64)).await;
            }
            if c.is_cancelled() {
                return unavailable(query, attempts, "搜索已停止", false, None);
            }
            started.fetch_add(1, Ordering::Relaxed);
            let (delay, reason) = match self.backend.fetch(url.as_str(), c).await {
                Ok(page) if page.status == 200 => {
                    let results = std::str::from_utf8(&page.body)
                        .ok()
                        .map(crate::domain::search_results)
                        .unwrap_or_default();
                    if results.is_empty() {
                        *cooldown = self.origin.elapsed().as_millis().saturating_add(60_000);
                        return unavailable(
                            query,
                            attempts + 1,
                            "未取得可解析的网页搜索结果；不得将挑战页视为来源",
                            true,
                            Some(60_000),
                        );
                    }
                    return ToolOutput::json(
                        json!({"query":query,"results":results,"status":"discovered",
                        "reason":"搜索发现不等于获取或内容核验","fetch_attempts":attempts+1,
                        "retryable":false,"upstream_requests":null,"provider":PROVIDER,
                        "search_url":page.final_url,"searched_at":chrono::Utc::now().to_rfc3339()}),
                    );
                }
                Ok(page) if page.status == 429 || matches!(page.status, 500 | 502 | 503 | 504) => (
                    retry_millis(page.retry_after.as_deref(), chrono::Utc::now()),
                    "搜索服务暂不可用",
                ),
                Ok(_) => {
                    return unavailable(
                        query,
                        attempts + 1,
                        "搜索服务拒绝或不支持本次请求",
                        false,
                        None,
                    );
                }
                Err(e) if e.denied => {
                    return unavailable(
                        query,
                        attempts + 1,
                        "搜索目标未通过网络边界检查",
                        false,
                        None,
                    );
                }
                Err(_) => (1000 * (1 << attempts), "搜索网络暂不可用"),
            };
            *cooldown = self.origin.elapsed().as_millis().saturating_add(delay);
            if attempts == 2 {
                return unavailable(query, 3, reason, true, Some(delay));
            }
        }
        unreachable!("three bounded attempts")
    }
}
#[async_trait]
impl DomainTool for Search {
    fn spec(&self) -> ToolSpec {
        ToolSpec{id:"inquiry.search".into(),description:"Discover public URLs; not fetched evidence. Legacy Flow: at most eight calls per entry; if unavailable, stop searching and submit actual queries/receipts and genuine candidates. Native Goal collection: at most eight distinct queries plus two bounded caller recovery calls; preserve every attempt and observe declared cooldown outside this callback. Transport recovery uses at most three fetch attempts in 30 seconds and shares provider cooldown.".into(),input_schema:json!({"type":"object","properties":{"query":{"type":"string","minLength":1,"maxLength":300}},"required":["query"],"additionalProperties":false})}
    }
    async fn call(&self, c: ToolContext, args: Value) -> Result<ToolOutput> {
        let query = args["query"]
            .as_str()
            .ok_or_else(|| Error::new("research_invalid", "/query", "缺少检索词"))?;
        let deadline = Instant::now() + Duration::from_secs(30);
        let started = AtomicU32::new(0);
        tokio::select! {
            output=self.recover(query,&c,deadline,&started)=>Ok(output),
            _=tokio::time::sleep_until(deadline)=>Ok(unavailable(query,started.load(Ordering::Relaxed),"搜索等待达到本次时间边界",true,None)),
            _=async { while !c.is_cancelled() { tokio::time::sleep(Duration::from_millis(20)).await; } }=>
                Ok(unavailable(query,started.load(Ordering::Relaxed),"搜索已停止",false,None)),
        }
    }
}

#[cfg(test)]
mod tests;
