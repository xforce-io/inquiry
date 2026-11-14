# Inquiry

以证据推进判断的研究应用：发现来源、探活留存、整理、证据分析和报告，支持有界补查及用户补充网址。

TypeScript Web → 薄 Rust Web 宿主 → TaskLoop Rust crate。Inquiry 提供研究 Skill、Playbook/Schema、公开网页工具与领域质量检查；TaskLoop 封装执行、状态推进、产物接纳、隔离和故障处理。应用不读取框架内部表，不建设第二套执行状态机。

## 本地运行

需要 Rust 1.91.1+、Node 20.19+、curl、Codex CLI **0.166.0** 和固定 Milkie CLI 构建（Goal 协议构建 `216522e1685df608514f02ce451666ece3f28c57`，已随 Milkie PR #282 合入；未发布包）。研究按用户2026-11-13要求改为固定 `gpt-6.1-sol/high`，TaskLoop→Milkie→Codex；没有旧执行器或模型回退。当前代码固定 TaskLoop `daf0fbaae905be555b543063a4e0d477ea522be8`，通过公开 Goal/DAG 接口运行方法 0.4.1；精确依赖以 Cargo.toml/Cargo.lock 为准。原生 Codex 还需要同版本的 `codex-code-mode-host` 辅助二进制，两者通过任务专用 PATH 选择。

宿主必须提供 `INQUIRY_EXECUTION_CONFIG` 指向绝对路径 JSON，基础字段必填；codex_binary和reuse_native_login为显式可选项。配置结构见[执行配置示例](docs/examples/execution.json)；其中限额仅展示字段，**不是默认或已经批准的生产取值**，真实运行前须由宿主固定。`node`、`milkie_cli`、`connection_file`均为实际绝对文件路径；`config_dir`与`session_dir`须提前准备且不与框架存储重叠；默认使用互不包含的专用目录，显式复用登录的例外见下文。连接文件只接受[固定模型的规范字段](docs/examples/connection.json)，不从环境继承模型/凭据。

未提供`codex_binary`时，`config_dir/config.toml`必须显式设置 `model_reasoning_effort = "high"`；所有模式均不选择覆盖profile。显式提供固定Codex绝对文件`codex_binary`时，应用在自身私有目录生成启动器，仅对子进程强制high，不修改全局配置。宿主可显式设置`reuse_native_login=true`复用已有登录，此时须提供codex_binary，并令session_dir指向config_dir/sessions既有实际目录；业务/Trace/Milkie存储仍保持独立。原生CLI正常管理该登录及本次会话，应用不复制凭据。原生登录由宿主在该目录准备，应用不复制、不读取或回显`auth.json`。配置通过只说明可装配，登录、CLI版本和模型权限仍由实际执行确认。Codex可执行文件必须在宿主PATH中；不自动替换全局安装。

```sh
npm ci --prefix web
VITE_BUILD_SHA=$(git rev-parse HEAD) npm run build --prefix web
INQUIRY_EXECUTION_CONFIG=/absolute/path/execution.json cargo run --locked
```

打开 http://127.0.0.1:3410 。只监听本地，不是多用户托管服务。`INQUIRY_PORT` 可指定非零端口，`INQUIRY_DATA_DIR` 可指定私有工作目录，默认为 `data/`。取消请求与取消确认分开；退出使用正常中断并等待执行器停止。刷新原研究地址可恢复视图，不自动重新执行未知工作。

创建页填写问题，网址/范围可选；详情页显示实际阶段、已接纳资料、报告和限制。引用打开保存版本并定位段落，不以当前远端网页替代历史内容。必要澄清可补充文本和网址；失败、未达标或停止保留已有内容。初稿与最终稿分别展示确切版本的质量评价，旧方法记录保持只读。

