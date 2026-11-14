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

受控Web驾驶已恢复至空间22（依据用户恢复完整端到端目标，原空间21确已不存在）：bb2cbe1界面完成达标、未达标、检查错误、初稿/最终版本、引用定位及Escape还原焦点/滚动、刷新和公开预算读取。它们全部synthetic/zero models，不作真实质量结论。检查错误页的通用错误文案不应仍称“停止需等确认”，因为该Goal已Finished且停止均已确认；仅改为核对原因后决定是否新建研究，停止确认继续由真实状态单独展示。证据work/verify/7-native-probe-bb2cbe1/controlled-*.txt/png。

## Sol 生产链路探针结果与身份错误调查（2026-11-13）

5ed0cbd单入口真实Planner已越过认证，原生会话白名单元数据确认gpt-6.1-sol/high，tl_artifact_dwrite实际接收并交付一次4075字节候选；随后Milkie以session_mismatch失败，TaskLoop终点planning_exhausted。它不是401，也没有形成文章。Trace记录input14782、cached0、output966；累计真实模型入口7次，已知执行累计600.094秒，早期未知token仍保留未知。登记、快照、Trace、预算与完整累计表均在work/verify/7-native-probe-5ed0cbd/，不重置首批失败。

源码调查：Milkie src/execution/codex.ts::message对所有通知的顶层threadId/turnId执行全局相等检查，run另检查turn/start回执和session路径；本次保存文件的session ID、cwd、model和effort均匹配。仅凭现有Trace尚不能判定具体触发通知或根因。固定原生CLI接本机合成Responses提供者，普通文本、动态工具、当前登录目录中的显式本机提供者和双app-server共享存储均完成，未复现相同错误；这些试验没有真实模型入口，不能当作修复证明。后续若必须真实复验，需事前登记只记录协议方法/身份的透传诊断，并保持现有校验及累计额度，不直接放宽身份边界。

受控停止未确认路径已实际点击并显示“正在确认停止”，没有提前显示已停止；随后向自有Rust测试子进程发送SIGINT，夹具完成runtime.shutdown并正常退出0，browser-fixture.log记录1 passed/0 failed。全部受控结果仍绑定bb2cbe1，不改标为后续候选；终态通用错误文案修正沿用L1 v0.2与L2 v0.4，不改变状态判定，前端构建通过（ui-wording-build.log）。所有真实文章及独立效果验收仍未完成，不能进入独立代码审查或合入。


## 全局警告根因与下一批依赖（2026-11-13）

新增单入口透传诊断仍失败，保留work/verify/7-sol-protocol-3dbe328。确切触发帧为warning(threadId:null)，原生0.166.0协议允许全局警告无具体会话目标；Milkie全局比较误当真实错配。累计真实入口8次，不是认证失败，不再以其他会话并发假设代替根因。

