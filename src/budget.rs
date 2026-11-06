//! Inquiry's per-entry tool policy, enforced through TaskLoop's public bridge.
use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use taskloop::{Error, Result, RuntimeOptions, ToolBridge, ToolSpec};

pub const ANALYSIS_CALLS: u64 = 80;
const DEFAULT_CALLS: u64 = 40;
const SUBMISSION_RESERVE: u64 = 2;

pub fn runtime_options(store: impl Into<std::path::PathBuf>) -> RuntimeOptions {
    let mut options = RuntimeOptions::new(store);
    options.max_tool_calls_per_run = ANALYSIS_CALLS;
    options
}

pub struct BudgetBridge {
    inner: Arc<dyn ToolBridge>,
    limit: u64,
    used: AtomicU64,
}
impl BudgetBridge {
    pub fn new(inner: Arc<dyn ToolBridge>, state: &str) -> Self {
        Self {
            inner,
            limit: if state == "analyze" {
                ANALYSIS_CALLS
            } else {
                DEFAULT_CALLS
            },
            used: AtomicU64::new(0),
        }
    }
    fn remaining(&self) -> Value {
        let used = self.used.load(Ordering::SeqCst);
        json!({"limit":self.limit,"used":used,"remaining":self.limit.saturating_sub(used),
            "read_remaining":(self.limit-SUBMISSION_RESERVE).saturating_sub(used),
            "submission_reserve":SUBMISSION_RESERVE})
    }
}
#[async_trait]
impl ToolBridge for BudgetBridge {
    fn specs(&self) -> Vec<ToolSpec> {
        let mut specs = self.inner.specs();
        for spec in &mut specs {
            spec.description.push_str(&format!(
                " Host budget: {} calls for this entry, including failed admitted calls; last {} calls reserved for artifact.write. Read _inquiry_budget in responses. When read_remaining is 0, stop reading and submit the required output with honest limitations. Do not retry a budget refusal.",
                self.limit, SUBMISSION_RESERVE));
        }
        specs
    }
    async fn call(&self, id: &str, args: Value) -> Result<Value> {
        let ceiling = if id == "artifact.write" {
            self.limit
        } else {
            self.limit - SUBMISSION_RESERVE
        };
        self.used.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |used| {
            (used < ceiling).then_some(used + 1)
        }).map_err(|_| Error::new("limit_reached", "/inquiry/tool_budget",
            format!("Inquiry tool budget: {}. Stop reading; submit the required output using artifact.write with existing evidence and explicit limitations. Never invent evidence or retry a refused read.", self.remaining())))?;
        match self.inner.call(id, args).await {
            Ok(mut value) => {
                // Metadata is outside trusted result/receipt and saved artifact bytes.
                if let Some(object) = value.as_object_mut() {
                    object.insert("_inquiry_budget".into(), self.remaining());
                }
                Ok(value)
            }
            Err(mut error) => {
                error
                    .message
                    .push_str(&format!("; Inquiry tool budget: {}", self.remaining()));
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Echo;
    #[async_trait]
    impl ToolBridge for Echo {
        fn specs(&self) -> Vec<ToolSpec> {
            vec![]
        }
        async fn call(&self, _: &str, args: Value) -> Result<Value> {
            Ok(json!({"result":args,"receipt_id":"unchanged"}))
        }
    }
    #[tokio::test]
    async fn phase_limits_reserve_submission_without_changing_receipts() {
        for (state, limit) in [("analyze", 80), ("capture", 40), ("report", 40)] {
            let bridge = BudgetBridge::new(Arc::new(Echo), state);
            for _ in 0..limit - 2 {
                let got = bridge
                    .call("inquiry.read_source", json!({"source":"saved"}))
                    .await
                    .unwrap();
                assert_eq!(got["receipt_id"], "unchanged");
                assert_eq!(got["result"], json!({"source":"saved"}));
            }
            assert_eq!(
                bridge
                    .call("artifact.read", json!({}))
                    .await
                    .unwrap_err()
                    .path,
                "/inquiry/tool_budget"
            );
            for remaining in [1, 0] {
                let got = bridge.call("artifact.write", json!({})).await.unwrap();
                assert_eq!(got["_inquiry_budget"]["remaining"], remaining);
            }
            assert!(bridge.call("artifact.write", json!({})).await.is_err());
        }
    }
    struct Reject;
    #[async_trait]
    impl ToolBridge for Reject {
        fn specs(&self) -> Vec<ToolSpec> {
            vec![ToolSpec {
                id: "artifact.write".into(),
                description: "Submit".into(),
                input_schema: json!({"type":"object"}),
            }]
        }
        async fn call(&self, _: &str, _: Value) -> Result<Value> {
            Err(Error::new(
                "artifact_invalid",
                "/output",
                "fixture rejection",
            ))
        }
    }
    #[tokio::test]
    async fn failed_calls_consume_budget_and_keep_original_error() {
        let bridge = BudgetBridge::new(Arc::new(Reject), "scope");
        assert!(bridge.specs()[0].description.contains("40 calls"));
        for used in 1..=40 {
            let error = bridge.call("artifact.write", json!({})).await.unwrap_err();
            assert_eq!(error.code, "artifact_invalid");
            assert_eq!(error.path, "/output");
            assert!(error.message.starts_with("fixture rejection"));
            assert_eq!(bridge.remaining()["used"], used);
        }
        assert_eq!(
            bridge
                .call("artifact.write", json!({}))
                .await
                .unwrap_err()
                .path,
            "/inquiry/tool_budget"
        );
    }
    #[tokio::test]
    async fn concurrent_reads_cannot_consume_submission_reserve() {
        let bridge = Arc::new(BudgetBridge::new(Arc::new(Echo), "analyze"));
        let mut tasks = Vec::new();
        for _ in 0..100 {
            let bridge = bridge.clone();
            tasks.push(tokio::spawn(async move {
                bridge.call("artifact.read", json!({})).await
            }));
        }
        let mut admitted = 0;
        for task in tasks {
            if task.await.unwrap().is_ok() {
                admitted += 1;
            }
        }
        assert_eq!(admitted, 78);
        assert_eq!(bridge.remaining()["remaining"], 2);
        assert!(bridge.call("artifact.write", json!({})).await.is_ok());
        assert!(bridge.call("artifact.write", json!({})).await.is_ok());
    }
}
