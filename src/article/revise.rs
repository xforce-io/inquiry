//! Required outputs must finish before the effective deadline. A late return is a failed run even when the artifact was written.
use super::*;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;
use tokio::sync::Mutex;

/// Reads stop this long before the registered stage cap so one model turn can still submit.
pub const REVISE_FINALIZE_BEFORE_MS: u64 = 90_000;
/// Stop the model and, if it has not submitted, keep an accepted draft or an honest gap before that cap.
pub const REVISE_HOST_RESERVE_MS: u64 = 25_000;
/// Two corpus indexes plus targeted pages. Further reads are refused so synthesis can still submit.
pub const SYNTHESIS_SOURCE_READS: u32 = 16;

pub struct ReviseGuard(pub Arc<dyn Executor>);

struct ReportBridge {
    inner: Arc<dyn ToolBridge>,
    output_slot: &'static str,
    report_written: AtomicBool,
    /// Set only by the host fallback, so a late model write cannot replace it.
    host_written: AtomicBool,
    /// Serializes the required slot so a host fallback cannot replace a model write.
    slot: Mutex<()>,
    source_reads: AtomicU32,
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
        if id == "inquiry.article_source" && self.output_slot == "analysis" {
            self.admit_synthesis_read()?;
        }
        if !(id == "artifact.write" && args["slot"] == self.output_slot) {
            let result = match metadata {
                Some(metadata) => self.inner.call_with_metadata(id, args, metadata).await?,
                None => self.inner.call(id, args).await?,
            };
            if id == "inquiry.article_source" && self.output_slot == "analysis" {
                self.source_reads.fetch_add(1, Ordering::Release);
            }
            return Ok(result);
        }
        let _guard = self.slot.lock().await;
        if self.host_written.load(Ordering::Acquire) {
            return Err(fault("该输出已经提交"));
        }
        let result = match metadata {
            Some(metadata) => self.inner.call_with_metadata(id, args, metadata).await?,
            None => self.inner.call(id, args).await?,
        };
        self.report_written.store(true, Ordering::Release);
        Ok(result)
    }

    fn admit_synthesis_read(&self) -> Result<()> {
        let reserved = self
            .inner
            .budget_hint()
            .is_some_and(|hint| hint.remaining_ms <= REVISE_FINALIZE_BEFORE_MS);
        if reserved || self.source_reads.load(Ordering::Acquire) >= SYNTHESIS_SOURCE_READS {
            return Err(fault(
                "综合分析的来源页已够提交判断，请立刻根据已读段落提交分析，不要继续翻页",
            ));
        }
        Ok(())
    }
}

