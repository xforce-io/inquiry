//! Configuration admission tests; never proof of authentication or a real model run.
use inquiry::execution::ExecutionSetup;
use serde_json::{Value, json};
use std::path::PathBuf;
struct ConfigFixture {
    root: tempfile::TempDir,
    path: PathBuf,
    value: Value,
}
impl ConfigFixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for name in ["native-config", "native-sessions", "data"] {
            std::fs::create_dir(root.path().join(name)).unwrap();
        }
        for name in ["node", "milkie.js"] {
            std::fs::write(root.path().join(name), "fixture-not-executed").unwrap();
        }
        std::fs::write(root.path().join("connection.json"),br#"{"contractVersion":1,"fields":{"transport":"agent-cli","runtime":"codex","model":"gpt-6.1-sol"}}"#).unwrap();
        std::fs::write(
            root.path().join("native-config/config.toml"),
            "model_reasoning_effort = \"high\"\n",
        )
        .unwrap();
        let value = json!({"node":root.path().join("node"),"milkie_cli":root.path().join("milkie.js"),"connection_file":root.path().join("connection.json"),"config_dir":root.path().join("native-config"),"session_dir":root.path().join("native-sessions"),"trace":{"policy_id":"test-explicit-v1","max_events_per_run":1000,"max_bytes_per_run":1048576,"source_max_events":1000,"source_max_bytes":262144,"max_total_events":10000,"max_total_bytes":4194304,"max_runs":16,"retention_ms":60000}});
        let path = root.path().join("execution.json");
        Self { root, path, value }
    }
    fn write(&self) {
        std::fs::write(&self.path, serde_json::to_vec(&self.value).unwrap()).unwrap();
    }
    fn load(&self) -> taskloop::Result<ExecutionSetup> {
        self.write();
        ExecutionSetup::load(&self.path, &self.root.path().join("data"))
    }
    fn error(&self) -> taskloop::Error {
        self.load().err().expect("configuration must be rejected")
    }
}
#[tokio::test]
async fn valid_explicit_configuration_opens_metadata_trace_without_loading_credentials() {
    let f = ConfigFixture::new();
    let setup = f.load().unwrap();
    assert!(!f.root.path().join("native-config/auth.json").exists());
    let (runtime, _) = setup
        .open(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
            &f.root.path().join("data"),
        )
        .await
        .unwrap();
    // No task has started and no native executable or account is needed merely to admit config.
    runtime.shutdown().await.unwrap();
    assert!(!f.root.path().join("native-config/auth.json").exists());
    assert!(!f.root.path().join("data/research-audit.ndjson").exists());
}
#[test]
fn missing_or_relative_sources_do_not_fall_back_to_global_settings() {
    let mut f = ConfigFixture::new();
    f.value["node"] = json!("node");
    assert_eq!(f.error().code, "input_invalid");
    f.value["node"] = json!(f.root.path().join("missing"));
    assert_eq!(f.error().code, "input_invalid");
    f.value["node"] = json!(f.root.path().join("node"));
    f.value.as_object_mut().unwrap().remove("trace");
    assert_eq!(f.error().code, "input_invalid");
}
#[test]
fn configuration_and_session_storage_must_be_separate_from_framework_storage() {
    let mut f = ConfigFixture::new();
    f.value["session_dir"] = f.value["config_dir"].clone();
    assert_eq!(f.error().path, "/execution/storage");
    f.value["session_dir"] = json!(f.root.path().join("data/taskloop"));
    std::fs::create_dir(f.root.path().join("data/taskloop")).unwrap();
    assert_eq!(f.error().path, "/execution/storage");
}
#[test]
fn low_or_profile_overridden_effort_is_rejected_without_disclosing_config_content() {
    let f = ConfigFixture::new();
    for native in [
        "model_reasoning_effort='low'\n",
        "model_reasoning_effort='high'\nprofile='low'\n",
        "api_key='CONFIG-SECRET\n",
    ] {
        std::fs::write(f.root.path().join("native-config/config.toml"), native).unwrap();
        let e = f.error();
        assert_eq!(e.path, "/execution/native");
        assert!(!e.to_string().contains("CONFIG-SECRET"));
    }
}
#[test]
fn connection_does_not_silently_change_model_runtime_or_inherit_environment() {
    let f = ConfigFixture::new();
    for value in [
        json!({"contractVersion":1,"fields":{"transport":"agent-cli","runtime":"codex","model":"gpt-6-astra"}}),
        json!({"contractVersion":1,"fields":{"transport":"agent-cli","runtime":"pi","model":"gpt-6.1-sol"}}),
        json!({"contractVersion":1,"fields":{"transport":"agent-cli","runtime":"codex","model":"gpt-6.1-sol"},"env":{"OPENAI_API_KEY":"CONFIG-SECRET"}}),
    ] {
        std::fs::write(
            f.root.path().join("connection.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        let e = f.error();
        assert_eq!(e.path, "/execution/connection");
        assert!(!e.to_string().contains("CONFIG-SECRET"));
    }
}
#[tokio::test]
async fn invalid_trace_policy_cannot_start_the_runtime_or_native_execution() {
    let mut f = ConfigFixture::new();
    f.value["trace"]["retention_ms"] = json!(0);
    let setup = f.load().unwrap();
    let result = setup
        .open(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
            &f.root.path().join("data"),
        )
        .await;
    assert_eq!(result.err().unwrap().code, "input_invalid");
    assert!(
        f.root
            .path()
            .join("data/milkie")
            .read_dir()
            .unwrap()
            .next()
            .is_none()
    );
}

#[cfg(unix)]
#[test]
fn framework_storage_symlinks_cannot_alias_native_configuration_or_sessions() {
    for framework in ["milkie", "taskloop"] {
        for native in ["native-config", "native-sessions"] {
            let f = ConfigFixture::new();
            std::os::unix::fs::symlink(
                f.root.path().join(native),
                f.root.path().join("data").join(framework),
            )
            .unwrap();
            assert_eq!(f.error().path, "/execution/storage");
        }
    }
}

#[cfg(unix)]
#[test]
fn explicit_native_binary_enforces_high_without_mutating_shared_configuration() {
    use std::os::unix::fs::PermissionsExt;
    let mut f = ConfigFixture::new();
    let native = f.root.path().join("native's codex");
    std::fs::write(&native, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n").unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
    f.value["codex_binary"] = json!(native);
    f.value["reuse_native_login"] = json!(true);
    let sessions = f.root.path().join("native-config/sessions");
    std::fs::create_dir(&sessions).unwrap();
    f.value["session_dir"] = json!(sessions);
    let config = f.root.path().join("native-config/config.toml");
    let before = b"model_reasoning_effort='medium'\n";
    std::fs::write(&config, before).unwrap();
    f.load().unwrap();
    let launcher = std::fs::read_dir(f.root.path().join("data"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("codex-launcher-")
        })
        .unwrap()
        .join("codex");
    let output = std::process::Command::new(&launcher)
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        ["-c", "model_reasoning_effort=\"high\"", "--version"]
    );
    assert_eq!(std::fs::read(&config).unwrap(), before);
    assert!(!f.root.path().join("native-config/auth.json").exists());
    // A second assembly may reuse identical generated policy, never overwrite a replacement.
    f.load().unwrap();
    std::fs::write(&launcher, "replacement").unwrap();
    assert_eq!(f.error().path, "/execution/codex");
}

#[test]
fn shared_login_requires_explicit_binary_and_the_existing_native_sessions() {
    let mut f = ConfigFixture::new();
    f.value["reuse_native_login"] = json!(true);
    let sessions = f.root.path().join("native-config/sessions");
    std::fs::create_dir(&sessions).unwrap();
    f.value["session_dir"] = json!(sessions);
    assert_eq!(f.error().path, "/execution/storage");
    f.value["codex_binary"] = f.value["node"].clone();
    f.value["session_dir"] = json!(f.root.path().join("native-sessions"));
    assert_eq!(f.error().path, "/execution/storage");
}
