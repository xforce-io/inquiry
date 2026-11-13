# Inquiry #7 交付记录

## 范围与授权

2026-11-12 用户要求“按照你的建议端到端完成这些任务”：校准版本与文档、固定质量标准和验证协议、完成 #7 契约及最小接入、验证复杂文章，再做优势对照。采用 GitHub / keel；不改写 #2 两轮 FAIL 或第三轮 BLOCKED，不以 #7 通过代替 #2 S4。

## 工作清单

- [x] 核对两仓 origin、远端分支、Issue、PR 和源码版本。
- [x] 将研究集成基线 d99bb16 合入 #7 工作树，解决名词表冲突并保留新增术语。
- [x] 固定产品基线与可核验的质量/收益验证协议。
- [x] 调查并处理 TaskLoop 两项已知 P2，记录各自的复现、修复和验证。
- [x] 完成唯一设计文件的技术契约与验收映射。
- [x] 实现 Goal/DAG 接入、独立评价、有界修订、最终评价及 Web 展示（真实验收尚未完成）。
- [ ] 冻结候选，完成确定性检查和真实 Web/研究验证。
- [ ] 发布初稿、评价、修订及事实/覆盖/结论变化的可复核对照。
- [ ] 独立审查、CI、人工门及 PR 交付；无部署要求。

## 现状调查（keel-how）

源码依据为 Inquiry d99bb16 和 TaskLoop 8276788。

Inquiry 的 `src/main.rs` 先绑定本地端口，再经 `ExecutionSetup::open` 装配 Runtime；`src/execution.rs` 显式校验 Milkie 配置、原生模型和存储隔离，当前仅装配旧 Flow。`src/web.rs::start` 以原问题、范围、网址和幂等键调用 `Runtime::start`；读取接口只消费公开快照和产物。迁移须保留旧实例只读入口、会话与 Origin 边界，不访问框架私表。

`src/domain.rs::check_domain` 的 report 分支校验引用编号、保存版本、分析血缘及引文上限，然后根据作者 `answer_status` 返回 ready/need_input。这是结构和关系校验，不能证明主张被引用内容支持或文章满足研究目标。

TaskLoop `src/orchestration/registry.rs::validate_profile` 当前仅验证 checker 的 `goal_input_fields` 出现在 Goal Schema 的 properties 中，没有要求其出现在 required 中。`src/orchestration/coordinator.rs::consume_return` 先持久化 Checking，再验证和接纳；最终事务失败会留下 Checking。`src/orchestration/lifecycle.rs::recover` 有重开恢复路径，而在线 maintain 尚无等价收束。两项须分别建立复现，不能用阅读源码冒充运行证据。

TaskLoop `tests/inquiry_consumer/` 已有公开 API 接入与旧领域校验适配示例，可复用装配经验；其固定两份网址、短研究及框架验收不满足本票自主复杂文章验收，不能整体搬用为正例。

进一步核对 `src/orchestration/driver.rs::prepare_execution`：Goal 工作节点只获得锁定请求明确列出的 input_artifact_ids，评价者只获得 CheckInput 中投影的 goal_inputs 和授权 targets。`nodes.rs::ready_node` 从显式输入端口构造这些引用，不自动授权所有上游附件；`driver.rs::persist_candidate` 将工具产生的候选先保存为 accepted=false。因而旧 Flow 的动态 capture 附件不能假定会随一个 materials 端口自动传递给下游，资料集合的公开引用与内容版本必须在技术契约中解决，不能偷读目录或扩大工具权限绕过。

`orchestration/check.rs::issue_check/check_evaluation` 已负责独立 CheckInput、精确目标身份、规则覆盖和证据权限；应用应提供领域语义规则及可读取来源，不再另造评价回执身份。初稿评价应作为可传给修订的 data 评价，最终必需评价才决定 achieved/not_met；具体规划仍待产品基线后确定，不能拿框架示例中的小题结果替代本票文章。

