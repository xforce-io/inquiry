//! Deterministic integration checks, not evidence of actual model research.
use async_trait::async_trait;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use inquiry::{domain, web};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc, time::Duration};
use taskloop::*;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;
struct Controlled;
#[async_trait]
impl Executor for Controlled {
    fn enforces_json_candidate_limit(&self) -> bool {
        true
    }
    fn capabilities(&self) -> ExecutorCapabilities {
        ExecutorCapabilities::MANAGED
    }
    async fn execute(
        &self,
        inv: Invocation,
        tools: Arc<dyn ToolBridge>,
        cancel: CancellationToken,
    ) -> ExecutionOutcome {
        if inv.inputs["question"] == "hold" {
            cancel.cancelled().await;
            return ExecutionOutcome::Cancelled;
        }
        let (slot, value) = match inv.state_id.as_str() {
            "scope" => (
                "plan",
                json!({"subquestions":[inv.inputs["question"]],"search_queries":["synthetic query"],"stop_reason":"synthetic fixture"}),
            ),
            "discover" => (
                "candidates",
                json!({"queries":[],"items":[],"stop_reason":"synthetic no-source case"}),
            ),
            _ => {
                return ExecutionOutcome::Failed {
                    code: "fixture_unexpected".into(),
                    message: inv.state_id,
                };
            }
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
struct Fixture {
    _root: tempfile::TempDir,
    runtime: Arc<Runtime>,
    app: axum::Router,
}
impl Fixture {
    async fn new() -> Self {
        Self::with_timeout(None).await
    }
    async fn with_timeout(timeout: Option<u64>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
        let d = if let Some(seconds) = timeout {
            let resources = root.path().join("resources");
            std::fs::create_dir(&resources).unwrap();
            for entry in std::fs::read_dir(repository.join("resources")).unwrap() {
                let p = entry.unwrap().path();
                if p.is_file() {
                    std::fs::copy(&p, resources.join(p.file_name().unwrap())).unwrap();
                }
            }
            let bundle_path = resources.join("research.bundle.json");
            let mut bundle: Value =
                serde_json::from_slice(&std::fs::read(&bundle_path).unwrap()).unwrap();
            bundle["flow"]["states"]["await_sources"]["timeout_seconds"] = json!(seconds);
            std::fs::write(&bundle_path, serde_json::to_vec(&bundle).unwrap()).unwrap();
            Definition::load(
                bundle_path,
                &std::collections::BTreeMap::from([(
                    "inquiry-deep-research@0.3.7".into(),
                    repository.join("skills/inquiry-deep-research"),
                )]),
            )
            .unwrap()
        } else {
            domain::definition(repository).unwrap()
        };
        let digest = d.digest.clone();
        let bindings = domain::bindings(&d, Arc::new(Controlled));
        let runtime = Arc::new(
            Runtime::open(
                inquiry::budget::runtime_options(root.path().join("store")),
                vec![d],
                bindings,
            )
            .await
            .unwrap(),
        );
        let webdir = root.path().join("web");
        std::fs::create_dir(&webdir).unwrap();
        std::fs::write(webdir.join("index.html"), "<html>synthetic</html>").unwrap();
        let app = web::router(web::Host {
            runtime: runtime.clone(),
            definition_digest: digest,
            port: 3410,
            session: "synthetic-session".into(),
            web_dir: webdir,
        });
        Self {
            _root: root,
            runtime,
            app,
        }
    }
    async fn request(
        &self,
        method: &str,
        path: &str,
        value: Option<Value>,
        cookie: bool,
        origin: Option<&str>,
        host: &str,
    ) -> (StatusCode, Value) {
        let mut r = Request::builder()
            .method(method)
            .uri(path)
            .header("Host", host);
        if cookie {
            r = r.header("Cookie", "inquiry_session=synthetic-session");
        }
        if let Some(origin) = origin {
            r = r.header("Origin", origin);
        }
        let body = if let Some(v) = value {
            r = r.header("Content-Type", "application/json");
            v.to_string()
        } else {
            String::new()
        };
        let response = self
            .app
            .clone()
            .oneshot(r.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
    async fn post(&self, path: &str, value: Value) -> (StatusCode, Value) {
        self.request(
            "POST",
            path,
            Some(value),
            true,
            Some("http://127.0.0.1:3410"),
            "127.0.0.1:3410",
        )
        .await
    }
    async fn start(&self, key: &str, question: &str) -> String {
        let (s, v) = self
            .post(
                "/api/research",
                json!({"question":question,"request_key":key}),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        v["id"].as_str().unwrap().into()
    }
    async fn waiting(&self, id: &str) -> ExecutionSnapshot {
        for _ in 0..150 {
            let s = self.runtime.get(id, &Caller::local()).await.unwrap();
            if s.status == Status::Waiting {
                return s;
            }
            assert!(!s.status.terminal(), "unexpected: {s:?}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("wait timed out")
    }
}
#[tokio::test]
async fn host_origin_cookie_and_trusted_caller_are_enforced() {
    let f = Fixture::new().await;
    let r = Request::builder()
        .uri("/")
        .header("Host", "127.0.0.1:3410")
        .body(Body::empty())
        .unwrap();
    let response = f.app.clone().oneshot(r).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response.headers()["set-cookie"].to_str().unwrap();
    assert!(cookie.contains("HttpOnly; SameSite=Strict"));
    for (method, path, cookie, origin, host, expected) in [
        (
            "GET",
            "/api/research/unknown",
            false,
            None,
            "127.0.0.1:3410",
            StatusCode::UNAUTHORIZED,
        ),
        (
            "GET",
            "/api/health",
            true,
            None,
            "evil.example:3410",
            StatusCode::FORBIDDEN,
        ),
        (
            "POST",
            "/api/research",
            true,
            None,
            "127.0.0.1:3410",
            StatusCode::FORBIDDEN,
        ),
        (
            "POST",
            "/api/research",
            true,
            Some("https://evil.example"),
            "127.0.0.1:3410",
            StatusCode::FORBIDDEN,
        ),
    ] {
        assert_eq!(
            f.request(method, path, Some(json!({})), cookie, origin, host)
                .await
                .0,
            expected
        );
    }
    let (s, _) = f
        .post(
            "/api/research",
            json!({"question":"hold","request_key":"spoof","caller":{"principal":"admin"}}),
        )
        .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (s, _) = f
        .post(
            "/api/research",
            json!({"question":" ","request_key":"empty"}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    f.runtime.shutdown().await.unwrap();
}
#[tokio::test]
async fn idempotent_start_signal_conflict_and_cross_research_artifact_paths() {
    let f = Fixture::new().await;
    let a = f.start("one", "synthetic A").await;
    let b = f.start("two", "synthetic B").await;
    assert_ne!(a, b);
    assert_eq!(a, f.start("one", "synthetic A").await);
    let (s, _) = f
        .post(
            "/api/research",
            json!({"question":"changed","request_key":"one"}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let sa = f.waiting(&a).await;
    let sb = f.waiting(&b).await;
    assert_eq!(sa.inputs["question"], "synthetic A");
    assert_eq!(sb.inputs["question"], "synthetic B");
    let aid = &sa.accepted_artifacts[0].id;
    let path = format!("/api/research/{b}/artifacts/{aid}");
    assert_eq!(
        f.request("GET", &path, None, true, None, "127.0.0.1:3410")
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let path = format!("/api/research/{a}/artifacts/{aid}");
    let (status, data) = f
        .request("GET", &path, None, true, None, "127.0.0.1:3410")
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(data["value"]["subquestions"][0], "synthetic A");
    let path = format!("/api/research/{a}/sources");
    let req = json!({"urls":["https://example.com/"],"request_key":"source","expected_revision":sa.revision-1});
    assert_eq!(f.post(&path, req).await.0, StatusCode::CONFLICT);
    let req = json!({"urls":["https://example.com/"],"request_key":"source-valid","expected_revision":sa.revision});
    let (status, first) = f.post(&path, req.clone()).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let (status, retry) = f.post(&path, req).await;
    assert_eq!(status, StatusCode::OK, "{retry}");
    assert_eq!(first, retry);
    f.runtime.shutdown().await.unwrap();
}
#[tokio::test]
async fn stopping_one_running_research_preserves_other_and_accepted_content() {
    let f = Fixture::new().await;
    let a = f.start("hold-a", "hold").await;
    let b = f.start("finish-b", "synthetic B").await;
    let (status, response) = f
        .post(
            &format!("/api/research/{a}/cancel"),
            json!({"request_key":"stop"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    for _ in 0..150 {
        if f.runtime.get(&a, &Caller::local()).await.unwrap().status == Status::Cancelled {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        f.runtime.get(&a, &Caller::local()).await.unwrap().status,
        Status::Cancelled
    );
    let sb = f.waiting(&b).await;
    assert!(!sb.accepted_artifacts.is_empty());
    let (s, view) = f
        .request(
            "GET",
            &format!("/api/research/{b}"),
            None,
            true,
            None,
            "127.0.0.1:3410",
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(view["snapshot"]["status"], "waiting");
    assert!(view["outputs"].as_array().unwrap().len() >= 2);
    f.runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn wait_expiry_has_partial_outcome_and_preserves_plan() {
    let f = Fixture::with_timeout(Some(1)).await;
    let id = f.start("expiry", "synthetic expiry").await;
    f.waiting(&id).await;
    let mut done = false;
    for _ in 0..200 {
        let s = f.runtime.get(&id, &Caller::local()).await.unwrap();
        if s.status.terminal() {
            assert_eq!(s.status, Status::Completed);
            assert_eq!(s.business_outcome.as_deref(), Some("insufficient_evidence"));
            assert!(!s.accepted_artifacts.is_empty());
            done = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(done, "shortened fixture timeout was not observed");
    f.runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn diagnostics_preserve_session_scope_query_errors_and_uncollected_history() {
    let f = Fixture::new().await;
    let id = f.start("diagnostics", "synthetic diagnostics").await;
    f.waiting(&id).await;
    for suffix in ["trace?limit=20", "trace/export?limit=20", "budgets"] {
        let path = format!("/api/research/{id}/{suffix}");
        let (status, _) = f
            .request("GET", &path, None, false, None, "127.0.0.1:3410")
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, value) = f
            .request("GET", &path, None, true, None, "127.0.0.1:3410")
            .await;
        assert_eq!(status, StatusCode::OK, "{value}");
        if suffix == "budgets" {
            assert_eq!(value["runs"].as_array().unwrap().len(), 2);
            assert!(
                value["runs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(
                        |r| r["policy"]["submission_reserve"] == 2 && r["policy_source"] == "entry"
                    )
            );
        } else {
            assert_eq!(value["enabled"], false);
            assert_eq!(value["complete"], false);
            assert_eq!(value["issues"], json!(["disabled"]));
        }
        let (status, _) = f
            .request(
                "GET",
                &format!("/api/research/missing/{suffix}"),
                None,
                true,
                None,
                "127.0.0.1:3410",
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
    for suffix in [
        "trace",
        "trace?limit=0",
        "trace?limit=1001",
        "trace?limit=10&after=-1",
        "trace?limit=10&unknown=true",
        "trace/export?limit=0",
    ] {
        let (status, value) = f
            .request(
                "GET",
                &format!("/api/research/{id}/{suffix}"),
                None,
                true,
                None,
                "127.0.0.1:3410",
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{value}");
        assert_eq!(value["code"], "input_invalid");
    }
    f.runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn new_goal_host_reads_persisted_legacy_flow_but_refuses_writes() {
    let f = Fixture::new().await;
    let id = f.start("legacy-before-upgrade", "synthetic legacy").await;
    let saved = f.waiting(&id).await;
    f.runtime.shutdown().await.unwrap();
    let Fixture {
        _root: root,
        runtime,
        app,
    } = f;
    drop(app);
    drop(runtime);
    let bindings =
        inquiry::orchestration::bindings(Arc::new(Controlled), "migration-fixture".into());
    let (registry, options) =
        inquiry::orchestration::registry(Path::new(env!("CARGO_MANIFEST_DIR")), &bindings).unwrap();
    let runtime = Arc::new(
        Runtime::open_orchestrated(
            inquiry::budget::runtime_options(root.path().join("store")),
            vec![],
            bindings,
            registry,
            options,
            None,
        )
        .await
        .unwrap(),
    );
    let app = web::router(web::Host {
        runtime: runtime.clone(),
        definition_digest: String::new(),
        port: 3410,
        session: "synthetic-session".into(),
        web_dir: root.path().join("web"),
    });
    let upgraded = Fixture {
        _root: root,
        runtime,
        app,
    };
    let (status, view) = upgraded
        .request(
            "GET",
            &format!("/api/research/{id}"),
            None,
            true,
            None,
            "127.0.0.1:3410",
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["read_only"], true);
    assert_eq!(view["snapshot"]["inputs"]["question"], "synthetic legacy");
    assert_eq!(view["snapshot"]["revision"], saved.revision);
    let artifact = &saved.accepted_artifacts[0];
    let (status, data) = upgraded
        .request(
            "GET",
            &format!("/api/research/{id}/artifacts/{}", artifact.id),
            None,
            true,
            None,
            "127.0.0.1:3410",
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{data}");
    assert_eq!(data["artifact"]["sha256"], artifact.sha256);
    assert_eq!(data["value"]["subquestions"][0], "synthetic legacy");
    for (path, body) in [
        (
            "cancel",
            json!({"request_key":"legacy-stop","expected_revision":saved.revision}),
        ),
        (
            "sources",
            json!({"urls":["https://example.com/"],"request_key":"legacy-signal","expected_revision":saved.revision}),
        ),
    ] {
        let (status, error) = upgraded
            .post(&format!("/api/research/{id}/{path}"), body)
            .await;
        assert_eq!(status, StatusCode::CONFLICT, "{error}");
        assert_eq!(error["code"], "read_only");
    }
    upgraded.runtime.shutdown().await.unwrap();
}
