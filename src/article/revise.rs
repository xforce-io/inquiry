//! Required outputs must finish before the effective deadline. A late return is a failed run even when the artifact was written.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::Mutex;

/// Reads stop this long before the registered stage cap so one model turn can still submit.
pub const REVISE_FINALIZE_BEFORE_MS: u64 = 90_000;
/// Stop the model and, if it has not submitted, keep an accepted draft or an honest gap before that cap.
pub const REVISE_HOST_RESERVE_MS: u64 = 25_000;
/// Distinct saved sources synthesis may open. Repeats and source indexes do not consume this.
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
    opened_sources: std::sync::Mutex<BTreeSet<String>>,
    /// Unopened sources from the latest non-empty keyword index.
    /// The paragraph, when present, is the index the first open must cover. Empty hits do not clear this map.
    pending_hits: std::sync::Mutex<BTreeMap<String, Option<u64>>>,
    /// One successful keyword index with a non-empty keyword and no source id.
    keyword_probed: AtomicBool,
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
        let source_id = args["source_id"]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let keyword = args["keyword"].as_str().unwrap_or("").trim().to_owned();
        if id == "inquiry.article_source"
            && self.output_slot == "evaluation"
            && let Some(refusal) = self.admit_review_read()
        {
            return Ok(refusal);
        }
        if id == "inquiry.article_source"
            && self.output_slot == "analysis"
            && let Some(refusal) = self.admit_synthesis_read(&args)
        {
            return Ok(refusal);
        }
        if !(id == "artifact.write" && args["slot"] == self.output_slot) {
            let result = match metadata {
                Some(metadata) => self.inner.call_with_metadata(id, args, metadata).await?,
                None => self.inner.call(id, args).await?,
            };
            if id == "inquiry.article_source" && self.output_slot == "analysis" {
                if let Some(source_id) = source_id {
                    self.opened_sources
                        .lock()
                        .expect("opened sources")
                        .insert(source_id.clone());
                    self.pending_hits
                        .lock()
                        .expect("pending hits")
                        .remove(&source_id);
                } else if !keyword.is_empty() {
                    self.keyword_probed.store(true, Ordering::Release);
                    if let Some(ids) = keyword_hit_ids(&result)
                        && !ids.is_empty()
                    {
                        let opened = self.opened_sources.lock().expect("opened sources");
                        let pending = ids
                            .into_iter()
                            .filter(|(id, _)| !opened.contains(id))
                            .collect();
                        drop(opened);
                        *self.pending_hits.lock().expect("pending hits") = pending;
                    }
                }
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

    fn admit_review_read(&self) -> Option<Value> {
        let reserved = self
            .inner
            .budget_hint()
            .is_some_and(|hint| hint.remaining_ms <= REVISE_FINALIZE_BEFORE_MS);
        if reserved {
            return Some(model_refusal(
                "评审的来源页已够提交判断，请立刻提交评审，不要继续翻页",
            ));
        }
        None
    }

    fn admit_synthesis_read(&self, args: &Value) -> Option<Value> {
        let reserved = self
            .inner
            .budget_hint()
            .is_some_and(|hint| hint.remaining_ms <= REVISE_FINALIZE_BEFORE_MS);
        if reserved {
            return Some(model_refusal(
                "综合分析的来源页已够提交判断，请立刻根据已读段落提交分析，不要继续翻页",
            ));
        }
        let source_id = args["source_id"]
            .as_str()
            .filter(|value| !value.is_empty())?;
        let opened = self.opened_sources.lock().expect("opened sources");
        if opened.contains(source_id) {
            return Some(model_refusal(
                "该来源已经读过，请改读尚未打开的已保存来源；不要把未读来源写成缺口",
            ));
        }
        if !self.keyword_probed.load(Ordering::Acquire) {
            return Some(model_refusal(
                "请先做一次不带来源编号、且关键词非空的来源索引，再打开来源页。不带关键词的完整索引不算",
            ));
        }
        let pending = self.pending_hits.lock().expect("pending hits");
        if !pending.is_empty()
            && !pending.contains_key(source_id)
            && opened.len() < SYNTHESIS_SOURCE_READS as usize
        {
            return Some(pending_refusal(&pending));
        }
        let required = pending.get(source_id).copied().flatten();
        let at_cap = opened.len() >= SYNTHESIS_SOURCE_READS as usize;
        drop(pending);
        drop(opened);
        if at_cap {
            return Some(model_refusal(
                "综合分析的来源页已够提交判断，请立刻根据已读段落提交分析，不要继续翻页",
            ));
        }
        if let Some(paragraph) = required
            && !page_covers(args, paragraph)
        {
            return Some(paragraph_refusal(source_id, paragraph));
        }
        None
    }
}

fn keyword_hit_ids(value: &Value) -> Option<BTreeMap<String, Option<u64>>> {
    let body = if value.get("hits").is_some() {
        value
    } else if value
        .get("result")
        .is_some_and(|result| result.get("hits").is_some())
    {
        &value["result"]
    } else {
        return None;
    };
    let hits = body.get("hits")?.as_array()?;
    let mut ids = BTreeMap::new();
    for hit in hits {
        if let Some(id) = hit
            .get("source_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        {
            let paragraph = hit.get("paragraph").and_then(Value::as_u64);
            ids.entry(id.to_owned()).or_insert(paragraph);
        }
    }
    Some(ids)
}

fn page_covers(args: &Value, paragraph: u64) -> bool {
    if args["keyword"]
        .as_str()
        .is_some_and(|keyword| !keyword.trim().is_empty())
    {
        return false;
    }
    let start = args["start"].as_u64().unwrap_or(0);
    let limit = args["limit"].as_u64().unwrap_or(5);
    if !(1..=8).contains(&limit) {
        return false;
    }
    start <= paragraph && paragraph < start.saturating_add(limit)
}

fn paragraph_refusal(source_id: &str, paragraph: u64) -> Value {
    model_refusal(format!(
        "请打开 {source_id} 的第 {paragraph} 段：不要加页内关键词，并且本次 start 到 start+limit 必须包含该段（limit 为 1 到 8，省略时为 5）。未盖住该段不算已经读过"
    ))
}

fn pending_refusal(pending: &BTreeMap<String, Option<u64>>) -> Value {
    let shown: Vec<&str> = pending.keys().map(String::as_str).take(8).collect();
    let mut list = shown.join("、");
    if pending.len() > shown.len() {
        list.push_str(&format!("等{}个", pending.len()));
    }
    model_refusal(format!(
        "关键词索引已经指出尚未打开的来源，请先打开它们：{list}。不要先读索引未指出的来源"
    ))
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

/// Accepted draft reports are published on slot `report`. The draft node is a
/// `state_id` path segment (`_tl.node.draft`), not the consumer input name.
fn is_accepted_draft(artifact: &ArtifactRef) -> bool {
    artifact.slot == "draft"
        || (artifact.slot == "report" && artifact.state_id.split('.').any(|part| part == "draft"))
}

async fn saved_draft(inv: &Invocation, tools: &Arc<dyn ToolBridge>) -> Option<Value> {
    let draft = inv
        .artifacts
        .iter()
        .find(|artifact| is_accepted_draft(artifact))?;
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
            opened_sources: std::sync::Mutex::new(BTreeSet::new()),
            pending_hits: std::sync::Mutex::new(BTreeMap::new()),
            keyword_probed: AtomicBool::new(false),
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
    use std::sync::atomic::AtomicU32;

    fn shown_refusal(result: Result<Value>) -> String {
        let value = result.expect("the refusal sentence reaches the caller");
        assert_eq!(value["ok"], false);
        assert_eq!(value["code"], "rejected");
        value["message"]
            .as_str()
            .expect("refusal message")
            .to_owned()
    }

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
            opened_sources: std::sync::Mutex::new(BTreeSet::new()),
            pending_hits: std::sync::Mutex::new(BTreeMap::new()),
            keyword_probed: AtomicBool::new(false),
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
                "artifact.read" => {
                    let id = args.get("id").and_then(Value::as_str).unwrap_or("");
                    let text = if id == "other-report" {
                        "{\"answer_status\":\"answered\",\"citations\":[{\"quote\":\"别的节点\",\"source_id\":\"r9s9\",\"paragraph\":1,\"artifact_id\":\"other\",\"sha256\":\"fff\"}]}".to_string()
                    } else {
                        self.draft_text.clone().unwrap_or_default()
                    };
                    Ok(json!({"text": text}))
                }
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
    fn artifact_ref(id: &str, slot: &str, state_id: &str) -> ArtifactRef {
        ArtifactRef {
            id: id.into(),
            sha256: "abc".into(),
            slot: slot.into(),
            state_id: state_id.into(),
            visit: 1,
        }
    }
    async fn drive(
        node: &'static str,
        remaining_ms: u64,
        write: Option<Value>,
        draft: Option<Value>,
    ) -> (ExecutionOutcome, Vec<Value>) {
        let artifacts = if draft.is_some() {
            vec![artifact_ref("draft-1", "draft", "draft")]
        } else {
            Vec::new()
        };
        drive_with(node, remaining_ms, write, draft, artifacts).await
    }
    async fn drive_with(
        node: &'static str,
        remaining_ms: u64,
        write: Option<Value>,
        draft: Option<Value>,
        artifacts: Vec<ArtifactRef>,
    ) -> (ExecutionOutcome, Vec<Value>) {
        let tools = Arc::new(SlotTools {
            node,
            remaining_ms,
            draft_text: draft.as_ref().map(|value| value.to_string()),
            writes: Mutex::new(Vec::new()),
            reads: AtomicU32::new(0),
        });
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

    fn public_draft() -> Value {
        json!({
            "answer_status": "answered",
            "conclusion": "已有判断",
            "citations": [{
                "artifact_id": "artifact-1",
                "sha256": "abc",
                "source_id": "r1s1",
                "paragraph": 10,
                "quote": "草稿原句"
            }],
            "limitations": []
        })
    }

    #[tokio::test]
    async fn revise_reserve_copies_the_public_draft_report_unchanged() {
        let draft = public_draft();
        for state_id in ["_tl.node.draft", "_tl.node.draft.member.worker"] {
            let (outcome, writes) = drive_with(
                "revise",
                0,
                None,
                Some(draft.clone()),
                vec![artifact_ref("draft-1", "report", state_id)],
            )
            .await;
            match outcome {
                ExecutionOutcome::Completed { usage } => {
                    assert_eq!(usage["submission"], "deadline_draft", "{state_id}");
                }
                other => panic!("{state_id} unexpected {other:?}"),
            }
            assert_eq!(writes.len(), 1, "{state_id}");
            assert_eq!(writes[0]["slot"], "report");
            assert_eq!(writes[0]["value"], draft, "{state_id}");
            assert_eq!(writes[0]["value"]["citations"].as_array().unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn revise_reserve_skips_another_nodes_report_and_copies_the_draft() {
        let draft = public_draft();
        let (outcome, writes) = drive_with(
            "revise",
            0,
            None,
            Some(draft.clone()),
            vec![
                artifact_ref("other-report", "report", "_tl.node.revise"),
                artifact_ref("draft-1", "report", "_tl.node.draft"),
            ],
        )
        .await;
        match outcome {
            ExecutionOutcome::Completed { usage } => {
                assert_eq!(usage["submission"], "deadline_draft");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0]["value"], draft);
        assert_ne!(writes[0]["value"]["citations"][0]["quote"], "别的节点");
    }

    #[tokio::test]
    async fn revise_reserve_does_not_copy_a_report_from_another_node() {
        let draft = public_draft();
        for state_id in [
            "_tl.node.collect",
            "_tl.node.revise",
            "_tl.node.review",
            "_tl.node.drafting",
        ] {
            let (outcome, writes) = drive_with(
                "revise",
                0,
                None,
                Some(draft.clone()),
                vec![artifact_ref("other-report", "report", state_id)],
            )
            .await;
            match outcome {
                ExecutionOutcome::Failed { code, message } => {
                    assert_eq!(code, "limit_reached", "{state_id}");
                    assert_eq!(message, "未在时限内提交报告，且没有可沿用的已接受草稿");
                }
                other => panic!("{state_id} copied a report: {other:?}"),
            }
            assert!(writes.is_empty(), "{state_id} wrote {writes:?}");
        }
    }

    #[tokio::test]
    async fn written_revise_report_survives_when_a_public_draft_exists() {
        let draft = public_draft();
        let report = json!({
            "answer_status": "answered",
            "conclusion": "模型已交",
            "citations": [{
                "artifact_id": "artifact-1",
                "sha256": "abc",
                "source_id": "r1s1",
                "paragraph": 10,
                "quote": "模型原句"
            }],
            "limitations": []
        });
        let (outcome, writes) = drive_with(
            "revise",
            400_000,
            Some(json!({"slot":"report","value":report})),
            Some(draft),
            vec![artifact_ref("draft-1", "report", "_tl.node.draft")],
        )
        .await;
        match outcome {
            ExecutionOutcome::Completed { usage } => {
                assert_eq!(usage["submission"], "report_written");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0]["value"]["citations"][0]["quote"], "模型原句");
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
        index_first: bool,
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
            if self.index_first {
                tools
                    .call("inquiry.article_source", json!({"keyword": "probe"}))
                    .await
                    .expect("keyword index before pages");
            }
            for index in 0..attempts {
                let result = tools
                    .call(
                        "inquiry.article_source",
                        json!({"source_id": format!("r1s{}", index + 1), "start": 0}),
                    )
                    .await;
                let refused = self.refuse_immediately
                    || (self.stop_at_cap && index == SYNTHESIS_SOURCE_READS);
                if refused {
                    let message = shown_refusal(result);
                    assert!(message.contains("请立刻根据已读段落提交分析"));
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
                index_first: true,
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
        assert_eq!(reads, SYNTHESIS_SOURCE_READS + 1);
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
                index_first: false,
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

    struct ReviewProbe {
        expect_refuse: bool,
    }
    #[async_trait]
    impl Executor for ReviewProbe {
        fn capabilities(&self) -> ExecutorCapabilities {
            ExecutorCapabilities::MANAGED
        }
        async fn execute(
            &self,
            _: Invocation,
            tools: Arc<dyn ToolBridge>,
            _: CancellationToken,
        ) -> ExecutionOutcome {
            let result = tools
                .call(
                    "inquiry.article_source",
                    json!({"source_id": "r1s5", "start": 0, "limit": 5}),
                )
                .await;
            if self.expect_refuse {
                let message = shown_refusal(result);
                assert!(message.contains("请立刻提交评审"));
                tools
                    .call(
                        "artifact.write",
                        json!({"slot": "evaluation", "value": {"verdict": "fail"}}),
                    )
                    .await
                    .expect("evaluation write");
            } else {
                result.expect("review source read reaches the tool");
            }
            ExecutionOutcome::Completed {
                usage: json!({"submission": "model"}),
            }
        }
    }

    async fn review_page(
        remaining_ms: u64,
        expect_refuse: bool,
    ) -> (ExecutionOutcome, Vec<Value>, u32) {
        let tools = Arc::new(SlotTools {
            node: "review",
            remaining_ms,
            draft_text: None,
            writes: Mutex::new(Vec::new()),
            reads: AtomicU32::new(0),
        });
        let guard = ReviseGuard(Arc::new(ReviewProbe { expect_refuse }));
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
    async fn review_refuses_a_source_read_inside_the_finalize_window() {
        let (outcome, writes, reads) = review_page(REVISE_FINALIZE_BEFORE_MS, true).await;
        assert!(matches!(outcome, ExecutionOutcome::Completed { .. }));
        assert_eq!(reads, 0);
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0]["slot"], "evaluation");
        assert_eq!(
            REVISE_FINALIZE_BEFORE_MS, 90_000,
            "the review window stays the existing finalize threshold"
        );
    }

    #[tokio::test]
    async fn review_source_read_outside_the_finalize_window_reaches_the_tool() {
        let (outcome, writes, reads) = review_page(REVISE_FINALIZE_BEFORE_MS + 1, false).await;
        assert!(matches!(outcome, ExecutionOutcome::Completed { .. }));
        assert_eq!(reads, 1);
        assert!(writes.is_empty());
    }

    #[tokio::test]
    async fn draft_source_reads_are_not_capped_at_the_synthesis_limit() {
        let (outcome, writes, reads) = page(
            "draft",
            400_000,
            PagingSynthesis {
                stop_at_cap: false,
                refuse_immediately: false,
                index_first: false,
                write: None,
            },
        )
        .await;
        assert!(matches!(outcome, ExecutionOutcome::Completed { .. }));
        assert_eq!(reads, SYNTHESIS_SOURCE_READS + 1);
        assert!(writes.is_empty());
    }

    struct Breadth;
    #[async_trait]
    impl Executor for Breadth {
        fn capabilities(&self) -> ExecutorCapabilities {
            ExecutorCapabilities::MANAGED
        }
        async fn execute(
            &self,
            _: Invocation,
            tools: Arc<dyn ToolBridge>,
            _: CancellationToken,
        ) -> ExecutionOutcome {
            tools
                .call("inquiry.article_source", json!({"keyword": "probe"}))
                .await
                .expect("a keyword index must precede source pages");
            tools
                .call("inquiry.article_source", json!({"source_id": "r1s1"}))
                .await
                .unwrap();
            let repeated = shown_refusal(
                tools
                    .call(
                        "inquiry.article_source",
                        json!({"source_id": "r1s1", "start": 8}),
                    )
                    .await,
            );
            assert!(repeated.contains("尚未打开的已保存来源"));
            tools
                .call("inquiry.article_source", json!({"source_id": "r1s2"}))
                .await
                .unwrap();
            for _ in 0..3 {
                tools
                    .call("inquiry.article_source", json!({}))
                    .await
                    .unwrap();
            }
            for index in 3..=SYNTHESIS_SOURCE_READS {
                tools
                    .call(
                        "inquiry.article_source",
                        json!({"source_id": format!("r1s{index}")}),
                    )
                    .await
                    .unwrap();
            }
            let full = shown_refusal(
                tools
                    .call("inquiry.article_source", json!({"source_id": "r1s99"}))
                    .await,
            );
            assert!(full.contains("请立刻根据已读段落提交分析"));
            tools
                .call("inquiry.article_source", json!({"keyword": "unicode61"}))
                .await
                .expect("a keyword index does not consume a distinct source");
            ExecutionOutcome::Completed {
                usage: json!({"submission": "model"}),
            }
        }
    }

    #[tokio::test]
    async fn synthesis_repeat_read_yields_to_an_unopened_source_and_indexes_are_free() {
        let tools = Arc::new(SlotTools {
            node: "synthesize",
            remaining_ms: 400_000,
            draft_text: None,
            writes: Mutex::new(Vec::new()),
            reads: AtomicU32::new(0),
        });
        let outcome = ReviseGuard(Arc::new(Breadth))
            .execute_observed(
                invocation(Vec::new()),
                tools.clone(),
                CancellationToken::new(),
                None,
            )
            .await;
        assert!(matches!(outcome, ExecutionOutcome::Completed { .. }));
        assert_eq!(
            tools.reads.load(Ordering::Acquire),
            5 + SYNTHESIS_SOURCE_READS
        );
        assert!(tools.writes.lock().await.is_empty());
    }

    struct HitTools {
        reads: AtomicU32,
        keyword_calls: AtomicU32,
    }
    #[async_trait]
    impl ToolBridge for HitTools {
        fn specs(&self) -> Vec<ToolSpec> {
            Vec::new()
        }
        fn goal_context(&self) -> Option<GoalRunContext> {
            Some(GoalRunContext {
                goal_id: "guard".into(),
                plan_digest: None,
                role: GoalRunRole::NodeMember {
                    node_id: "synthesize".into(),
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
                remaining_ms: 400_000,
                mode: "explore".into(),
            })
        }
        async fn reject_json_candidate(&self, _: u64, _: ToolCallMetadata) -> Result<Value> {
            Ok(json!({}))
        }
        async fn call(&self, id: &str, args: Value) -> Result<Value> {
            if id != "inquiry.article_source" {
                return Err(fault("测试桥不接受该工具"));
            }
            if args["source_id"]
                .as_str()
                .filter(|value| !value.is_empty())
                .is_none()
            {
                self.keyword_calls.fetch_add(1, Ordering::Release);
                if args.get("keyword").is_none() {
                    return Ok(json!({"sources": []}));
                }
                let hits = if args["keyword"] == "trigram" {
                    json!([{"source_id": "r1s16"}, {"source_id": "r1s17"}])
                } else {
                    json!([])
                };
                return Ok(json!({"result": {"hits": hits}}));
            }
            self.reads.fetch_add(1, Ordering::Release);
            Ok(json!({"ok": true}))
        }
    }

    struct KeywordFirst;
    #[async_trait]
    impl Executor for KeywordFirst {
        fn capabilities(&self) -> ExecutorCapabilities {
            ExecutorCapabilities::MANAGED
        }
        async fn execute(
            &self,
            _: Invocation,
            tools: Arc<dyn ToolBridge>,
            _: CancellationToken,
        ) -> ExecutionOutcome {
            tools
                .call("inquiry.article_source", json!({"keyword": "trigram"}))
                .await
                .unwrap();
            tools
                .call("inquiry.article_source", json!({}))
                .await
                .unwrap();
            let blocked = shown_refusal(
                tools
                    .call("inquiry.article_source", json!({"source_id": "r1s2"}))
                    .await,
            );
            assert!(blocked.contains("请先打开"));
            assert!(blocked.contains("r1s16"));
            tools
                .call("inquiry.article_source", json!({"keyword": "missing"}))
                .await
                .unwrap();
            let still = shown_refusal(
                tools
                    .call("inquiry.article_source", json!({"source_id": "r1s2"}))
                    .await,
            );
            assert!(still.contains("请先打开"));
            tools
                .call("inquiry.article_source", json!({"source_id": "r1s16"}))
                .await
                .unwrap();
            tools
                .call("inquiry.article_source", json!({"source_id": "r1s17"}))
                .await
                .unwrap();
            tools
                .call("inquiry.article_source", json!({"source_id": "r1s2"}))
                .await
                .unwrap();
            ExecutionOutcome::Completed {
                usage: json!({"submission": "model"}),
            }
        }
    }

    #[tokio::test]
    async fn keyword_hits_open_before_any_other_source_and_do_not_count() {
        let tools = Arc::new(HitTools {
            reads: AtomicU32::new(0),
            keyword_calls: AtomicU32::new(0),
        });
        let outcome = ReviseGuard(Arc::new(KeywordFirst))
            .execute_observed(
                invocation(Vec::new()),
                tools.clone(),
                CancellationToken::new(),
                None,
            )
            .await;
        assert!(matches!(outcome, ExecutionOutcome::Completed { .. }));
        assert_eq!(tools.reads.load(Ordering::Acquire), 3);
        assert_eq!(tools.keyword_calls.load(Ordering::Acquire), 3);
    }

    struct ParagraphTools {
        reads: AtomicU32,
        keyword_calls: AtomicU32,
    }
    #[async_trait]
    impl ToolBridge for ParagraphTools {
        fn specs(&self) -> Vec<ToolSpec> {
            Vec::new()
        }
        fn goal_context(&self) -> Option<GoalRunContext> {
            Some(GoalRunContext {
                goal_id: "guard".into(),
                plan_digest: None,
                role: GoalRunRole::NodeMember {
                    node_id: "synthesize".into(),
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
                remaining_ms: 400_000,
                mode: "explore".into(),
            })
        }
        async fn reject_json_candidate(&self, _: u64, _: ToolCallMetadata) -> Result<Value> {
            Ok(json!({}))
        }
        async fn call(&self, id: &str, args: Value) -> Result<Value> {
            if id != "inquiry.article_source" {
                return Err(fault("测试桥不接受该工具"));
            }
            if args["source_id"]
                .as_str()
                .filter(|value| !value.is_empty())
                .is_none()
            {
                self.keyword_calls.fetch_add(1, Ordering::Release);
                let keyword = args["keyword"].as_str().unwrap_or("").trim();
                if keyword.is_empty() {
                    return Ok(json!({"sources": []}));
                }
                if keyword == "trigram" {
                    return Ok(
                        json!({"result": {"hits": [{"source_id": "r1s3", "paragraph": 25}]}}),
                    );
                }
                return Ok(json!({"result": {"hits": []}}));
            }
            self.reads.fetch_add(1, Ordering::Release);
            Ok(json!({"ok": true}))
        }
    }

    struct CoverParagraph;
    #[async_trait]
    impl Executor for CoverParagraph {
        fn capabilities(&self) -> ExecutorCapabilities {
            ExecutorCapabilities::MANAGED
        }
        async fn execute(
            &self,
            _: Invocation,
            tools: Arc<dyn ToolBridge>,
            _: CancellationToken,
        ) -> ExecutionOutcome {
            let early = shown_refusal(
                tools
                    .call("inquiry.article_source", json!({"source_id": "r1s3"}))
                    .await,
            );
            assert!(early.contains("完整索引不算"));
            tools
                .call("inquiry.article_source", json!({}))
                .await
                .unwrap();
            let after_full = shown_refusal(
                tools
                    .call("inquiry.article_source", json!({"source_id": "r1s3"}))
                    .await,
            );
            assert!(after_full.contains("完整索引不算"));
            tools
                .call("inquiry.article_source", json!({"keyword": "none"}))
                .await
                .unwrap();
            tools
                .call("inquiry.article_source", json!({"source_id": "r1s9"}))
                .await
                .expect("empty hits unlock a page when nothing is pending");
            tools
                .call("inquiry.article_source", json!({"keyword": "trigram"}))
                .await
                .unwrap();
            let other = shown_refusal(
                tools
                    .call("inquiry.article_source", json!({"source_id": "r1s2"}))
                    .await,
            );
            assert!(other.contains("请先打开"));
            assert!(other.contains("r1s3"));
            let filtered = shown_refusal(
                tools
                    .call(
                        "inquiry.article_source",
                        json!({"source_id": "r1s3", "keyword": "benchmark", "start": 25, "limit": 1}),
                    )
                    .await,
            );
            assert!(filtered.contains("r1s3"));
            assert!(filtered.contains("25"));
            let window = shown_refusal(
                tools
                    .call(
                        "inquiry.article_source",
                        json!({"source_id": "r1s3", "start": 0, "limit": 5}),
                    )
                    .await,
            );
            assert!(window.contains("25"));
            tools
                .call("inquiry.article_source", json!({"keyword": "missing"}))
                .await
                .unwrap();
            let still = shown_refusal(
                tools
                    .call("inquiry.article_source", json!({"source_id": "r1s2"}))
                    .await,
            );
            assert!(still.contains("请先打开"));
            tools
                .call(
                    "inquiry.article_source",
                    json!({"source_id": "r1s3", "start": 25, "limit": 1}),
                )
                .await
                .expect("a window that includes paragraph 25 reaches the tool");
            tools
                .call("inquiry.article_source", json!({"source_id": "r1s2"}))
                .await
                .expect("other sources open after the hit paragraph is covered");
            let repeat = shown_refusal(
                tools
                    .call(
                        "inquiry.article_source",
                        json!({"source_id": "r1s3", "start": 25, "limit": 1}),
                    )
                    .await,
            );
            assert!(repeat.contains("尚未打开"));
            ExecutionOutcome::Completed {
                usage: json!({"submission": "model"}),
            }
        }
    }

    #[tokio::test]
    async fn keyword_index_precedes_pages_and_the_first_open_covers_the_hit_paragraph() {
        let tools = Arc::new(ParagraphTools {
            reads: AtomicU32::new(0),
            keyword_calls: AtomicU32::new(0),
        });
        let outcome = ReviseGuard(Arc::new(CoverParagraph))
            .execute_observed(
                invocation(Vec::new()),
                tools.clone(),
                CancellationToken::new(),
                None,
            )
            .await;
        assert!(matches!(outcome, ExecutionOutcome::Completed { .. }));
        assert_eq!(tools.reads.load(Ordering::Acquire), 3);
        assert_eq!(tools.keyword_calls.load(Ordering::Acquire), 4);
    }
}