## 当前基线与门槛

2026-11-13 用户回复“沿用这套基线”，确认创建/详情两页、仅必要时澄清、Q1–Q6全部通过才达标、未达标文章保留可读、评价错误单独显示。唯一设计已补齐为 v0.2；技术契约固定 TaskLoop main `daf0fbaae905be555b543063a4e0d477ea522be8`，新方法版本为 0.4.0。此确认不代替未来确切交付候选的人工审查门。

已实现待验收：公开 Goal Workflow 接入、原生两轮资料获取、保存正文分段工具、精确资料包引用、初稿 data 评价、一次修订、两项最终评价，以及详情版本/质量/只读旧记录展示。没有新真实模型实验；真实 Web 驾驶与独立效果评价仍为 not_run。

开发检查已发现并修正：提案误带 template_ref、输入生产节点未全部列入直接依赖、测试目录权限与 resources 子目录复制、受控执行器未声明候选字节限制。原失败日志保留于 target；不能将修正后的通过当作文章质量证明。正式模型实验仍须先提交固定协议，包含问题、截止、候选、模型、预算、评价隔离和全部尝试记录。

所有 #7 Story 尚未完成真实验收，不宣称验证或交付完成。

## 旧接口兼容性调查

2026-11-12 对 `d31671cfbf57c519994d413b12a344cdfb8dbce6` 的归档副本临时使用 TaskLoop `384bad9566449571b163c9744beb9f52475d3b82` 路径依赖，执行原有 `cargo test --offline`：34 passed、0 failed、3 ignored。副本、版本组合、命令输出和退出码位于本工作树 `target/compat-taskloop-20/`；正式 Cargo.toml/Cargo.lock 未改。此调查证明该组合的旧 Flow 代码兼容，不证明 Goal/DAG 迁移、真实模型运行或新质量闭环。未来正式接入候选仍须重新验证相应路径。

