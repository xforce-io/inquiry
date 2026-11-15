//! Trusted projection of Milkie's public receipt; search snippets are never evidence.
use super::*;

pub struct DiscoveryExecutor(pub Arc<MilkieExecutor>);
struct AgentBridge(Arc<dyn ToolBridge>);
fn allowed(id: &str, args: &Value) -> Result<()> {
    if id == "artifact.write" && args["slot"] == "discoveries" {
        return Err(fault("discoveries仅由宿主原生回执写入"));
    }
    Ok(())
}
#[async_trait]
impl ToolBridge for AgentBridge {
    fn specs(&self) -> Vec<ToolSpec> {
        self.0.specs()
    }
    fn goal_context(&self) -> Option<GoalRunContext> {
        self.0.goal_context()
    }
    fn json_candidate_limit(&self) -> Option<usize> {
        self.0.json_candidate_limit()
    }
    fn budget_hint(&self) -> Option<BudgetHint> {
        self.0.budget_hint()
    }
    async fn reject_json_candidate(&self, bytes: u64, metadata: ToolCallMetadata) -> Result<Value> {
        self.0.reject_json_candidate(bytes, metadata).await
    }
    async fn call(&self, id: &str, args: Value) -> Result<Value> {
        allowed(id, &args)?;
        self.0.call(id, args).await
    }
    async fn call_with_metadata(
        &self,
        id: &str,
        args: Value,
        metadata: ToolCallMetadata,
    ) -> Result<Value> {
        allowed(id, &args)?;
        self.0.call_with_metadata(id, args, metadata).await
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
        let require_search = matches!(tools.goal_context().map(|c|c.role),Some(GoalRunRole::NodeMember{node_id,..})if node_id=="scope");
        inv.entry.outputs.remove("discoveries");
        inv.output_schemas.remove("discoveries");
        let outcome = self
            .0
            .execute_observed(
                inv,
                Arc::new(AgentBridge(tools.clone())),
                cancel.clone(),
                observer,
            )
            .await;
        if !matches!(outcome, ExecutionOutcome::Completed { .. }) {
            return outcome;
        }
        let result = async {
            let page = self.0.native_web_searches(&run).await?;
            if cancel.is_cancelled() {
                return Err(fault("搜索回执采集已取消"));
            }
            let value = project(&page, require_search)?;
            tools
                .call(
                    "artifact.write",
                    json!({"slot":"discoveries","value":value}),
                )
                .await?;
            Ok::<_, Error>(())
        }
        .await;
        match result {
            Ok(()) => outcome,
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
}
