use super::*;
use std::{collections::VecDeque, sync::Mutex, time::Duration};

struct SearchFixture {
    replies: Mutex<VecDeque<Value>>,
    calls: Mutex<Vec<String>>,
}
impl SearchFixture {
    fn new(replies: Vec<Value>) -> Arc<Self> {
        Arc::new(Self {
            replies: Mutex::new(replies.into()),
            calls: Mutex::new(vec![]),
        })
    }
}
#[async_trait]
impl ToolBridge for SearchFixture {
    fn specs(&self) -> Vec<ToolSpec> {
        vec![]
    }
    async fn call(&self, id: &str, args: Value) -> Result<Value> {
        assert_eq!(id, "inquiry.search");
        self.calls
            .lock()
            .unwrap()
            .push(args["query"].as_str().unwrap().into());
        Ok(
            json!({"result": self.replies.lock().unwrap().pop_front().expect("unexpected extra call")}),
        )
    }
}
fn unavailable(ms: u64) -> Value {
    json!({"query":"q","status":"unavailable","retryable":true,"retry_after_ms":ms,"fetch_attempts":1,"upstream_requests":null,"results":[]})
}
fn discovered() -> Value {
    json!({"query":"q","status":"discovered","retryable":false,"fetch_attempts":1,"results":[{"url":"https://example.com/vertex"}]})
}
#[test]
fn selection_reaches_actual_results_beyond_fifth_and_caps_at_twenty() {
    let urls = (0..30)
        .map(|n| json!({"url":format!("https://example.com/{n}")}))
        .collect::<Vec<_>>();
    let selected = candidates(
        &[],
        &[json!({"query":"q","status":"discovered","results":urls})],
        &BTreeSet::new(),
    );
    assert_eq!(selected.len(), 20);
    assert_eq!(selected[19].0, "https://example.com/19");
}
#[test]
fn supplement_retries_temporary_failures_without_repeating_saved_or_denied_sources() {
    assert!(!exclude_source(
        &json!({"usable":false,"fetch_status":"unavailable","http_status":null})
    ));
    for status in [408, 425, 429, 500, 503] {
        assert!(!exclude_source(
            &json!({"usable":false,"fetch_status":"unavailable","http_status":status})
        ));
    }
    assert!(!exclude_source(
        &json!({"usable":false,"fetch_status":"captured","http_status":200})
    ));
    for fact in [
        json!({"usable":true,"fetch_status":"captured","http_status":200}),
        json!({"usable":false,"fetch_status":"denied","http_status":null}),
        json!({"usable":false,"fetch_status":"unavailable","http_status":404}),
        json!({"usable":false,"fetch_status":"unavailable","http_status":200}),
    ] {
        assert!(exclude_source(&fact), "{fact}");
    }
}
#[tokio::test]
async fn caller_recovers_same_query_and_preserves_actual_attempts() {
    let fixture = SearchFixture::new(vec![unavailable(2), discovered()]);
    let bridge: Arc<dyn ToolBridge> = fixture.clone();
    let out = collect_searches(vec!["q".into()], &bridge, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0]["status"], "discovered");
    assert_eq!(*fixture.calls.lock().unwrap(), vec!["q", "q"]);
    assert_eq!(out[0]["recovery"]["previous_results"][0], unavailable(2));
    assert_eq!(out[0]["recovery"]["scheduled_wait_ms"], json!([2]));
}
#[tokio::test]
async fn cancellation_during_provider_cooldown_issues_no_further_search() {
    let fixture = SearchFixture::new(vec![unavailable(60000), discovered()]);
    let bridge: Arc<dyn ToolBridge> = fixture.clone();
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let pending =
        tokio::spawn(async move { collect_searches(vec!["q".into()], &bridge, &cancel).await });
    while fixture.calls.lock().unwrap().is_empty() {
        tokio::task::yield_now().await;
    }
    trigger.cancel();
    assert!(
        tokio::time::timeout(Duration::from_secs(1), pending)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    assert_eq!(fixture.calls.lock().unwrap().len(), 1);
}

#[test]
fn recovery_respects_total_wait_and_call_boundaries_without_shortening_retry_after() {
    let mut policy = SearchRecovery::default();
    assert_eq!(policy.reserve(&unavailable(60000)), Some(60000));
    assert_eq!(policy.reserve(&unavailable(60001)), None);
    assert_eq!(policy.reserve(&unavailable(60000)), Some(60000));
    assert_eq!(policy.reserve(&unavailable(0)), None);
    let mut policy = SearchRecovery::default();
    assert_eq!(policy.reserve(&unavailable(120001)), None);
    assert_eq!(
        policy.reserve(&json!({"status":"unavailable","retryable":true})),
        None
    );
    assert_eq!(
        policy.reserve(&json!({"status":"unavailable","retryable":false,"retry_after_ms":1})),
        None
    );
    assert_eq!(policy.reserve(&discovered()), None);
}
#[tokio::test]
async fn eight_queries_share_two_retries_and_do_not_invent_network_measurements() {
    let fixture = SearchFixture::new(vec![unavailable(1); 10]);
    let bridge: Arc<dyn ToolBridge> = fixture.clone();
    let out = collect_searches(
        (0..8).map(|n| format!("q{n}")).collect(),
        &bridge,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(out.len(), 8);
    assert_eq!(fixture.calls.lock().unwrap().len(), 10);
    assert_eq!(
        out[0]["recovery"]["previous_results"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(out[0]["fetch_attempts"], 1);
    assert!(out.iter().all(|s| s["upstream_requests"].is_null()));
    assert!(out[1..].iter().all(|s| s.get("recovery").is_none()));
}
#[tokio::test]
async fn cancelled_collection_never_calls_search() {
    let fixture = SearchFixture::new(vec![]);
    let bridge: Arc<dyn ToolBridge> = fixture.clone();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(
        collect_searches(vec!["q".into()], &bridge, &cancel)
            .await
            .is_err()
    );
    assert!(fixture.calls.lock().unwrap().is_empty());
}
#[tokio::test]
async fn oversized_provider_delay_is_preserved_without_early_retry() {
    let original = unavailable(120001);
    let fixture = SearchFixture::new(vec![original.clone()]);
    let bridge: Arc<dyn ToolBridge> = fixture.clone();
    let out = collect_searches(vec!["q".into()], &bridge, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(out, vec![original]);
    assert_eq!(fixture.calls.lock().unwrap().len(), 1);
}
