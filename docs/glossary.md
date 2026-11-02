# 名词表

| 规范名 | 一句话定义 | 禁止别称 |
|---|---|---|
| Inquiry | 基于 TaskLoop、以证据推进判断的研究应用，负责领域方法、数据和用户交互。 | — |
| TaskLoop | 通过公开实例/产物接口承接执行并封装内部运行机制的轻量通用 Rust crate。 | — |
| Web 宿主 | Inquiry 中承接 HTTP、用户授权、公开接口转换和领域能力注册的薄 Rust 进程。 | — |
| Skill | 以 SKILL.md 向 Agent 提供研究方法与资源的版本化包，本身不是服务或状态机。 | — |
| Schema | 定义 Inquiry 领域数据与内容结构的规范；通用交换格式由 TaskLoop 维护。 | — |
| 研究问题 | 用户确认的问题及其范围、约束和完成标准，一期保存在实例原始 inputs 中。 | — |
| 资料 | 有来源身份、处置事实和内容版本的研究输入，包含可用来源及失败记录。 | — |
| 获取事实 | 研究工具报告的 URL、时间、处置和内容摘要等领域数据，由受信框架回执承载。 | — |
| 证据 | 在特定保存内容版本定位的原文及引用关系，用于支持或反驳判断。 | — |
| 判断 | 基于明确证据形成、包含支持或反驳关联及限制的解释或主张。 | — |
| 结论 | 针对研究问题综合已接受判断与证据形成的回答。 | — |
| 报告 | 汇集结论、可回溯引用、限制及未决项的交付物。 | — |
| ExecutionId | TaskLoop 公开的不透明实例标识，一期同时作为 Inquiry 研究地址中的标识。 | — |
| ExecutionSnapshot | TaskLoop 公开的业务输入、执行状态、版本、等待要求和阶段产物视图，Inquiry 只消费它。 | — |
| Artifact | TaskLoop 管理、按公开引用读取固定字节和摘要的产物，内容 Schema 由 Inquiry 定义。 | — |
| ArtifactReceipt | TaskLoop 提供的受信工具事实信封，执行归属由框架保证，领域 payload 属于 Inquiry。 | — |
| Playbook | Inquiry 按 TaskLoop 格式提供的研究阶段、业务分支和限制定义。 | — |
| EntryContract | Inquiry 按 TaskLoop 格式声明的当前阶段方法、工具、输入输出及领域检查要求。 | — |
| CheckResult | TaskLoop 规定的领域检查返回契约，Inquiry 提供有证据的有限结果，不直接指定下一状态。 | — |
| HostBindings | TaskLoop 提供的公开初始化面，Inquiry 用于注册方法包、领域工具和检查器。 | — |
| ArtifactRef | TaskLoop 公开的产物标识、摘要与阶段定位，Inquiry 用它读取和展示内容而不反查内部执行。 | — |
| TaskLoopError | TaskLoop 公开返回的调用错误信息，Inquiry 据此展示失败原因而不修改执行状态。 | — |
| Agent Harness | TaskLoop 适配复用的第三方 Agent 执行框架，具体运行机制不由 Inquiry 定义。 | — |
