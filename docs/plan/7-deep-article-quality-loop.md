# Inquiry #7 交付记录

## 范围与授权

2026-11-12 用户要求“按照你的建议端到端完成这些任务”：校准版本与文档、固定质量标准和验证协议、完成 #7 契约及最小接入、验证复杂文章，再做优势对照。采用 GitHub / keel；不改写 #2 两轮 FAIL 或第三轮 BLOCKED，不以 #7 通过代替 #2 S4。

## 工作清单

- [x] 核对两仓 origin、远端分支、Issue、PR 和源码版本。
- [x] 将研究集成基线 d99bb16 合入 #7 工作树，解决名词表冲突并保留新增术语。
- [ ] 固定产品基线与可核验的质量/收益验证协议。
- [ ] 调查并处理 TaskLoop 两项已知 P2，记录各自的复现、修复和验证。
- [ ] 完成唯一设计文件的技术契约与验收映射。
- [ ] 实现 Goal/DAG 接入、独立评价、有界修订、最终评价及 Web 展示。
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

## 当前门槛

设计文件仍为产品草稿 v0.1。两页路径和六项全通过的选择已向用户集中确认；未获得确认前只做调查、基线整理和独立的可靠性修复，不进入新产品实现。正式模型实验须先提交固定协议，包含问题、截止、候选、模型、预算、评价隔离和全部尝试的记录方式。

所有 #7 验收当前为 not_run；本记录不宣称验证或交付完成。
