//! Inquiry-owned finite research graph, registered through public TaskLoop contracts.
use crate::article;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, sync::Arc};
use taskloop::*;

pub const PROFILE: &str = "inquiry-article@1";
pub const TEMPLATE: &str = "inquiry-article@1";
pub fn limits() -> GoalLimits {
    GoalLimits {
        max_wall_ms: 1_800_000,
        max_active_ms: 1_800_000,
        max_runs: 16,
        max_transitions: 2000,
        max_tool_calls: 300,
        max_total_candidate_bytes: 67_108_864,
        max_concurrent_runs: 2,
    }
}
pub fn bindings(agent: Arc<dyn Executor>, version: String) -> HostBindings {
    let search = Arc::new(crate::domain::Search::default());
    let mut b = HostBindings::default()
        .configuration_version(version)
        .executor("article.agent", agent)
        .executor("article.collect", Arc::new(article::Native::Collect))
        .executor("article.integrity", Arc::new(article::Native::Integrity))
        .tool(search.clone())
        .tool(Arc::new(article::FetchSource))
        .tool(Arc::new(article::ReadSource));
    b.grants.insert(
        "local-readonly".into(),
        Grant {
            definitions: vec![],
            tools: tools(),
            limits: ResourceLimits {
                max_active_ms: limits().max_active_ms,
                max_wall_ms: limits().max_wall_ms,
                max_runs: limits().max_runs,
                max_transitions: limits().max_transitions,
            },
        },
    );
    b
}
fn tools() -> Vec<String> {
    [
        "artifact.read",
        "artifact.write",
        "inquiry.search",
        "inquiry.article_fetch",
        "inquiry.article_source",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}
fn policy(id: &str, ms: u64, calls: u64) -> RunBudgetPolicy {
    RunBudgetPolicy {
        policy_id: format!("article-{id}-v1"),
        max_run_ms: Some(ms),
        max_check_ms: Some(2000),
        max_tool_calls: calls,
        submission_reserve: 2,
        finalize_before_ms: 2000,
        max_model_iterations: None,
        max_candidate_bytes: 4_194_304,
    }
}
fn entry(
    executor: &str,
    instruction: &str,
    input: &str,
    tools: &[&str],
    outputs: &[(&str, &str)],
    skill: bool,
) -> EntryContract {
    EntryContract {
        instruction: instruction.into(),
        executor: executor.into(),
        skills: if skill {
            vec!["inquiry-deep-article@0.4.0".into()]
        } else {
            vec![]
        },
        tools: tools.iter().map(|v| (*v).into()).collect(),
        input_schema: input.into(),
        requires: vec![],
        outputs: outputs
            .iter()
            .map(|(slot, schema)| {
                (
                    (*slot).into(),
                    OutputSlot {
                        schema: (*schema).into(),
                        required: true,
                    },
                )
            })
            .collect(),
        attachments: None,
        gate: "_tl.schema@1".into(),
        outcomes: vec!["completed".into()],
    }
}
fn port(schema: &str, required: bool) -> Value {
    json!({"schema_ref":schema,"required":required,"source_policy":{"kind":"artifact"}})
}
fn capability(id: &str, inputs: Value, outputs: Value, question: bool) -> Value {
    let projections = outputs
        .as_object()
        .expect("output object")
        .keys()
        .map(|slot| {
            (
                slot.clone(),
                json!({"kind":"member","member_id":"worker","slot":slot}),
            )
        })
        .collect::<BTreeMap<_, _>>();
    json!({"protocol_version":1,"id":id,"version":"1","kind":"work","inputs":inputs,"outputs":outputs,"gate":"_tl.schema@1","max_rule_check_ms":2000,"members":{"worker":{"input_mode":"node_ports","entry_ref":format!("{id}@1"),"input_bindings":if question {json!({"question":{"port":"question","pointer":""}})} else {json!({})}}},"join":{"kind":"object-by-worker","outputs":projections}})
}
fn question_port() -> Value {
    json!({"schema_ref":"article-text","required":true,"source_policy":{"kind":"goal","pointer":"/goal"}})
}
fn from(node: &str, port: &str) -> Value {
    json!({"kind":"node","node_id":node,"port":port,"pointer":""})
}
fn template_node(cap: &str, deps: &[&str], inputs: Value) -> Value {
    let mut dependencies = deps
        .iter()
        .map(|s| (*s).to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    for binding in inputs.as_object().expect("typed template inputs").values() {
        if binding["kind"] == "node" {
            dependencies.insert(
                binding["node_id"]
                    .as_str()
                    .expect("typed node reference")
                    .to_owned(),
            );
        }
    }
    json!({"capabilities":[format!("{cap}@1")],"depends_on":dependencies,"inputs":inputs,"fillable_inputs":[]})
}

pub fn registry(
    root: &Path,
    bindings: &HostBindings,
) -> Result<(OrchestrationRegistry, OrchestrationOptions)> {
    let mut r = OrchestrationRegistry::new();
    for name in [
        "text",
        "input",
        "question",
        "empty",
        "planner-input",
        "clarification",
        "plan",
        "report",
        "analysis",
        "corpus",
        "archive",
    ] {
        r.register_schema_file(
            format!("article-{name}"),
            root,
            &format!("resources/article/{name}.json"),
        )?;
    }
    r.register_skill_root(
        "inquiry-deep-article@0.4.0",
        &root.join("skills/inquiry-deep-article"),
    )?;
    let planner = "Plan ONLY the supplied frozen Workflow template. The original goal, flow_input.scope and signal_facts are authoritative. If the object is ambiguous or a decisive clarification is missing, use a focused inquiry.search query to check its identity; submit PlannerResult kind=needs_input,input_schema_ref=article-clarification,reason explaining exactly what the user must clarify. Never substitute another similarly named object. Search transport failure alone does not mean the user must provide URLs; preserve it as a service limitation rather than inventing missing user input. When clear, copy task_input.template into a PlanProposal: protocol_version=1, goal_spec_digest/profile_digest from task_input, planning_mode=workflow and EXACT template_digest from task_input. For each template node select its sole capability as capability_ref, copy depends_on/inputs, copy outputs/checks. Do not include capabilities or fillable_inputs in a proposal node. Do not add a template_ref field to PlanProposal (it belongs to GoalPlanning only). No extra nodes or execution here. Submit PlannerResult {protocol_version:1,kind:proposal,proposal:...} to proposal through artifact.write. Do not request plan approval when the question is already clear.";
    let scope = "Execute only research scope. Preserve the full original question and user scope, incorporate explicit signal_facts without overwriting the original. Submit plan with all substantive subquestions, at most eight targeted search_queries (each <=300 characters), and stop_reason defining completion. Cover identity, mechanism, real evidence, alternatives, limitations and counterevidence as applicable. No article or claims of verified facts yet.";
    let analysis = "Analyze the actual saved sources for the full original question. Read the plan and any prior analysis via artifact.read. For every authorized corpus artifact first call inquiry.article_source with artifact_id alone for its source index, then with source_id/keyword/start/limit to read relevant saved paragraphs. Do not whole-read corpus with artifact.read. Evidence tuples must use actual package artifact_id+sha256+source_id+zero-based paragraph+exact short quote. Give linked support/counter judgments and per-subquestion coverage with an evidence-based conclusion or unknown. Distinguish hypotheses, recommendations and unmeasured results. Initial analysis: declare decisive gaps and up to eight focused follow-up search_queries. If two corpora or prior analysis are present, this is final synthesis: integrate both, resolve source relationships/conflicts, preserve unresolved gaps honestly and set search_queries=[]; do not propose unlimited work. decision=ready only when no decisive gaps remain, otherwise need_input. Empty sources require honest limitations and no fabricated evidence. Submit analysis.";
    let draft = "Write ONLY the initial complete Chinese article for the original question. Read accepted plan/analysis, use inquiry.article_source for relevant saved evidence. Develop mechanisms, cross-source synthesis, evidence-based conclusions, credible counterarguments and limits; do not produce a short summary, source list or process log. Copy citation tuples from analysis, use [1]... in body, cumulative short quotations <=25 words per URL. Answer each required part; label conditional recommendations and unknowns. Insufficient evidence remains an honest partial article, not answered by inventing claims. Submit full report to report slot.";
    let revision = "Make ONLY the one predefined revision. Read initial draft, independent review and final analysis via artifact.read; read saved evidence with inquiry.article_source when needed. Address each decisive review deficiency using existing saved material, retain unresolved gaps honestly, and preserve original scope. No searching, new tasks, repeated revision or self-awarded pass. Submit a complete standalone final report, with exact citations copied from the accepted analysis and <=25 quoted words per URL. If no correction is justified, retain the article with explicit rationale in limitations. This is not a chat response: submit report.";
    let semantic = "Independently evaluate the EXACT CheckInput subject article (alias article for final or draft for initial) against the ORIGINAL goal and goal_inputs.scope, plan, analysis, and frozen Q1-Q6 rules. You are isolated from the author; never accept answer_status as proof. Each result message MUST name the relevant article section/claim, actual saved source_id+paragraph and support/conflict or decisive gap. Use inquiry.article_source on authorized corpus1/corpus2 IDs to verify key facts and counterclaims against actual saved paragraphs. Do not just read the citation list or analysis summaries. Check all original subquestions even if the author's plan omitted them. Research quality is distinct from deployment validation: when the question explicitly lacks measurements and asks for a pilot proposal, judge the supported reasoning, measurement/exit plan and resource feasibility; do not demand nonexistent results or invent facts. Unknown essential comparison facts cannot be deferred as if already answered. All six rules are mandatory; output pass/fail/error per rule, error takes precedence then fail. Copy exact rule IDs and allowed evidence_refs only from CheckInput. No article rewriting, extra search or author context. Submit only Core Evaluation candidate to evaluation slot; Runtime owns identity and version binding.";
    for (id, executor, instruction, input, allowed, outputs, ms, calls, skill) in [
        (
            "article-planner",
            "article.agent",
            planner,
            "article-planner-input",
            vec!["inquiry.search", "artifact.write"],
            vec![("proposal", "_tl.planner-result@1")],
            180000,
            12,
            false,
        ),
        (
            "article-scope",
            "article.agent",
            scope,
            "article-question",
            vec!["artifact.write"],
            vec![("plan", "article-plan")],
            180000,
            6,
            true,
        ),
        (
            "article-collect",
            "article.collect",
            "Collect and preserve real public sources.",
            "article-empty",
            vec![
                "inquiry.search",
                "inquiry.article_fetch",
                "artifact.read",
                "artifact.write",
            ],
            vec![("corpus", "article-corpus"), ("archive", "article-archive")],
            480000,
            40,
            false,
        ),
        (
            "article-analysis",
            "article.agent",
            analysis,
            "article-question",
            vec!["artifact.read", "artifact.write", "inquiry.article_source"],
            vec![("analysis", "article-analysis")],
            300000,
            80,
            true,
        ),
        (
            "article-draft",
            "article.agent",
            draft,
            "article-question",
            vec!["artifact.read", "artifact.write", "inquiry.article_source"],
            vec![("report", "article-report")],
            300000,
            40,
            true,
        ),
        (
            "article-revise",
            "article.agent",
            revision,
            "article-question",
            vec!["artifact.read", "artifact.write", "inquiry.article_source"],
            vec![("report", "article-report")],
            300000,
            40,
            true,
        ),
        (
            "article-semantic",
            "article.agent",
            semantic,
            "_tl.check-input@1",
            vec!["inquiry.article_source", "artifact.write"],
            vec![("evaluation", "_tl.evaluation-candidate@1")],
            240000,
            60,
            true,
        ),
        (
            "article-integrity",
            "article.integrity",
            "Check exact source versions and citation lineage.",
            "_tl.check-input@1",
            vec!["artifact.write"],
            vec![("evaluation", "_tl.evaluation-candidate@1")],
            30000,
            2,
            false,
        ),
    ] {
        r.register_entry(
            format!("{id}@1"),
            entry(executor, instruction, input, &allowed, &outputs, skill),
            policy(id, ms, calls),
        )?;
    }
    r.register_capability(serde_json::from_value(capability(
        "article-scope",
        json!({"question":question_port()}),
        json!({"plan":{"schema_ref":"article-plan"}}),
        true,
    ))?)?;
    r.register_capability(serde_json::from_value(capability("article-collect",json!({"plan":port("article-plan",true),"analysis":port("article-analysis",false),"corpus":port("article-corpus",false)}),json!({"corpus":{"schema_ref":"article-corpus"},"archive":{"schema_ref":"article-archive"}}),false))?)?;
    let common = json!({"question":question_port(),"plan":port("article-plan",true),"corpus1":port("article-corpus",true),"corpus2":port("article-corpus",false),"analysis":port("article-analysis",false)});
    r.register_capability(serde_json::from_value(capability(
        "article-analysis",
        common.clone(),
        json!({"analysis":{"schema_ref":"article-analysis"}}),
        true,
    ))?)?;
    let mut writing = common;
    writing["analysis"] = port("article-analysis", true);
    writing["corpus2"] = port("article-corpus", true);
    r.register_capability(serde_json::from_value(capability(
        "article-draft",
        writing.clone(),
        json!({"report":{"schema_ref":"article-report"}}),
        true,
    ))?)?;
    writing["draft"] = port("article-report", true);
    writing["review"] = port("_tl.evaluation-candidate@1", true);
    r.register_capability(serde_json::from_value(capability(
        "article-revise",
        writing,
        json!({"report":{"schema_ref":"article-report"}}),
        true,
    ))?)?;
    let review_ports = json!({"draft":port("article-report",true),"plan":port("article-plan",true),"analysis":port("article-analysis",true),"corpus1":port("article-corpus",true),"corpus2":port("article-corpus",true)});
    r.register_capability(serde_json::from_value(json!({"protocol_version":1,"id":"article-review","version":"1","kind":"work","inputs":review_ports,"outputs":{"evaluation":{"schema_ref":"_tl.evaluation-candidate@1"}},"gate":"_tl.schema@1","max_rule_check_ms":2000,"members":{"checker":{"entry_ref":"article-semantic@1","input_mode":"check_request"}},"join":{"kind":"object-by-worker","outputs":{"evaluation":{"kind":"member","member_id":"checker","slot":"evaluation"}}},"evaluation":{"purpose":"data","checker_ref":"article-semantic@1","target_ports":{"draft":"draft","plan":"plan","analysis":"analysis","corpus1":"corpus1","corpus2":"corpus2"}}}))?)?;
    let mut requirements = Vec::new();
    let mut plan_checks = BTreeMap::new();
    let semantic_rules = [
        (
            "Q1",
            "问题覆盖",
            "Original question and every substantive required subquestion are answered, with honest supported limits; no substitution or deferred essential comparison.",
        ),
        (
            "Q2",
            "机制解释",
            "Evidence-based explanation of how the object works and its key relationships; not marketing labels or invented internals.",
        ),
        (
            "Q3",
            "跨源综合",
            "Key judgments explain source relationships, independence, conditions, differences and conflicts; not citation stacking.",
        ),
        (
            "Q4",
            "论证与证据",
            "Key claims are actually supported by saved source content, with traceable reasoning and conditions; no major unsupported claim.",
        ),
        (
            "Q5",
            "反证与边界",
            "Credible counterevidence, alternatives and conditions that could change the conclusion are substantively addressed.",
        ),
        (
            "Q6",
            "成稿与结论",
            "A coherent complete article develops and resolves the original research question, rather than snippets, repetition or a future to-do list.",
        ),
    ];
    for (name,rules) in [
        ("integrity",vec![json!({"id":"citation_integrity","instruction":"Verify exact saved source bytes, citations and analysis lineage.","pass_condition":"All citation checks pass and report is answered.","evidence_required":true})]),
        ("semantic",semantic_rules.iter().map(|(id,title,condition)|json!({"id":id,"instruction":format!("{title}: inspect actual article sections and saved source paragraphs."),"pass_condition":condition,"evidence_required":true})).collect()),
    ] {
        let digest=r.register_rules(format!("article-{name}"),&serde_json::to_vec(&json!({"protocol_version":1,"id":format!("article-{name}"),"version":"1","rules":rules}))?)?;
        r.register_checker(format!("article-{name}@1"),CheckerContract{entry_ref:format!("article-{name}@1"),rules_ref:format!("article-{name}"),result_slot:"evaluation".into(),fresh_context:true,goal_input_fields:vec!["scope".into()]})?;
        let targets=vec!["article","analysis","plan","corpus1","corpus2"];
        requirements.push(json!({"id":name,"phase":"goal_final","checker_ref":format!("article-{name}@1"),"rules_digest":digest,"target_outputs":targets,"required":true}));
        plan_checks.insert(name,json!({"phase":"goal_final","checker_ref":format!("article-{name}@1"),"targets":targets}));
    }
    let goal = json!({"kind":"goal","pointer":"/goal"});
    let mut nodes = BTreeMap::new();
    nodes.insert(
        "scope",
        template_node("article-scope", &[], json!({"question":goal})),
    );
    nodes.insert(
        "collect",
        template_node(
            "article-collect",
            &["scope"],
            json!({"plan":from("scope","plan")}),
        ),
    );
    nodes.insert(
        "analyze",
        template_node(
            "article-analysis",
            &["collect"],
            json!({"question":goal,"plan":from("scope","plan"),"corpus1":from("collect","corpus")}),
        ),
    );
    nodes.insert("supplement",template_node("article-collect",&["analyze"],json!({"plan":from("scope","plan"),"analysis":from("analyze","analysis"),"corpus":from("collect","corpus")})));
    let analysis_inputs = json!({"question":goal,"plan":from("scope","plan"),"corpus1":from("collect","corpus"),"corpus2":from("supplement","corpus"),"analysis":from("analyze","analysis")});
    nodes.insert(
        "synthesize",
        template_node("article-analysis", &["supplement"], analysis_inputs.clone()),
    );
    let mut draft_inputs = analysis_inputs;
    draft_inputs["analysis"] = from("synthesize", "analysis");
    nodes.insert(
        "draft",
        template_node("article-draft", &["synthesize"], draft_inputs.clone()),
    );
    let review_inputs = json!({"draft":from("draft","report"),"plan":from("scope","plan"),"analysis":from("synthesize","analysis"),"corpus1":from("collect","corpus"),"corpus2":from("supplement","corpus")});
    nodes.insert(
        "review",
        template_node("article-review", &["draft"], review_inputs),
    );
    draft_inputs["draft"] = from("draft", "report");
    draft_inputs["review"] = from("review", "evaluation");
    nodes.insert(
        "revise",
        template_node("article-revise", &["review"], draft_inputs),
    );
    let outputs = json!({"plan":{"node_id":"scope","port":"plan"},"corpus1":{"node_id":"collect","port":"corpus"},"archive1":{"node_id":"collect","port":"archive"},"analysis_initial":{"node_id":"analyze","port":"analysis"},"corpus2":{"node_id":"supplement","port":"corpus"},"archive2":{"node_id":"supplement","port":"archive"},"analysis":{"node_id":"synthesize","port":"analysis"},"draft":{"node_id":"draft","port":"report"},"review":{"node_id":"review","port":"evaluation"},"article":{"node_id":"revise","port":"report"}});
    r.register_template(serde_json::from_value(json!({"id":"inquiry-article","version":"1","nodes":nodes,"outputs":outputs,"checks":plan_checks}))?)?;
    let mut profile_outputs = BTreeMap::new();
    for (alias, schema, cap) in [
        ("plan", "article-plan", "article-scope"),
        ("corpus1", "article-corpus", "article-collect"),
        ("corpus2", "article-corpus", "article-collect"),
        ("archive1", "article-archive", "article-collect"),
        ("archive2", "article-archive", "article-collect"),
        ("analysis_initial", "article-analysis", "article-analysis"),
        ("analysis", "article-analysis", "article-analysis"),
        ("draft", "article-report", "article-draft"),
        ("article", "article-report", "article-revise"),
        ("review", "_tl.evaluation-candidate@1", "article-review"),
    ] {
        profile_outputs.insert(
            alias,
            json!({"schema_ref":schema,"allowed_capabilities":[format!("{cap}@1")]}),
        );
    }
    let capabilities = [
        "article-scope@1",
        "article-collect@1",
        "article-analysis@1",
        "article-draft@1",
        "article-review@1",
        "article-revise@1",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    r.register_profile(serde_json::from_value(json!({"id":"inquiry-article","version":"1","goal_input_schema":"article-input","goal_selectors":{"/goal":"article-text"},"planner_entry_ref":"article-planner@1","capabilities":capabilities,"planning_input_schemas":["article-clarification"],"templates":[TEMPLATE],"outputs":profile_outputs,"final_outputs":["article"],"checks":requirements,"allowed_tools":tools()}))?)?;
    let receipt = r.validate_and_freeze(bindings)?;
    let options = OrchestrationOptions {
        grants: BTreeMap::from([(
            "local-readonly".into(),
            GoalGrant {
                profile_digests: vec![receipt.profiles[PROFILE].clone()],
                capabilities,
                tools: tools(),
                limits: limits(),
            },
        )]),
        limits: limits(),
        max_admitted_goals: 8,
        max_spec_bytes: 1_048_576,
        max_plan_bytes: 4_194_304,
        max_nodes: 32,
        max_edges: 128,
        max_workers_per_node: 2,
        max_json_candidate_bytes: 4_194_304,
    };
    Ok((r, options))
}
