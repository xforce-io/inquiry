//! A thin, local, single-user HTTP host. It never reads TaskLoop private tables.
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, Query, State, rejection::QueryRejection},
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
        .route("/research/{id}/trace", get(trace))
        .route("/research/{id}/trace/export", get(export_trace))
        .route("/research/{id}/budgets", get(budgets))
        .route("/research/{id}/sources", post(sources))
        .route("/research/{id}/cancel", post(cancel))
        .route("/research/{id}/resume", post(resume))
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
            "revision_conflict" | "request_conflict" | "not_resumable" | "read_only" => {
                StatusCode::CONFLICT
            }
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
    #[serde(default)]
    parent_goal_id: Option<String>,
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
    if let Ok(goals) = host.runtime.goals() {
        let receipt = goals
            .submit(
                GoalRequest {
                    protocol_version: 1,
                    request_key: req.request_key,
                    grant_ref: "local-readonly".into(),
                    spec: GoalSpec {
                        goal: req.question.clone(),
                        inputs: json!({"question":req.question,"seed_urls":urls,"scope":scope}),
                        profile_ref: crate::orchestration::PROFILE.into(),
                        planning: GoalPlanning {
                            mode: PlanningMode::Workflow,
                            max_revisions: 2,
                            max_planning_ms: 540_000,
                            template_ref: Some(crate::orchestration::TEMPLATE.into()),
                        },
                        limits: crate::orchestration::limits(),
                        parent_goal_id: req.parent_goal_id,
                        imports: vec![],
                    },
                },
                &Caller::local(),
            )
            .await?;
        return Ok(Json(json!({"id":receipt.goal_id})));
    }
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
        json!({"ready":true,"build":env!("INQUIRY_BUILD_SHA"),"taskloop_revision":crate::TASKLOOP_REVISION,"method_version":crate::ARTICLE_METHOD_VERSION,"search_provider":crate::search::PROVIDER,"dns_mode":crate::net::dns_mode().ok()}),
    )
}
async fn goal_snapshot(host: &Host, id: &str) -> Result<Option<GoalSnapshot>> {
    let Ok(goals) = host.runtime.goals() else {
        return Ok(None);
    };
    match goals.get(id, &Caller::local()).await {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.code == "not_found" => Ok(None),
        Err(e) => Err(e),
    }
}
async fn research(State(host): State<Host>, Path(id): Path<String>) -> ApiResult {
    if let Some(goal) = goal_snapshot(&host, &id).await? {
        let goals = host.runtime.goals().expect("Goal runtime");
        let mut outputs = Vec::new();
        let mut refs = Vec::new();
        for artifact in goal
            .artifacts
            .iter()
            .filter(|a| a.accepted && a.producer_kind == "node_projection")
        {
            refs.push(artifact.artifact.clone());
            if artifact.artifact.slot == "archive" {
                continue;
            }
            let data = goals
                .read_artifact(&artifact.artifact.id, &Caller::local())
                .await?;
            outputs.push(json!({"artifact":artifact.artifact,"value":parse_unique(&data.bytes)?,"stage":artifact.node_id}));
        }
        let status = match goal.status {
            GoalStatus::Running => "running",
            GoalStatus::Waiting => "waiting",
            GoalStatus::Blocked => "blocked",
            GoalStatus::Stopping => "cancelling",
            GoalStatus::Stopped => "stopped",
            GoalStatus::Finished => match goal.outcome {
                Some(GoalOutcome::Achieved) => "completed",
                Some(GoalOutcome::NotMet) => "not_met",
                Some(GoalOutcome::Cancelled) => "cancelled",
                _ => "failed",
            },
        };
        let active_stage = [
            "scope",
            "collect",
            "analyze",
            "supplement",
            "synthesize",
            "draft",
            "review",
            "revise",
        ]
        .into_iter()
        .find(|name| {
            goal.nodes.get(*name).is_some_and(|n| {
                matches!(
                    n.status,
                    GoalNodeStatus::Ready | GoalNodeStatus::Running | GoalNodeStatus::Checking
                )
            })
        })
        .unwrap_or(if goal.waiting.is_empty() {
            "final_evaluation"
        } else {
            "await_clarification"
        });
        let stage = match goal.stage {
            taskloop::GoalStage::Planning | taskloop::GoalStage::PlanChecking => {
                if goal.status == GoalStatus::Waiting {
                    "await_clarification"
                } else {
                    "planning"
                }
            }
            taskloop::GoalStage::GoalChecking => "final_evaluation",
            taskloop::GoalStage::Executing => active_stage,
        };
        let evaluations = goals.evaluations(&id, &Caller::local()).await?;
        let snapshot = json!({"execution_id":goal.goal_id,"inputs":goal.spec.inputs,"status":status,"current_state":stage,"visit":1,"revision":goal.revision,"business_outcome":goal.outcome,"progress":{"phase":goal.stage},"accepted_artifacts":refs,"error":goal.error});
        return Ok(Json(
            json!({"engine":"goal","snapshot":snapshot,"goal":goal,"outputs":outputs,"evaluations":evaluations}),
        ));
    }
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
    Ok(Json(
        json!({"snapshot":snapshot,"outputs":outputs,"read_only":host.runtime.goals().is_ok()}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Sources {
    #[serde(default)]
    urls: Vec<String>,
    #[serde(default)]
    clarification: String,
    #[serde(default)]
    wait_id: Option<String>,
    #[serde(default)]
    nonce: Option<String>,
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
    if let Some(goal) = goal_snapshot(&host, &id).await? {
        if urls.len() > 20
            || req.clarification.chars().count() > 4000
            || urls.is_empty() && req.clarification.trim().is_empty()
        {
            return Err(
                Error::new("input_invalid", "/sources", "请填写澄清内容或提供公开网址").into(),
            );
        }
        let (wait_id, nonce) = match (req.wait_id, req.nonce) {
            (Some(id), Some(nonce)) => (id, nonce),
            (None, None) => {
                let waiting = goal.waiting.iter().find(|w| !w.consumed).ok_or_else(|| {
                    Error::new(
                        "signal_invalid",
                        "/sources",
                        "研究当前不等待补充；可显式新建后续研究",
                    )
                })?;
                (waiting.wait_id.clone(), waiting.nonce.clone())
            }
            _ => {
                return Err(
                    Error::new("input_invalid", "/sources", "补充请求的等待身份不完整").into(),
                );
            }
        };
        let receipt = host
            .runtime
            .goals()
            .expect("Goal runtime")
            .signal(
                &id,
                GoalSignalRequest {
                    protocol_version: 1,
                    wait_id,
                    nonce,
                    payload: json!({"clarification":req.clarification,"urls":urls}),
                    request_key: req.request_key,
                    expected_revision: req.expected_revision,
                },
                &Caller::local(),
            )
            .await?;
        return Ok(Json(serde_json::to_value(receipt).map_err(Error::from)?));
    }
    if host.runtime.goals().is_ok() {
        return Err(Error::new("read_only", "/research", "旧研究保持只读，请新建研究").into());
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
    #[serde(default)]
    expected_revision: Option<u64>,
}
async fn cancel(
    State(host): State<Host>,
    Path(id): Path<String>,
    Json(req): Json<Cancel>,
) -> ApiResult {
    if let Some(goal) = goal_snapshot(&host, &id).await? {
        let revision = req.expected_revision.ok_or_else(|| {
            Error::new(
                "input_invalid",
                "/expected_revision",
                "停止请求必须关联所见研究版本",
            )
        })?;
        let receipt = host
            .runtime
            .goals()
            .expect("Goal runtime")
            .cancel(&goal.goal_id, &req.request_key, revision, &Caller::local())
            .await?;
        return Ok(Json(serde_json::to_value(receipt).map_err(Error::from)?));
    }
    if host.runtime.goals().is_ok() {
        return Err(Error::new("read_only", "/research", "旧研究保持只读，请新建研究").into());
    }
    let snapshot = host
        .runtime
        .cancel(&id, &req.request_key, &Caller::local())
        .await?;
    Ok(Json(json!({"snapshot":snapshot})))
}
async fn resume(
    State(host): State<Host>,
    Path(id): Path<String>,
    Json(req): Json<Cancel>,
) -> ApiResult {
    let goal = goal_snapshot(&host, &id)
        .await?
        .ok_or_else(|| Error::new("not_found", "/goal", "没有该新研究"))?;
    let revision = req.expected_revision.ok_or_else(|| {
        Error::new(
            "input_invalid",
            "/expected_revision",
            "继续请求必须关联所见版本",
        )
    })?;
    let receipt = host
        .runtime
        .goals()
        .expect("Goal runtime")
        .resume(&goal.goal_id, &req.request_key, revision, &Caller::local())
        .await?;
    Ok(Json(serde_json::to_value(receipt).map_err(Error::from)?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceQuery {
    source_id: Option<String>,
}
async fn artifact(
    State(host): State<Host>,
    Path((id, artifact)): Path<(String, String)>,
    Query(query): Query<SourceQuery>,
) -> ApiResult {
    if let Some(goal) = goal_snapshot(&host, &id).await? {
        let reference = goal
            .artifacts
            .iter()
            .find(|a| a.accepted && a.artifact.id == artifact)
            .ok_or_else(|| Error::new("not_found", "/artifact", "此研究没有该资料"))?;
        let data = host
            .runtime
            .goals()
            .expect("Goal runtime")
            .read_artifact(&artifact, &Caller::local())
            .await?;
        let value = if let Some(source_id) = query.source_id {
            serde_json::to_value(crate::article::source_view(&data, &source_id)?)
                .map_err(Error::from)?
        } else {
            parse_unique(&data.bytes)?
        };
        return Ok(Json(
            json!({"artifact":reference.artifact,"value":value,"text":null}),
        ));
    }
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DiagnosticsQuery {
    run_id: Option<String>,
    #[serde(default)]
    after: u64,
    limit: u16,
}
fn trace_query(
    query: std::result::Result<Query<DiagnosticsQuery>, QueryRejection>,
) -> std::result::Result<TraceQuery, ApiError> {
    let Query(q) = query.map_err(|_| {
        Error::new(
            "input_invalid",
            "/trace/query",
            "请提供1–1000的limit及有效查询参数",
        )
    })?;
    Ok(TraceQuery {
        run_id: q.run_id,
        after: q.after,
        limit: q.limit,
    })
}
async fn trace(
    State(host): State<Host>,
    Path(id): Path<String>,
    query: std::result::Result<Query<DiagnosticsQuery>, QueryRejection>,
) -> ApiResult {
    if goal_snapshot(&host, &id).await?.is_some() {
        let page = host.runtime.goals().expect("Goal runtime").trace(
            &id,
            trace_query(query)?,
            &Caller::local(),
        )?;
        return Ok(Json(serde_json::to_value(page).map_err(Error::from)?));
    }
    let page = host
        .runtime
        .trace(&id, trace_query(query)?, &Caller::local())?;
    Ok(Json(serde_json::to_value(page).map_err(Error::from)?))
}
async fn export_trace(
    State(host): State<Host>,
    Path(id): Path<String>,
    query: std::result::Result<Query<DiagnosticsQuery>, QueryRejection>,
) -> std::result::Result<Response, ApiError> {
    if goal_snapshot(&host, &id).await?.is_some() {
        let bytes = host.runtime.goals().expect("Goal runtime").export_trace(
            &id,
            trace_query(query)?,
            &Caller::local(),
        )?;
        return Ok((
            [
                (header::CONTENT_TYPE, "application/json"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=inquiry-trace.json",
                ),
            ],
            bytes,
        )
            .into_response());
    }
    let bytes = host
        .runtime
        .export_trace(&id, trace_query(query)?, &Caller::local())?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/json"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=inquiry-trace.json",
            ),
        ],
        bytes,
    )
        .into_response())
}
async fn budgets(State(host): State<Host>, Path(id): Path<String>) -> ApiResult {
    if goal_snapshot(&host, &id).await?.is_some() {
        let view = host
            .runtime
            .goals()
            .expect("Goal runtime")
            .budgets(&id, &Caller::local())
            .await?;
        return Ok(Json(serde_json::to_value(view).map_err(Error::from)?));
    }
    Ok(Json(
        serde_json::to_value(host.runtime.budgets(&id, &Caller::local())?).map_err(Error::from)?,
    ))
}
