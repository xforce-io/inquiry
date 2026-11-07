//! Inquiry declares its policy; TaskLoop owns accounting and admission.
use taskloop::{RunBudgetPolicy, RuntimeOptions};

pub const ANALYSIS_CALLS: u64 = 80;
pub const DEFAULT_CALLS: u64 = 40;
pub const SUBMISSION_RESERVE: u64 = 2;

pub fn runtime_options(store: impl Into<std::path::PathBuf>) -> RuntimeOptions {
    let mut options = RuntimeOptions::new(store);
    options.max_tool_calls_per_run = ANALYSIS_CALLS;
    for state in [
        "scope",
        "discover",
        "capture",
        "catalogue",
        "analyze",
        "report",
    ] {
        options.entry_budgets.insert(
            format!("inquiry.{state}"),
            RunBudgetPolicy {
                policy_id: "inquiry-tools-v1".into(),
                max_run_ms: None,
                max_check_ms: None,
                max_tool_calls: if state == "analyze" {
                    ANALYSIS_CALLS
                } else {
                    DEFAULT_CALLS
                },
                submission_reserve: SUBMISSION_RESERVE,
                finalize_before_ms: 0,
                max_model_iterations: None,
                max_candidate_bytes: options.max_candidate_bytes,
            },
        );
    }
    options
}
