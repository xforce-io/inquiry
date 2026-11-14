//! Real native CLI + actual Milkie + production Inquiry host, but a loopback 401 provider.
//! This proves configured reasoning effort and failure/trace wiring, never a real model success.
use axum::{Json, Router, body::Bytes, extract::State, http::StatusCode};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
#[derive(Clone)]
struct Provider {
    effort: Arc<Mutex<Option<String>>>,
    calls: Arc<AtomicUsize>,
    hang: Arc<AtomicBool>,
    release: CancellationToken,
}
async fn reject(State(p): State<Provider>, body: Bytes) -> (StatusCode, Json<Value>) {
    p.calls.fetch_add(1, Ordering::SeqCst);
    let value: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    *p.effort.lock().unwrap() = value["reasoning"]["effort"].as_str().map(str::to_owned);
    if p.hang.load(Ordering::SeqCst) {
        p.release.cancelled().await;
    }
    (
        StatusCode::UNAUTHORIZED,
        Json(
            json!({"error":{"message":"Local fixture rejection; no real credentials","type":"authentication_error","code":"invalid_api_key"}}),
        ),
    )
}
async fn request(
    port: u16,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    body: Option<Value>,
) -> (u16, String, Value) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut stream=tokio::net::TcpStream::connect(("127.0.0.1",port)).await.unwrap();
        let body=body.map(|v|v.to_string()).unwrap_or_default();
        let mut head=format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: {}\r\nContent-Type: application/json\r\n",body.len());
        if let Some(cookie)=cookie {head.push_str(&format!("Cookie: {cookie}\r\n"));}
        if method!="GET" {head.push_str(&format!("Origin: http://127.0.0.1:{port}\r\n"));}
        head.push_str("\r\n");head.push_str(&body);stream.write_all(head.as_bytes()).await.unwrap();
        let mut bytes=Vec::new();stream.take(2*1024*1024).read_to_end(&mut bytes).await.unwrap();
        let text=String::from_utf8(bytes).unwrap();let (headers,body)=text.split_once("\r\n\r\n").unwrap();
        let status=headers.split_whitespace().nth(1).unwrap().parse().unwrap();
        (status,headers.to_owned(),serde_json::from_str(body).unwrap_or(Value::Null))
    }).await.expect("local Inquiry HTTP timeout")
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires explicit MILKIE_NODE/MILKIE_CLI/INQUIRY_NATIVE_CODEX_DIR; offline native transport, not real model acceptance"]
async fn native_high_effort_failure_and_cancel_preserve_trace_and_stop() {
    let node = PathBuf::from(std::env::var_os("MILKIE_NODE").expect("MILKIE_NODE"));
    let cli = PathBuf::from(std::env::var_os("MILKIE_CLI").expect("MILKIE_CLI"));
    let codex_dir = PathBuf::from(
        std::env::var_os("INQUIRY_NATIVE_CODEX_DIR").expect("INQUIRY_NATIVE_CODEX_DIR"),
    );
    assert!(node.is_absolute() && cli.is_absolute() && codex_dir.is_absolute());
    let root = tempfile::tempdir().unwrap();
    for name in ["config", "sessions", "data"] {
        std::fs::create_dir(root.path().join(name)).unwrap();
    }
    let provider = Provider {
        effort: Arc::new(Mutex::new(None)),
        calls: Arc::new(AtomicUsize::new(0)),
        hang: Arc::new(AtomicBool::new(false)),
        release: CancellationToken::new(),
    };
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let provider_port = listener.local_addr().unwrap().port();
    let stop = CancellationToken::new();
    let stopper = stop.clone();
    let server = tokio::spawn(
        axum::serve(
            listener,
            Router::new().fallback(reject).with_state(provider.clone()),
        )
        .with_graceful_shutdown(async move {
            stopper.cancelled().await;
        })
        .into_future(),
    );
    std::fs::write(
        root.path().join("config/auth.json"),
        br#"{"OPENAI_API_KEY":"fixture-not-real"}"#,
    )
    .unwrap();
    std::fs::write(root.path().join("config/config.toml"),format!("model_reasoning_effort = \"medium\"\nmodel_provider = \"fixture\"\n[model_providers.fixture]\nname = \"fixture\"\nbase_url = \"http://127.0.0.1:{provider_port}/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nrequest_max_retries = 0\nstream_max_retries = 0\n")).unwrap();
    std::fs::write(root.path().join("connection.json"),br#"{"contractVersion":1,"fields":{"transport":"agent-cli","runtime":"codex","model":"gpt-6.1-sol"}}"#).unwrap();
    let native_search = std::env::var("INQUIRY_TEST_NATIVE_SEARCH").as_deref() == Ok("1");
    let config = json!({"native_search":native_search,"node":node,"milkie_cli":cli,"connection_file":root.path().join("connection.json"),"config_dir":root.path().join("config"),"session_dir":root.path().join("sessions"),"codex_binary":codex_dir.join("codex"),"trace":{"policy_id":"native-offline-v1","max_events_per_run":2000,"max_bytes_per_run":1048576,"source_max_events":1000,"source_max_bytes":262144,"max_total_events":10000,"max_total_bytes":4194304,"max_runs":16,"retention_ms":60000}});
    std::fs::write(
        root.path().join("execution.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let reservation = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);
    let path = std::env::join_paths(
        [codex_dir, node.parent().unwrap().to_owned()]
            .into_iter()
            .chain(std::env::split_paths(
                &std::env::var_os("PATH").unwrap_or_default(),
            )),
    )
    .unwrap();
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_inquiry"));
    command
        .env(
            "INQUIRY_EXECUTION_CONFIG",
            root.path().join("execution.json"),
        )
        .env("INQUIRY_DATA_DIR", root.path().join("data"))
        .env("INQUIRY_PORT", port.to_string())
        .env("PATH", path)
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("no_proxy", "127.0.0.1,localhost")
        .kill_on_drop(true);
    command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut child = command.spawn().unwrap();
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            break;
        }
        assert!(
            child.try_wait().unwrap().is_none(),
            "Inquiry exited before readiness"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let (_, headers, _) = request(port, "GET", "/", None, None).await;
    let cookie = headers
        .lines()
        .find_map(|line| {
            line.strip_prefix("set-cookie: ")
                .or_else(|| line.strip_prefix("Set-Cookie: "))
        })
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let (_, _, health) = request(port, "GET", "/api/health", None, None).await;
    assert_eq!(
        health["search_provider"],
        if native_search {
            "codex-native"
        } else {
            "duckduckgo-html"
        }
    );
    let (status,_,created)=request(port,"POST","/api/research",Some(cookie),Some(json!({"question":"Offline transport rejection fixture, not a research request","request_key":"native-offline"}))).await;
    assert_eq!(status, 200, "{created}");
    let id = created["id"].as_str().unwrap();
    let mut final_view = None;
    for _ in 0..300 {
        let (status, _, view) = request(
            port,
            "GET",
            &format!("/api/research/{id}"),
            Some(cookie),
            None,
        )
        .await;
        assert_eq!(status, 200);
        if ["failed", "blocked", "cancelled", "completed"]
            .contains(&view["snapshot"]["status"].as_str().unwrap())
        {
            final_view = Some(view);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let view = final_view.expect("native rejection did not settle");
    assert_eq!(view["snapshot"]["status"], "failed");
    assert_ne!(view["snapshot"]["business_outcome"], "report_ready");
    assert!(provider.calls.load(Ordering::SeqCst) > 0);
    assert_eq!(provider.effort.lock().unwrap().as_deref(), Some("high"));
    let (status, _, trace) = request(
        port,
        "GET",
        &format!("/api/research/{id}/trace?limit=1000"),
        Some(cookie),
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(trace["enabled"], true);
    assert_eq!(trace["policy"]["content_mode"], "metadata_only");
    let events = trace["events"].as_array().unwrap();
    assert!(
        events
            .iter()
            .any(|e| e["event"]["kind"] == "executor.finished")
    );
    assert!(
        events
            .iter()
            .any(|e| e["event"]["milkie_run_id"].is_string())
    );
    assert!(!trace.to_string().contains("fixture-not-real"));
    assert!(!root.path().join("data/research-audit.ndjson").exists());
    // A second owned native request stays in flight until the actual cancellation path stops it.
    provider.hang.store(true, Ordering::SeqCst);
    let before = provider.calls.load(Ordering::SeqCst);
    let (status, _, held) = request(port, "POST", "/api/research", Some(cookie), Some(json!({"question":"Offline held request cancellation fixture","request_key":"native-held"}))).await;
    assert_eq!(status, 200);
    let held = held["id"].as_str().unwrap();
    for _ in 0..200 {
        if provider.calls.load(Ordering::SeqCst) > before {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(
        provider.calls.load(Ordering::SeqCst) > before,
        "native request never entered the held provider"
    );
    let (_, _, held_view) = request(
        port,
        "GET",
        &format!("/api/research/{held}"),
        Some(cookie),
        None,
    )
    .await;
    let (status, _, _) = request(
        port,
        "POST",
        &format!("/api/research/{held}/cancel"),
        Some(cookie),
        Some(json!({"request_key":"cancel-native-held","expected_revision":held_view["snapshot"]["revision"]})),
    )
    .await;
    assert_eq!(status, 200);
    let mut cancelled = false;
    for _ in 0..200 {
        let (_, _, view) = request(
            port,
            "GET",
            &format!("/api/research/{held}"),
            Some(cookie),
            None,
        )
        .await;
        if view["snapshot"]["status"] == "cancelled" {
            assert!(
                view["snapshot"]["accepted_artifacts"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
            cancelled = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(cancelled, "native stop was not confirmed");
    let (_, _, first) = request(
        port,
        "GET",
        &format!("/api/research/{id}"),
        Some(cookie),
        None,
    )
    .await;
    assert_eq!(
        first["snapshot"]["status"], "failed",
        "cancelling second execution changed the first"
    );
    provider.release.cancel();
    let pid = child.id().unwrap();
    assert!(
        std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        tokio::time::timeout(Duration::from_secs(20), child.wait())
            .await
            .expect("shutdown did not settle")
            .unwrap()
            .success()
    );
    stop.cancel();
    server.await.unwrap().unwrap();
    println!(
        "{}",
        json!({"native_transport":"pass","native_search":native_search,"provider":"local 401 fixture","reasoning_effort":"high","real_model_success":false,"credentials":"fixture only","metadata_trace":true,"graceful_shutdown":true,"native_cancel_confirmed":true})
    );
}
