//! Trusted projection of Milkie's public receipt; search snippets are never evidence.
use super::*;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

/// Reads stop this long before the registered 300000 ms cap so one model turn can still submit.
pub const INITIAL_ANALYSIS_FINALIZE_BEFORE_MS: u64 = 90_000;
/// Stop the model and, if it has not submitted, write an honest need_input before that cap.
pub const INITIAL_ANALYSIS_HOST_RESERVE_MS: u64 = 25_000;
/// Page reads stop here so the same run can still search public gaps and submit.
pub const INITIAL_ANALYSIS_SOURCE_READS: u32 = 8;

pub struct DiscoveryExecutor(pub Arc<MilkieExecutor>);
struct AgentBridge {
    inner: Arc<dyn ToolBridge>,
    analysis_written: AtomicBool,
    source_reads: AtomicU32,
    analysis: std::sync::Mutex<Option<Value>>,
}
fn allowed(id: &str, args: &Value) -> Result<()> {
    if id == "artifact.write" && args["slot"] == "discoveries" {
        return Err(fault("discoveries仅由宿主原生回执写入"));
    }
    Ok(())
}
#[async_trait]
impl ToolBridge for AgentBridge {
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
        allowed(id, &args)?;
        self.admit_source_read(id)?;
        let result = self.inner.call(id, args.clone()).await?;
        self.note_source_read(id);
        self.store_analysis(id, &args);
        Ok(result)
    }
    async fn call_with_metadata(
        &self,
        id: &str,
        args: Value,
        metadata: ToolCallMetadata,
    ) -> Result<Value> {
        allowed(id, &args)?;
        self.admit_source_read(id)?;
        let result = self
            .inner
            .call_with_metadata(id, args.clone(), metadata)
            .await?;
        self.note_source_read(id);
        self.store_analysis(id, &args);
        Ok(result)
    }
}
impl AgentBridge {
    fn on_initial_analysis(&self) -> bool {
        matches!(
            self.goal_context().as_ref().map(|context| &context.role),
            Some(GoalRunRole::NodeMember { node_id, .. }) if node_id == "analyze"
        )
    }
    fn admit_source_read(&self, id: &str) -> Result<()> {
        if id != "inquiry.article_source" || !self.on_initial_analysis() {
            return Ok(());
        }
        if self.source_reads.load(Ordering::Acquire) >= INITIAL_ANALYSIS_SOURCE_READS {
            return Err(fault(
                "初次分析的来源页已够判断缺口，请立刻原生搜索可公开补齐的缺口并提交分析",
            ));
        }
        Ok(())
    }
    fn note_source_read(&self, id: &str) {
        if id == "inquiry.article_source" && self.on_initial_analysis() {
            self.source_reads.fetch_add(1, Ordering::Release);
        }
    }
    fn store_analysis(&self, id: &str, args: &Value) {
        if id == "artifact.write" && args["slot"] == "analysis" {
            *self.analysis.lock().expect("analysis") = Some(args["value"].clone());
            self.analysis_written.store(true, Ordering::Release);
        }
    }
    fn needs_follow_up_search(&self) -> bool {
        let guard = self.analysis.lock().expect("analysis");
        let Some(value) = guard.as_ref() else {
            return false;
        };
        let gaps = value["gaps"]
            .as_array()
            .is_some_and(|items| !items.is_empty());
        let queries = value["search_queries"]
            .as_array()
            .is_some_and(|items| !items.is_empty());
        let ready = value["decision"] == "ready" && !gaps;
        !ready || queries
    }
}
/// The adapter already checks receipt ownership. This boundary validates the domain projection.
pub fn project(page: &Value, require_search: bool) -> Result<Value> {
    if page["version"] != 1 || page["mode"] != "live" || page["complete"] != true {
        return Err(fault("原生搜索回执未完整或模式不匹配"));
    }
    let run = page["runId"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| fault("缺少原生运行归属"))?;
    let context = page["contextId"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| fault("缺少原生上下文归属"))?;
    let items = page["items"]
        .as_array()
        .ok_or_else(|| fault("原生条目不是数组"))?;
    let mut searches = Vec::new();
    let mut count = 0;
    let mut seen = BTreeSet::new();
    for item in items {
        if item["phase"] != "completed" {
            return Err(fault("原生搜索条目尚未完成"));
        }
        if item["action"]["type"] != "search" {
            continue;
        }
        let id = item["nativeItemId"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| fault("缺少原生搜索条目标识"))?;
        if !seen.insert(id) {
            return Err(fault("重复原生搜索条目"));
        }
        let queries = if let Some(values) = item["action"]["queries"]
            .as_array()
            .filter(|a| !a.is_empty())
        {
            values
                .iter()
                .map(|q| {
                    q.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| fault("原生查询不是文本"))
                })
                .collect::<Result<Vec<_>>>()?
        } else {
            vec![
                item["action"]["query"]
                    .as_str()
                    .or_else(|| item["query"].as_str())
                    .ok_or_else(|| fault("原生搜索缺少实际查询"))?
                    .to_owned(),
            ]
        };
        count += queries.len();
        if count > 8
            || queries
                .iter()
                .any(|q| q.trim().is_empty() || q.chars().count() > 300)
        {
            return Err(fault("原生实际查询超过每轮八条或三百字符边界"));
        }
        let results = item["results"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| r["type"] == "text_result")
            .filter_map(|r| {
                let url = crate::net::normalize_url(r["url"].as_str()?).ok()?;
                Some(json!({"url":url.as_str(),"title":r["title"].as_str().unwrap_or("")}))
            })
            .collect::<Vec<_>>();
        let available = !results.is_empty();
        searches.push(json!({"query":queries.join(" | "),"queries":queries,"origin":format!("native-search:{run}/{id}"),"native_item_id":id,"status":if available {"discovered"}else{"unavailable"},"results":results,"limitation":if available {"多查询结果为原生聚合，未假定逐网址查询归因"}else{"原生回执未提供可识别的公开网址结果"}}));
    }
    if require_search && searches.is_empty() {
        return Err(fault("首轮研究缺少实际原生搜索"));
    }
    Ok(
        json!({"provider":"codex-native","run_id":run,"context_id":context,"mode":"live","searches":searches}),
    )
}
fn reject_unsearched_gaps(discoveries: &Value, require_follow_up: bool) -> Result<()> {
    let empty = discoveries["searches"]
        .as_array()
        .map(|items| items.is_empty())
        .unwrap_or(true);
    if require_follow_up && empty {
        return Err(fault("初次分析仍有可公开补齐的缺口，但没有实际原生搜索"));
    }
    Ok(())
}
pub(crate) fn honest_need_input(subquestions: &[String]) -> Value {
    let questions = if subquestions.is_empty() {
        vec!["原始研究问题".to_owned()]
    } else {
        subquestions.to_vec()
    };
    let coverage = questions
        .iter()
        .map(|question| {
            json!({
                "question": question,
                "status": "unknown",
                "conclusion": "分析在登记的阶段时限前提交失败，此项保持未知，不补写来源引文。",
                "evidence_ids": []
            })
        })
        .collect::<Vec<_>>();
    json!({
        "judgments": [{
            "text": "分析没有在阶段时限内提交基于已保存来源的证据判断。",
            "support": [],
            "counter": [],
            "limitations": ["宿主只记录这个时限缺口，不编造引用。"]
        }],
        "gaps": ["分析未在阶段时限内提交已核对证据"],
        "decision": "need_input",
        "evidence": [],
        "search_queries": [],
        "coverage": coverage
    })
}
fn unfinished_discoveries(run_id: &str) -> Value {
    json!({
        "provider": "codex-native",
        "run_id": run_id,
        "context_id": run_id,
        "mode": "live",
        "searches": []
    })
}
pub(crate) async fn plan_subquestions(
    inv: &Invocation,
    tools: &Arc<dyn ToolBridge>,
) -> Vec<String> {
    let Some(plan) = inv
        .artifacts
        .iter()
        .find(|artifact| artifact.slot == "plan")
    else {
        return Vec::new();
    };
    let Ok(response) = tools.call("artifact.read", json!({"id": plan.id})).await else {
        return Vec::new();
    };
    let Some(text) = response["text"].as_str() else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    value["subquestions"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}
impl DiscoveryExecutor {
    async fn write_discoveries(
        &self,
        run: &str,
        tools: &Arc<dyn ToolBridge>,
        cancel: &CancellationToken,
        require_search: bool,
        allow_empty: bool,
        require_follow_up: bool,
    ) -> Result<()> {
        if !allow_empty {
            let page = self.0.native_web_searches(run).await?;
            if cancel.is_cancelled() {
                return Err(fault("搜索回执采集已取消"));
            }
            let value = project(&page, require_search)?;
            reject_unsearched_gaps(&value, require_follow_up)?;
            tools
                .call(
                    "artifact.write",
                    json!({"slot":"discoveries","value":value}),
                )
                .await?;
            return Ok(());
        }
        let projected =
            tokio::time::timeout(Duration::from_secs(3), self.0.native_web_searches(run))
                .await
                .ok()
                .and_then(Result::ok)
                .and_then(|page| project(&page, false).ok());
        if cancel.is_cancelled() {
            return Err(fault("搜索回执采集已取消"));
        }
        let value = projected.unwrap_or_else(|| unfinished_discoveries(run));
        reject_unsearched_gaps(&value, require_follow_up)?;
        tools
            .call(
                "artifact.write",
                json!({"slot":"discoveries","value":value}),
            )
            .await?;
        Ok(())
    }
}
pub(crate) async fn race_model_with_reserve<T>(
    parent: &CancellationToken,
    child: &CancellationToken,
    should_stop: impl Fn() -> bool,
    enabled: bool,
    model: impl std::future::Future<Output = T>,
) -> T {
    tokio::pin!(model);
    loop {
        tokio::select! {
            biased;
            outcome = &mut model => return outcome,
            _ = parent.cancelled() => child.cancel(),
            _ = tokio::time::sleep(Duration::from_millis(50)), if enabled && !child.is_cancelled() => {
                if should_stop() {
                    child.cancel();
                }
            }
        }
    }
}
#[async_trait]
impl Executor for DiscoveryExecutor {
    fn enforces_json_candidate_limit(&self) -> bool {
        self.0.enforces_json_candidate_limit()
    }
    fn budget_limits(&self) -> ExecutorBudgetLimits {
        self.0.budget_limits()
    }
    fn identity(&self) -> Value {
        json!({"adapter":self.0.identity(),"projection":"inquiry-discoveries-v1"})
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
        mut inv: Invocation,
        tools: Arc<dyn ToolBridge>,
        cancel: CancellationToken,
        observer: Option<Arc<dyn MilkieObserver>>,
    ) -> ExecutionOutcome {
        let run = inv.run_id.clone();
        let analyze = matches!(tools.goal_context().map(|c|c.role),Some(GoalRunRole::NodeMember{node_id,..})if node_id=="analyze");
        let require_search = matches!(tools.goal_context().map(|c|c.role),Some(GoalRunRole::NodeMember{node_id,..})if node_id=="scope");
        let questions = if analyze {
            plan_subquestions(&inv, &tools).await
        } else {
            Vec::new()
        };
        inv.entry.outputs.remove("discoveries");
        inv.output_schemas.remove("discoveries");
        let bridge = Arc::new(AgentBridge {
            inner: tools.clone(),
            analysis_written: AtomicBool::new(false),
            source_reads: AtomicU32::new(0),
            analysis: std::sync::Mutex::new(None),
        });
        let child = cancel.child_token();
        let outcome = race_model_with_reserve(
            &cancel,
            &child,
            || {
                !bridge.analysis_written.load(Ordering::Acquire)
                    && tools
                        .budget_hint()
                        .is_some_and(|hint| hint.remaining_ms <= INITIAL_ANALYSIS_HOST_RESERVE_MS)
            },
            analyze,
            self.0
                .execute_observed(inv, bridge.clone(), child.clone(), observer),
        )
        .await;
        if cancel.is_cancelled() {
            return outcome;
        }
        let timed_out = child.is_cancelled()
            || matches!(outcome, ExecutionOutcome::Failed { ref code, .. } if code == "limit_reached");
        let submitted = bridge.analysis_written.load(Ordering::Acquire);
        if analyze
            && timed_out
            && !submitted
            && let Err(e) = tools
                .call(
                    "artifact.write",
                    json!({"slot":"analysis","value":honest_need_input(&questions)}),
                )
                .await
        {
            return ExecutionOutcome::Failed {
                code: e.code,
                message: e.message,
            };
        }
        if !(matches!(outcome, ExecutionOutcome::Completed { .. }) || analyze && timed_out) {
            return outcome;
        }
        let follow_up = analyze && bridge.needs_follow_up_search();
        let result = self
            .write_discoveries(
                &run,
                &tools,
                &cancel,
                require_search,
                analyze && timed_out && !follow_up,
                follow_up,
            )
            .await;
        match result {
            Ok(()) => {
                if analyze && timed_out {
                    ExecutionOutcome::Completed {
                        usage: json!({"submission":"deadline_need_input","native_model":true}),
                    }
                } else {
                    outcome
                }
            }
            Err(_) if cancel.is_cancelled() => ExecutionOutcome::Cancelled,
            Err(e) => ExecutionOutcome::Failed {
                code: e.code,
                message: e.message,
            },
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn page() -> Value {
        json!({"version":1,"runId":"real-run","contextId":"real-context","mode":"live","complete":true,"items":[{"nativeItemId":"item-1","phase":"completed","query":"actual","action":{"type":"search","query":"actual","queries":["a","b"]},"results":[{"type":"text_result","url":"https://example.com/page","title":"Title","snippet":"UNTRUSTED SNIPPET"}]}]})
    }
    #[test]
    fn projection_retains_aggregate_provenance_without_promoting_snippets() {
        let result = project(&page(), true).unwrap();
        assert_eq!(result["searches"][0]["queries"], json!(["a", "b"]));
        assert_eq!(
            result["searches"][0]["origin"],
            "native-search:real-run/item-1"
        );
        assert!(!result.to_string().contains("UNTRUSTED"));
        let mut p = page();
        p["items"][0]["results"] = Value::Null;
        assert_eq!(
            project(&p, true).unwrap()["searches"][0]["status"],
            "unavailable"
        );
    }
    #[test]
    fn incomplete_unknown_or_over_budget_receipts_fail_and_model_cannot_write_reserved_slot() {
        for key in ["complete", "mode"] {
            let mut p = page();
            p[key] = Value::Null;
            assert!(project(&p, true).is_err());
        }
        let mut p = page();
        p["items"][0]["action"]["queries"] = json!(vec!["q"; 9]);
        assert!(project(&p, true).is_err());
        let mut p = page();
        p["items"] = json!([]);
        assert!(project(&p, true).is_err());
        assert!(project(&p, false).is_ok());
        assert!(
            allowed(
                "artifact.write",
                &json!({"slot":"discoveries","value":{"fake":true}})
            )
            .is_err()
        );
        assert!(allowed("artifact.write", &json!({"slot":"plan"})).is_ok());
    }
    #[tokio::test]
    async fn initial_analysis_stops_inside_the_reserve_and_need_input_invents_no_evidence() {
        let value = honest_need_input(&["分词如何比较".into()]);
        assert_eq!(value["decision"], "need_input");
        assert!(value["evidence"].as_array().unwrap().is_empty());
        assert!(value["search_queries"].as_array().unwrap().is_empty());
        assert_eq!(value["coverage"][0]["status"], "unknown");
        assert_eq!(
            value["coverage"][0]["evidence_ids"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert!(!value.to_string().contains("https://"));
        let empty = unfinished_discoveries("run-deadline");
        assert_eq!(empty["searches"].as_array().unwrap().len(), 0);
        assert_eq!(empty["provider"], "codex-native");

        let parent = CancellationToken::new();
        let child = parent.child_token();
        let started = std::time::Instant::now();
        let wait = child.clone();
        let outcome = race_model_with_reserve(&parent, &child, || true, true, async move {
            wait.cancelled().await;
            "stopped-for-submission"
        })
        .await;
        assert_eq!(outcome, "stopped-for-submission");
        assert!(child.is_cancelled());
        assert!(started.elapsed() < Duration::from_secs(2));

        let parent = CancellationToken::new();
        let child = parent.child_token();
        let outcome =
            race_model_with_reserve(&parent, &child, || true, false, async { "scope-unchanged" })
                .await;
        assert_eq!(outcome, "scope-unchanged");
        assert!(!child.is_cancelled());
    }
    struct ReadProbe {
        node: &'static str,
        reads: AtomicU32,
    }
    #[async_trait]
    impl ToolBridge for ReadProbe {
        fn specs(&self) -> Vec<ToolSpec> {
            Vec::new()
        }
        fn goal_context(&self) -> Option<GoalRunContext> {
            Some(GoalRunContext {
                goal_id: "read-cap".into(),
                plan_digest: None,
                role: GoalRunRole::NodeMember {
                    node_id: self.node.into(),
                    member_id: "worker".into(),
                },
                evaluation_id: None,
                epoch: 1,
            })
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
        async fn call(&self, id: &str, _: Value) -> Result<Value> {
            if id == "inquiry.article_source" {
                self.reads.fetch_add(1, Ordering::Relaxed);
            }
            Ok(json!({"ok": true}))
        }
    }
    fn read_bridge(inner: Arc<ReadProbe>) -> AgentBridge {
        AgentBridge {
            inner,
            analysis_written: AtomicBool::new(false),
            source_reads: AtomicU32::new(0),
            analysis: std::sync::Mutex::new(None),
        }
    }
    #[tokio::test]
    async fn initial_analysis_stops_paging_and_rejects_a_public_gap_without_a_search() {
        let analyze = Arc::new(ReadProbe {
            node: "analyze",
            reads: AtomicU32::new(0),
        });
        let bridge = read_bridge(analyze.clone());
        for _ in 0..INITIAL_ANALYSIS_SOURCE_READS {
            bridge
                .call("inquiry.article_source", json!({}))
                .await
                .unwrap();
        }
        let rejected = bridge
            .call("inquiry.article_source", json!({}))
            .await
            .expect_err("the next source read must stop");
        assert!(rejected.to_string().contains("请立刻原生搜索"));
        assert_eq!(
            analyze.reads.load(Ordering::Relaxed),
            INITIAL_ANALYSIS_SOURCE_READS
        );
        let other = Arc::new(ReadProbe {
            node: "synthesize",
            reads: AtomicU32::new(0),
        });
        let other_bridge = read_bridge(other.clone());
        for _ in 0..=INITIAL_ANALYSIS_SOURCE_READS {
            other_bridge
                .call("inquiry.article_source", json!({}))
                .await
                .unwrap();
        }
        assert_eq!(
            other.reads.load(Ordering::Relaxed),
            INITIAL_ANALYSIS_SOURCE_READS + 1
        );
        bridge
            .call(
                "artifact.write",
                json!({"slot":"analysis","value":{"decision":"need_input","gaps":["公开机制未保存"],"evidence":[],"search_queries":[]}}),
            )
            .await
            .unwrap();
        assert!(bridge.needs_follow_up_search());
        let mut bare = page();
        bare["items"] = json!([]);
        let empty = project(&bare, false).unwrap();
        let missing = reject_unsearched_gaps(&empty, true).unwrap_err();
        assert!(missing.to_string().contains("没有实际原生搜索"));
        assert!(reject_unsearched_gaps(&empty, false).is_ok());
        assert!(reject_unsearched_gaps(&project(&page(), false).unwrap(), true).is_ok());
        bridge
            .call(
                "artifact.write",
                json!({"slot":"analysis","value":{"decision":"ready","gaps":[],"evidence":[],"search_queries":[]}}),
            )
            .await
            .unwrap();
        assert!(!bridge.needs_follow_up_search());
    }
}
