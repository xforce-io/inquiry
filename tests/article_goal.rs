//! Controlled public Goal paths. These tests do not prove real article quality.
use async_trait::async_trait;
use inquiry::{
    article::{self, Citation, Corpus, Report, Source},
    orchestration,
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc, time::Duration};
use taskloop::*;
use tokio_util::sync::CancellationToken;

struct Agent {
    final_verdict: &'static str,
}
fn package(round: u8) -> Corpus {
    let paragraphs =
        vec!["A saved official statement with a real mechanism and limitation.".to_owned()];
    Corpus {
        version: 1,
        round,
        sources: if round == 1 {
            vec![Source {
                source_id: "r1s1".into(),
                origin: "user-input".into(),
                fact: json!({"usable":true,"final_url":"https://example.com/official","requested_url":"https://example.com/official","title":"Controlled source (synthetic)","reason":"Controlled saved paragraph, not real research","captured_at":"2026-11-12T17:00:00Z","fetch_status":"captured","http_status":200,"truncated":false,"normalized_sha256":inquiry::domain::hash(&serde_json::to_vec(&paragraphs).unwrap())}),
                paragraphs,
            }]
        } else {
            vec![]
        },
        searches: vec![],
        limitations: vec![],
    }
}
fn artifact(value: &Value) -> ArtifactData {
    let bytes = serde_json::to_vec(value).unwrap();
    ArtifactData {
        reference: ArtifactRef {
            id: "saved".into(),
            sha256: inquiry::domain::hash(&bytes),
            slot: "corpus".into(),
            state_id: "collect".into(),
            visit: 1,
        },
        bytes: bytes.into(),
    }
}
fn report_and_analysis(a: &ArtifactData) -> (Report, Value) {
    let citation = Citation {
        artifact_id: a.reference.id.clone(),
        sha256: a.reference.sha256.clone(),
        source_id: "r1s1".into(),
        paragraph: 0,
        quote: "official statement".into(),
    };
    let mut evidence = serde_json::to_value(&citation).unwrap();
    evidence["id"] = json!("e1");
    (
        Report {
            answer_status: "answered".into(),
            conclusion:
                "## Mechanism\nSupported statement [1].\n## Limits\nConditional conclusion.".into(),
            citations: vec![citation],
            limitations: vec![],
        },
        json!({"evidence":[evidence],"judgments":[{"text":"Supported judgment","support":["e1"],"counter":[],"limitations":[]}],"decision":"ready","gaps":[],"coverage":[{"question":"Mechanism","status":"answered","conclusion":"A supported mechanism","evidence_ids":["e1"]}],"search_queries":[]}),
    )
}
#[test]
fn exact_source_versions_and_analysis_lineage_are_required() {
    let a = artifact(&serde_json::to_value(package(1)).unwrap());
    let (report, analysis) = report_and_analysis(&a);
    article::check_report(&report, &analysis, std::slice::from_ref(&a)).unwrap();
    for kind in [
        "version",
        "source",
        "paragraph",
        "quote",
        "lineage",
        "coverage",
        "gap",
        "unsupported",
    ] {
        let mut r = report.clone();
        let mut an = analysis.clone();
        match kind {
            "version" => r.citations[0].sha256 = "0".repeat(64),
            "source" => r.citations[0].source_id = "r1s9".into(),
            "paragraph" => r.citations[0].paragraph = 7,
            "quote" => r.citations[0].quote = "invented".into(),
            "lineage" => an["judgments"][0]["support"] = json!(["unknown"]),
            "coverage" => an["coverage"][0]["evidence_ids"] = json!(["unknown"]),
            "gap" => an["gaps"] = json!(["Unanswered decisive comparison"]),
            _ => r.answer_status = "insufficient_evidence".into(),
        }
        assert!(
            article::check_report(&r, &an, std::slice::from_ref(&a)).is_err(),
            "{kind}"
        );
    }
}
#[async_trait]
impl Executor for Agent {
    fn capabilities(&self) -> ExecutorCapabilities {
        ExecutorCapabilities::MANAGED
    }
    fn enforces_json_candidate_limit(&self) -> bool {
        true
    }
    async fn execute(
        &self,
        inv: Invocation,
        tools: Arc<dyn ToolBridge>,
        _: CancellationToken,
    ) -> ExecutionOutcome {
        let role = tools.goal_context().unwrap().role;
        let (slot, value) = match role {
            GoalRunRole::Planner { .. } => {
                let t = &inv.stage_input["template"];
                let nodes=t["nodes"].as_object().unwrap().iter().map(|(id,n)| (id.clone(),json!({"capability_ref":n["capabilities"][0],"depends_on":n["depends_on"],"inputs":n["inputs"]}))).collect::<serde_json::Map<_,_>>();
                (
                    "proposal",
                    json!({"protocol_version":1,"kind":"proposal","proposal":{"protocol_version":1,"goal_spec_digest":inv.stage_input["goal_spec_digest"],"profile_digest":inv.stage_input["profile_digest"],"planning_mode":"workflow","template_digest":inv.stage_input["template_digest"],"nodes":nodes,"outputs":t["outputs"],"checks":t["checks"]}}),
                )
            }
            GoalRunRole::NodeMember { node_id, .. } if node_id == "scope" => (
                "plan",
                json!({"subquestions":["Explain the mechanism and boundaries"],"search_queries":["official mechanism"],"stop_reason":"Full question with evidence"}),
            ),
            GoalRunRole::NodeMember { node_id, .. }
                if node_id == "collect" || node_id == "supplement" =>
            {
                let round = if node_id == "collect" { 1 } else { 2 };
                tools
                    .call(
                        "artifact.write",
                        json!({"slot":"corpus","value":package(round)}),
                    )
                    .await
                    .unwrap();
                ("archive", json!({}))
            }
            GoalRunRole::NodeMember { node_id, .. }
                if node_id == "analyze" || node_id == "synthesize" =>
            {
                let reference = inv
                    .artifacts
                    .iter()
                    .find(|a| a.slot == "corpus" && a.state_id.contains("collect"))
                    .unwrap();
                let response = tools
                    .call(
                        "inquiry.article_source",
                        json!({"artifact_id":reference.id,"source_id":"r1s1"}),
                    )
                    .await
                    .unwrap();
                assert!(
                    !response["result"]["paragraphs"]
                        .as_array()
                        .unwrap()
                        .is_empty()
                );
                let a = ArtifactData {
                    reference: reference.clone(),
                    bytes: Arc::from([]),
                };
                ("analysis", report_and_analysis(&a).1)
            }
            GoalRunRole::NodeMember { node_id, .. }
                if node_id == "draft" || node_id == "revise" =>
            {
                let a = inv
                    .artifacts
                    .iter()
                    .find(|a| a.slot == "analysis" && a.state_id.contains("synthesize"))
                    .unwrap();
                let read = tools
                    .call("artifact.read", json!({"id":a.id}))
                    .await
                    .unwrap();
                let mut evidence: Value =
                    serde_json::from_str(read["text"].as_str().unwrap()).unwrap();
                let mut c = evidence["evidence"][0].take();
                c.as_object_mut().unwrap().remove("id");
                (
                    "report",
                    json!({"answer_status":"answered","conclusion":"## Mechanism\nA supported official statement [1].\n## Boundary\nA conditional conclusion.","citations":[c],"limitations":[]}),
                )
            }
            GoalRunRole::NodeMember { node_id, .. } if node_id == "review" => {
                let input: CheckInput = serde_json::from_value(inv.stage_input).unwrap();
                // A data fail must reach revision rather than terminating the graph.
                ("evaluation", evaluation(&input, "fail"))
            }
            GoalRunRole::FinalCheck { .. } => {
                let input: CheckInput = serde_json::from_value(inv.stage_input).unwrap();
                ("evaluation", evaluation(&input, self.final_verdict))
            }
            other => panic!("Unexpected controlled role {other:?}"),
        };
        tools
            .call("artifact.write", json!({"slot":slot,"value":value}))
            .await
            .unwrap();
        ExecutionOutcome::Completed {
            usage: json!({"controlled_fixture":true}),
        }
    }
}
fn evaluation(input: &CheckInput, verdict: &str) -> Value {
    json!({"protocol_version":1,"verdict":verdict,"results":input.rules.rules.iter().map(|r|json!({"id":r.id,"verdict":verdict,"message":"Controlled evaluation, not real semantic quality proof.","evidence_refs":input.evidence_refs})).collect::<Vec<_>>(),"evidence_refs":input.evidence_refs,"violations":[]})
}
#[test]
fn article_registry_freezes_without_running_a_model() {
    let bindings = orchestration::bindings(
        Arc::new(Agent {
            final_verdict: "pass",
        }),
        "test".into(),
    );
    let (mut registry, _) =
        orchestration::registry(Path::new(env!("CARGO_MANIFEST_DIR")), &bindings).unwrap();
    let receipt = registry.validate_and_freeze(&bindings).unwrap();
    assert_eq!(receipt.capabilities.len(), 6);
    assert!(receipt.profiles.contains_key(orchestration::PROFILE));
}
#[tokio::test]
async fn data_review_failure_reaches_revision_and_final_goal_verdict_is_independent() {
    for (verdict, outcome) in [
        ("pass", GoalOutcome::Achieved),
        ("fail", GoalOutcome::NotMet),
        ("error", GoalOutcome::Failed),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let agent = Arc::new(Agent {
            final_verdict: verdict,
        });
        let bindings = orchestration::bindings(agent.clone(), "controlled-goal-test".into())
            .executor("article.collect", agent);
        let (registry, options) =
            orchestration::registry(Path::new(env!("CARGO_MANIFEST_DIR")), &bindings).unwrap();
        let runtime = Runtime::open_orchestrated(
            inquiry::budget::runtime_options(dir.path().join("store")),
            vec![],
            bindings,
            registry,
            options,
            None,
        )
        .await
        .unwrap();
        let goals = runtime.goals().unwrap();
        let id=goals.submit(GoalRequest{protocol_version:1,request_key:"first".into(),grant_ref:"local-readonly".into(),spec:GoalSpec{goal:"Explain a mechanism with evidence and boundaries".into(),inputs:json!({"question":"Explain a mechanism with evidence and boundaries","scope":"fixture","seed_urls":[]}),profile_ref:orchestration::PROFILE.into(),planning:GoalPlanning{mode:PlanningMode::Workflow,max_revisions:2,max_planning_ms:180000,template_ref:Some(orchestration::TEMPLATE.into())},limits:orchestration::limits(),parent_goal_id:None,imports:vec![]}},&Caller::local()).await.unwrap().goal_id;
        let final_state = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let s = goals.get(&id, &Caller::local()).await.unwrap();
                if s.status == GoalStatus::Finished {
                    break s;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            final_state.outcome,
            Some(outcome),
            "goal error: {:?}; plan issues: {:?}",
            final_state.error,
            final_state
                .runs
                .values()
                .filter_map(|r| r.plan_validation_failure.as_ref())
                .collect::<Vec<_>>()
        );
        assert!(
            final_state
                .nodes
                .values()
                .all(|n| n.status == GoalNodeStatus::Succeeded)
        );
        assert!(final_state.nodes["revise"].outputs.contains_key("report"));
        assert_eq!(final_state.budget.runs_used, 11);
        assert!(final_state.budget.reserved.is_empty());
        let evaluations = goals.evaluations(&id, &Caller::local()).await.unwrap();
        assert!(
            evaluations
                .iter()
                .any(|e| e.phase == CheckInputPhase::NodeEvaluation
                    && e.verdict == EvaluationVerdict::Fail)
        );
        runtime.shutdown().await.unwrap();
    }
}

