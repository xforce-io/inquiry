//! Synthetic Integration only: exercises real contracts/gates/attachments, not research quality.
use async_trait::async_trait;
use inquiry::domain::{self, Fetch, hash};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path, sync::Arc, time::Duration};
use taskloop::*;
use tokio_util::sync::CancellationToken;
const LIMIT: &str = "Synthetic fixture: no measured team success rate exists.";
struct SyntheticSearch;
#[async_trait]
impl DomainTool for SyntheticSearch {
    fn spec(&self) -> ToolSpec {
        domain::Search::default().spec()
    }
    async fn call(&self, _: ToolContext, args: Value) -> Result<ToolOutput> {
        Ok(ToolOutput::json(
            json!({"query":args["query"],"status":"discovered","results":[{"url":"https://example.com/fixture"}]}),
        ))
    }
}
struct SyntheticFetch;
#[async_trait]
impl DomainTool for SyntheticFetch {
    fn spec(&self) -> ToolSpec {
        Fetch.spec()
    }
    async fn call(&self, _: ToolContext, args: Value) -> Result<ToolOutput> {
        let raw = b"Synthetic source statement.".to_vec();
        let paragraphs = json!(["Synthetic source statement."]);
        let fact = json!({"source_id":args["source_id"],"requested_url":args["url"],"final_url":args["url"],"captured_at":"2026-11-03T00:00:00Z","fetch_status":"captured","http_status":200,"reason":"synthetic integration fixture; no actual HTTP request","title":"Synthetic source","body_sha256":hash(&raw),"normalized_sha256":hash(&serde_json::to_vec(&paragraphs).unwrap()),"transform_version":"synthetic-v1","usable":true,"raw_slot":format!("capture.raw.{}",args["source_id"].as_str().unwrap()),"snapshot_slot":format!("capture.snapshot.{}",args["source_id"].as_str().unwrap()),"truncated":false});
        Ok(ToolOutput {
            payload: fact.clone(),
            files: vec![
                Candidate::bytes(fact["raw_slot"].as_str().unwrap(), raw),
                Candidate::json(
                    fact["snapshot_slot"].as_str().unwrap(),
                    &json!({"fact":fact,"paragraphs":paragraphs}),
                )?,
            ],
        })
    }
}
#[derive(Default)]
struct SyntheticExecutor {
    loop_from: Option<&'static str>,
}
#[async_trait]
impl Executor for SyntheticExecutor {
    fn capabilities(&self) -> ExecutorCapabilities {
        ExecutorCapabilities::MANAGED
    }
    async fn execute(
        &self,
        inv: Invocation,
        tools: Arc<dyn ToolBridge>,
        _: CancellationToken,
    ) -> ExecutionOutcome {
        let result:Result<()> = async {
   let (slot,out)=match inv.state_id.as_str(){
    "scope"=>("plan",json!({"subquestions":["Synthetic fixture"],"search_queries":["Synthetic fixture"],"stop_reason":"Synthetic only"})),
    "discover"=>{
     let historical = inv.artifacts.iter().filter(|a|a.state_id != "scope").collect::<Vec<_>>();
     if inv.visit == 1 { assert!(historical.is_empty()); } else {
       assert!(inv.artifacts.iter().any(|a|a.state_id == "catalogue"));
       assert!(inv.artifacts.iter().any(|a|a.state_id == "capture" && a.slot == "materials"));
       assert!(inv.artifacts.iter().any(|a|a.state_id == "discover"));
       assert!(!inv.artifacts.iter().any(|a|a.slot.starts_with("capture.snapshot.")));
       if self.loop_from == Some("analyze") {assert!(inv.artifacts.iter().any(|a|a.state_id == "analyze"));}
       let gap = inv.artifacts.iter().find(|a|Some(a.state_id.as_str())==self.loop_from).unwrap();
       let got=tools.call("artifact.read",json!({"id":gap.id})).await?;
       assert!(got["text"].as_str().unwrap().contains("Focused synthetic gap"));
     }
     let query=format!("Focused synthetic query visit{}",inv.visit);
     let result=tools.call("inquiry.search",json!({"query":query})).await?;
     if inv.visit>1 {let previous=inv.artifacts.iter().find(|a|a.state_id=="discover").unwrap();let previous=tools.call("artifact.read",json!({"id":previous.id})).await?;assert!(previous["text"].as_str().unwrap().contains("Focused synthetic query visit1"));}
     ("candidates",json!({"queries":[{"query":query,"receipt_id":result["receipt_id"],"status":"discovered"}],"items":[{"source_id":format!("r{}s1",inv.visit),"url":"https://example.com/fixture","discovery_receipt_id":"user-input","subquestion":"Synthetic fixture"}],"stop_reason":"Explicit synthetic seed"}))},
    "capture"=>{
     let got=tools.call("inquiry.fetch",json!({"source_id":format!("r{}s1",inv.visit),"url":"https://example.com/fixture"})).await?;
     let fact=&got["result"];let mut item=serde_json::Map::new();
     for k in ["source_id","requested_url","final_url","fetch_status","http_status","reason","raw_slot","snapshot_slot"]{item.insert(k.into(),fact[k].clone());}
     item.insert("receipt_id".into(),got["receipt_id"].clone());item.insert("fact".into(),fact.clone());
     ("materials",json!({"items":[item],"stop_reason":"Synthetic only"}))
    },
    "catalogue"=>{ let more=self.loop_from==Some("catalogue") && inv.visit==1;
      ("catalogue",json!({"groups":[{"topic":"Synthetic fixture","source_ids":["r1s1"]}],"gaps":if more {vec!["Focused synthetic gap"]}else{vec![]},"limitations":[LIMIT],"decision":if more {"need_more"}else{"ready"}}))},
    "analyze"=>{
     let snapshot=inv.artifacts.iter().find(|a|a.slot=="capture.snapshot.r1s1").expect("attachments must be explicitly authorized");
     let mut got=Value::Null;
     for _ in 0..78 { got=tools.call("inquiry.read_source",json!({"artifact_id":snapshot.id,"start":0,"limit":1})).await?; }
     let refused=tools.call("inquiry.read_source",json!({"artifact_id":snapshot.id,"start":0,"limit":1})).await.unwrap_err();
     assert_eq!(refused.path,"/budget/tools");
     let got=&got["result"];
     ("analysis",json!({"evidence":[{"id":"e1","artifact_id":got["artifact_id"],"sha256":got["sha256"],"paragraph":got["paragraphs"][0]["paragraph"],"quote":"Synthetic"}],"judgments":[{"text":"Synthetic fixture only, not actual research","support":["e1"],"counter":[],"limitations":[LIMIT]}],"gaps":if self.loop_from==Some("analyze") && inv.visit==1 {vec!["Focused synthetic gap"]}else{vec![]},"decision":if self.loop_from==Some("analyze") && inv.visit==1 {"need_more"}else{"ready"}}))
    },
    "report"=>{
     let analysis=inv.artifacts.iter().find(|a|a.slot=="analysis").unwrap();
     let got=tools.call("artifact.read",json!({"id":analysis.id})).await?;
     let analysis:Value=serde_json::from_str(got["text"].as_str().unwrap())?;let mut citation=analysis["evidence"][0].clone();citation.as_object_mut().unwrap().remove("id");
     ("report",json!({"answer_status":"answered","conclusion":"Synthetic fixture only [1].","citations":[citation],"limitations":[LIMIT]}))
    },
    _=>return Err(Error::new("fixture_unexpected","/state","unexpected fixture state")),
   };
   tools.call("artifact.write",json!({"slot":slot,"value":out})).await?;Ok(())
  }.await;
        match result {
            Ok(()) => ExecutionOutcome::Completed {
                usage: json!({"model":null,"kind":"synthetic"}),
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
#[tokio::test]
async fn six_stages_accept_exact_evidence_while_preserving_known_limitations() {
    run_pipeline(None).await;
}
#[tokio::test]
async fn catalogue_and_analysis_reentry_receive_authorized_gaps_and_history() {
    for origin in ["catalogue", "analyze"] {
        run_pipeline(Some(origin)).await;
    }
}
async fn run_pipeline(loop_from: Option<&'static str>) {
    let root = tempfile::tempdir().unwrap();
    let d = domain::definition(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let digest = d.digest.clone();
    let mut bindings = domain::bindings(&d, Arc::new(SyntheticExecutor { loop_from }));
    bindings
        .tools
        .insert("inquiry.search".into(), Arc::new(SyntheticSearch));
    bindings
        .tools
        .insert("inquiry.fetch".into(), Arc::new(SyntheticFetch));
    let rt = Runtime::open(
        inquiry::budget::runtime_options(root.path().join("store")),
        vec![d],
        bindings,
    )
    .await
    .unwrap();
    let inputs = json!({"question":"Synthetic Integration fixture; no team measurements exist.","scope":"Synthetic only","seed_urls":["https://example.com/fixture"]});
    let id = rt
        .start(
            StartRequest {
                definition_digest: digest,
                inputs: inputs.clone(),
                grant_ref: "local-readonly".into(),
                request_key: "full-synthetic".into(),
                limits: domain::limits(),
            },
            &Caller::local(),
        )
        .await
        .unwrap();
    let mut completed = None;
    for _ in 0..200 {
        let snapshot = rt.get(&id, &Caller::local()).await.unwrap();
        if snapshot.status.terminal() {
            completed = Some(snapshot);
            break;
        }
        assert_ne!(snapshot.status, Status::Blocked, "{snapshot:?}");
        assert_ne!(snapshot.status, Status::Waiting, "{snapshot:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let snapshot = completed.expect("synthetic pipeline timeout");
    assert_eq!(snapshot.status, Status::Completed, "{snapshot:?}");
    assert_eq!(snapshot.business_outcome.as_deref(), Some("report_ready"));
    assert_eq!(snapshot.inputs, inputs);
    let states: BTreeSet<_> = snapshot
        .accepted_artifacts
        .iter()
        .map(|a| a.state_id.as_str())
        .collect();
    assert_eq!(
        states,
        BTreeSet::from([
            "scope",
            "discover",
            "capture",
            "catalogue",
            "analyze",
            "report"
        ])
    );
    let reference = snapshot
        .accepted_artifacts
        .iter()
        .find(|a| a.slot == "report")
        .unwrap();
    let report = rt
        .read_artifact(&reference.id, &Caller::local())
        .await
        .unwrap();
    let report: Value = serde_json::from_slice(&report.bytes).unwrap();
    assert_eq!(report["limitations"], json!([LIMIT]));
    assert_eq!(report["citations"].as_array().unwrap().len(), 1);
    let budgets = rt.budgets(&id, &Caller::local()).unwrap();
    assert_eq!(
        budgets.runs.len(),
        match loop_from {
            None => 6,
            Some("catalogue") => 9,
            Some("analyze") => 10,
            _ => unreachable!(),
        }
    );
    assert_eq!(budgets.task_limits.max_active_ms, 1_800_000);
    for run in budgets.runs {
        assert_eq!(run.policy_source, "entry");
        assert_eq!(run.policy.submission_reserve, 2);
        assert_eq!(run.policy.max_run_ms, None);
        assert_eq!(run.policy.max_check_ms, None);
        if run.state_id == "analyze" {
            assert_eq!(run.effective_tool_calls, 80);
            assert_eq!(run.calls_used, 79);
            assert_eq!(run.calls_rejected, 1);
            assert!(run.limits_hit.contains(&BudgetLimit::ExplorationCalls));
        } else {
            assert_eq!(run.effective_tool_calls, 40);
        }
    }
    rt.shutdown().await.unwrap();
}
