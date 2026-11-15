//! Playbook bindings resolve registered tiers. The test does not reimplement the choice.
use async_trait::async_trait;
use inquiry::{budget, domain, tier};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
};
use taskloop::*;
use tokio_util::sync::CancellationToken;

struct Idle;
#[async_trait]
impl ToolBridge for Idle {
    fn specs(&self) -> Vec<ToolSpec> {
        vec![]
    }
    async fn call(&self, id: &str, _: Value) -> Result<Value> {
        Err(Error::new(
            "fixture",
            id,
            "tier binding does not call tools",
        ))
    }
}
struct Recording {
    model: &'static str,
    log: Arc<Mutex<Vec<(String, String)>>>,
}
#[async_trait]
impl Executor for Recording {
    fn capabilities(&self) -> ExecutorCapabilities {
        ExecutorCapabilities::MANAGED
    }
    async fn execute(
        &self,
        invocation: Invocation,
        _: Arc<dyn ToolBridge>,
        _: CancellationToken,
    ) -> ExecutionOutcome {
        self.log
            .lock()
            .unwrap()
            .push((invocation.state_id, self.model.to_string()));
        ExecutionOutcome::Completed {
            usage: json!({"model": self.model}),
        }
    }
}
fn ordinary() -> Value {
    json!({"question":"ordinary facts","seed_urls":[],"scope":"public"})
}
async fn drive(
    bindings: &HostBindings,
    state: &str,
    entry: &EntryContract,
    inputs: Value,
    dir: &Path,
) -> ExecutionOutcome {
    let executor = bindings
        .executors
        .get(&entry.executor)
        .expect("playbook executor is bound");
    executor
        .execute(
            Invocation {
                stage_input: inputs.clone(),
                execution_id: "tier-binding".into(),
                run_id: format!("{state}-run"),
                state_id: state.into(),
                visit: 1,
                entry: entry.clone(),
                skills: BTreeMap::new(),
                output_schemas: BTreeMap::new(),
                inputs,
                artifacts: vec![],
                signal_facts: vec![],
                work_dir: dir.to_path_buf(),
                timeout_ms: 1_000,
                budget: None,
            },
            Arc::new(Idle),
            CancellationToken::new(),
        )
        .await
}
#[tokio::test]
async fn playbook_states_resolve_registered_tiers_and_one_hard_task_upgrades() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let definition = domain::definition(root).unwrap();
    let scope = definition.bundle.entries["inquiry.scope"].clone();
    let analyze = definition.bundle.entries["inquiry.analyze"].clone();
    assert_eq!(scope.executor, "research.routine");
    assert_eq!(analyze.executor, "research.higher");
    let log = Arc::new(Mutex::new(Vec::new()));
    let router: Arc<dyn Executor> = Arc::new(tier::RoutingExecutor {
        routine: Arc::new(Recording {
            model: tier::ROUTINE.model,
            log: log.clone(),
        }),
        higher: Arc::new(Recording {
            model: tier::HIGHER.model,
            log: log.clone(),
        }),
    });
    let bindings = domain::bindings(&definition, router);
    let tmp = tempfile::tempdir().unwrap();
    let runtime = Runtime::open(
        budget::runtime_options(tmp.path().join("store")),
        vec![definition.clone()],
        bindings.clone(),
    )
    .await
    .unwrap();
    runtime.shutdown().await.unwrap();

    drive(&bindings, "scope", &scope, ordinary(), tmp.path()).await;
    drive(&bindings, "analyze", &analyze, ordinary(), tmp.path()).await;
    let seen = log.lock().unwrap().clone();
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert_ne!(seen[0].1, seen[1].1);
    for (_, model) in &seen {
        assert!(
            tier::registered().iter().any(|tier| tier.model == model),
            "{model}"
        );
    }
    assert_eq!(seen[0], ("scope".into(), tier::ROUTINE.model.into()));
    assert_eq!(seen[1], ("analyze".into(), tier::HIGHER.model.into()));

    log.lock().unwrap().clear();
    let mut hard = ordinary();
    hard["difficulty"] = json!("hard");
    drive(&bindings, "scope", &scope, hard, tmp.path()).await;
    let upgraded = log.lock().unwrap().clone();
    assert_eq!(upgraded, vec![("scope".into(), tier::HIGHER.model.into())]);

    log.lock().unwrap().clear();
    drive(&bindings, "scope", &scope, ordinary(), tmp.path()).await;
    let stayed = log.lock().unwrap().clone();
    assert_eq!(stayed, vec![("scope".into(), tier::ROUTINE.model.into())]);

    let options = budget::runtime_options(tmp.path().join("budgets"));
    assert_eq!(options.entry_budgets["inquiry.analyze"].max_tool_calls, 80);
    assert_eq!(options.entry_budgets["inquiry.scope"].max_tool_calls, 40);

    let flow = serde_json::to_value(&definition.bundle.flow).unwrap();
    assert!(flow.get("difficulty").is_none());
    assert!(flow.get("model").is_none());
    let mut with_difficulty = flow.clone();
    with_difficulty["difficulty"] = json!("hard");
    assert!(serde_json::from_value::<Flow>(with_difficulty).is_err());
    let mut with_model = flow;
    with_model["model"] = json!(tier::HIGHER.model);
    assert!(serde_json::from_value::<Flow>(with_model).is_err());
    let mut entry = serde_json::to_value(&scope).unwrap();
    entry["model"] = json!(tier::HIGHER.model);
    assert!(serde_json::from_value::<EntryContract>(entry).is_err());
}