公开 HTTP(S) 获取每页正文上限1MiB、最多5次重定向，逐跳检查DNS并固定目标IP，拒绝内网、凭据URL和非80/443端口；一期不支持PDF/OCR或登录网页。规范化节选会标记截断。网络实现依据 [curl connect-to](https://curl.se/docs/manpage.html#--connect-to) 与 [IANA 特殊地址注册表](https://www.iana.org/assignments/iana-ipv4-special-registry/)。网页是非可信文本，不能向Agent授予额外工具或权限。系统DNS不可用时，宿主可显式设置 `INQUIRY_DNS_MODE=cloudflare-doh` 使用固定Cloudflare HTTPS解析；默认system。此模式仍校验全部A/AAAA地址并固定目标IP，失败不换解析器，`/api/health`显示所选模式。详见[设计中的网络边界](docs/design/7-deep-article-quality-loop.md#8-apicli)。

方法0.4.1固定为范围、资料获取、分析、有界补查、综合、初稿、独立评价、一次修订及最终评价，无回边。每轮最多8条查询和20个实际来源候选，保存正文和原文档案分别有容量上限。作者通过授权工具分段读取资料，评价者读取独立 CheckInput 和保存证据；引用核对与 Q1–Q6 语义评价分别运行。只有针对确切最终稿的两项必需评价全部通过才达标。

总墙钟/活动时长上限30分钟、16 Run、300次 TaskLoop 工具调用，单项入口另有时间/调用限制，保留最后2次提交。原生采集在一个Run内逐次调用搜索和单页获取工具；搜索恢复、DNS和重定向可能产生多个网络请求，另外保留获取事实，不把工具调用数混为网络请求数。边界耗尽不保证文章成功。旧方法0.3.7资源保持原样，用于历史记录解释。

原始网页、运行目录和审计日志均为本地私有数据，不进入公开仓库。引用检查仅证明版本/定位/分析关系一致；产品中的独立语义评价也不代替最终效果实验。旧`research-audit.ndjson`原样保留；新执行诊断由TaskLoop统一元数据Trace与预算事实承接，不再另写原始工具正文日志。原生功能会话仍位于宿主提供的专用会话目录，不受Trace TTL代删；未知用量不补零。

## 验证与状态

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
npm test --prefix web
npm run build --prefix web
```

确定性测试不是实际模型研究。研究集成分支 `feat/2-research-web-mvp` 已有真实六阶段报告、引用回溯、停止/隔离与 Web 驾驶证据，迁移 PR #4 和搜索恢复 PR #6 均已合入该分支；应用尚未进入 main。两轮质量对照均为 FAIL，第三轮为 BLOCKED，不宣称已优于直接生成。各证据仅支持其绑定候选及记录范围，不能当作当前 #7 新质量闭环已验收。

`feat/7-deep-article-quality-loop` 承接已集成实现；产品基线已于2026-11-13由用户确认，[设计（产品基线v0.2、技术契约v0.8）](docs/design/7-deep-article-quality-loop.md)及新闭环已实现待验收。当前确定性检查不能证明深度文章质量；正式生成前必须冻结输入、模型、规则、预算与匿名映射，旧实验不改写。

- [实现与验收 Issue #2](https://github.com/xforce-io/inquiry/issues/2)
- [唯一实现 L2 与实验协议](docs/design/2-research-web-mvp.md)
- [功能验证手册](.agents/skills/verify-inquiry/SKILL.md)
- [名词表](docs/glossary.md)
- [前置领域设计](docs/design/1-minimal-research-loop.md)
- [TaskLoop 已交付 PR #4](https://github.com/xforce-io/taskloop/pull/4)

#7 首篇真实复杂文章在 `78ce9da` 完成初稿、独立评价与一次修订，终态 **未达标**：Vertex 专属资料抓取失败，关键用户路径和产品边界仍有决定性缺口。最终原生检查拒绝，最终语义评价未启动；本批对照/效果评价依协议停止。详见[首篇记录](docs/experiments/7-first-article-78ce9da.md)。采集修复和后续验证进行中，不能宣称深度文章能力已通过。

## 执行诊断与迁移状态（Issue #3）

阶段策略由TaskLoop强制，分析80、其它40、最后2次仅允许产物提交；失败准入扣额，拒绝探索不吃提交保留。阶段/检查时间显式继承原任务剩余，任务active上限仍30分钟。方法0.3.7的`_taskloop_budget`元数据不进入业务回执/产物。

本地会话内的只读入口：`GET /api/research/{id}/trace?limit=100`、`/trace/export?limit=100`、`/budgets`。Trace支持可选`run_id`及`after`游标，limit必须为1–1000；导出同样有界。旧未采集实例返回未知/disabled或缺口，不补造历史。Host/Origin/session边界保持，不提供正文采集或独立监控页面。

启动器已使用MilkieExecutor并开启宿主显式容量/保留的metadata-only治理；SIGINT/SIGTERM及HTTP服务异常均等待Runtime停止。真实原生Codex+实际Milkie+本机401端点测试核对请求high、失败关联、原生取消和正常关闭；该端点没有真实模型或账号，不代表真实研究验收。

真实Vertex报告/引用与浏览器迁移验收仍未完成；两次旧Vertex失败和#2质量实验FAIL保持，不启动第三轮SOTA实验。详细规格见[迁移设计](docs/design/3-milkie-trace-research-migration.md)，验收地图见[执行与Trace](.agents/skills/verify-inquiry/features/execution.md)。

方法0.3.7的来源发现显式读取授权的历史缺口、候选查询和资料索引，首次无历史也可运行。搜索工具每次最多3次获取尝试、总等待30秒，遵守Retry-After并共享冷却；持续不可用显示专门等待，60秒后自动继续，仍共享原每状态3次访问上限。页面可停止，耗尽后保留资料并明确服务不可用；真正缺少用户资料沿用补来源入口。查询记录由实际搜索回执核对，旧实例和质量实验结果不改写。


2026-11-14：固定Sol/high的0826faf重试已跑完8个工作节点，最终为`not_met`，未证明深度文章达标；初稿独立评价Q1/Q2/Q5/Q6失败，最终原生检查拒绝决定性缺口，最终语义评价未调用。累计25个模型入口（研究23、代码审查2），后续对照未启动。已据真实记录修复原文档案容量利用、原有发现候选补查及引用提交/回溯；修复候选仍须冻结复验，不能据确定性测试宣称研究质量提高。详见[完整进展与失败依据](docs/plan/7-deep-article-quality-loop.md)。

2026-11-14按用户要求，本轮搜索固定为DuckDuckGo HTML（`duckduckgo-html`），不再请求Brave，不自动轮换供应方；无需新增API认证。搜索仍只负责发现，正文另行抓取、保存和校验，Q1–Q6与原问题不变。旧Brave失败和实验结果保留，新方法0.4.1须重新登记验证。
