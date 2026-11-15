//! Revise must finish before the effective deadline. A late return is a failed run even when the report was written.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::Mutex;

/// Reads stop this long before the registered 300000 ms cap so one model turn can still submit.
pub const REVISE_FINALIZE_BEFORE_MS: u64 = 90_000;
/// Stop the model and, if it has not submitted, keep the accepted draft before that cap.
pub const REVISE_HOST_RESERVE_MS: u64 = 25_000;

pub struct ReviseGuard(pub Arc<dyn Executor>);

struct ReportBridge {
    inner: Arc<dyn ToolBridge>,
    report_written: AtomicBool,
    /// Serializes the one report write so a late draft cannot replace a report already stored.
    slot: Mutex<()>,
}

enum Settle {
    Forward,
    CompletedModel,
    CopyDraft,
}

fn settle(report_written: bool, missed: bool) -> Settle {
    if report_written && missed {
        Settle::CompletedModel
    } else if missed {
        Settle::CopyDraft
    } else {
        Settle::Forward
    }
}

fn missed_outcome(outcome: &ExecutionOutcome) -> bool {
    matches!(
        outcome,
        ExecutionOutcome::Failed { code, .. } if code == "limit_reached"
    ) || matches!(outcome, ExecutionOutcome::Cancelled)
}

fn accepted_draft(text: &str) -> Option<Value> {
    let value: Value = serde_json::from_str(text).ok()?;
    value.as_object()?;
    Some(value)
}

enum GuardFinish<T> {
    Model(T),
    Submitted,
    Fallback,
}

async fn watch_revise_submission<T: Send + 'static>(
    parent: &CancellationToken,
    child: &CancellationToken,
    submitted: &AtomicBool,
    should_fallback: impl Fn() -> bool,
    enabled: bool,
    model: impl std::future::Future<Output = T> + Send + 'static,
) -> GuardFinish<T> {
    let mut model = Box::pin(model);
    loop {
        tokio::select! {
            biased;
            outcome = &mut model => return GuardFinish::Model(outcome),
            _ = parent.cancelled() => {
                child.cancel();
                return GuardFinish::Model(model.await);
            }
            _ = tokio::time::sleep(Duration::from_millis(50)), if enabled && !child.is_cancelled() => {
                if submitted.load(Ordering::Acquire) {
                    child.cancel();
                    tokio::spawn(async move {
                        let _ = model.await;
                    });
                    return GuardFinish::Submitted;
                }
                if should_fallback() {
                    child.cancel();
                    tokio::spawn(async move {
                        let _ = model.await;
                    });
                    return GuardFinish::Fallback;
                }
            }
        }
    }
}

#[async_trait]
impl ToolBridge for ReportBridge {
    fn specs(&self) -> Vec<ToolSpec> {
        self.inner.specs()
    }
    fn goal_context(&self) -> Option<GoalRunContext> {
        self.inner.goal_context()
    }
    fn json_candidate_limit(&self) -> Option<usize> {
        self.inner.json_candidate_limit()
    }
    fn budget_hint(&self) -> Option<BudgetHint> {
        self.inner.budget_hint()
    }
    async fn reject_json_candidate(&self, bytes: u64, metadata: ToolCallMetadata) -> Result<Value> {
        self.inner.reject_json_candidate(bytes, metadata).await
    }
    async fn call(&self, id: &str, args: Value) -> Result<Value> {
        self.forward(id, args, None).await
    }
    async fn call_with_metadata(
        &self,
        id: &str,
        args: Value,
        metadata: ToolCallMetadata,
    ) -> Result<Value> {
        self.forward(id, args, Some(metadata)).await
    }
}

