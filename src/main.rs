use std::path::PathBuf;

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    inquiry::net::dns_mode().map_err(|e| e.reason)?;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let data = std::env::var_os("INQUIRY_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("data"));
    let config = std::env::var_os("INQUIRY_EXECUTION_CONFIG")
        .map(PathBuf::from)
        .ok_or("INQUIRY_EXECUTION_CONFIG must name an explicit absolute configuration file")?;
    let setup = inquiry::execution::ExecutionSetup::load(&config, &data)?;
    let port = std::env::var("INQUIRY_PORT")
        .unwrap_or_else(|_| "3410".into())
        .parse::<u16>()?;
    if port == 0 {
        return Err("INQUIRY_PORT must be nonzero".into());
    }
    // Bind before opening the runtime: a port conflict must not resume queued research.
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    let (runtime, digest) = setup.open(&root, &data).await?;
    let app = inquiry::web::router(inquiry::web::Host {
        runtime: runtime.clone(),
        definition_digest: digest,
        port,
        session: uuid::Uuid::new_v4().to_string(),
        web_dir: root.join("web/dist"),
    });
    println!(
        "Inquiry: http://127.0.0.1:{port} · build {}",
        env!("INQUIRY_BUILD_SHA")
    );
    let serving = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await;
    // Always wait for native stop, including when the HTTP server itself returns an error.
    let stopped = runtime.shutdown().await;
    serving?;
    stopped?;
    Ok(())
}
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