TaskLoop 两项 P2 已由 [Issue #20](https://github.com/xforce-io/taskloop/issues/20) / [PR #21](https://github.com/xforce-io/taskloop/pull/21) 收尾：232 passed、0 failed、5 ignored，严格 Clippy、独立旧 Flow 消费者、公开 Runtime 验证、原生独立审查 PASS（human: optional）、分支及 PR 双平台 CI、keel check 均通过。合入 `main@daf0fbaae905be555b543063a4e0d477ea522be8` 后，已核对文件树与经审 `384bad9` 相同；本地 main 干净并已同步。兼容性试验仍保留原组合 SHA，不改标成新 Inquiry 的验收证据。没有发布包或部署服务。

## 新采集器网络预检

2026-11-13首次零模型采集探针未通过：系统DNS对search.brave.com返回103.200.30.143和保留IPv6 2001::430f:81d2；原公开地址边界拒绝，没有发起正文获取，日志保留target/native-collection-probe.log。通过现有代理向固定Cloudflare端点查询则返回公开AWS地址，证明解析路径不一致。没有把探针失败改成模型失败或文章未达标。技术契约增加宿主显式DoH选项，继续保留全地址集合校验和目标IP固定，不改系统网络配置。其独立预检日志为target/native-collection-doh-probe.log，结果以原输出为准。

首次DoH探针越过地址拒绝后，整轮采集在45秒公共工具回调边界超时（target/native-collection-doh-probe.log）。根因为应用将最多8查询/20获取装入一次回调，虽然Run预算480秒，回调仍有独立45秒上限。已改为同一原生Run内逐次搜索/单页获取和最终组包，每个回调保留现有边界，不扩展TaskLoop；重新验证另记target/native-collection-doh-split-probe.log。

拆分回调后的零模型网络探针通过：5条真实候选、4条可用保存来源，逐条核对搜索来源标记、原文base64还原SHA-256、规范化段落SHA-256。该结果不证明文章质量；前两次网络/回调失败均原样保留。原生Codex0.166.0+固定Milkie的本机401/挂起提供者验证也通过：high、Trace、实际停止、正常关闭；不代表真实账号或模型研究通过。

## 首批真实研究与后续检查

已冻结并推送35a8dce，干净树重跑46 passed/0 failed/4 ignored、fmt、严格Clippy、前端构建，以及显式原生401/停止和DoH真实采集探针（5候选/4可用）。事前登记完成后，实际浏览器创建复杂研究；三次原生规划均auth_failed，公共终点planning_exhausted/failed，无文章、工具调用或质量评价。原始证据与[公开结果](../experiments/7-first-attempt.md)保留；后续效果比较未启动。已请求用户完成专用账号交互，不凭本地Logged in声称远端可用。

等待期间只补应用诊断入口及项目内受控Goal浏览器夹具；夹具明示synthetic/zero models，不能用于S3.A2真实语义负例或S5文章达标。首次夹具编译发现shutdown异步块需move，已修正并定向检查；不修改既有真实失败证据的SHA。下一候选须重新冻结界面检查，正式模型复验另核对历史和预算。

## 用户指定Sol及认证问题定位（2026-11-13）

用户要求改用GPT-6.1-Sol，并指出当前会话正在正常使用。按同一固定Codex0.166.0、Sol/high及无工具OK提示对照：旧专用CODEX_HOME仍401；当前用户~/.codex成功退出0、返回OK、无工具调用。证据work/verify/7-auth-sol-1/result.json及7-auth-sol-current-home/result.json；累计6次模型入口，不把失败清零。当前登录有效，先前将旧隔离环境失败泛化为当前账号失效不准确，不应继续要求用户重登有效账号。未复制认证文件或修改全局配置。

当前模型约束、示例连接、配置/原生测试及现行方法说明切换为gpt-6.1-sol/high；设计v0.3、协议v0.4，保留原Astra历史与首批登记。此修改不宣称Milkie/Goal/Web或文章质量通过。当前用户原生配置默认medium，直接烟测通过显式high覆盖；应用接入有效登录时仍须保留显式high、配置与会话隔离边界，不暗改全局配置来绕过校验。

## 有效登录接入调查与修正（2026-11-13）

直接原生Sol/high成功后，按相同当前登录目录配置实际Web宿主仍在启动前拒绝：/execution/storage“配置与会话必须使用互不包含的专用目录”，日志work/verify/7-native-shared-config/before.log。源码src/execution.rs的目录包含检查及默认high要求阻碍标准原生布局；Milkie src/execution/adapters.ts::bindGrokSessions要求session_dir恰等于config_dir/sessions实际路径，codex.ts通过当前CODEX_HOME继承配置，没有已有请求级effort字段。因此不复制旧认证、不改用户global config、不在Milkie/TaskLoop扩展通用协议，采用Inquiry拥有的显式codex_binary私有启动器与reuse_native_login选项。L1 reuse：已确认产品路径/模型/高推理不变；L2 write：设计v0.4说明存储/启动参数及失败边界。

新增配置集成检查实际执行带单引号的二进制路径，验证high参数完整转发、用户medium原字节不变、没有读取/创建认证、旧模式拒绝嵌套、复用必须显式指定二进制和真实sessions。原生401测试改为medium默认+启动器，须由实际提供者请求证明high而非只看脚本。开发首编译发现PATH临时值生命周期错误，已改为持有变量；原错误日志保留，不属于模型失败。

单入口真实探针首批在提交前被预算准入拒绝：Goal必须同时预留最终检查和首个Planner；将整体活动时间设为180秒不足以容纳270秒最终检查保护，因而没有创建Goal或原生Run，模型入口实际0次。证据work/verify/7-native-probe-bb2cbe1/actual-probe.log。只修正探针的总体时限为既有Goal上限，仍max_runs=3/max_revisions=0、Planner180秒、测试205秒超时收束，完整文章图因Run不足拒绝；不扩大正式研究限额。