impl ReportBridge {
    async fn forward(
        &self,
        id: &str,
        args: Value,
        metadata: Option<ToolCallMetadata>,
    ) -> Result<Value> {
        if !(id == "artifact.write" && args["slot"] == "report") {
            return match metadata {
                Some(metadata) => self.inner.call_with_metadata(id, args, metadata).await,
                None => self.inner.call(id, args).await,
            };
        }
        let _guard = self.slot.lock().await;
        if self.report_written.load(Ordering::Acquire) {
            return Err(fault("report已经提交"));
        }
        let result = match metadata {
            Some(metadata) => self.inner.call_with_metadata(id, args, metadata).await?,
            None => self.inner.call(id, args).await?,
        };
        self.report_written.store(true, Ordering::Release);
        Ok(result)
    }
}

fn is_revise(tools: &Arc<dyn ToolBridge>) -> bool {
    matches!(
        tools.goal_context().map(|ctx| ctx.role),
        Some(GoalRunRole::NodeMember { node_id, .. }) if node_id == "revise"
    )
}

async fn saved_draft(inv: &Invocation, tools: &Arc<dyn ToolBridge>) -> Option<Value> {
    let draft = inv
        .artifacts
        .iter()
        .find(|artifact| artifact.slot == "draft")?;
    let response = tools
        .call("artifact.read", json!({"id": draft.id}))
        .await
        .ok()?;
    accepted_draft(response["text"].as_str()?)
}

impl ReviseGuard {
    async fn keep_draft(bridge: &ReportBridge, draft: Option<&Value>) -> ExecutionOutcome {
        let _guard = bridge.slot.lock().await;
        if bridge.report_written.load(Ordering::Acquire) {
            return ExecutionOutcome::Completed {
                usage: json!({"submission":"report_written","native_model":true}),
            };
        }
        let Some(draft) = draft else {
            return ExecutionOutcome::Failed {
                code: "limit_reached".into(),
                message: "修订未在时限内提交报告，且没有可沿用的已接受草稿".into(),
            };
        };
        match bridge
            .inner
            .call("artifact.write", json!({"slot":"report","value":draft}))
            .await
        {
            Ok(_) => {
                bridge.report_written.store(true, Ordering::Release);
                ExecutionOutcome::Completed {
                    usage: json!({"submission":"deadline_draft","native_model":true}),
                }
            }
            Err(e) => ExecutionOutcome::Failed {
                code: e.code,
                message: e.message,
            },
        }
    }

    async fn execute_revise(
        &self,
        inv: Invocation,
        tools: Arc<dyn ToolBridge>,
        cancel: CancellationToken,
        observer: Option<Arc<dyn MilkieObserver>>,
    ) -> ExecutionOutcome {
        let draft = saved_draft(&inv, &tools).await;
        let bridge = Arc::new(ReportBridge {
            inner: tools.clone(),
            report_written: AtomicBool::new(false),
            slot: Mutex::new(()),
        });
        let child = cancel.child_token();
        let executor = Arc::clone(&self.0);
        let model_bridge = bridge.clone();
        let model_cancel = child.clone();
        let model = async move {
            executor
                .execute_observed(inv, model_bridge, model_cancel, observer)
                .await
        };
        let finish = watch_revise_submission(
            &cancel,
            &child,
            &bridge.report_written,
            || {
                tools
                    .budget_hint()
                    .is_some_and(|hint| hint.remaining_ms <= REVISE_HOST_RESERVE_MS)
            },
            true,
            model,
        )
        .await;
        if cancel.is_cancelled() {
            return match finish {
                GuardFinish::Model(outcome) => outcome,
                _ => ExecutionOutcome::Cancelled,
            };
        }
        let model_report = ExecutionOutcome::Completed {
            usage: json!({"submission":"report_written","native_model":true}),
        };
        match finish {
            GuardFinish::Model(outcome) => match settle(
                bridge.report_written.load(Ordering::Acquire),
                missed_outcome(&outcome),
            ) {
                Settle::CompletedModel => model_report,
                Settle::CopyDraft => Self::keep_draft(&bridge, draft.as_ref()).await,
                Settle::Forward => outcome,
            },
            GuardFinish::Submitted => model_report,
            GuardFinish::Fallback => {
                match settle(bridge.report_written.load(Ordering::Acquire), true) {
                    Settle::CopyDraft => Self::keep_draft(&bridge, draft.as_ref()).await,
                    _ => model_report,
                }
            }
        }
    }
}