fn output_slot(tools: &Arc<dyn ToolBridge>) -> Option<&'static str> {
    let role = tools.goal_context().map(|ctx| ctx.role)?;
    match role {
        GoalRunRole::NodeMember { node_id, .. } => match node_id.as_str() {
            "synthesize" => Some("analysis"),
            "draft" | "revise" => Some("report"),
            "review" => Some("evaluation"),
            _ => None,
        },
        _ => None,
    }
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
                message: "未在时限内提交报告，且没有可沿用的已接受草稿".into(),
            };
        };
        match bridge
            .inner
            .call("artifact.write", json!({"slot":"report","value":draft}))
            .await
        {
            Ok(_) => {
                bridge.host_written.store(true, Ordering::Release);
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

    async fn keep_analysis(bridge: &ReportBridge, questions: &[String]) -> ExecutionOutcome {
        let _guard = bridge.slot.lock().await;
        if bridge.report_written.load(Ordering::Acquire) {
            return ExecutionOutcome::Completed {
                usage: json!({"submission":"report_written","native_model":true}),
            };
        }
        let value = super::discovery::honest_need_input(questions);
        match bridge
            .inner
            .call("artifact.write", json!({"slot":"analysis","value":value}))
            .await
        {
            Ok(_) => {
                bridge.host_written.store(true, Ordering::Release);
                bridge.report_written.store(true, Ordering::Release);
                ExecutionOutcome::Completed {
                    usage: json!({"submission":"deadline_need_input","native_model":true}),
                }
            }
            Err(e) => ExecutionOutcome::Failed {
                code: e.code,
                message: e.message,
            },
        }
    }

    async fn host_fallback(
        bridge: &ReportBridge,
        draft: Option<&Value>,
        questions: &[String],
        missed: Option<ExecutionOutcome>,
    ) -> ExecutionOutcome {
        match bridge.output_slot {
            "report" => Self::keep_draft(bridge, draft).await,
            "analysis" => Self::keep_analysis(bridge, questions).await,
            _ => missed.unwrap_or(ExecutionOutcome::Failed {
                code: "limit_reached".into(),
                message: "工作状态未在时限内提交，宿主不编造评审".into(),
            }),
        }
    }

    async fn execute_revise(
        &self,
        inv: Invocation,
        tools: Arc<dyn ToolBridge>,
        cancel: CancellationToken,
        observer: Option<Arc<dyn MilkieObserver>>,
    ) -> ExecutionOutcome {
        let slot = output_slot(&tools).unwrap_or("report");
        let draft = if slot == "report" {
            saved_draft(&inv, &tools).await
        } else {
            None
        };
        let questions = if slot == "analysis" {
            super::discovery::plan_subquestions(&inv, &tools).await
        } else {
            Vec::new()
        };
        let bridge = Arc::new(ReportBridge {
            inner: tools.clone(),
            output_slot: slot,
            report_written: AtomicBool::new(false),
            host_written: AtomicBool::new(false),
            slot: Mutex::new(()),
            source_reads: AtomicU32::new(0),
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
            GuardFinish::Model(outcome) => {
                let missed = missed_outcome(&outcome);
                match settle(bridge.report_written.load(Ordering::Acquire), missed) {
                    Settle::CompletedModel => model_report,
                    Settle::CopyDraft => {
                        Self::host_fallback(&bridge, draft.as_ref(), &questions, Some(outcome))
                            .await
                    }
                    Settle::Forward => outcome,
                }
            }
            GuardFinish::Submitted => model_report,
            GuardFinish::Fallback => {
                match settle(bridge.report_written.load(Ordering::Acquire), true) {
                    Settle::CopyDraft => {
                        Self::host_fallback(&bridge, draft.as_ref(), &questions, None).await
                    }
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
        if output_slot(&tools).is_none() {
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
            output_slot: "report",
            report_written: AtomicBool::new(false),
            host_written: AtomicBool::new(false),
            slot: Mutex::new(()),
            source_reads: AtomicU32::new(0),
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

    struct SlotTools {
        node: &'static str,
        remaining_ms: u64,
        draft_text: Option<String>,
        writes: Mutex<Vec<Value>>,
        reads: AtomicU32,
    }
    #[async_trait]
    impl ToolBridge for SlotTools {
        fn specs(&self) -> Vec<ToolSpec> {
            Vec::new()
        }
        fn goal_context(&self) -> Option<GoalRunContext> {
            Some(GoalRunContext {
                goal_id: "guard".into(),
                plan_digest: None,
                role: GoalRunRole::NodeMember {
                    node_id: self.node.into(),
                    member_id: "writer".into(),
                },
                evaluation_id: None,
                epoch: 1,
            })
        }
        fn json_candidate_limit(&self) -> Option<usize> {
            None
        }
        fn budget_hint(&self) -> Option<BudgetHint> {
            Some(BudgetHint {
                limit: 80,
                used: 1,
                remaining: 79,
                read_remaining: 0,
                submission_reserve: 2,
                remaining_ms: self.remaining_ms,
                mode: "finalize".into(),
            })
        }
        async fn reject_json_candidate(&self, _: u64, _: ToolCallMetadata) -> Result<Value> {
            Ok(json!({}))
        }
        async fn call(&self, id: &str, args: Value) -> Result<Value> {
            match id {
                "artifact.write" => {
                    self.writes.lock().await.push(args);
                    Ok(json!({}))
                }
                "artifact.read" => Ok(json!({"text": self.draft_text.clone().unwrap_or_default()})),
                "inquiry.article_source" => {
                    self.reads.fetch_add(1, Ordering::Release);
                    Ok(json!({"ok": true}))
                }
                _ => Err(fault("测试桥不接受该工具")),
            }
        }
    }
    struct Scripted {
        write: Option<Value>,
    }
    #[async_trait]
    impl Executor for Scripted {
        fn capabilities(&self) -> ExecutorCapabilities {
            ExecutorCapabilities::MANAGED
        }
        async fn execute(
            &self,
            _: Invocation,
            tools: Arc<dyn ToolBridge>,
            cancel: CancellationToken,
        ) -> ExecutionOutcome {
            if let Some(args) = &self.write {
                tools.call("artifact.write", args.clone()).await.unwrap();
                return ExecutionOutcome::Failed {
                    code: "limit_reached".into(),
                    message: "execution deadline exceeded".into(),
                };
            }
            cancel.cancelled().await;
            ExecutionOutcome::Cancelled
        }
    }
    fn invocation(artifacts: Vec<ArtifactRef>) -> Invocation {
        Invocation {
            stage_input: json!({}),
            execution_id: "submit-guard".into(),
            run_id: "run".into(),
            state_id: "synthesize".into(),
            visit: 1,
            entry: EntryContract {
                instruction: String::new(),
                executor: "article.agent".into(),
                skills: Vec::new(),
                tools: Vec::new(),
                input_schema: String::new(),
                requires: Vec::new(),
                outputs: BTreeMap::new(),
                attachments: None,
                gate: String::new(),
                outcomes: Vec::new(),
            },
            skills: BTreeMap::new(),
            output_schemas: BTreeMap::new(),
            inputs: json!({}),
            artifacts,
            signal_facts: Vec::new(),
            work_dir: std::env::temp_dir(),
            timeout_ms: 1_000,
            budget: None,
        }
    }
    async fn drive(
        node: &'static str,
        remaining_ms: u64,
        write: Option<Value>,
        draft: Option<Value>,
    ) -> (ExecutionOutcome, Vec<Value>) {
        let tools = Arc::new(SlotTools {
            node,
            remaining_ms,
            draft_text: draft.as_ref().map(|value| value.to_string()),
            writes: Mutex::new(Vec::new()),
            reads: AtomicU32::new(0),
        });
        let artifacts = if draft.is_some() {
            vec![ArtifactRef {
                id: "draft-1".into(),
                sha256: "abc".into(),
                slot: "draft".into(),
                state_id: "draft".into(),
                visit: 1,
            }]
        } else {
            Vec::new()
        };
        let guard = ReviseGuard(Arc::new(Scripted { write }));
        let bridge: Arc<dyn ToolBridge> = tools.clone();
        let outcome = guard
            .execute_observed(
                invocation(artifacts),
                bridge,
                CancellationToken::new(),
                None,
            )
            .await;
        let writes = tools.writes.lock().await.clone();
        (outcome, writes)
    }

    #[tokio::test]
    async fn written_analysis_survives_the_deadline_without_replacement() {
        let analysis = json!({
            "decision": "ready",
            "evidence": [{
                "artifact_id": "artifact-1",
                "sha256": "abc",
                "source_id": "r1s1",
                "paragraph": 4,
                "quote": "已读原句"
            }],
            "judgments": [],
            "gaps": [],
            "coverage": [],
            "search_queries": []
        });
        let (outcome, writes) = drive(
            "synthesize",
            400_000,
            Some(json!({"slot":"analysis","value":analysis})),
            None,
        )
        .await;
        match outcome {
            ExecutionOutcome::Completed { usage } => {
                assert_eq!(usage["submission"], "report_written");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0]["slot"], "analysis");
        assert_eq!(writes[0]["value"]["decision"], "ready");
        assert_eq!(writes[0]["value"]["evidence"][0]["quote"], "已读原句");
        assert_eq!(writes[0]["value"]["evidence"][0]["source_id"], "r1s1");
    }

    #[tokio::test]
    async fn missing_analysis_records_need_input_without_evidence() {
        let (outcome, writes) = drive("synthesize", REVISE_HOST_RESERVE_MS, None, None).await;
        match outcome {
            ExecutionOutcome::Completed { usage } => {
                assert_eq!(usage["submission"], "deadline_need_input");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(writes.len(), 1);
        let value = &writes[0]["value"];
        assert_eq!(writes[0]["slot"], "analysis");
        assert_eq!(value["decision"], "need_input");
        assert!(value["evidence"].as_array().unwrap().is_empty());
        assert!(!value.to_string().contains("https://"));
        assert!(value["citations"].is_null());
    }

    #[tokio::test]
    async fn missing_review_and_draft_are_not_invented() {
        for node in ["review", "draft"] {
            let (outcome, writes) = drive(node, 0, None, None).await;
            assert!(
                matches!(outcome, ExecutionOutcome::Failed { ref code, .. } if code == "limit_reached"),
                "{node} invented a completion: {outcome:?}"
            );
            assert!(writes.is_empty(), "{node} wrote {writes:?}");
        }
    }

    #[tokio::test]
    async fn revise_reserve_copies_the_accepted_draft_unchanged() {
        let draft = json!({
            "answer_status": "insufficient_evidence",
            "conclusion": "已有判断",
            "citations": [{
                "artifact_id": "artifact-1",
                "sha256": "abc",
                "source_id": "r1s1",
                "paragraph": 10,
                "quote": "草稿原句"
            }],
            "limitations": []
        });
        let (outcome, writes) = drive("revise", 0, None, Some(draft.clone())).await;
        match outcome {
            ExecutionOutcome::Completed { usage } => {
                assert_eq!(usage["submission"], "deadline_draft");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0]["value"], draft);
        assert_eq!(writes[0]["value"]["citations"].as_array().unwrap().len(), 1);
    }

    fn saved_analysis() -> Value {
        json!({
            "slot": "analysis",
            "value": {
                "decision": "ready",
                "evidence": [{
                    "artifact_id": "artifact-1",
                    "sha256": "abc",
                    "source_id": "r2s7",
                    "paragraph": 26,
                    "quote": "已读原句"
                }],
                "judgments": [],
                "gaps": [],
                "coverage": [],
                "search_queries": []
            }
        })
    }

    struct PagingSynthesis {
        stop_at_cap: bool,
        refuse_immediately: bool,
        write: Option<Value>,
    }
    #[async_trait]
    impl Executor for PagingSynthesis {
        fn capabilities(&self) -> ExecutorCapabilities {
            ExecutorCapabilities::MANAGED
        }
        async fn execute(
            &self,
            _: Invocation,
            tools: Arc<dyn ToolBridge>,
            _: CancellationToken,
        ) -> ExecutionOutcome {
            let attempts = if self.refuse_immediately {
                1
            } else {
                SYNTHESIS_SOURCE_READS + 1
            };
            for index in 0..attempts {
                let result = tools
                    .call("inquiry.article_source", json!({"start": index}))
                    .await;
                let refused = self.refuse_immediately
                    || (self.stop_at_cap && index == SYNTHESIS_SOURCE_READS);
                if refused {
                    let err = result.expect_err("synthesis must refuse this source read");
                    assert!(err.to_string().contains("请立刻根据已读段落提交分析"));
                } else {
                    result.expect("source read reaches the tool");
                }
            }
            if let Some(args) = &self.write {
                tools
                    .call("artifact.write", args.clone())
                    .await
                    .expect("analysis write");
                return ExecutionOutcome::Failed {
                    code: "limit_reached".into(),
                    message: "execution deadline exceeded".into(),
                };
            }
            ExecutionOutcome::Completed {
                usage: json!({"submission": "model"}),
            }
        }
    }

    async fn page(
        node: &'static str,
        remaining_ms: u64,
        paging: PagingSynthesis,
    ) -> (ExecutionOutcome, Vec<Value>, u32) {
        let tools = Arc::new(SlotTools {
            node,
            remaining_ms,
            draft_text: None,
            writes: Mutex::new(Vec::new()),
            reads: AtomicU32::new(0),
        });
        let guard = ReviseGuard(Arc::new(paging));
        let outcome = guard
            .execute_observed(
                invocation(Vec::new()),
                tools.clone(),
                CancellationToken::new(),
                None,
            )
            .await;
        let writes = tools.writes.lock().await.clone();
        let reads = tools.reads.load(Ordering::Acquire);
        (outcome, writes, reads)
    }

    #[tokio::test]
    async fn synthesis_stops_after_sixteen_source_reads_and_keeps_the_analysis() {
        let (outcome, writes, reads) = page(
            "synthesize",
            400_000,
            PagingSynthesis {
                stop_at_cap: true,
                refuse_immediately: false,
                write: Some(saved_analysis()),
            },
        )
        .await;
        match outcome {
            ExecutionOutcome::Completed { usage } => {
                assert_eq!(usage["submission"], "report_written");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(reads, SYNTHESIS_SOURCE_READS);
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0]["value"]["decision"], "ready");
        assert_eq!(writes[0]["value"]["evidence"][0]["quote"], "已读原句");
        assert_eq!(writes[0]["value"]["evidence"][0]["source_id"], "r2s7");
        assert!(
            !writes[0]["value"]["evidence"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn synthesis_refuses_a_source_read_inside_the_finalize_window() {
        let (outcome, writes, reads) = page(
            "synthesize",
            REVISE_FINALIZE_BEFORE_MS,
            PagingSynthesis {
                stop_at_cap: false,
                refuse_immediately: true,
                write: Some(saved_analysis()),
            },
        )
        .await;
        match outcome {
            ExecutionOutcome::Completed { usage } => {
                assert_eq!(usage["submission"], "report_written");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(reads, 0);
        assert_eq!(writes[0]["value"]["evidence"][0]["paragraph"], 26);
    }

    #[tokio::test]
    async fn draft_source_reads_are_not_capped_at_the_synthesis_limit() {
        let (outcome, writes, reads) = page(
            "draft",
            400_000,
            PagingSynthesis {
                stop_at_cap: false,
                refuse_immediately: false,
                write: None,
            },
        )
        .await;
        assert!(matches!(outcome, ExecutionOutcome::Completed { .. }));
        assert_eq!(reads, SYNTHESIS_SOURCE_READS + 1);
        assert!(writes.is_empty());
    }
}
