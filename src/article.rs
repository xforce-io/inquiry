//! Deep-article evidence contracts. Scheduling and result authority remain in TaskLoop.
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use taskloop::*;
use tokio_util::sync::CancellationToken;

pub const CORPUS_BYTES: usize = 1_048_576;
pub const ARCHIVE_BYTES: usize = 2_621_440;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub source_id: String,
    pub origin: String,
    pub fact: Value,
    pub paragraphs: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Corpus {
    pub version: u32,
    pub round: u8,
    pub sources: Vec<Source>,
    pub searches: Vec<Value>,
    pub limitations: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Citation {
    pub artifact_id: String,
    pub sha256: String,
    pub source_id: String,
    pub paragraph: usize,
    pub quote: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub answer_status: String,
    pub conclusion: String,
    pub citations: Vec<Citation>,
    pub limitations: Vec<String>,
}
pub fn fault(message: impl Into<String>) -> Error {
    Error::new("research_invalid", "/article", message)
}
fn corpus(artifact: &ArtifactData) -> Result<Corpus> {
    let data: Corpus = serde_json::from_value(parse_unique(&artifact.bytes)?)?;
    if data.version != 1 || !(1..=2).contains(&data.round) {
        return Err(fault("资料包版本或轮次不受支持"));
    }
    Ok(data)
}
pub fn source_view(artifact: &ArtifactData, source_id: &str) -> Result<Source> {
    corpus(artifact)?
        .sources
        .into_iter()
        .find(|s| s.source_id == source_id)
        .ok_or_else(|| Error::new("not_found", "/source_id", "此资料包没有该来源"))
}
pub fn verify_citation(c: &Citation, accepted: &[ArtifactData]) -> Result<String> {
    let artifact = accepted
        .iter()
        .find(|a| a.reference.id == c.artifact_id)
        .ok_or_else(|| fault("引用资料包未被授权"))?;
    if artifact.reference.sha256 != c.sha256 || crate::domain::hash(&artifact.bytes) != c.sha256 {
        return Err(fault("引用资料包版本不匹配"));
    }
    let source = source_view(artifact, &c.source_id)?;
    if source.fact["usable"] != true
        || c.quote.trim().is_empty()
        || c.quote.chars().count() > 200
        || !source
            .paragraphs
            .get(c.paragraph)
            .is_some_and(|p| p.contains(&c.quote))
        || source.fact["normalized_sha256"]
            != crate::domain::hash(&serde_json::to_vec(&source.paragraphs)?)
    {
        return Err(fault("引用不是已保存来源版本的精确短引文"));
    }
    source.fact["final_url"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| fault("引用来源缺少实际获取网址"))
}
pub fn check_analysis(analysis: &Value, accepted: &[ArtifactData]) -> Result<()> {
    let evidence = analysis["evidence"]
        .as_array()
        .ok_or_else(|| fault("缺少分析证据"))?;
    let mut ids = BTreeSet::new();
    for item in evidence {
        let id = item["id"].as_str().ok_or_else(|| fault("缺少证据身份"))?;
        if !ids.insert(id) {
            return Err(fault("分析证据身份重复"));
        }
        let mut c = item.clone();
        c.as_object_mut()
            .ok_or_else(|| fault("证据必须为对象"))?
            .remove("id");
        verify_citation(&serde_json::from_value(c)?, accepted)?;
    }
    for judgment in analysis["judgments"]
        .as_array()
        .ok_or_else(|| fault("缺少分析判断"))?
    {
        let support = judgment["support"]
            .as_array()
            .ok_or_else(|| fault("缺少支持关系"))?;
        let counter = judgment["counter"]
            .as_array()
            .ok_or_else(|| fault("缺少反驳关系"))?;
        if support.is_empty()
            || support
                .iter()
                .chain(counter)
                .any(|v| v.as_str().is_none_or(|id| !ids.contains(id)))
        {
            return Err(fault("分析判断关联悬空或缺少支持证据"));
        }
    }
    for coverage in analysis["coverage"]
        .as_array()
        .ok_or_else(|| fault("缺少问题覆盖关系"))?
    {
        if coverage["evidence_ids"]
            .as_array()
            .ok_or_else(|| fault("缺少覆盖证据关系"))?
            .iter()
            .any(|v| v.as_str().is_none_or(|id| !ids.contains(id)))
        {
            return Err(fault("问题覆盖关联悬空证据"));
        }
    }
    Ok(())
}
pub fn check_report(report: &Report, analysis: &Value, accepted: &[ArtifactData]) -> Result<()> {
    check_analysis(analysis, accepted)?;
    if analysis["decision"] != "ready" || !analysis["gaps"].as_array().is_some_and(|g| g.is_empty())
    {
        return Err(fault("分析仍有决定性缺口，不能接纳为完整回答"));
    }
    if report.answer_status != "answered" || report.citations.is_empty() {
        return Err(fault("资料不足的文章不能通过最终验收"));
    }
    let evidence = analysis["evidence"]
        .as_array()
        .ok_or_else(|| fault("缺少分析证据"))?;
    let mut used = BTreeSet::new();
    let mut quotes = BTreeMap::<String, usize>::new();
    for (index, c) in report.citations.iter().enumerate() {
        let url = verify_citation(c, accepted)?;
        let key = serde_json::to_string(c)?;
        if !used.insert(key)
            || !evidence.iter().any(|v| {
                let mut v = v.clone();
                if let Some(o) = v.as_object_mut() {
                    o.remove("id");
                }
                serde_json::from_value::<Citation>(v).is_ok_and(|e| e == *c)
            })
        {
            return Err(fault("报告引入重复或未分析的引用"));
        }
        if !report.conclusion.contains(&format!("[{}]", index + 1)) {
            return Err(fault("引用未在正文中使用"));
        }
        let cjk = c
            .quote
            .chars()
            .filter(|x| ('\u{3400}'..='\u{9fff}').contains(x))
            .count();
        let words = if cjk == 0 {
            c.quote.split_whitespace().count()
        } else {
            cjk + c
                .quote
                .split_whitespace()
                .filter(|w| w.chars().any(|x| x.is_ascii_alphanumeric()))
                .count()
        };
        *quotes.entry(url).or_default() += words;
    }
    if quotes.values().any(|n| *n > 25) {
        return Err(fault("同一来源累计短引文超过二十五词"));
    }
    for part in report.conclusion.split('[').skip(1) {
        if let Some((n, _)) = part.split_once(']')
            && let Ok(n) = n.parse::<usize>()
            && !(1..=report.citations.len()).contains(&n)
        {
            return Err(fault("正文引用编号悬空"));
        }
    }
    Ok(())
}

/// Selection is derived only from actual search responses or explicit user input.
fn candidates(
    seeds: &[String],
    searches: &[Value],
    excluded: &BTreeSet<String>,
) -> Vec<(String, String)> {
    let mut seen = excluded.clone();
    let mut result = Vec::new();
    let mut add = |url: &str, origin: String| {
        if result.len() < 20
            && let Ok(url) = crate::net::normalize_url(url)
            && seen.insert(url.to_string())
        {
            result.push((url.to_string(), origin));
        }
    };
    for url in seeds {
        add(url, "user-input".into());
    }
    for rank in 0..5 {
        for search in searches {
            if search["status"] == "discovered"
                && let Some(url) = search["results"][rank]["url"].as_str()
            {
                add(
                    url,
                    format!("search:{}", search["query"].as_str().unwrap_or("")),
                );
            }
        }
    }
    result
}
fn trim_paragraphs(paragraphs: &[Value]) -> (Vec<String>, bool) {
    let mut remaining = 20_000;
    let mut out = Vec::new();
    let mut truncated = false;
    for p in paragraphs {
        let text = p.as_str().expect("trusted normalizer emits strings");
        if remaining == 0 {
            truncated = true;
            break;
        }
        let value: String = text.chars().take(remaining).collect();
        truncated |= value.chars().count() < text.chars().count();
        remaining -= value.chars().count();
        out.push(value);
    }
    (out, truncated)
}
/// Each actual search/fetch is one bounded callback; native assembly is inside one Run.
async fn collect(inv: &Invocation, tools: &Arc<dyn ToolBridge>, round: u8) -> Result<Value> {
    let mut queries = Vec::new();
    let mut excluded = BTreeSet::new();
    for reference in &inv.artifacts {
        if !matches!(reference.slot.as_str(), "plan" | "analysis" | "corpus") {
            continue;
        }
        let response = tools
            .call("artifact.read", json!({"id":reference.id}))
            .await?;
        let a = ArtifactData {
            reference: reference.clone(),
            bytes: response["text"]
                .as_str()
                .ok_or_else(|| fault("资料输入不是文本"))?
                .as_bytes()
                .to_vec()
                .into(),
        };
        if a.reference.slot == "plan" && round == 1 || a.reference.slot == "analysis" && round == 2
        {
            let v = parse_unique(&a.bytes)?;
            queries = v["search_queries"]
                .as_array()
                .ok_or_else(|| fault("缺少声明的查询"))?
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| fault("查询必须是文本"))
                })
                .collect::<Result<_>>()?;
        }
        if a.reference.slot == "corpus" {
            for s in corpus(&a)?.sources {
                if let Some(url) = s.fact["requested_url"].as_str() {
                    excluded.insert(url.to_owned());
                }
            }
        }
    }
    if queries.len() > 8 {
        return Err(fault("每轮最多八条查询"));
    }
    let mut seeds = inv.inputs["seed_urls"]
        .as_array()
        .ok_or_else(|| fault("缺少用户网址字段"))?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for signal in &inv.signal_facts {
        if let Some(urls) = signal.payload["urls"].as_array() {
            seeds.extend(urls.iter().filter_map(Value::as_str).map(str::to_owned));
        }
    }
    let mut output = Corpus {
        version: 1,
        round,
        sources: vec![],
        searches: vec![],
        limitations: vec![],
    };
    for query in queries {
        let search = tools.call("inquiry.search", json!({"query":query})).await?;
        output.searches.push(search["result"].clone());
    }
    let selected = candidates(&seeds, &output.searches, &excluded);
    let mut archive = BTreeMap::<String, Value>::new();
    for (index, (url, origin)) in selected.into_iter().enumerate() {
        let id = format!("r{round}s{}", index + 1);
        let response = tools
            .call("inquiry.article_fetch", json!({"source_id":id,"url":url}))
            .await?;
        let acquired = &response["result"];
        let mut fact = acquired["fact"].clone();
        let mut paragraphs: Vec<String> = serde_json::from_value(acquired["paragraphs"].clone())?;
        if !acquired["raw"].is_null() {
            archive.insert(id.clone(), acquired["raw"].clone());
            if serde_json::to_vec(&archive)?.len() > ARCHIVE_BYTES {
                archive.remove(&id);
                paragraphs.clear();
                fact["usable"] = json!(false);
                fact["reason"] = json!("原文档案容量到界，未接纳为可引用来源");
            }
        }
        let mut source = Source {
            source_id: id.clone(),
            origin,
            fact,
            paragraphs,
        };
        output.sources.push(source.clone());
        if serde_json::to_vec(&output)?.len() > CORPUS_BYTES {
            output.sources.pop();
            archive.remove(&id);
            source.paragraphs.clear();
            source.fact["usable"] = json!(false);
            source.fact["reason"] = json!("资料包容量到界，未接纳为可引用来源");
            output.sources.push(source);
        }
    }
    if output.sources.iter().all(|s| s.fact["usable"] != true) {
        output.limitations.push(
            if output.searches.iter().any(|s| s["status"] == "unavailable") {
                "搜索服务暂不可用；这不是要求用户必须提供网址".into()
            } else {
                "本轮未取得新的可用保存资料".into()
            },
        );
    }
    if serde_json::to_vec(&output)?.len() > CORPUS_BYTES {
        return Err(fault("资料包处置记录超过容量边界"));
    }
    Ok(json!({"corpus":output,"archive":archive}))
}
pub struct FetchSource;
#[async_trait]
impl DomainTool for FetchSource {
    fn spec(&self) -> ToolSpec {
        ToolSpec { id:"inquiry.article_fetch".into(), description:"Native collection only: fetch and preserve one public source within the callback deadline.".into(), input_schema:json!({"type":"object","properties":{"source_id":{"type":"string","pattern":"^r[12]s[0-9]+$"},"url":{"type":"string","maxLength":4096}},"required":["source_id","url"],"additionalProperties":false}) }
    }
    async fn call(&self, c: ToolContext, args: Value) -> Result<ToolOutput> {
        if !matches!(c.goal().map(|g| &g.role), Some(GoalRunRole::NodeMember{node_id,..}) if node_id=="collect" || node_id=="supplement")
        {
            return Err(Error::new(
                "capability_denied",
                "/collect",
                "资料获取只能由锁定的原生节点执行",
            ));
        }
        let id = args["source_id"]
            .as_str()
            .ok_or_else(|| fault("缺少来源身份"))?;
        let url = args["url"].as_str().ok_or_else(|| fault("缺少来源网址"))?;
        let acquired = crate::domain::acquire_source(&c, id, url).await?;
        let mut fact = acquired.payload;
        let snapshot = acquired
            .files
            .iter()
            .find(|f| f.slot.starts_with("capture.snapshot."));
        let (paragraphs, truncated) = match snapshot {
            Some(f) => trim_paragraphs(
                f.value()?["paragraphs"]
                    .as_array()
                    .ok_or_else(|| fault("规范化段落缺失"))?,
            ),
            None => (vec![], false),
        };
        fact["truncated"] = json!(truncated || fact["truncated"] == true);
        fact["transform_version"] = json!("html-text-v2-20k");
        fact["normalized_sha256"] = json!(crate::domain::hash(&serde_json::to_vec(&paragraphs)?));
        let raw = acquired
            .files
            .iter()
            .find(|f| f.slot.starts_with("capture.raw."))
            .map(|f| json!({"body_sha256":f.sha256(),"base64":STANDARD.encode(&f.bytes)}));
        Ok(ToolOutput::json(
            json!({"fact":fact,"paragraphs":paragraphs,"raw":raw}),
        ))
    }
}

