//! Registered model tiers and the Inquiry difficulty choice.
//! TaskLoop dispatches an executor id; it does not judge difficulty or store a model.
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use taskloop::*;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tier {
    pub id: &'static str,
    pub model: &'static str,
    pub effort: &'static str,
}

pub const HIGHER: Tier = Tier {
    id: "higher",
    model: "gpt-6.1-sol",
    effort: "high",
};
pub const ROUTINE: Tier = Tier {
    id: "routine",
    model: "gpt-6-astra",
    effort: "medium",
};

pub fn registered() -> &'static [Tier] {
    &[HIGHER, ROUTINE]
}
pub fn by_pair(model: &str, effort: &str) -> Option<&'static Tier> {
    registered()
        .iter()
        .find(|tier| tier.model == model && tier.effort == effort)
}
pub fn model_known(model: &str) -> bool {
    registered().iter().any(|tier| tier.model == model)
}
pub fn default_for_executor(executor_id: &str) -> Option<&'static Tier> {
    if executor_id.ends_with(".higher") {
        Some(&HIGHER)
    } else if executor_id.ends_with(".routine") {
        Some(&ROUTINE)
    } else {
        None
    }
}
/// Returns the registered tier id's record. Only `difficulty == "hard"` upgrades a routine default.
pub fn select(executor_id: &str, inputs: &Value) -> Option<&'static Tier> {
    let default = default_for_executor(executor_id)?;
    if default.id == ROUTINE.id && inputs.get("difficulty").and_then(Value::as_str) == Some("hard")
    {
        Some(&HIGHER)
    } else {
        Some(default)
    }
}

/// One host executor registered under every playbook executor id.
/// The entry id and task facts choose the tier; the chosen inner performs the call.
pub struct RoutingExecutor {
    pub routine: Arc<dyn Executor>,
    pub higher: Arc<dyn Executor>,
}
#[async_trait]
impl Executor for RoutingExecutor {
    fn capabilities(&self) -> ExecutorCapabilities {
        let routine = self.routine.capabilities();
        let higher = self.higher.capabilities();
        ExecutorCapabilities {
            bounded_entry: routine.bounded_entry && higher.bounded_entry,
            collect_artifacts: routine.collect_artifacts && higher.collect_artifacts,
            cancel: routine.cancel && higher.cancel,
            isolated_workspace: routine.isolated_workspace && higher.isolated_workspace,
            isolated_context: routine.isolated_context && higher.isolated_context,
        }
    }
    async fn execute(
        &self,
        invocation: Invocation,
        tools: Arc<dyn ToolBridge>,
        cancel: CancellationToken,
    ) -> ExecutionOutcome {
        let Some(tier) = select(&invocation.entry.executor, &invocation.inputs) else {
            return ExecutionOutcome::Failed {
                code: "input_invalid".into(),
                message: "工作状态没有已登记档位".into(),
            };
        };
        let inner = if tier.id == HIGHER.id {
            &self.higher
        } else {
            &self.routine
        };
        inner.execute(invocation, tools, cancel).await
    }
}
