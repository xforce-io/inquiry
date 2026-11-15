//! Opt-in real native execution admission probe; never article or Web quality acceptance.
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use taskloop::*;
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "One real registered Sol Planner entry; requires explicit probe config, fresh evidence dir and registration"]
async fn real_native_planner_submits_through_production_goal_binding() {
    let out =
        PathBuf::from(std::env::var_os("INQUIRY_REAL_PROBE_DIR").expect("explicit evidence dir"));
    let config = PathBuf::from(
        std::env::var_os("INQUIRY_REAL_PROBE_CONFIG").expect("explicit execution config"),
    );
    assert!(out.is_absolute() && config.is_absolute());
    let registration: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("registration.json")).unwrap()).unwrap();
    assert_eq!(registration["candidate_sha"], env!("INQUIRY_BUILD_SHA"));
    assert_eq!(registration["model"], "gpt-6.1-sol");
    assert_eq!(registration["max_model_entries"], 1);
    assert!(
        !out.join("data").exists(),
        "never resume an old probe silently"
    );
    let setup = inquiry::execution::ExecutionSetup::load(&config, &out.join("data")).unwrap();
    let (runtime, _) = setup
        .open(Path::new(env!("CARGO_MANIFEST_DIR")), &out.join("data"))
        .await
        .unwrap();
    let goals = runtime.goals().unwrap();
    let question = "请生成一份Palantir Foundry中Vertex的中文产品深度研究，覆盖定位、能力、用户路径、技术机制、与Ontology/Foundry/AIP/Workshop关系、场景、限制和替代方案。不是Google Vertex AI。当前入口只规划给定模板，不执行研究。";
    let mut limits = inquiry::orchestration::limits();
    limits.max_runs = 3;
    let id=goals.submit(GoalRequest{protocol_version:1,request_key:"one-native-planner-probe".into(),grant_ref:"local-readonly".into(),spec:GoalSpec{goal:question.into(),inputs:json!({"question":question,"scope":"Registered one-entry execution admission probe; do not treat as article quality","seed_urls":[]}),profile_ref:inquiry::orchestration::PROFILE.into(),planning:GoalPlanning{mode:PlanningMode::Workflow,max_revisions:0,max_planning_ms:180000,template_ref:Some(inquiry::orchestration::TEMPLATE.into())},limits,parent_goal_id:None,imports:vec![]}},&Caller::local()).await.unwrap().goal_id;
    std::fs::write(out.join("goal-id.txt"), &id).unwrap();
    let settled = tokio::time::timeout(Duration::from_secs(205), async {
        loop {
            let view = goals.get(&id, &Caller::local()).await.unwrap();
            if view.status == GoalStatus::Finished {
                break view;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await;
    if settled.is_err() {
        let s = goals.get(&id, &Caller::local()).await.unwrap();
        let _ = goals
            .cancel(&id, "probe-timeout", s.revision, &Caller::local())
            .await;
    }
    let snapshot = goals.get(&id, &Caller::local()).await.unwrap();
    std::fs::write(
        out.join("snapshot.json"),
        serde_json::to_vec_pretty(&snapshot).unwrap(),
    )
    .unwrap();
    let trace = goals
        .trace(
            &id,
            TraceQuery {
                run_id: None,
                after: 0,
                limit: 1000,
            },
            &Caller::local(),
        )
        .unwrap();
    std::fs::write(
        out.join("trace.json"),
        serde_json::to_vec_pretty(&trace).unwrap(),
    )
    .unwrap();
    let budgets = goals.budgets(&id, &Caller::local()).await.unwrap();
    std::fs::write(
        out.join("budgets.json"),
        serde_json::to_vec_pretty(&budgets).unwrap(),
    )
    .unwrap();
    runtime.shutdown().await.unwrap();
    assert!(settled.is_ok(), "bounded native probe did not settle");
    assert_eq!(
        snapshot.runs.len(),
        1,
        "no retries or article workers are permitted"
    );
    let run = snapshot.runs.values().next().unwrap();
    assert_eq!(
        run.budget.as_ref().unwrap().execution_outcome.as_deref(),
        Some("completed"),
        "native execution error: {:?}",
        run.error
    );
    assert!(
        run.budget.as_ref().unwrap().calls_used > 0,
        "must submit via a real host tool"
    );
    let failure = run
        .plan_validation_failure
        .as_ref()
        .expect("whole graph exceeds probe budget");
    assert!(
        !failure.issues.is_empty()
            && failure
                .issues
                .iter()
                .all(|issue| issue.path == "/limits/max_runs"),
        "unexpected planning failure: {failure:?}"
    );
    assert!(snapshot.plan_digest.is_none());
    println!(
        "{}",
        json!({"native_goal_probe":"pass","goal_id":id,"real_model_entries":1,"model":"gpt-6.1-sol","whole_graph_rejected_by_small_probe_budget":true,"article_quality_proven":false,"web_verified":false})
    );
}