pub struct ReadSource;
#[async_trait]
impl DomainTool for ReadSource {
    fn spec(&self) -> ToolSpec {
        ToolSpec { id: "inquiry.article_source".into(), description: "Read exact saved paragraphs of an explicitly authorized source package; use its package id/hash and source_id for citations.".into(),
            input_schema: json!({"type":"object","properties":{"artifact_id":{"type":"string"},"source_id":{"type":"string"},"keyword":{"type":"string","maxLength":200},"start":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":8}},"required":["artifact_id"],"additionalProperties":false}) }
    }
    async fn call(&self, c: ToolContext, args: Value) -> Result<ToolOutput> {
        let id = args["artifact_id"]
            .as_str()
            .ok_or_else(|| fault("缺少资料包身份"))?;
        let artifact = c
            .accepted
            .iter()
            .find(|a| a.reference.id == id && a.reference.slot == "corpus")
            .ok_or_else(|| fault("资料包未被当前阶段授权"))?;
        let Some(source_id) = args["source_id"].as_str() else {
            let package = corpus(artifact)?;
            return Ok(ToolOutput::json(
                json!({"artifact_id":id,"sha256":artifact.reference.sha256,"sources":package.sources.iter().map(|s| json!({"source_id":s.source_id,"origin":s.origin,"fact":s.fact,"paragraphs":s.paragraphs.len()})).collect::<Vec<_>>(),"limitations":package.limitations}),
            ));
        };
        let source = source_view(artifact, source_id)?;
        let keyword = args["keyword"].as_str().unwrap_or("").to_lowercase();
        let start = args["start"].as_u64().unwrap_or(0) as usize;
        let limit = args["limit"].as_u64().unwrap_or(5) as usize;
        let paragraphs = source.paragraphs.iter().enumerate().skip(start)
            .filter(|(_, p)| keyword.is_empty() || p.to_lowercase().contains(&keyword)).take(limit)
            .map(|(i,p)| json!({"paragraph":i,"text":p.chars().take(4000).collect::<String>(),"excerpted":p.chars().count()>4000})).collect::<Vec<_>>();
        Ok(ToolOutput::json(
            json!({"artifact_id":id,"sha256":artifact.reference.sha256,"source_id":source.source_id,"fact":source.fact,"total_paragraphs":source.paragraphs.len(),"paragraphs":paragraphs}),
        ))
    }
}
pub enum Native {
    Collect,
    Integrity,
}
#[async_trait]
impl Executor for Native {
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
        let result: Result<()> = async {
            match self {
                Self::Collect => {
                    let GoalRunRole::NodeMember { node_id, .. } = tools.goal_context().ok_or_else(|| fault("缺少 Goal 归属"))?.role else { return Err(fault("原生资料节点归属无效")); };
                    let response = collect(&inv, &tools, if node_id=="collect" {1} else {2}).await?;
                    for slot in ["corpus", "archive"] {
                        tools.call("artifact.write", json!({"slot":slot,"value":response[slot]})).await?;
                    }
                }
                Self::Integrity => {
                    let input: CheckInput = serde_json::from_value(inv.stage_input)?;
                    let CheckSubject::Artifacts { targets, .. } = &input.subject else { return Err(fault("需要确切报告目标")); };
                    let report: Report = serde_json::from_value(targets["article"].value.clone())?;
                    let accepted = targets.values().map(|t| ArtifactData { reference: ArtifactRef { id:t.artifact_id.clone(),sha256:t.sha256.clone(),slot:if t.value.get("sources").is_some(){"corpus"}else{"input"}.into(),state_id:"goal".into(),visit:1 },bytes:serde_json::to_vec(&t.value).expect("JSON value").into() }).collect::<Vec<_>>();
                    let result = check_report(&report, &targets["analysis"].value, &accepted);
                    let (verdict,message) = match result { Ok(()) => ("pass","引用版本、来源段落、分析血缘和短引文边界通过".to_owned()), Err(e) => ("fail",e.message) };
                    tools.call("artifact.write", json!({"slot":"evaluation","value":{"protocol_version":1,"verdict":verdict,"results":[{"id":"citation_integrity","verdict":verdict,"message":message,"evidence_refs":input.evidence_refs}],"evidence_refs":input.evidence_refs,"violations":[]}})).await?;
                }
            }
            Ok(())
        }.await;
        match result {
            Ok(()) => ExecutionOutcome::Completed {
                usage: json!({"native_model":false}),
            },
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
    #[test]
    fn actual_search_selection_is_round_robin_scoped_and_deduplicated() {
        let searches = vec![
            json!({"query":"a","status":"discovered","results":[{"url":"https://a.example/1"},{"url":"https://a.example/2"}]}),
            json!({"query":"b","status":"discovered","results":[{"url":"https://b.example/1"}]}),
            json!({"query":"bad","status":"unavailable","results":[{"url":"https://invented.example/"}]}),
        ];
        let found = candidates(
            &["https://seed.example/".into()],
            &searches,
            &BTreeSet::from(["https://a.example/2".into()]),
        );
        assert_eq!(
            found,
            vec![
                ("https://seed.example/".into(), "user-input".into()),
                ("https://a.example/1".into(), "search:a".into()),
                ("https://b.example/1".into(), "search:b".into())
            ]
        );
    }
    #[test]
    fn saved_paragraph_limit_is_character_based_and_explicit() {
        let (saved, truncated) = trim_paragraphs(&[json!("中".repeat(21000)), json!("later")]);
        assert!(truncated);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].chars().count(), 20000);
    }
}