已建[Milkie #281](https://github.com/xforce-io/milkie/issues/281)，修复候选216522e只为合法无id、message为字符串的全局warning允许null目标；其它会话警告、工具缺会话、错误轮次仍拒绝。固定真实Codex配本地合成提供者的原入口对照：旧代码session_mismatch，新代码succeeded/OK、1次宿主效果、2次本地提供者请求、停止确认；零真实模型。Milkie独立审查及合入仍待完成，不能冒充已发布依赖。本批Inquiry技术设计v0.5及本地驾驶手册显式锁定该修复，产品基线v0.2和质量协议v0.4不变。

剩余22个真实入口以内安排：复杂正例最多8、真实语义负例1、两对照2、隔离效果评价8、分歧裁决最多1、两仓独立代码审查预留2。总量仍30，不保证失败后必有重试余量；每批前核对累计，若需越界再集中报告具体缺口。首篇未成稿/未达标仍停止后续效果比较，不把接入探针算作代表文章。

## 首篇真实文章失败与采集调查（2026-11-13）

`78ce9da4b814ea189adec1a2ea2fbc7fc29576de` 从创建页实际启动 `goal-3270ba0c-8a25-467f-bc1f-f1cee4c062c5`，固定 Sol/high、原 Vertex 复杂问题、质量协议v0.4。8个工作节点执行成功，文章结果 **not_met**。初稿独立评价Q1/Q2/Q5/Q6 fail，Q3/Q4 pass；最终原生检查因“分析仍有决定性缺口，不能接纳为完整回答”fail，最终语义评价为 stale_due_to_terminal，未启动。保留可读初/终稿及引用面板、关闭还原阅读位置、刷新后的实际浏览器证据。这不是达标正例，也不改标成事前登记的语义负例。按协议停止本批直接生成、同资料对照及隔离效果评价。

7个真实模型入口的起止和最后累计用量已从连续1315条公开Trace逐一提取，未知模型迭代/上游请求仍为null。累计17入口（研究15、代码审查2），可测模型入口及零模型Run累计耗时3389.525秒；全局30入口/9000秒和研究27入口/7200秒均不变。证据：`work/verify/7-78ce9da/{actual-attempt-ledger,attempt-summary,cumulative-after-first-article,accepted-goal-budget}.json`。原 `real-budget-final.json` 是错误路由的not_found响应，原样保留；真实预算使用同一公开Goal视图中的budget，不把该错误响应当预算。

### Overview / 怎么跑

既有产品预期来自设计v0.5 §6.2/6.4：两轮原生采集、最多8查询/20候选、复用有界搜索恢复，资料失败与限制保留。真实首轮四个Vertex专属页面因DoH/网络超时失败，四个通用Ontology页面可用；补查有8条具体queries，却未接纳任何新来源。预期应在既定边界内使用真实候选并恢复暂时失败，不要求用户必须提供网址。

### 东西在哪 / Gotchas

- `src/article.rs::candidates` 按查询轮转，但硬编码 `0..5`，真实返回22–24条时只选出8个去重URL；后续实际结果被漏掉，尚未碰到20来源上限。
- `src/article.rs::collect` 从首轮corpus提取所有requested_url作排除集合，不区分usable/denied/暂时网络失败。补查真实响应再次发现失败Vertex地址也被排除。
- `src/search.rs::recover` 每次调用最多3次fetch/30秒，provider结构化冷却为60秒；`article::collect` 未消费 retryable/retry_after_ms，立即执行后续query，后续返回fetch_attempts=0。45秒回调不允许简单在Search内部等待60秒；节点Run本身480秒，恢复应在回调外承接，并响应取消。
- `domain::acquire_source` 已保存fetch_status/http_status/usable，不需要匹配中文reason决定重取。URL/DNS/TLS和逐跳固定IP边界继续复用 `net`。

问题依据为真实原入口、确切78版本及已保存corpus1/2和Search记录，确定性回归将分别复现。无尾斜杠、单IP或重定向是否导致这四次网络失败仍为假设，未经验证不修改网络安全策略、不增加解析回退。先修有证据的候选与恢复契约，再用现有零模型采集探针核对真实正文。L1: reuse 已确认产品基线v0.2、S2/S4及Q1–Q6；L2: write，仅增量修订资料恢复的技术边界，不改变产品路径/质量标准。Milkie #281修复216522e已获用户批准，并经PR #282/CI合入c01de337；无包发布或服务部署。

### 采集修复候选（产品基线不变）

四项针对原逻辑的确定性回归先红后绿，覆盖前五项截断、暂时失败被永久去重、同query恢复及等待取消；扩展为8项，额外检查全轮共享2次重试/120秒等待边界、超时不提前请求、未知计量不补零。生产 `Native::Collect` 已接收真实CancellationToken。通过公开HTTP/Runtime的集成用例进一步验证：一次搜索不可用后恢复、首轮保留暂时失败、预定补查只重取该失败而不重复成功/安全拒绝/404来源；全程受控、零模型。

修复开发树的既有真实采集探针通过（128.48秒）：一条查询返回24个真实链接，选择20个并保存4份来源，其中Vertex概览32段、用例53段、场景入门54段，另有模型文档38段。所有可用来源的原文及规范化摘要一致；16份因DoH波动或2.5MiB原文档案容量到界不可用，完整记录，未放宽网络边界或增加容量。日志 `work/verify/7-collector-recovery/network-probe.log`。没有尾斜杠或IP机制的根因结论，不声称网络波动已经消失。此证据绑定开发树，冻结候选后另跑必需检查与探针；不把它改标成新候选验收。

采集修复仍待冻结检查、真实复杂文章/语义负例及效果评价；S1–S5不可汇总pass，不进入独立代码审查或合入。下一批模型入口须先核对累计、补足原预算许可，未获扩额前只运行零模型检查。按现有明确路径，剩余完整正例8入口、真实语义负例1、两对照2、隔离评价8、最终独立审查1，共20入口；加上已发生17入口需总上限37、研究上限34，原限额30/27不能覆盖。分歧裁决未预支，若真需要另报告，不删必需对照或评价来凑原额度。

公开取消的集成检查也已通过：原生资料节点正在等待60秒provider冷却时，通过HTTP取消研究，终态cancelled，正常shutdown后确认只有首次搜索、没有重试或来源获取。该用例补足CancellationToken从公开Runtime传到Native等待的链路，不用局部函数测试代替停止确认。


## 2026-11-14 实际重试与新瓶颈

用户先以“继续”承接37入口/10800秒（研究34/9000秒）具体扩额，随后以“go”批准38入口（研究35，时间不变）及仅一次带非敏感停止诊断的重试。第一次0826 Planner已认证并提交宿主结果，后SDK unknown/process_failed、停止未确认；正常SIGINT宿主退出1/stop_unconfirmed，原证据保留，不重标成功。3次隔离原生/本机提供者探针通过；共享登录目录探针停止确认，但全局warning/工具夹具断言未通过，不能当全链路PASS。根因未完全复现，不要求重登。

获准重试 goal-a7e66ffc-9872-490e-b5c6-6f823de9857b 实际7模型入口、8工作节点成功、终态not_met；初稿独立Q1/Q2/Q5/Q6 fail、Q3/Q4 pass，最终原生检查因决定性缺口fail，最终语义检查未调用。7次原生停止均确认，诊断无清点异常；不能据此宣称上次停止问题已修复。累计25入口（研究23、代码审查2），可测执行4669.969秒；总38/10800秒及研究35/9000秒上限保持，不自动开下一批或执行对照。

真实输入/版本/Trace、1177连续事件、初/终稿、评价和逐入口账本在work/verify/7-0826faf/retry-20261114-stopdiag。首轮20份抓取仅4份接纳，16份因2.5MiB原文档案上限清空段落；补查搜索全部不可用而无新来源。公开原文及规范化SHA核对通过；四页原文436117/442592/445083/496534字节，archive约2427738字节。只读离线gzip诊断为52760/54319/54268/68177字节，未修改原文或模型输入。具体how见该目录how-storage-bottleneck.md。

另实际发现：初稿52处数字引用，修订稿40处[E…]标记、0处数字引用；页面报告正文0个引用按钮。src/orchestration.rs的draft要求[1]，revision遗漏相同约束；resources/article/report.json只要求非空正文，允许这一退化，src/web前端inline只识别数字。对最终稿23种已使用E标记逐个核对，均能以完整证据元组对应报告数组，不靠编号猜测。原生check_report本来会拒绝未在正文使用的数组引用，但本次先被分析缺口拒绝，未到后续引用规则。

L1 reuse已确认v0.2；L2 write v0.7：依次修复无损归档、原有实际候选重取、报告提交格式与旧稿引用显示；维持Q1–Q6和所有资源边界。先做确定性红/绿及零模型真实网络/旧报告驾驶，再冻结候选核算后续真实验证额度；没有新模型实验授权或质量通过结论。最终独立审查及交付仍未完成。


本轮修复的确定性检查：63项Rust测试通过、0失败、6项显式ignored；5项前端引用解析测试通过，严格Clippy和前端构建通过。原文容量、既有候选补查、错误引用格式均保留修改前红与修改后绿记录，目录work/verify/7-storage-citation-recovery。此为冻结前开发检查，后续候选SHA上的验证另记，S1–S5不汇总通过。