#[async_trait]
impl Executor for ReviseGuard {
    fn enforces_json_candidate_limit(&self) -> bool {
        self.0.enforces_json_candidate_limit()
    }
    fn budget_limits(&self) -> ExecutorBudgetLimits {
        self.0.budget_limits()
    }
    fn identity(&self) -> Value {
        json!({"adapter":self.0.identity(),"revise":"submit-before-deadline"})
    }
    fn capabilities(&self) -> ExecutorCapabilities {
        self.0.capabilities()
    }
    fn internal_observation(&self) -> bool {
        self.0.internal_observation()
    }
    async fn finish_observations(&self) -> Result<()> {
        self.0.finish_observations().await
    }
    async fn cleanup_observation(&self, id: &str) -> ObservationCleanup {
        self.0.cleanup_observation(id).await
    }
    async fn stop_or_confirm(&self, id: &str) -> StopState {
        self.0.stop_or_confirm(id).await
    }
    async fn execute(
        &self,
        inv: Invocation,
        tools: Arc<dyn ToolBridge>,
        cancel: CancellationToken,
    ) -> ExecutionOutcome {
        self.execute_observed(inv, tools, cancel, None).await
    }
    async fn execute_observed(
        &self,
        inv: Invocation,
        tools: Arc<dyn ToolBridge>,
        cancel: CancellationToken,
        observer: Option<Arc<dyn MilkieObserver>>,
    ) -> ExecutionOutcome {
        if !is_revise(&tools) {
            return self.0.execute_observed(inv, tools, cancel, observer).await;
        }
        self.execute_revise(inv, tools, cancel, observer).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_draft_keeps_existing_citations() {
        let text = r#"{"answer_status":"insufficient_evidence","conclusion":"已有判断","citations":[{"artifact_id":"artifact-1","sha256":"abc","source_id":"r1s1","paragraph":10,"quote":"原句"}],"limitations":[]}"#;
        let value = accepted_draft(text).unwrap();
        assert_eq!(value["citations"].as_array().unwrap().len(), 1);
        assert_eq!(value["citations"][0]["quote"], "原句");
        assert_eq!(value["citations"][0]["source_id"], "r1s1");
        assert!(accepted_draft("[]").is_none());
        assert!(accepted_draft("not-json").is_none());
    }

    #[tokio::test]
    async fn written_report_returns_without_waiting_for_the_model() {
        let parent = CancellationToken::new();
        let child = parent.child_token();
        let submitted = AtomicBool::new(true);
        let started = std::time::Instant::now();
        let finish = watch_revise_submission(
            &parent,
            &child,
            &submitted,
            || true,
            true,
            std::future::pending::<()>(),
        )
        .await;
        assert!(matches!(finish, GuardFinish::Submitted));
        assert!(child.is_cancelled());
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn reserve_falls_back_when_the_report_is_missing() {
        let parent = CancellationToken::new();
        let child = parent.child_token();
        let submitted = AtomicBool::new(false);
        let finish = watch_revise_submission(
            &parent,
            &child,
            &submitted,
            || true,
            true,
            std::future::pending::<()>(),
        )
        .await;
        assert!(matches!(finish, GuardFinish::Fallback));
        assert!(child.is_cancelled());
    }

    #[tokio::test]
    async fn disabled_watch_waits_for_the_model() {
        let parent = CancellationToken::new();
        let child = parent.child_token();
        let submitted = AtomicBool::new(false);
        let finish = watch_revise_submission(&parent, &child, &submitted, || true, false, async {
            "model"
        })
        .await;
        assert!(matches!(finish, GuardFinish::Model("model")));
        assert!(!child.is_cancelled());
    }

    #[test]
    fn written_report_then_deadline_still_completes() {
        let deadline = ExecutionOutcome::Failed {
            code: "limit_reached".into(),
            message: "execution deadline exceeded".into(),
        };
        assert!(matches!(
            settle(true, missed_outcome(&deadline)),
            Settle::CompletedModel
        ));
        assert!(matches!(
            settle(true, missed_outcome(&ExecutionOutcome::Cancelled)),
            Settle::CompletedModel
        ));
        assert!(matches!(
            settle(false, missed_outcome(&deadline)),
            Settle::CopyDraft
        ));
        let other = ExecutionOutcome::Failed {
            code: "harness_unavailable".into(),
            message: "down".into(),
        };
        assert!(matches!(
            settle(true, missed_outcome(&other)),
            Settle::Forward
        ));
        assert!(matches!(settle(true, true), Settle::CompletedModel));
        assert!(matches!(settle(false, true), Settle::CopyDraft));
    }

    struct Store {
        report: Mutex<Option<Value>>,
    }
    #[async_trait]
    impl ToolBridge for Store {
        fn specs(&self) -> Vec<ToolSpec> {
            Vec::new()
        }
        fn goal_context(&self) -> Option<GoalRunContext> {
            None
        }
        fn json_candidate_limit(&self) -> Option<usize> {
            None
        }
        fn budget_hint(&self) -> Option<BudgetHint> {
            None
        }
        async fn reject_json_candidate(&self, _: u64, _: ToolCallMetadata) -> Result<Value> {
            Ok(json!({}))
        }
        async fn call(&self, id: &str, args: Value) -> Result<Value> {
            assert_eq!(id, "artifact.write");
            *self.report.lock().await = Some(args["value"].clone());
            Ok(json!({}))
        }
    }

    fn bridge(store: Arc<Store>) -> ReportBridge {
        ReportBridge {
            inner: store,
            report_written: AtomicBool::new(false),
            slot: Mutex::new(()),
        }
    }

    #[tokio::test]
    async fn fallback_keeps_the_model_report() {
        let store = Arc::new(Store {
            report: Mutex::new(None),
        });
        let bridge = bridge(store.clone());
        bridge
            .call(
                "artifact.write",
                json!({"slot":"report","value":{"citations":[{"quote":"模型原句","source_id":"r1s1"}]}}),
            )
            .await
            .unwrap();
        let draft = json!({"citations":[{"quote":"草稿","source_id":"r1s1"},{"quote":"多出来的","source_id":"r1s2"}]});
        let outcome = ReviseGuard::keep_draft(&bridge, Some(&draft)).await;
        assert!(matches!(outcome, ExecutionOutcome::Completed { .. }));
        let saved = store.report.lock().await.clone().unwrap();
        assert_eq!(saved["citations"].as_array().unwrap().len(), 1);
        assert_eq!(saved["citations"][0]["quote"], "模型原句");
    }

    #[tokio::test]
    async fn fallback_copies_the_draft_without_adding_citations() {
        let store = Arc::new(Store {
            report: Mutex::new(None),
        });
        let bridge = bridge(store.clone());
        let draft = json!({"citations":[{"quote":"草稿原句","source_id":"r1s1"}]});
        let outcome = ReviseGuard::keep_draft(&bridge, Some(&draft)).await;
        match outcome {
            ExecutionOutcome::Completed { usage } => {
                assert_eq!(usage["submission"], "deadline_draft");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(store.report.lock().await.clone().unwrap(), draft);
        let rejected = bridge
            .call(
                "artifact.write",
                json!({"slot":"report","value":{"citations":[{"quote":"后来的","source_id":"r9s9"}]}}),
            )
            .await;
        assert!(rejected.is_err());
        assert_eq!(store.report.lock().await.clone().unwrap(), draft);
    }
}