struct Clarifying;
#[async_trait]
impl Executor for Clarifying {
    fn capabilities(&self) -> ExecutorCapabilities {
        ExecutorCapabilities::MANAGED
    }
    fn enforces_json_candidate_limit(&self) -> bool {
        true
    }
    async fn execute(
        &self,
        inv: Invocation,
        tools: Arc<dyn ToolBridge>,
        cancel: CancellationToken,
    ) -> ExecutionOutcome {
        if matches!(
            tools.goal_context().unwrap().role,
            GoalRunRole::Planner { .. }
        ) && inv.signal_facts.is_empty()
        {
            tools.call("artifact.write",json!({"slot":"proposal","value":{"protocol_version":1,"kind":"needs_input","input_schema_ref":"article-clarification","reason":"Which object is intended?"}})).await.unwrap();
            ExecutionOutcome::Completed {
                usage: json!({"controlled_fixture":true}),
            }
        } else {
            Agent {
                final_verdict: "pass",
            }
            .execute(inv, tools, cancel)
            .await
        }
    }
}
struct HttpFixture {
    _dir: tempfile::TempDir,
    runtime: Arc<Runtime>,
    app: axum::Router,
}
impl HttpFixture {
    async fn new(agent: Arc<dyn Executor>) -> Self {
        Self::with_collection(agent, false).await
    }
    async fn with_collection(agent: Arc<dyn Executor>, native: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bindings = orchestration::bindings(agent.clone(), "http-goal-test".into());
        let bindings = if native {
            bindings
        } else {
            bindings.executor("article.collect", agent)
        };
        let (registry, options) =
            orchestration::registry(Path::new(env!("CARGO_MANIFEST_DIR")), &bindings).unwrap();
        let runtime = Arc::new(
            Runtime::open_orchestrated(
                inquiry::budget::runtime_options(dir.path().join("store")),
                vec![],
                bindings,
                registry,
                options,
                None,
            )
            .await
            .unwrap(),
        );
        let app = inquiry::web::router(inquiry::web::Host {
            runtime: runtime.clone(),
            definition_digest: String::new(),
            port: 3410,
            session: "goal-test".into(),
            web_dir: Path::new(env!("CARGO_MANIFEST_DIR")).join("web/dist"),
        });
        Self {
            _dir: dir,
            runtime,
            app,
        }
    }
    async fn request(&self, method: &str, path: &str, value: Option<Value>) -> (u16, Value) {
        use axum::{
            body::{Body, to_bytes},
            http::Request,
        };
        use tower::ServiceExt;
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("Host", "127.0.0.1:3410")
            .header("Cookie", "inquiry_session=goal-test");
        if method == "POST" {
            request = request
                .header("Origin", "http://127.0.0.1:3410")
                .header("Content-Type", "application/json");
        }
        let response = self
            .app
            .clone()
            .oneshot(
                request
                    .body(
                        value
                            .map(|v| Body::from(serde_json::to_vec(&v).unwrap()))
                            .unwrap_or(Body::empty()),
                    )
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status().as_u16();
        let bytes = to_bytes(response.into_body(), 4_194_304).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }
    async fn create(&self, key: &str) -> String {
        let (status,result)=self.request("POST","/api/research",Some(json!({"question":"Explain the mechanism and boundaries","scope":"fixture","seed_urls":[],"request_key":key}))).await;
        assert_eq!(status, 200, "{result}");
        result["id"].as_str().unwrap().into()
    }
    async fn wait(&self, id: &str, status: &str) -> Value {
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let (code, data) = self
                    .request("GET", &format!("/api/research/{id}"), None)
                    .await;
                assert_eq!(code, 200, "{data}");
                if data["snapshot"]["status"] == status {
                    break data;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap()
    }
}
#[tokio::test]
async fn goal_http_preserves_versions_and_denies_cross_research_artifacts() {
    let fixture = HttpFixture::new(Arc::new(Agent {
        final_verdict: "pass",
    }))
    .await;
    let (code, _) = fixture
        .request(
            "POST",
            "/api/research",
            Some(json!({"question":"  ","request_key":"empty"})),
        )
        .await;
    assert_eq!(code, 400);
    let id = fixture.create("same").await;
    let replay = fixture.create("same").await;
    assert_eq!(id, replay);
    let (code, _) = fixture
        .request(
            "POST",
            "/api/research",
            Some(json!({"question":"Different","request_key":"same"})),
        )
        .await;
    assert_eq!(code, 409);
    let view = fixture.wait(&id, "completed").await;
    assert_eq!(view["engine"], "goal");
    assert!(
        view["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["stage"] == "draft")
    );
    assert!(
        view["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["stage"] == "revise")
    );
    let corpus = &view["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["stage"] == "collect" && o["artifact"]["slot"] == "corpus")
        .unwrap()["artifact"];
    let artifact = corpus["id"].as_str().unwrap();
    let (code, source) = fixture
        .request(
            "GET",
            &format!("/api/research/{id}/artifacts/{artifact}?source_id=r1s1"),
            None,
        )
        .await;
    assert_eq!(code, 200);
    assert_eq!(source["artifact"]["sha256"], corpus["sha256"]);
    assert!(
        source["value"]["paragraphs"][0]
            .as_str()
            .unwrap()
            .contains("official statement")
    );
    let other = fixture.create("other").await;
    let (code, _) = fixture
        .request(
            "GET",
            &format!("/api/research/{other}/artifacts/{artifact}?source_id=r1s1"),
            None,
        )
        .await;
    assert_eq!(code, 404);
    let (code, _) = fixture
        .request(
            "GET",
            &format!("/api/research/{id}/artifacts/{artifact}?source_id=missing"),
            None,
        )
        .await;
    assert_eq!(code, 404);
    fixture.runtime.shutdown().await.unwrap();
}
#[tokio::test]
async fn goal_clarification_replay_keeps_original_wait_identity_after_completion() {
    let fixture = HttpFixture::new(Arc::new(Clarifying)).await;
    let id = fixture.create("clarify").await;
    let view = fixture.wait(&id, "waiting").await;
    let wait = &view["goal"]["waiting"][0];
    let input = json!({"urls":[],"clarification":"The official object and original scope","wait_id":wait["wait_id"],"nonce":wait["nonce"],"request_key":"clarification-once","expected_revision":view["snapshot"]["revision"]});
    let (code, receipt) = fixture
        .request(
            "POST",
            &format!("/api/research/{id}/sources"),
            Some(input.clone()),
        )
        .await;
    assert_eq!(code, 200, "{receipt}");
    let finished = fixture.wait(&id, "completed").await;
    let (code, replay) = fixture
        .request("POST", &format!("/api/research/{id}/sources"), Some(input))
        .await;
    assert_eq!(code, 200, "{replay}");
    assert_eq!(receipt, replay);
    let (_, again) = fixture
        .request("GET", &format!("/api/research/{id}"), None)
        .await;
    assert_eq!(
        again["goal"]["budget"]["runs_used"],
        finished["goal"]["budget"]["runs_used"]
    );
    fixture.runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn goal_http_cancel_is_idempotent_and_other_research_stays_readable() {
    let fixture = HttpFixture::new(Arc::new(Clarifying)).await;
    let a = fixture.create("cancel-a").await;
    let b = fixture.create("unrelated-b").await;
    let saved = fixture.wait(&a, "waiting").await;
    let other = fixture.wait(&b, "waiting").await;
    let input =
        json!({"request_key":"cancel-once","expected_revision":saved["snapshot"]["revision"]});
    let (code, receipt) = fixture
        .request(
            "POST",
            &format!("/api/research/{a}/cancel"),
            Some(input.clone()),
        )
        .await;
    assert_eq!(code, 200, "{receipt}");
    let final_state = fixture.wait(&a, "cancelled").await;
    assert!(
        final_state["goal"]["budget"]["reserved"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    let (code, replay) = fixture
        .request("POST", &format!("/api/research/{a}/cancel"), Some(input))
        .await;
    assert_eq!(code, 200, "{replay}");
    assert_eq!(receipt, replay);
    let (_, unchanged) = fixture
        .request("GET", &format!("/api/research/{b}"), None)
        .await;
    assert_eq!(
        unchanged["snapshot"]["revision"],
        other["snapshot"]["revision"]
    );
    assert_eq!(unchanged["snapshot"]["status"], "waiting");
    fixture.runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn goal_http_resume_replays_exact_revision_and_preserves_wait_nonce() {
    let fixture = HttpFixture::new(Arc::new(Clarifying)).await;
    let id = fixture.create("resume").await;
    let before = fixture.wait(&id, "waiting").await;
    // Public stop models the state retained by an orderly host shutdown.
    fixture
        .runtime
        .goals()
        .unwrap()
        .stop(
            &id,
            "host-pause",
            before["snapshot"]["revision"].as_u64().unwrap(),
            &Caller::local(),
        )
        .await
        .unwrap();
    let stopped = fixture.wait(&id, "stopped").await;
    let input =
        json!({"request_key":"resume-once","expected_revision":stopped["snapshot"]["revision"]});
    let (code, receipt) = fixture
        .request(
            "POST",
            &format!("/api/research/{id}/resume"),
            Some(input.clone()),
        )
        .await;
    assert_eq!(code, 200, "{receipt}");
    let resumed = fixture.wait(&id, "waiting").await;
    assert_eq!(
        resumed["goal"]["waiting"][0]["nonce"],
        before["goal"]["waiting"][0]["nonce"]
    );
    assert_eq!(
        resumed["goal"]["budget"]["runs_used"],
        before["goal"]["budget"]["runs_used"]
    );
    let (code, replay) = fixture
        .request("POST", &format!("/api/research/{id}/resume"), Some(input))
        .await;
    assert_eq!(code, 200, "{replay}");
    assert_eq!(receipt, replay);
    fixture.runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn insufficient_goal_run_budget_rejects_plan_without_implicit_revision_or_new_goal() {
    let fixture = HttpFixture::new(Arc::new(Agent {
        final_verdict: "pass",
    }))
    .await;
    let mut limits = orchestration::limits();
    limits.max_runs = 3;
    let goals = fixture.runtime.goals().unwrap();
    let id = goals.submit(GoalRequest {
        protocol_version: 1, request_key: "bounded".into(), grant_ref: "local-readonly".into(),
        spec: GoalSpec {
            goal: "Explain the mechanism and boundaries".into(),
            inputs: json!({"question":"Explain the mechanism and boundaries","scope":"fixture","seed_urls":[]}),
            profile_ref: orchestration::PROFILE.into(),
            planning: GoalPlanning { mode: PlanningMode::Workflow, max_revisions: 2, max_planning_ms: 180000, template_ref: Some(orchestration::TEMPLATE.into()) },
            limits, parent_goal_id: None, imports: vec![],
        },
    }, &Caller::local()).await.unwrap().goal_id;
    let view = fixture.wait(&id, "failed").await;
    // The whole graph cannot fit: admission fails before executing workers.
    assert_eq!(view["goal"]["budget"]["runs_used"], 1);
    assert!(view["goal"]["plan_digest"].is_null());
    assert_eq!(view["goal"]["limits"]["max_runs"], 3);
    assert!(
        view["goal"]["budget"]["reserved"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    assert!(
        view["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|o| o["stage"] != "revise")
    );
    let replay = goals
        .submit(
            GoalRequest {
                protocol_version: 1,
                request_key: "bounded".into(),
                grant_ref: "local-readonly".into(),
                spec: serde_json::from_value(view["goal"]["spec"].clone()).unwrap(),
            },
            &Caller::local(),
        )
        .await
        .unwrap();
    assert_eq!(replay.goal_id, id);
    fixture.runtime.shutdown().await.unwrap();
}

struct CollectorProbe;
#[async_trait]
impl Executor for CollectorProbe {
    fn capabilities(&self) -> ExecutorCapabilities {
        ExecutorCapabilities::MANAGED
    }
    fn enforces_json_candidate_limit(&self) -> bool {
        true
    }
    async fn execute(
        &self,
        inv: Invocation,
        tools: Arc<dyn ToolBridge>,
        cancel: CancellationToken,
    ) -> ExecutionOutcome {
        match tools.goal_context().unwrap().role {
            GoalRunRole::Planner { .. } => {
                Agent {
                    final_verdict: "pass",
                }
                .execute(inv, tools, cancel)
                .await
            }
            GoalRunRole::NodeMember { node_id, .. } if node_id == "scope" => {
                tools.call("artifact.write", json!({"slot":"plan","value":{"subquestions":["Identify Palantir Foundry Vertex"],"search_queries":["site:palantir.com/docs/foundry vertex overview"],"stop_reason":"Network collection probe; not article quality"}})).await.unwrap();
                ExecutionOutcome::Completed {
                    usage: json!({"controlled_fixture":true,"model":false}),
                }
            }
            _ => ExecutionOutcome::Failed {
                code: "probe_complete".into(),
                message: "Stop after real native collection; zero models".into(),
            },
        }
    }
}
#[tokio::test]
#[ignore = "External public-network acquisition through production Goal collector; zero models, not article quality"]
async fn native_collection_saves_actual_search_origins_normalized_text_and_raw_bytes() {
    use base64::Engine;
    let fixture = HttpFixture::with_collection(Arc::new(CollectorProbe), true).await;
    let id = fixture.create("real-collection-probe").await;
    let view = tokio::time::timeout(Duration::from_secs(480), async {
        loop {
            let (_, view) = fixture
                .request("GET", &format!("/api/research/{id}"), None)
                .await;
            if view["snapshot"]["status"] == "failed" {
                break view;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    let corpus = view["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["stage"] == "collect" && o["artifact"]["slot"] == "corpus")
        .unwrap_or_else(|| panic!("collector did not publish: {}", view["snapshot"]));
    let value: Corpus = serde_json::from_value(corpus["value"].clone()).unwrap();
    assert!(!value.searches.is_empty());
    assert!(
        value.sources.iter().any(|s| s.fact["usable"] == true),
        "no saved source: {value:?}"
    );
    let archive = view["goal"]["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| {
            a["accepted"] == true
                && a["producer_kind"] == "node_projection"
                && a["node_id"] == "collect"
                && a["artifact"]["slot"] == "archive"
        })
        .unwrap();
    let data = fixture
        .runtime
        .goals()
        .unwrap()
        .read_artifact(
            archive["artifact"]["id"].as_str().unwrap(),
            &Caller::local(),
        )
        .await
        .unwrap();
    let raw: Value = serde_json::from_slice(&data.bytes).unwrap();
    for source in value.sources.iter().filter(|s| s.fact["usable"] == true) {
        assert!(source.origin.starts_with("search:"));
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(raw[&source.source_id]["base64"].as_str().unwrap())
            .unwrap();
        assert_eq!(inquiry::domain::hash(&bytes), source.fact["body_sha256"]);
        assert_eq!(
            raw[&source.source_id]["body_sha256"],
            source.fact["body_sha256"]
        );
        assert_eq!(
            inquiry::domain::hash(&serde_json::to_vec(&source.paragraphs).unwrap()),
            source.fact["normalized_sha256"]
        );
    }
    println!(
        "{}",
        json!({"probe":"production Goal native collector","real_sources":value.sources.len(),"usable":value.sources.iter().filter(|s|s.fact["usable"]==true).count(),"model_calls":0,"article_quality_proven":false})
    );
    fixture.runtime.shutdown().await.unwrap();
}

struct BrowserAgent {
    stop_confirmed: Arc<std::sync::atomic::AtomicBool>,
}
#[async_trait]
impl Executor for BrowserAgent {
    fn capabilities(&self) -> ExecutorCapabilities {
        ExecutorCapabilities::MANAGED
    }
    fn enforces_json_candidate_limit(&self) -> bool {
        true
    }
    async fn stop_or_confirm(&self, _: &str) -> StopState {
        if self
            .stop_confirmed
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            StopState::Confirmed
        } else {
            StopState::Unknown
        }
    }
    async fn execute(
        &self,
        inv: Invocation,
        tools: Arc<dyn ToolBridge>,
        cancel: CancellationToken,
    ) -> ExecutionOutcome {
        let role = tools.goal_context().unwrap().role;
        let question = if matches!(role, GoalRunRole::FinalCheck { .. })
            || inv.stage_input.get("check_id").is_some()
        {
            inv.stage_input["goal"].as_str().unwrap().to_owned()
        } else {
            inv.inputs["question"].as_str().unwrap().to_owned()
        };
        if question.contains("停止未确认") && matches!(role, GoalRunRole::Planner { .. }) {
            return ExecutionOutcome::StopUnknown {
                message: "Controlled unknown stop; no native process exists".into(),
            };
        }
        if question.contains("澄清") {
            return Clarifying.execute(inv, tools, cancel).await;
        }
        let verdict = if question.contains("未达标") {
            "fail"
        } else if question.contains("检查错误") {
            "error"
        } else {
            "pass"
        };
        Agent {
            final_verdict: verdict,
        }
        .execute(inv, tools, cancel)
        .await
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "Manual controlled Goal browser host; explicit private store/port, zero models; never article-quality acceptance"]
async fn browser_goal_host() {
    let store = std::path::PathBuf::from(
        std::env::var_os("INQUIRY_GOAL_TEST_STORE").expect("explicit private store"),
    );
    assert!(store.is_absolute());
    let port: u16 = std::env::var("INQUIRY_GOAL_TEST_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let confirmed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let agent = Arc::new(BrowserAgent {
        stop_confirmed: confirmed.clone(),
    });
    let bindings = orchestration::bindings(agent.clone(), "controlled-browser-zero-models".into())
        .executor("article.collect", agent);
    let (registry, options) =
        orchestration::registry(Path::new(env!("CARGO_MANIFEST_DIR")), &bindings).unwrap();
    let runtime = Arc::new(
        Runtime::open_orchestrated(
            inquiry::budget::runtime_options(store),
            vec![],
            bindings,
            registry,
            options,
            None,
        )
        .await
        .unwrap(),
    );
    let app = inquiry::web::router(inquiry::web::Host {
        runtime: runtime.clone(),
        definition_digest: String::new(),
        port,
        session: uuid::Uuid::new_v4().to_string(),
        web_dir: Path::new(env!("CARGO_MANIFEST_DIR")).join("web/dist"),
    });
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .unwrap();
    println!(
        "Controlled Goal browser fixture http://127.0.0.1:{port}; zero models. Use questions containing 达标/未达标/检查错误/澄清/停止未确认. Synthetic articles are not quality evidence."
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::signal::ctrl_c().await.unwrap();
            confirmed.store(true, std::sync::atomic::Ordering::SeqCst);
        })
        .await
        .unwrap();
    runtime.shutdown().await.unwrap();
}
