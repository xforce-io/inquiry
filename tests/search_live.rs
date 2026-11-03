//! Actual public-network producer check. No language model and no report.
use async_trait::async_trait;
use inquiry::domain;
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use taskloop::*;
use tokio_util::sync::CancellationToken;
struct SearchProbe(Arc<Mutex<Vec<Value>>>);
#[async_trait]
impl Executor for SearchProbe {
    fn capabilities(&self) -> ExecutorCapabilities {
        ExecutorCapabilities::MANAGED
    }
    async fn execute(
        &self,
        inv: Invocation,
        tools: Arc<dyn ToolBridge>,
        _: CancellationToken,
    ) -> ExecutionOutcome {
        let (slot, value) = if inv.state_id == "scope" {
            (
                "plan",
                json!({"question":inv.inputs["question"],"subquestions":["synthetic producer probe"],"search_queries":["Playwright locators auto waiting official documentation"],"stop_reason":"public search tool only; no model/report"}),
            )
        } else {
            for query in [
                "Playwright locators auto waiting official documentation",
                "WebArena benchmark browser agents official",
            ] {
                let result = match tools.call("inquiry.search", json!({"query":query})).await {
                    Ok(v) => v,
                    Err(e) => {
                        return ExecutionOutcome::Failed {
                            code: e.code,
                            message: e.message,
                        };
                    }
                };
                self.0.lock().unwrap().push(result);
            }
            (
                "candidates",
                json!({"items":[],"stop_reason":"synthetic producer probe intentionally stops before capture/report"}),
            )
        };
        match tools
            .call("artifact.write", json!({"slot":slot,"value":value}))
            .await
        {
            Ok(_) => ExecutionOutcome::Completed {
                usage: json!({"kind":"synthetic","model":null}),
            },
            Err(e) => ExecutionOutcome::Failed {
                code: e.code,
                message: e.message,
            },
        }
    }
    async fn stop_or_confirm(&self, _: &str) -> StopState {
        StopState::Confirmed
    }
}
#[tokio::test]
#[ignore = "Actual external public-network check; no model invocation"]
async fn public_search_producer_returns_relevant_real_links() {
    let root = tempfile::tempdir().unwrap();
    let d = domain::definition(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let digest = d.digest.clone();
    let results = Arc::new(Mutex::new(Vec::new()));
    let b = domain::bindings(&d, Arc::new(SearchProbe(results.clone())));
    let rt = Runtime::open(RuntimeOptions::new(root.path().join("store")), vec![d], b)
        .await
        .unwrap();
    let id=rt.start(StartRequest{definition_digest:digest,inputs:json!({"question":"synthetic search producer probe","seed_urls":[],"scope":"No actual report"}),grant_ref:"local-readonly".into(),request_key:"search-probe".into(),limits:domain::limits()},&Caller::local()).await.unwrap();
    let mut stopped = false;
    for _ in 0..1500 {
        let s = rt.get(&id, &Caller::local()).await.unwrap();
        if s.status == Status::Waiting {
            stopped = true;
            break;
        }
        assert!(!s.status.terminal(), "{s:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    rt.shutdown().await.unwrap();
    assert!(stopped);
    let results = results.lock().unwrap();
    assert_eq!(results.len(), 2);
    for result in results.iter() {
        println!("{}", result);
        assert_eq!(result["result"]["status"], "discovered");
        assert_eq!(result["result"]["provider"], "brave-html");
        assert!(!result["result"]["results"].as_array().unwrap().is_empty());
    }
    assert!(
        results[0]["result"]["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["url"]
                .as_str()
                .unwrap_or("")
                .starts_with("https://playwright.dev/docs/"))
    );
    assert!(
        results[1]["result"]["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["title"]
                .as_str()
                .unwrap_or("")
                .to_ascii_lowercase()
                .contains("webarena"))
    );
}
