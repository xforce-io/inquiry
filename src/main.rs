use async_trait::async_trait;
use serde_json::{Value, json};
use std::{
    io::Write,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use taskloop::*;
use tokio_util::sync::CancellationToken;
struct ObservedBridge {
    inner: Arc<dyn ToolBridge>,
    called: Arc<AtomicBool>,
    log: Arc<Mutex<std::fs::File>>,
    invocation: Value,
}
#[async_trait]
impl ToolBridge for ObservedBridge {
    fn specs(&self) -> Vec<ToolSpec> {
        self.inner.specs()
    }
    async fn call(&self, id: &str, args: Value) -> Result<Value> {
        self.called.store(true, Ordering::SeqCst);
        let clock = Instant::now();
        let arguments = if id == "artifact.write" {
            json!({"slot":args["slot"],"value_sha256":inquiry::domain::hash(args["value"].to_string().as_bytes())})
        } else {
            args.clone()
        };
        let result = self.inner.call(id, args).await;
        let outcome = match &result {
            Ok(v) if id == "artifact.read" => {
                json!({"artifact":v["artifact"],"id":v["id"],"sha256":v["sha256"],"text_bytes":v["text"].as_str().map(str::len),"_taskloop_budget":v["_taskloop_budget"]})
            }
            Ok(v) => json!({"value":v}),
            Err(e) => json!({"error":e}),
        };
        write_audit(
            &self.log,
            &json!({"kind":"host_tool_call","invocation":self.invocation,"tool":id,"arguments":arguments,"elapsed_seconds":clock.elapsed().as_secs_f64(),"outcome":outcome}),
        );
        result
    }
}
struct Audited {
    inner: CodexExecutor,
    log: Arc<Mutex<std::fs::File>>,
}
#[async_trait]
impl Executor for Audited {
    fn capabilities(&self) -> ExecutorCapabilities {
        self.inner.capabilities()
    }
    fn identity(&self) -> Value {
        json!({"host_adapter":"inquiry-audit@2","budget_policy":"analysis80-other40-reserve2@1","inner":self.inner.identity()})
    }
    async fn execute(
        &self,
        inv: Invocation,
        tools: Arc<dyn ToolBridge>,
        cancel: CancellationToken,
    ) -> ExecutionOutcome {
        let record = json!({"execution_id":inv.execution_id,"run_id":inv.run_id,"state":inv.state_id,"visit":inv.visit});
        let clock = Instant::now();
        let called = Arc::new(AtomicBool::new(false));
        let tools = Arc::new(ObservedBridge {
            inner: tools,
            called: called.clone(),
            log: self.log.clone(),
            invocation: record.clone(),
        });
        let result = self.inner.execute(inv, tools, cancel).await;
        let usage = match &result {
            ExecutionOutcome::Completed { usage } => usage.clone(),
            _ => Value::Null,
        };
        let known = called.load(Ordering::SeqCst) || usage["model"].is_string();
        let event = json!({"kind":"native_research_attempt","invocation":record,"elapsed_seconds":clock.elapsed().as_secs_f64(),"model_calls":if known{json!(1)}else{Value::Null},"usage":usage,"outcome":result});
        write_audit(&self.log, &event);
        result
    }
    async fn stop_or_confirm(&self, id: &str) -> StopState {
        self.inner.stop_or_confirm(id).await
    }
}
fn write_audit(log: &Mutex<std::fs::File>, event: &Value) {
    match log.lock() {
        Ok(mut file) => {
            if let Err(e) = writeln!(file, "{event}") {
                eprintln!("research audit unavailable: {e}");
            }
        }
        Err(_) => eprintln!("research audit unavailable: log lock poisoned"),
    }
}
#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let data = std::env::var_os("INQUIRY_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("data"));
    std::fs::create_dir_all(&data)?;
    use std::os::unix::fs::OpenOptionsExt;
    let log = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(data.join("research-audit.ndjson"))?;
    let executor = Arc::new(Audited {
        inner: CodexExecutor::new("gpt-6-astra")?,
        log: Arc::new(Mutex::new(log)),
    });
    let definition = inquiry::domain::definition(&root)?;
    let digest = definition.digest.clone();
    let bindings = inquiry::domain::bindings(&definition, executor);
    let runtime = Arc::new(
        Runtime::open(
            inquiry::budget::runtime_options(data.join("taskloop")),
            vec![definition],
            bindings,
        )
        .await?,
    );
    let port = std::env::var("INQUIRY_PORT")
        .unwrap_or_else(|_| "3410".into())
        .parse::<u16>()?;
    if port == 0 {
        return Err("INQUIRY_PORT must be nonzero".into());
    }
    let host = inquiry::web::Host {
        runtime: runtime.clone(),
        definition_digest: digest,
        port,
        session: uuid::Uuid::new_v4().to_string(),
        web_dir: root.join("web/dist"),
    };
    let app = inquiry::web::router(host);
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    println!(
        "Inquiry: http://127.0.0.1:{port} · build {}",
        env!("INQUIRY_BUILD_SHA")
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    runtime.shutdown().await?;
    Ok(())
}
