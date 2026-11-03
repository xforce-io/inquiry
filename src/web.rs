//! A thin, local, single-user HTTP host. It never reads TaskLoop private tables.
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderValue, Request, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::PathBuf, sync::Arc};
use taskloop::*;
use tower_http::services::{ServeDir, ServeFile};

#[derive(Clone)]
pub struct Host {
    pub runtime: Arc<Runtime>,
    pub definition_digest: String,
    pub port: u16,
    pub session: String,
    pub web_dir: PathBuf,
}
pub fn router(host: Host) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .route("/research", post(start))
        .route("/research/{id}", get(research))
        .route("/research/{id}/sources", post(sources))
        .route("/research/{id}/cancel", post(cancel))
        .route("/research/{id}/artifacts/{artifact}", get(artifact))
        .fallback(|| async {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"code":"not_found","message":"接口不存在"})),
            )
        });
    let web = ServeDir::new(&host.web_dir)
        .not_found_service(ServeFile::new(host.web_dir.join("index.html")));
    Router::new()
        .nest("/api", api)
        .fallback_service(web)
        .layer(DefaultBodyLimit::max(128 * 1024))
        .layer(middleware::from_fn_with_state(host.clone(), guard))
        .with_state(host)
}
fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({"code":code,"message":message}))).into_response()
}
async fn guard(State(host): State<Host>, request: Request<Body>, next: Next) -> Response {
    let domain = request
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ![
        format!("127.0.0.1:{}", host.port),
        format!("localhost:{}", host.port),
    ]
    .contains(&domain)
    {
        return error(StatusCode::FORBIDDEN, "host_denied", "请求主机不允许");
    }
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|h| h.to_str().ok());
    let expected = format!("http://{domain}");
    if origin.is_some_and(|o| o != expected)
        || (!matches!(
            *request.method(),
            axum::http::Method::GET | axum::http::Method::HEAD
        ) && origin != Some(expected.as_str()))
    {
        return error(
            StatusCode::FORBIDDEN,
            "origin_denied",
            "写操作必须来自当前页面",
        );
    }
    let valid_session = request
        .headers()
        .get(header::COOKIE)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|cookies| {
            cookies
                .split(';')
                .any(|v| v.trim() == format!("inquiry_session={}", host.session))
        });
    let api = request.uri().path().starts_with("/api/");
    if api && request.uri().path() != "/api/health" && !valid_session {
        return error(
            StatusCode::UNAUTHORIZED,
            "session_required",
            "本地会话已更新，请刷新页面",
        );
    }
    let mut response = next.run(request).await;
    if !api && !valid_session {
        response.headers_mut().insert(
            header::SET_COOKIE,
            HeaderValue::from_str(&format!(
                "inquiry_session={}; HttpOnly; SameSite=Strict; Path=/",
                host.session
            ))
            .expect("generated session cookie"),
        );
    }
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_SECURITY_POLICY,HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    if api {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    response
}
struct ApiError(Error);
impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        Self(e)
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self.0.code.as_str() {
            "not_found" => StatusCode::NOT_FOUND,
            "revision_conflict" | "request_conflict" => StatusCode::CONFLICT,
            "capability_denied" => StatusCode::FORBIDDEN,
            "input_invalid" | "research_invalid" | "signal_invalid" => StatusCode::BAD_REQUEST,
            _ => StatusCode::SERVICE_UNAVAILABLE,
        };
        (status, Json(self.0)).into_response()
    }
}
type ApiResult = std::result::Result<Json<Value>, ApiError>;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Start {
    question: String,
    #[serde(default)]
    seed_urls: Vec<String>,
    #[serde(default)]
    scope: String,
    request_key: String,
}
async fn start(State(host): State<Host>, Json(req): Json<Start>) -> ApiResult {
    if req.question.trim().is_empty() {
        return Err(Error::new("input_invalid", "/question", "请填写研究问题").into());
    }
    let mut urls = Vec::new();
    for raw in req.seed_urls {
        let url = crate::net::normalize_url(raw.trim())
            .map_err(|e| Error::new("input_invalid", "/seed_urls", e.reason))?
            .to_string();
        if !urls.contains(&url) {
            urls.push(url);
        }
    }
    let scope = if req.scope.trim().is_empty() {
        "研究公开资料，使用中文，保留限制与未知".to_string()
    } else {
        req.scope
    };
    let id = host
        .runtime
        .start(
            StartRequest {
                definition_digest: host.definition_digest.clone(),
                inputs: json!({"question":req.question,"seed_urls":urls,"scope":scope}),
                grant_ref: "local-readonly".into(),
                request_key: req.request_key,
                limits: crate::domain::limits(),
            },
            &Caller::local(),
        )
        .await?;
    Ok(Json(json!({"id":id})))
}
async fn health() -> Json<Value> {
    Json(
        json!({"ready":true,"build":env!("INQUIRY_BUILD_SHA"),"taskloop_revision":"1c69bcf8a1bc0a7d9a4564bbe93f8b13d7317d04","method_version":"0.3.4"}),
    )
}
async fn research(State(host): State<Host>, Path(id): Path<String>) -> ApiResult {
    let snapshot = host.runtime.get(&id, &Caller::local()).await?;
    let mut outputs = Vec::new();
    for reference in snapshot.accepted_artifacts.iter().filter(|a| {
        matches!(
            a.slot.as_str(),
            "plan" | "candidates" | "materials" | "catalogue" | "analysis" | "report"
        )
    }) {
        let artifact = host
            .runtime
            .read_artifact(&reference.id, &Caller::local())
            .await?;
        let v = parse_unique(&artifact.bytes)?;
        outputs.push(json!({"artifact":reference,"value":v}));
    }
    Ok(Json(json!({"snapshot":snapshot,"outputs":outputs})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Sources {
    urls: Vec<String>,
    request_key: String,
    expected_revision: u64,
}
async fn sources(
    State(host): State<Host>,
    Path(id): Path<String>,
    Json(req): Json<Sources>,
) -> ApiResult {
    let mut urls = BTreeSet::new();
    for raw in req.urls {
        urls.insert(
            crate::net::normalize_url(raw.trim())
                .map_err(|e| Error::new("input_invalid", "/urls", e.reason))?
                .to_string(),
        );
    }
    if urls.is_empty() || urls.len() > 20 {
        return Err(Error::new("input_invalid", "/urls", "请提供1–20个公开网址").into());
    }
    let receipt = host
        .runtime
        .signal(
            &id,
            SignalRequest {
                event_name: "sources.added".into(),
                payload: json!({"urls":urls,"reason":"用户在研究等待时补充公开资料"}),
                request_key: req.request_key,
                expected_revision: req.expected_revision,
            },
            &Caller::local(),
        )
        .await?;
    Ok(Json(serde_json::to_value(receipt).map_err(Error::from)?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cancel {
    request_key: String,
}
async fn cancel(
    State(host): State<Host>,
    Path(id): Path<String>,
    Json(req): Json<Cancel>,
) -> ApiResult {
    let snapshot = host
        .runtime
        .cancel(&id, &req.request_key, &Caller::local())
        .await?;
    Ok(Json(json!({"snapshot":snapshot})))
}
async fn artifact(
    State(host): State<Host>,
    Path((id, artifact)): Path<(String, String)>,
) -> ApiResult {
    let snapshot = host.runtime.get(&id, &Caller::local()).await?;
    let reference = snapshot
        .accepted_artifacts
        .iter()
        .find(|a| a.id == artifact)
        .ok_or_else(|| Error::new("not_found", "/artifact", "此研究没有该资料"))?;
    let data = host
        .runtime
        .read_artifact(&artifact, &Caller::local())
        .await?;
    let value = parse_unique(&data.bytes).ok();
    let text = value
        .is_none()
        .then(|| String::from_utf8_lossy(&data.bytes).into_owned());
    Ok(Json(
        json!({"artifact":reference,"value":value,"text":text}),
    ))
}
