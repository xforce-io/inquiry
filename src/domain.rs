//! Research semantics live here, not in TaskLoop's runtime.
use async_trait::async_trait;
use scraper::{Html, Selector};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};
use taskloop::*;

pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn fault(message: impl Into<String>) -> Error {
    Error::new("research_invalid", "/research", message)
}
fn value(a: &ArtifactData) -> Result<Value> {
    parse_unique(&a.bytes)
}
fn array<'a>(v: &'a Value, key: &str) -> Result<&'a Vec<Value>> {
    v[key]
        .as_array()
        .ok_or_else(|| fault(format!("缺少数组{key}")))
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .ok_or_else(|| fault(format!("缺少文本{key}")))
}
fn canonical(raw: &str) -> Result<String> {
    crate::net::normalize_url(raw)
        .map(|u| u.to_string())
        .map_err(|e| fault(e.reason))
}
fn source_id(id: &str, visit: u64) -> bool {
    id.strip_prefix(&format!("r{visit}s"))
        .and_then(|s| s.parse::<usize>().ok())
        .is_some_and(|n| (1..=20).contains(&n))
}

#[derive(Debug)]
pub struct Normalized {
    pub title: String,
    pub paragraphs: Vec<String>,
    pub truncated: bool,
}
pub fn normalize(body: &[u8], content_type: &str) -> Result<Normalized> {
    let decoded = String::from_utf8_lossy(body);
    if decoded.chars().filter(|c| *c == '\u{fffd}').count() > decoded.chars().count() / 20 + 3 {
        return Err(fault("正文不适合文本规范化"));
    }
    if !content_type.is_empty()
        && !["html", "text/", "json", "xml"]
            .iter()
            .any(|s| content_type.contains(s))
    {
        return Err(fault("一期不支持此类正文（仅HTML/文本）"));
    }
    let html = Html::parse_document(&decoded);
    let selector = |s| Selector::parse(s).expect("constant selector");
    let title = html
        .select(&selector("title"))
        .next()
        .map(|e| e.text().collect::<String>())
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(300)
        .collect();
    let container = html
        .select(&selector("main, article"))
        .next()
        .unwrap_or_else(|| html.root_element());
    let mut paragraphs = Vec::new();
    let mut seen = BTreeSet::new();
    let mut total = 0;
    let mut truncated = false;
    for el in container.select(&selector("h1,h2,h3,h4,p,li,pre,blockquote")) {
        let raw = el
            .descendants()
            .filter_map(|node| {
                let t = node.value().as_text()?;
                if node.ancestors().any(|a| {
                    a.value().as_element().is_some_and(|e| {
                        matches!(
                            e.name(),
                            "script"
                                | "style"
                                | "noscript"
                                | "nav"
                                | "footer"
                                | "aside"
                                | "svg"
                                | "iframe"
                                | "form"
                        )
                    })
                }) {
                    None
                } else {
                    Some(t.to_string())
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        let raw = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        if raw.is_empty() || !seen.insert(raw.clone()) {
            continue;
        }
        if paragraphs.len() == 1000 || total >= 200000 {
            truncated = true;
            break;
        }
        let limit = 8000usize.min(200000 - total);
        let short: String = raw.chars().take(limit).collect();
        truncated |= short.chars().count() < raw.chars().count();
        total += short.chars().count();
        paragraphs.push(short);
    }
    if paragraphs.is_empty() {
        let raw = if content_type.contains("html") {
            container
                .descendants()
                .filter_map(|node| {
                    let text = node.value().as_text()?;
                    let excluded = node.ancestors().any(|a| {
                        a.value().as_element().is_some_and(|e| {
                            matches!(
                                e.name(),
                                "script"
                                    | "style"
                                    | "noscript"
                                    | "nav"
                                    | "footer"
                                    | "aside"
                                    | "svg"
                                    | "iframe"
                                    | "form"
                            )
                        })
                    });
                    (!excluded).then(|| text.to_string())
                })
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            decoded.to_string()
        };
        let raw = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        let chars: Vec<_> = raw.chars().collect();
        for chunk in chars.chunks(4000).take(50) {
            paragraphs.push(chunk.iter().collect());
        }
        truncated |= chars.len() > 200000;
    }
    if paragraphs.iter().all(|s| s.trim().is_empty()) {
        return Err(fault("规范化后没有可用文本"));
    }
    Ok(Normalized {
        title,
        paragraphs,
        truncated,
    })
}

fn search_results(html: &str) -> Vec<Value> {
    let doc = Html::parse_document(html);
    let snippets = Selector::parse(".snippet[data-type=web]").expect("constant selector");
    let anchors = Selector::parse("a[href]").expect("constant selector");
    let title = Selector::parse(".search-snippet-title").expect("constant selector");
    let mut results = Vec::new();
    let mut seen = BTreeSet::new();
    for snippet in doc.select(&snippets) {
        for anchor in snippet.select(&anchors) {
            let label = if let Some(t) = anchor.select(&title).next() {
                Some(t.text().collect::<String>())
            } else if anchor.value().classes().any(|c| c == "title") {
                Some(anchor.text().collect::<String>())
            } else {
                None
            };
            let Some(label) = label else { continue };
            let Ok(url) = canonical(anchor.value().attr("href").unwrap_or("")) else {
                continue;
            };
            if !seen.insert(url.clone()) {
                continue;
            }
            let label = label.split_whitespace().collect::<Vec<_>>().join(" ");
            if label.is_empty() {
                continue;
            }
            results.push(
                json!({"url":url,"title":label.chars().take(300).collect::<String>(),"snippet":""}),
            );
            if results.len() == 30 {
                return results;
            }
        }
    }
    results
}
pub struct Search;
#[async_trait]
impl DomainTool for Search {
    fn spec(&self) -> ToolSpec {
        ToolSpec{id:"inquiry.search".into(),description:"Discover public source URLs via actual search. A result is not a fetched or verified page. Use at most eight searches in this entry, then submit candidates, even if empty.".into(),input_schema:json!({"type":"object","properties":{"query":{"type":"string","minLength":1,"maxLength":300}},"required":["query"],"additionalProperties":false})}
    }
    async fn call(&self, c: ToolContext, args: Value) -> Result<ToolOutput> {
        let query = text(&args, "query")?;
        let mut url = url::Url::parse("https://search.brave.com/search").expect("constant URL");
        url.query_pairs_mut()
            .append_pair("q", query)
            .append_pair("source", "web");
        let page = match crate::net::fetch(url.as_str(), &c).await {
            Ok(p) if p.status == 200 => p,
            Ok(p) => {
                return Ok(ToolOutput::json(
                    json!({"query":query,"results":[],"status":"unavailable","reason":format!("搜索HTTP状态{}",p.status)}),
                ));
            }
            Err(e) => {
                return Ok(ToolOutput::json(
                    json!({"query":query,"results":[],"status":"unavailable","reason":e.reason}),
                ));
            }
        };
        let html = std::str::from_utf8(&page.body).map_err(|_| fault("搜索响应不是UTF-8"))?;
        let results = search_results(html);
        Ok(ToolOutput::json(
            json!({"query":query,"status":if results.is_empty(){"unavailable"}else{"discovered"},"reason":if results.is_empty(){"未取得可解析的网页搜索结果；不得将挑战页视为来源"}else{"搜索发现不等于获取或内容核验"},"results":results,"searched_at":chrono::Utc::now().to_rfc3339(),"search_url":page.final_url,"provider":"brave-html"}),
        ))
    }
}
pub struct Fetch;
#[async_trait]
impl DomainTool for Fetch {
    fn spec(&self) -> ToolSpec {
        ToolSpec{id:"inquiry.fetch".into(),description:"Fetch one accepted candidate URL, retain raw/normalized bytes, return actual disposition and acquisition facts.".into(),input_schema:json!({"type":"object","properties":{"source_id":{"type":"string","maxLength":20},"url":{"type":"string","maxLength":4096}},"required":["source_id","url"],"additionalProperties":false})}
    }
    async fn call(&self, c: ToolContext, args: Value) -> Result<ToolOutput> {
        let id = text(&args, "source_id")?;
        let url = canonical(text(&args, "url")?)?;
        let candidates = c
            .latest("discover", "candidates")
            .ok_or_else(|| fault("没有已接受候选"))?;
        let candidates = value(candidates)?;
        if !array(&candidates, "items")?
            .iter()
            .any(|i| i["source_id"] == id && i["url"] == url)
        {
            return Err(fault("网址不属于当前已接受候选"));
        }
        let mut fact = json!({"source_id":id,"requested_url":url,"final_url":null,"captured_at":chrono::Utc::now().to_rfc3339(),"fetch_status":"unavailable","http_status":null,"reason":"未取得可用正文","title":"","body_sha256":null,"normalized_sha256":null,"transform_version":null,"usable":false,"raw_slot":null,"snapshot_slot":null,"truncated":false});
        let page = match crate::net::fetch(&url, &c).await {
            Ok(p) => p,
            Err(e) => {
                fact["fetch_status"] = json!(if e.denied { "denied" } else { "unavailable" });
                fact["reason"] = json!(e.reason);
                return Ok(ToolOutput::json(fact));
            }
        };
        fact["final_url"] = json!(page.final_url);
        fact["http_status"] = json!(page.status);
        fact["body_sha256"] = json!(hash(&page.body));
        let raw_slot = format!("capture.raw.{id}");
        fact["raw_slot"] = json!(raw_slot);
        let mut files = vec![Candidate::bytes(&raw_slot, page.body.clone())];
        if !(200..300).contains(&page.status) {
            fact["reason"] = json!(format!("来源HTTP状态{}，不能作为已获取证据", page.status));
            return Ok(ToolOutput {
                payload: fact,
                files,
            });
        }
        let normalized = match normalize(&page.body, &page.content_type) {
            Ok(n) => n,
            Err(e) => {
                fact["reason"] = json!(e.message);
                return Ok(ToolOutput {
                    payload: fact,
                    files,
                });
            }
        };
        let snapshot_slot = format!("capture.snapshot.{id}");
        fact["snapshot_slot"] = json!(snapshot_slot);
        fact["title"] = json!(normalized.title);
        fact["normalized_sha256"] = json!(hash(&serde_json::to_vec(&normalized.paragraphs)?));
        fact["transform_version"] = json!("html-text-v1");
        fact["usable"] = json!(true);
        fact["fetch_status"] = json!("captured");
        fact["reason"] = json!("公开网页已获取并保存");
        fact["truncated"] = json!(normalized.truncated);
        files.push(Candidate::json(
            snapshot_slot,
            &json!({"fact":fact,"paragraphs":normalized.paragraphs}),
        )?);
        Ok(ToolOutput {
            payload: fact,
            files,
        })
    }
}
pub struct ReadSource;
#[async_trait]
impl DomainTool for ReadSource {
    fn spec(&self) -> ToolSpec {
        ToolSpec{id:"inquiry.read_source".into(),description:"Read bounded paragraphs of an authorized saved normalized source. Returns exact artifact id/hash and zero-based paragraph positions for citations.".into(),input_schema:json!({"type":"object","properties":{"artifact_id":{"type":"string"},"keyword":{"type":"string","maxLength":200},"start":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":8}},"required":["artifact_id"],"additionalProperties":false})}
    }
    async fn call(&self, c: ToolContext, args: Value) -> Result<ToolOutput> {
        let id = text(&args, "artifact_id")?;
        let a = c
            .accepted
            .iter()
            .find(|a| a.reference.id == id && a.reference.slot.starts_with("capture.snapshot."))
            .ok_or_else(|| fault("来源快照不在当前授权集合"))?;
        let saved = value(a)?;
        let paragraphs = array(&saved, "paragraphs")?;
        let start = args["start"].as_u64().unwrap_or(0) as usize;
        let limit = args["limit"].as_u64().unwrap_or(5) as usize;
        let keyword = args["keyword"].as_str().unwrap_or("").to_lowercase();
        let mut result = Vec::new();
        for (index, p) in paragraphs.iter().enumerate().skip(start) {
            let original = p.as_str().ok_or_else(|| fault("快照段落异常"))?;
            let lower = original.to_lowercase();
            if !keyword.is_empty() && !lower.contains(&keyword) {
                continue;
            }
            let offset = if keyword.is_empty() {
                0
            } else {
                lower.find(&keyword).unwrap_or(0).saturating_sub(300)
            };
            let boundary = original
                .char_indices()
                .map(|(i, _)| i)
                .find(|i| *i >= offset)
                .unwrap_or(0);
            let excerpt: String = original[boundary..].chars().take(4000).collect();
            result.push(json!({"paragraph":index,"text":excerpt,"excerpted":boundary!=0||excerpt.len()<original.len()}));
            if result.len() >= limit {
                break;
            }
        }
        Ok(ToolOutput::json(
            json!({"artifact_id":a.reference.id,"sha256":a.reference.sha256,"fact":saved["fact"],"total_paragraphs":paragraphs.len(),"paragraphs":result}),
        ))
    }
}

fn accepted_json(accepted: &[ArtifactData], state: &str, slot: &str) -> Result<Value> {
    let a = accepted
        .iter()
        .filter(|a| a.reference.state_id == state && a.reference.slot == slot)
        .max_by_key(|a| a.reference.visit)
        .ok_or_else(|| fault(format!("缺少已接受{slot}")))?;
    value(a)
}
fn materials(accepted: &[ArtifactData]) -> Result<Vec<(u64, Value)>> {
    let mut result = Vec::new();
    for a in accepted
        .iter()
        .filter(|a| a.reference.state_id == "capture" && a.reference.slot == "materials")
    {
        for item in array(&value(a)?, "items")? {
            result.push((a.reference.visit, item.clone()));
        }
    }
    Ok(result)
}
pub fn verify_citation(citation: &Value, accepted: &[ArtifactData]) -> Result<()> {
    let id = text(citation, "artifact_id")?;
    let a = accepted
        .iter()
        .find(|a| a.reference.id == id && a.reference.slot.starts_with("capture.snapshot."))
        .ok_or_else(|| fault("引用不属于已授权快照"))?;
    if citation["sha256"] != a.reference.sha256 {
        return Err(fault("引用内容版本不匹配"));
    }
    let snapshot = value(a)?;
    if snapshot["fact"]["usable"] != true {
        return Err(fault("不可用来源不能作为证据"));
    }
    let paragraph = citation["paragraph"]
        .as_u64()
        .ok_or_else(|| fault("引用段落无效"))? as usize;
    let quote = text(citation, "quote")?;
    if quote.trim().is_empty()
        || quote.chars().count() > 200
        || !snapshot["paragraphs"]
            .get(paragraph)
            .and_then(Value::as_str)
            .is_some_and(|p| p.contains(quote))
    {
        return Err(fault("引用不是保存版本该段落的精确短引文"));
    }
    Ok(())
}
fn citation_key(c: &Value) -> String {
    format!(
        "{}|{}|{}|{}",
        c["artifact_id"], c["sha256"], c["paragraph"], c["quote"]
    )
}
fn quoted_words(s: &str) -> usize {
    let cjk = s
        .chars()
        .filter(|c| ('\u{3400}'..='\u{9fff}').contains(c))
        .count();
    if cjk > 0 {
        cjk + s
            .split_whitespace()
            .filter(|w| w.chars().any(|c| c.is_ascii_alphanumeric()))
            .count()
    } else {
        s.split_whitespace().count()
    }
}
pub fn check_domain(
    state: &str,
    visit: u64,
    inputs: &Value,
    out: &Value,
    accepted: &[ArtifactData],
    facts: &[ArtifactReceipt],
    signals: &[SignalFact],
) -> Result<String> {
    match state {
        // The original question is immutable flow input, not model-authored plan data.
        "scope" => Ok("ready".into()),
        "discover" => {
            let mut user_urls = BTreeSet::new();
            for v in inputs["seed_urls"].as_array().into_iter().flatten().chain(
                signals
                    .iter()
                    .filter(|s| s.event_name == "sources.added")
                    .flat_map(|s| s.payload["urls"].as_array().into_iter().flatten()),
            ) {
                if let Some(s) = v.as_str() {
                    user_urls.insert(canonical(s)?);
                }
            }
            let mut urls = BTreeSet::new();
            let mut ids = BTreeSet::new();
            let items = array(out, "items")?;
            for item in items {
                let id = text(item, "source_id")?;
                let u = canonical(text(item, "url")?)?;
                if item["url"] != u
                    || !source_id(id, visit)
                    || !ids.insert(id)
                    || !urls.insert(u.clone())
                {
                    return Err(fault("候选ID/规范网址重复或无效（使用r当前轮s序号）"));
                }
                let receipt = text(item, "discovery_receipt_id")?;
                let genuine = (receipt == "user-input" && user_urls.contains(&u))
                    || facts.iter().any(|f| {
                        f.id == receipt
                            && f.tool_id == "inquiry.search"
                            && f.payload["results"]
                                .as_array()
                                .is_some_and(|r| r.iter().any(|r| r["url"] == u))
                    });
                if !genuine {
                    return Err(fault("候选没有真实发现依据"));
                }
            }
            Ok(if items.is_empty() {
                "need_input"
            } else {
                "ready"
            }
            .into())
        }
        "capture" => {
            let candidates = accepted_json(accepted, "discover", "candidates")?;
            let candidates = array(&candidates, "items")?;
            let items = array(out, "items")?;
            if items.len() != candidates.len() {
                return Err(fault("必须记录每个候选的实际处置"));
            }
            let mut seen = BTreeSet::new();
            let mut usable = 0;
            for item in items {
                let id = text(item, "source_id")?;
                if !seen.insert(id)
                    || !candidates
                        .iter()
                        .any(|a| a["source_id"] == id && a["url"] == item["requested_url"])
                {
                    return Err(fault("资料与候选不一致"));
                }
                let fact = &item["fact"];
                let receipt = text(item, "receipt_id")?;
                if !facts
                    .iter()
                    .any(|f| f.id == receipt && f.tool_id == "inquiry.fetch" && f.payload == *fact)
                {
                    return Err(fault("获取事实不匹配真实工具回执"));
                }
                for key in [
                    "source_id",
                    "requested_url",
                    "final_url",
                    "fetch_status",
                    "http_status",
                    "reason",
                    "snapshot_slot",
                    "raw_slot",
                ] {
                    if item[key] != fact[key] {
                        return Err(fault("资料处置字段与获取事实不一致"));
                    }
                }
                if fact["usable"] == true {
                    usable += 1;
                }
            }
            Ok(if usable > 0 { "ready" } else { "need_input" }.into())
        }
        "catalogue" => {
            if out["decision"] == "ready" && array(out, "groups")?.is_empty() {
                return Err(fault("整理就绪必须有来源分组"));
            }
            let known: BTreeSet<_> = materials(accepted)?
                .into_iter()
                .filter(|(_, m)| m["fact"]["usable"] == true)
                .filter_map(|(_, m)| m["source_id"].as_str().map(str::to_string))
                .collect();
            for group in array(out, "groups")? {
                for id in array(group, "source_ids")? {
                    if !known.contains(id.as_str().unwrap_or("")) {
                        return Err(fault("分类引用未成功获取的来源"));
                    }
                }
            }
            decision(out, visit)
        }
        "analyze" => {
            if out["decision"] == "ready"
                && (array(out, "evidence")?.is_empty() || array(out, "judgments")?.is_empty())
            {
                return Err(fault("分析就绪必须有证据与判断"));
            }
            let mut ids = BTreeSet::new();
            for evidence in array(out, "evidence")? {
                verify_citation(evidence, accepted)?;
                if !ids.insert(text(evidence, "id")?) {
                    return Err(fault("证据ID重复"));
                }
            }
            for judgment in array(out, "judgments")? {
                if array(judgment, "support")?.is_empty() {
                    return Err(fault("判断缺少支持证据"));
                }
                for id in array(judgment, "support")?
                    .iter()
                    .chain(array(judgment, "counter")?)
                {
                    if !ids.contains(id.as_str().unwrap_or("")) {
                        return Err(fault("判断关联悬空证据"));
                    }
                }
            }
            decision(out, visit)
        }
        "report" => {
            let analysis = accepted_json(accepted, "analyze", "analysis")?;
            let evidence = array(&analysis, "evidence")?;
            let mut used = BTreeSet::new();
            let mut budgets: BTreeMap<String, usize> = BTreeMap::new();
            let body = text(out, "conclusion")?;
            let citations = array(out, "citations")?;
            for (i, citation) in citations.iter().enumerate() {
                verify_citation(citation, accepted)?;
                if !used.insert(citation_key(citation))
                    || !evidence
                        .iter()
                        .any(|e| citation_key(e) == citation_key(citation))
                {
                    return Err(fault("报告引入未分析或重复的引用"));
                }
                if !body.contains(&format!("[{}]", i + 1)) {
                    return Err(fault("引用未在正文中使用"));
                }
                let source = accepted
                    .iter()
                    .find(|a| a.reference.id == citation["artifact_id"])
                    .expect("citation verified");
                let saved = value(source)?;
                let key = text(&saved["fact"], "final_url")?.to_string();
                *budgets.entry(key).or_default() += quoted_words(text(citation, "quote")?);
            }
            if budgets.values().any(|n| *n > 25) {
                return Err(fault("报告对同一来源的短引文累计超过25词"));
            }
            if out["answer_status"] == "answered" && citations.is_empty() {
                return Err(fault("完整研究报告不能没有依据"));
            }
            for part in body.split('[').skip(1) {
                if let Some((number, _)) = part.split_once(']')
                    && let Ok(number) = number.parse::<usize>()
                    && !(1..=citations.len()).contains(&number)
                {
                    return Err(fault("正文引用编号悬空"));
                }
            }
            Ok(if out["answer_status"] == "answered" {
                "ready"
            } else {
                "need_input"
            }
            .into())
        }
        _ => Err(fault("未知研究阶段")),
    }
}
fn decision(out: &Value, visit: u64) -> Result<String> {
    let decision = text(out, "decision")?;
    if decision == "ready" && !array(out, "gaps")?.is_empty() {
        return Err(fault("存在缺口时不能声称整理/分析完成"));
    }
    Ok(if decision == "need_more" && visit >= 3 {
        "need_input"
    } else {
        decision
    }
    .into())
}
fn verify_capture(out: &Value, candidates: &BTreeMap<String, Candidate>) -> Result<()> {
    let mut allowed = BTreeSet::from(["materials".to_string()]);
    for item in array(out, "items")? {
        for key in ["raw_slot", "snapshot_slot"] {
            if let Some(slot) = item["fact"][key].as_str() {
                allowed.insert(slot.to_string());
            }
        }
    }
    if candidates.keys().any(|slot| !allowed.contains(slot)) {
        return Err(fault("存在未由已核对获取事实声明的附件"));
    }
    for item in array(out, "items")? {
        let fact = &item["fact"];
        if let Some(slot) = fact["raw_slot"].as_str() {
            let raw = candidates
                .get(slot)
                .ok_or_else(|| fault("原始正文未实际留存"))?;
            if fact["body_sha256"] != hash(&raw.bytes) {
                return Err(fault("原始正文摘要不匹配"));
            }
        }
        if fact["usable"] == true {
            let slot = text(fact, "snapshot_slot")?;
            let snapshot = candidates
                .get(slot)
                .ok_or_else(|| fault("规范化快照未实际留存"))?
                .value()?;
            if snapshot["fact"] != *fact
                || fact["normalized_sha256"]
                    != hash(&serde_json::to_vec(array(&snapshot, "paragraphs")?)?)
            {
                return Err(fault("规范化版本与获取事实不匹配"));
            }
        }
    }
    Ok(())
}
pub struct ResearchGate;
#[async_trait]
impl DomainGate for ResearchGate {
    async fn check(&self, c: GateContext) -> CheckResult {
        let slot = match c.state_id.as_str() {
            "scope" => "plan",
            "discover" => "candidates",
            "capture" => "materials",
            "catalogue" => "catalogue",
            "analyze" => "analysis",
            "report" => "report",
            _ => return CheckResult::unavailable("未知领域阶段"),
        };
        let result = (|| -> Result<String> {
            let out = c.output(slot)?;
            if c.state_id == "capture" {
                verify_capture(&out, &c.candidates)?;
            }
            check_domain(
                &c.state_id,
                c.visit,
                &c.inputs,
                &out,
                &c.accepted,
                &c.tool_facts,
                &c.signal_facts,
            )
        })();
        match result {
            Ok(outcome) => CheckResult::pass(outcome, vec![c.check_evidence_ref]),
            Err(e) => CheckResult::fail(e.code, e.path, e.message),
        }
    }
}
pub fn definition(root: &Path) -> Result<Definition> {
    Definition::load(
        root.join("resources/research.bundle.json"),
        &BTreeMap::from([(
            "inquiry-deep-research@0.3.5".into(),
            root.join("skills/inquiry-deep-research"),
        )]),
    )
}
pub fn bindings(d: &Definition, executor: Arc<dyn Executor>) -> HostBindings {
    let tools: Vec<Arc<dyn DomainTool>> =
        vec![Arc::new(Search), Arc::new(Fetch), Arc::new(ReadSource)];
    let mut b = HostBindings::default()
        .configuration_version("inquiry@0.1.0")
        .executor("research.agent", executor);
    for state in [
        "scope",
        "discover",
        "capture",
        "catalogue",
        "analyze",
        "report",
    ] {
        b = b.gate(format!("inquiry.{state}.gate"), Arc::new(ResearchGate));
    }
    let mut ids = vec!["artifact.read".into(), "artifact.write".into()];
    for tool in tools {
        let id = tool.spec().id;
        ids.push(id.clone());
        b.tools.insert(id, tool);
    }
    b.grants.insert(
        "local-readonly".into(),
        Grant {
            definitions: vec![d.digest.clone()],
            tools: ids,
            limits: limits(),
        },
    );
    b
}
pub fn limits() -> ResourceLimits {
    ResourceLimits {
        max_active_ms: 1_800_000,
        ..ResourceLimits::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn artifact(state: &str, slot: &str, id: &str, v: Value) -> ArtifactData {
        let bytes = serde_json::to_vec(&v).unwrap();
        ArtifactData {
            reference: ArtifactRef {
                id: id.into(),
                sha256: hash(&bytes),
                slot: slot.into(),
                state_id: state.into(),
                visit: 1,
            },
            bytes: bytes.into(),
        }
    }
    fn saved() -> ArtifactData {
        artifact(
            "capture",
            "capture.snapshot.r1s1",
            "saved-one",
            json!({"fact":{"usable":true,"final_url":"https://example.com/"},"paragraphs":["A precise, saved statement about this example."]}),
        )
    }
    fn cite(a: &ArtifactData) -> Value {
        json!({"artifact_id":a.reference.id,"sha256":a.reference.sha256,"paragraph":0,"quote":"precise, saved statement"})
    }
    #[test]
    fn search_parser_keeps_only_actual_result_titles_and_links() {
        let html = r#"<a href="https://advert.example/">Advertisement</a><div class="snippet" data-type="web"><a href="https://example.com/docs"><div class="search-snippet-title">Example Docs</div></a><div class="result-cluster"><div class="snippet"><a class="title" href="https://example.com/deep">Deep documentation</a></div></div><a href="https://ignored.example/">non-result action</a><a class="title" href="javascript:alert(1)">Invalid</a></div><script>invented.example</script>"#;
        let results = search_results(html);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["url"], "https://example.com/docs");
        assert_eq!(results[1]["url"], "https://example.com/deep");
        assert!(search_results("<html>Complete a challenge</html>").is_empty());
    }
    #[test]
    fn production_contract_loads_from_static_package() {
        let d = definition(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        assert!(!d.digest.is_empty());
    }
    #[test]
    fn normalization_removes_active_and_navigation_content_even_in_fallback() {
        let n = normalize(b"<html><title>Title</title><main><div>Useful text</div><script>steal credentials</script><nav>Menu</nav></main></html>", "text/html").unwrap();
        assert_eq!(n.title, "Title");
        assert_eq!(n.paragraphs, ["Useful text"]);
        let n = normalize(b"<main><h1>Heading</h1><p>Statement<script>injected</script></p><p>Statement</p></main>", "text/html").unwrap();
        assert_eq!(n.paragraphs, ["Heading", "Statement"]);
        assert!(normalize(b"<script>only active content</script>", "text/html").is_err());
        assert!(normalize(b"%PDF-1.4", "application/pdf").is_err());
    }
    #[test]
    fn normalization_limits_are_explicit() {
        let body = format!("<main><p>{}</p></main>", "x".repeat(9000));
        let n = normalize(body.as_bytes(), "text/html").unwrap();
        assert!(n.truncated);
        assert_eq!(n.paragraphs[0].len(), 8000);
    }
    #[test]
    fn exact_citation_rejects_other_version_and_location() {
        let a = saved();
        let good = cite(&a);
        verify_citation(&good, std::slice::from_ref(&a)).unwrap();
        for (key, wrong) in [
            ("sha256", json!("wrong")),
            ("artifact_id", json!("other")),
            ("paragraph", json!(1)),
            ("quote", json!("fabricated statement")),
        ] {
            let mut c = good.clone();
            c[key] = wrong;
            assert!(
                verify_citation(&c, std::slice::from_ref(&a)).is_err(),
                "{key}"
            );
        }
    }
    #[test]
    fn discovery_requires_actual_origin_and_current_visit() {
        let inputs = json!({"seed_urls":["https://example.com/"]});
        let mut out = json!({"items":[{"source_id":"r1s1","url":"https://example.com/","discovery_receipt_id":"user-input"}]});
        assert_eq!(
            check_domain("discover", 1, &inputs, &out, &[], &[], &[]).unwrap(),
            "ready"
        );
        assert!(check_domain("discover", 2, &inputs, &out, &[], &[], &[]).is_err());
        out["items"][0]["url"] = json!("https://invented.example/");
        assert!(check_domain("discover", 1, &inputs, &out, &[], &[], &[]).is_err());
    }
    #[test]
    fn capture_checks_bytes_not_only_claimed_hashes() {
        let raw = b"actual body";
        let paragraphs = json!(["actual body"]);
        let fact = json!({"raw_slot":"capture.raw.r1s1","snapshot_slot":"capture.snapshot.r1s1","usable":true,"body_sha256":hash(raw),"normalized_sha256":hash(&serde_json::to_vec(&paragraphs).unwrap())});
        let out = json!({"items":[{"fact":fact}]});
        let mut files = BTreeMap::from([
            (
                "materials".into(),
                Candidate::json("materials", &out).unwrap(),
            ),
            (
                "capture.raw.r1s1".into(),
                Candidate::bytes("capture.raw.r1s1", raw.to_vec()),
            ),
            (
                "capture.snapshot.r1s1".into(),
                Candidate::json(
                    "capture.snapshot.r1s1",
                    &json!({"fact":fact,"paragraphs":paragraphs}),
                )
                .unwrap(),
            ),
        ]);
        verify_capture(&out, &files).unwrap();
        files.insert(
            "capture.raw.r1s1".into(),
            Candidate::bytes("capture.raw.r1s1", b"other version".to_vec()),
        );
        assert!(verify_capture(&out, &files).is_err());
        files.insert(
            "capture.raw.r1s1".into(),
            Candidate::bytes("capture.raw.r1s1", raw.to_vec()),
        );
        files.insert(
            "capture.snapshot.r1s1".into(),
            Candidate::json(
                "capture.snapshot.r1s1",
                &json!({"fact":fact,"paragraphs":["invented"]}),
            )
            .unwrap(),
        );
        assert!(verify_capture(&out, &files).is_err());
    }
    #[test]
    fn material_claims_must_match_current_trusted_receipt() {
        let candidates = artifact(
            "discover",
            "candidates",
            "discovery",
            json!({"items":[{"source_id":"r1s1","url":"https://example.com/"}]}),
        );
        let fact = json!({"source_id":"r1s1","requested_url":"https://example.com/","final_url":null,"fetch_status":"unavailable","http_status":null,"reason":"network failure","snapshot_slot":null,"raw_slot":null,"usable":false});
        let mut item = fact.clone();
        item["fact"] = fact.clone();
        item["receipt_id"] = json!("actual-receipt");
        let mut out = json!({"items":[item]});
        let receipt = ArtifactReceipt {
            protocol_version: 1,
            id: "actual-receipt".into(),
            execution_id: "research-A".into(),
            run_id: "run-A".into(),
            tool_id: "inquiry.fetch".into(),
            occurred_at: "2026-11-03T00:00:00Z".into(),
            request_sha256: "request".into(),
            result_sha256: "result".into(),
            payload: fact,
        };
        assert_eq!(
            check_domain(
                "capture",
                1,
                &Value::Null,
                &out,
                std::slice::from_ref(&candidates),
                std::slice::from_ref(&receipt),
                &[]
            )
            .unwrap(),
            "need_input"
        );
        assert!(
            check_domain(
                "capture",
                1,
                &Value::Null,
                &out,
                std::slice::from_ref(&candidates),
                &[],
                &[]
            )
            .is_err()
        );
        out["items"][0]["fact"]["usable"] = json!(true);
        assert!(
            check_domain(
                "capture",
                1,
                &Value::Null,
                &out,
                &[candidates],
                &[receipt],
                &[]
            )
            .is_err()
        );
    }
    #[test]
    fn reports_cannot_introduce_new_or_dangling_evidence() {
        let a = saved();
        let c = cite(&a);
        let analysis = artifact(
            "analyze",
            "analysis",
            "analysis-one",
            json!({"evidence":[c]}),
        );
        let accepted = vec![a, analysis];
        let mut report =
            json!({"answer_status":"answered","conclusion":"The conclusion [1].","citations":[c]});
        assert_eq!(
            check_domain("report", 1, &Value::Null, &report, &accepted, &[], &[]).unwrap(),
            "ready"
        );
        report["conclusion"] = json!("Dangling [2]");
        assert!(check_domain("report", 1, &Value::Null, &report, &accepted, &[], &[]).is_err());
        report["conclusion"] = json!("New exact quote [1]");
        report["citations"][0]["quote"] = json!("about this example");
        assert!(check_domain("report", 1, &Value::Null, &report, &accepted, &[], &[]).is_err());
    }
    #[test]
    fn gaps_and_bounded_replanning_cannot_be_hidden() {
        assert!(decision(&json!({"decision":"ready","gaps":["missing evidence"]}), 1).is_err());
        assert_eq!(
            decision(
                &json!({"decision":"need_more","gaps":["missing evidence"]}),
                3
            )
            .unwrap(),
            "need_input"
        );
        let out = json!({"decision":"ready","gaps":[],"evidence":[],"judgments":[{"support":["missing"],"counter":[]}]});
        assert!(check_domain("analyze", 1, &Value::Null, &out, &[], &[], &[]).is_err());
    }
}
