//! Deterministic transport/Runtime fixtures. Never real model or live-search evidence.
use super::*;
use crate::domain;
use std::{collections::VecDeque, path::Path, sync::atomic::AtomicUsize};
use taskloop::*;
use tokio_util::sync::CancellationToken;

enum Response {
    Http(u16, Option<&'static str>),
    Network,
    Denied,
    Hang,
}
struct Script {
    responses: std::sync::Mutex<VecDeque<Response>>,
    calls: AtomicUsize,
}
impl Script {
    fn new(responses: Vec<Response>) -> Arc<Self> {
        Arc::new(Self {
            responses: std::sync::Mutex::new(responses.into()),
            calls: AtomicUsize::new(0),
        })
    }
}
const HTML: &[u8] = br#"<div id="links"><div class="web-result"><h2 class="result__title"><a class="result__a" href="https://example.com/source">Controlled source</a></h2></div></div>"#;
#[async_trait]
impl Backend for Script {
    async fn fetch(
        &self,
        url: &str,
        _: &ToolContext,
    ) -> std::result::Result<crate::net::Page, crate::net::FetchError> {
        assert!(url.starts_with("https://html.duckduckgo.com/html/?"));
        self.calls.fetch_add(1, Ordering::SeqCst);
        let response = self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected extra request");
        match response {
            Response::Http(status, retry_after) => Ok(crate::net::Page {
                requested_url: url.into(),
                final_url: url.into(),
                status,
                retry_after: retry_after.map(str::to_owned),
                content_type: "text/html".into(),
                body: HTML.to_vec(),
            }),
            Response::Network | Response::Denied => Err(crate::net::FetchError {
                denied: matches!(response, Response::Denied),
                reason: "private diagnostic must never become public".into(),
            }),
            Response::Hang => std::future::pending().await,
        }
    }
}
struct Probe {
    results: Arc<std::sync::Mutex<Vec<Value>>>,
}
#[async_trait]
impl Executor for Probe {
    fn capabilities(&self) -> ExecutorCapabilities {
        ExecutorCapabilities::MANAGED
    }
    async fn execute(
        &self,
        inv: Invocation,
        tools: Arc<dyn ToolBridge>,
        cancel: CancellationToken,
    ) -> ExecutionOutcome {
        let result:Result<()> = async {
            if inv.state_id=="capture" {cancel.cancelled().await;return Ok(());}
            let (slot,out)=if inv.state_id=="scope" {("plan",json!({"subquestions":["controlled recovery"],"search_queries":["controlled recovery"],"stop_reason":"No model"}))} else {
                let query=inv.inputs["question"].as_str().unwrap();
                let got=tools.call("inquiry.search",json!({"query":query})).await?;
                self.results.lock().unwrap().push(got.clone());
                let items=if got["result"]["status"]=="discovered" {vec![json!({"source_id":format!("r{}s1",inv.visit),"url":"https://example.com/source","discovery_receipt_id":got["receipt_id"],"subquestion":"Controlled recovery"})]}else{vec![]};
                ("candidates",json!({"items":items,"queries":[{"query":query,"receipt_id":got["receipt_id"],"status":got["result"]["status"]}],"stop_reason":"Controlled transport; not actual research"}))
            };
            tools.call("artifact.write",json!({"slot":slot,"value":out})).await?;Ok(())
        }.await;
        match result {
            Ok(()) => ExecutionOutcome::Completed {
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
async fn runtime(
    root: &Path,
    backend: Arc<dyn Backend>,
    results: Arc<std::sync::Mutex<Vec<Value>>>,
) -> (Arc<Runtime>, String) {
    let d = domain::definition(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let digest = d.digest.clone();
    let mut bindings = domain::bindings(&d, Arc::new(Probe { results }));
    bindings.tools.insert(
        "inquiry.search".into(),
        Arc::new(Search {
            backend,
            cooldown: Mutex::new(0),
            origin: Instant::now(),
        }),
    );
    (
        Arc::new(
            Runtime::open(
                crate::budget::runtime_options(root.join("runtime-store")),
                vec![d],
                bindings,
            )
            .await
            .unwrap(),
        ),
        digest,
    )
}
async fn start(rt: &Runtime, digest: &str, key: &str) -> String {
    rt.start(
        StartRequest {
            definition_digest: digest.into(),
            inputs: json!({"question":key,"seed_urls":[],"scope":"Controlled fixture only"}),
            grant_ref: "local-readonly".into(),
            request_key: key.into(),
            limits: domain::limits(),
        },
        &Caller::local(),
    )
    .await
    .unwrap()
}
async fn until_result(results: &std::sync::Mutex<Vec<Value>>, count: usize) {
    tokio::time::timeout(Duration::from_secs(35), async {
        while results.lock().unwrap().len() < count {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}
#[test]
fn retry_after_supports_dates_invalid_values_and_overflow_without_wrapping() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-11-08T00:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert_eq!(
        retry_millis(Some("Sun, 08 Nov 2026 00:00:05 GMT"), now),
        5000
    );
    assert_eq!(retry_millis(Some("-1"), now), 60_000);
    assert_eq!(retry_millis(Some("bad"), now), 60_000);
    assert_eq!(retry_millis(None, now), 60_000);
    assert_eq!(
        retry_millis(
            Some("999999999999999999999999999999999999999999999999"),
            now
        ),
        u128::MAX
    );
}
#[tokio::test]
async fn bounded_http_and_network_recovery_and_provider_cooldown_preserve_receipts() {
    for (script, expected, attempts) in [
        (
            vec![Response::Http(429, Some("0")), Response::Http(200, None)],
            "discovered",
            2,
        ),
        (
            vec![Response::Http(503, Some("0")), Response::Http(200, None)],
            "discovered",
            2,
        ),
        (
            vec![Response::Network, Response::Http(200, None)],
            "discovered",
            2,
        ),
        (vec![Response::Denied], "unavailable", 1),
        (vec![Response::Http(403, None)], "unavailable", 1),
        (vec![Response::Http(429, None)], "unavailable", 1),
        (
            vec![
                Response::Http(429, Some("0")),
                Response::Http(429, Some("0")),
                Response::Http(429, Some("0")),
            ],
            "unavailable",
            3,
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let backend = Script::new(script);
        let results: Arc<std::sync::Mutex<Vec<Value>>> = Arc::default();
        let (rt, digest) = runtime(root.path(), backend.clone(), results.clone()).await;
        let id = start(&rt, &digest, "first").await;
        until_result(&results, 1).await;
        let output = results.lock().unwrap()[0].clone();
        assert_eq!(output["result"]["status"], expected);
        assert_eq!(output["result"]["fetch_attempts"], attempts);
        assert!(output["receipt_id"].is_string());
        assert!(!output.to_string().contains("private diagnostic"));
        assert_eq!(backend.calls.load(Ordering::SeqCst), attempts);
        if output["result"]["retry_after_ms"].as_u64().unwrap_or(0) > 30_000 {
            let second = start(&rt, &digest, "second").await;
            until_result(&results, 2).await;
            assert_eq!(results.lock().unwrap()[1]["result"]["fetch_attempts"], 0);
            assert_eq!(backend.calls.load(Ordering::SeqCst), attempts);
            rt.cancel(&second, "cancel-second", &Caller::local())
                .await
                .unwrap();
        }
        rt.cancel(&id, "cancel-first", &Caller::local())
            .await
            .unwrap();
        rt.shutdown().await.unwrap();
    }
}
#[tokio::test]
async fn cancellation_during_retry_sleep_prevents_another_request() {
    let root = tempfile::tempdir().unwrap();
    let backend = Script::new(vec![Response::Http(429, Some("5"))]);
    let results: Arc<std::sync::Mutex<Vec<Value>>> = Arc::default();
    let (rt, digest) = runtime(root.path(), backend.clone(), results).await;
    let id = start(&rt, &digest, "cancel-cooldown").await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while backend.calls.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let begin = Instant::now();
    rt.cancel(&id, "cancel-cooldown", &Caller::local())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while rt.get(&id, &Caller::local()).await.unwrap().status != Status::Cancelled {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(begin.elapsed() < Duration::from_secs(2));
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    rt.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancellation_while_queued_for_shared_provider_does_not_issue_a_fetch() {
    let root = tempfile::tempdir().unwrap();
    let backend = Script::new(vec![Response::Hang]);
    let results: Arc<std::sync::Mutex<Vec<Value>>> = Arc::default();
    let (rt, digest) = runtime(root.path(), backend.clone(), results).await;
    let first = start(&rt, &digest, "holding-provider").await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while backend.calls.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let second = start(&rt, &digest, "queued-provider").await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    rt.cancel(&second, "cancel-queued", &Caller::local())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while rt.get(&second, &Caller::local()).await.unwrap().status != Status::Cancelled {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    rt.cancel(&first, "cancel-holder", &Caller::local())
        .await
        .unwrap();
    rt.shutdown().await.unwrap();
}

#[tokio::test]
async fn processing_deadline_during_provider_cooldown_preserves_plan_without_retry() {
    let root = tempfile::tempdir().unwrap();
    let backend = Script::new(vec![Response::Http(429, Some("5"))]);
    let results: Arc<std::sync::Mutex<Vec<Value>>> = Arc::default();
    let (rt, digest) = runtime(root.path(), backend.clone(), results).await;
    let mut limits = domain::limits();
    limits.max_active_ms = 1500;
    let id=rt.start(StartRequest{definition_digest:digest,inputs:json!({"question":"deadline-cooldown","scope":"Controlled fixture","seed_urls":[]}),grant_ref:"local-readonly".into(),request_key:"deadline-cooldown".into(),limits},&Caller::local()).await.unwrap();
    let snapshot = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let s = rt.get(&id, &Caller::local()).await.unwrap();
            if s.status.terminal() {
                break s;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(snapshot.status, Status::Failed);
    assert!(snapshot.error.is_some());
    assert!(snapshot.accepted_artifacts.iter().any(|a| a.slot == "plan"));
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    rt.shutdown().await.unwrap();
}
#[tokio::test]
async fn network_timeout_includes_fetch_and_does_not_fabricate_zero_attempts() {
    let root = tempfile::tempdir().unwrap();
    let backend = Script::new(vec![Response::Hang]);
    let results: Arc<std::sync::Mutex<Vec<Value>>> = Arc::default();
    let (rt, digest) = runtime(root.path(), backend.clone(), results.clone()).await;
    let begin = Instant::now();
    let id = start(&rt, &digest, "timeout").await;
    until_result(&results, 1).await;
    assert!(begin.elapsed() < Duration::from_secs(34));
    assert_eq!(results.lock().unwrap()[0]["result"]["fetch_attempts"], 1);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    rt.cancel(&id, "cancel-timeout", &Caller::local())
        .await
        .unwrap();
    rt.shutdown().await.unwrap();
}

struct BrowserScript {
    calls: std::sync::Mutex<Vec<Value>>,
    log: std::path::PathBuf,
}
#[async_trait]
impl Backend for BrowserScript {
    async fn fetch(
        &self,
        url: &str,
        _: &ToolContext,
    ) -> std::result::Result<crate::net::Page, crate::net::FetchError> {
        let u = url::Url::parse(url).unwrap();
        let query = u
            .query_pairs()
            .find(|(k, _)| k == "q")
            .unwrap()
            .1
            .into_owned();
        let mut calls = self.calls.lock().unwrap();
        let previous = calls.iter().filter(|v| v["query"] == query).count();
        let (status, retry_after) = match query.as_str() {
            "恢复" if previous == 0 => (429, Some("61")),
            "恢复" => (200, None),
            "冷却" => (429, Some("5")),
            _ => (429, Some("0")),
        };
        calls.push(json!({"query":query,"status":status,"at":chrono::Utc::now().to_rfc3339()}));
        std::fs::write(&self.log, serde_json::to_vec_pretty(&*calls).unwrap()).unwrap();
        Ok(crate::net::Page {
            requested_url: url.into(),
            final_url: url.into(),
            status,
            retry_after: retry_after.map(str::to_owned),
            content_type: "text/html".into(),
            body: HTML.to_vec(),
        })
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "Manual browser fixture; requires explicit private data path and built candidate UI; zero models"]
async fn browser_recovery_host() {
    let data = std::path::PathBuf::from(
        std::env::var_os("INQUIRY_RECOVERY_TEST_DATA").expect("explicit private data path"),
    );
    assert!(data.is_absolute());
    std::fs::create_dir_all(&data).unwrap();
    let port: u16 = std::env::var("INQUIRY_RECOVERY_TEST_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let backend = Arc::new(BrowserScript {
        calls: Default::default(),
        log: data.join("http-fixture.json"),
    });
    let (rt, digest) = runtime(&data.join("store"), backend, Arc::default()).await;
    let app = crate::web::router(crate::web::Host {
        runtime: rt.clone(),
        definition_digest: digest,
        port,
        session: uuid::Uuid::new_v4().to_string(),
        web_dir: Path::new(env!("CARGO_MANIFEST_DIR")).join("web/dist"),
        search_provider: crate::search::PROVIDER,
    });
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .unwrap();
    println!("Controlled recovery fixture at http://127.0.0.1:{port}; zero models");
    axum::serve(listener, app)
        .with_graceful_shutdown(async { tokio::signal::ctrl_c().await.unwrap() })
        .await
        .unwrap();
    rt.shutdown().await.unwrap();
}
