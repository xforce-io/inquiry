//! Explicit host configuration; no credential copying or native harness fallback.
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use taskloop::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecutionConfig {
    node: PathBuf,
    milkie_cli: PathBuf,
    connection_file: PathBuf,
    config_dir: PathBuf,
    session_dir: PathBuf,
    trace: TraceConfig,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TraceConfig {
    policy_id: String,
    max_events_per_run: u64,
    max_bytes_per_run: u64,
    source_max_events: u64,
    source_max_bytes: u64,
    max_total_events: u64,
    max_total_bytes: u64,
    max_runs: u32,
    retention_ms: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Connection {
    contract_version: u32,
    fields: ConnectionFields,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectionFields {
    transport: String,
    runtime: String,
    model: String,
}
fn invalid(path: &str, message: &str) -> Error {
    Error::new("input_invalid", path, message)
}
fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path)
        .map_err(|_| invalid("/execution/config", "无法读取指定配置文件"))?;
    if !file
        .metadata()
        .map_err(|_| invalid("/execution/config", "无法读取配置文件属性"))?
        .is_file()
    {
        return Err(invalid("/execution/config", "配置必须为普通文件"));
    }
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid("/execution/config", "无法读取指定配置文件"))?;
    if bytes.len() as u64 > max {
        return Err(invalid("/execution/config", "配置文件超过大小边界"));
    }
    Ok(bytes)
}
fn absolute_file(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err(invalid("/execution/config", "执行配置必须使用绝对路径"));
    }
    let real = path
        .canonicalize()
        .map_err(|_| invalid("/execution/config", "指定文件不存在"))?;
    if !real.is_file() {
        return Err(invalid("/execution/config", "指定路径必须为文件"));
    }
    Ok(real)
}
fn absolute_directory(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err(invalid("/execution/storage", "专用目录必须使用绝对路径"));
    }
    let real = path
        .canonicalize()
        .map_err(|_| invalid("/execution/storage", "专用目录必须由宿主提前准备"))?;
    if !real.is_dir() {
        return Err(invalid("/execution/storage", "专用存储必须为目录"));
    }
    Ok(real)
}
/// A validated assembly. Only the native adapter reads auth.json.
pub struct ExecutionSetup {
    executor: Arc<MilkieExecutor>,
    trace: TraceOptions,
    policy: TracePolicy,
    binding: String,
}
impl ExecutionSetup {
    pub fn load(path: &Path, data: &Path) -> Result<Self> {
        let bytes = read_bounded(&absolute_file(path)?, 65536)?;
        let config: ExecutionConfig = serde_json::from_value(
            parse_unique(&bytes)
                .map_err(|_| invalid("/execution/config", "执行配置不是有效且无重复字段的JSON"))?,
        )
        .map_err(|_| invalid("/execution/config", "执行配置字段缺失或不受支持"))?;
        let node = absolute_file(&config.node)?;
        let cli = absolute_file(&config.milkie_cli)?;
        let connection = absolute_file(&config.connection_file)?;
        let config_dir = absolute_directory(&config.config_dir)?;
        let session_dir = absolute_directory(&config.session_dir)?;
        if config_dir.starts_with(&session_dir) || session_dir.starts_with(&config_dir) {
            return Err(invalid(
                "/execution/storage",
                "配置与会话必须使用互不包含的专用目录",
            ));
        }
        let conn_bytes = read_bounded(&connection, 65536)?;
        let conn: Connection = serde_json::from_value(parse_unique(&conn_bytes).map_err(|_| {
            invalid(
                "/execution/connection",
                "连接文件不是有效且无重复字段的JSON",
            )
        })?)
        .map_err(|_| {
            invalid(
                "/execution/connection",
                "研究连接必须显式指定原生Codex及固定模型",
            )
        })?;
        if conn.contract_version != 1
            || conn.fields.transport != "agent-cli"
            || conn.fields.runtime != "codex"
            || conn.fields.model != "gpt-6.1-sol"
        {
            return Err(invalid(
                "/execution/connection",
                "研究固定使用gpt-6.1-sol及原生Codex，不回退模型或执行器",
            ));
        }
        // Preserve the old explicit turn effort. Milkie inherits this native CODEX_HOME setting.
        // Reject profile selection, which could override the root effort.
        let native = read_bounded(&absolute_file(&config_dir.join("config.toml"))?, 1_048_576)?;
        let settings: toml::Value = std::str::from_utf8(&native)
            .ok()
            .and_then(|s| toml::from_str(s).ok())
            .ok_or_else(|| {
                invalid(
                    "/execution/native",
                    "专用原生配置不是有效TOML；未输出配置内容",
                )
            })?;
        if settings
            .get("model_reasoning_effort")
            .and_then(toml::Value::as_str)
            != Some("high")
            || settings.get("profile").is_some()
        {
            return Err(invalid(
                "/execution/native",
                "专用Codex配置必须显式设置model_reasoning_effort=high且不选择覆盖profile",
            ));
        }
        std::fs::create_dir_all(data)?;
        let data = data.canonicalize()?;
        // Neither an ancestor nor descendant can be allowed to double as framework storage.
        for store in [data.join("milkie"), data.join("taskloop")] {
            let store = match std::fs::symlink_metadata(&store) {
                Ok(_) => store
                    .canonicalize()
                    .map_err(|_| invalid("/execution/storage", "框架存储路径无法解析"))?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => store,
                Err(_) => return Err(invalid("/execution/storage", "无法核对框架存储路径")),
            };
            for native_dir in [&config_dir, &session_dir] {
                if store.starts_with(native_dir) || native_dir.starts_with(&store) {
                    return Err(invalid(
                        "/execution/storage",
                        "原生配置/会话不得与框架存储重叠",
                    ));
                }
            }
        }
        let milkie = MilkieConfig::new(
            node,
            connection,
            data.join("milkie"),
            MilkieStorage {
                config_dir,
                session_dir,
            },
        )?
        .args([cli.as_os_str()]);
        // Source settings are host policy facts, not Agent-provided configuration.
        let binding = crate::domain::hash(&[bytes, native, conn_bytes].concat());
        Ok(Self {
            executor: Arc::new(MilkieExecutor::new(milkie)?),
            trace: TraceOptions {
                max_events_per_run: config.trace.max_events_per_run,
                max_bytes_per_run: config.trace.max_bytes_per_run,
                source_max_events: config.trace.source_max_events,
                source_max_bytes: config.trace.source_max_bytes,
            },
            policy: TracePolicy {
                policy_id: config.trace.policy_id,
                max_total_events: config.trace.max_total_events,
                max_total_bytes: config.trace.max_total_bytes,
                max_runs: config.trace.max_runs,
                retention_ms: config.trace.retention_ms,
                max_content_bytes: 0,
                content_tools: BTreeMap::new(),
                filter_version: None,
                filter: None,
            },
            binding,
        })
    }
    pub async fn open(self, root: &Path, data: &Path) -> Result<(Arc<Runtime>, String)> {
        let bindings = crate::orchestration::bindings(
            self.executor,
            format!("inquiry-article@0.4.0:{}", self.binding),
        );
        let (registry, options) = crate::orchestration::registry(root, &bindings)?;
        let digest = registry.profile_digest(crate::orchestration::PROFILE)?;
        let runtime = Runtime::open_orchestrated(
            crate::budget::runtime_options(data.join("taskloop")),
            vec![],
            bindings,
            registry,
            options,
            Some(TraceSetup {
                options: self.trace,
                policy: self.policy,
            }),
        )
        .await?;
        Ok((Arc::new(runtime), digest))
    }
}
