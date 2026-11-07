# 执行迁移与 Trace 关联（Issue #3 S2）

标准：`docs/design/3-milkie-trace-research-migration.md` v0.1 §11，S2.A1/S2.A2。这是拟交付路径的验收地图，不能把登记当实现或验证通过。

1. 沿研究页的同一 ExecutionId，经本地会话保护的 `/api/research/{id}/trace` 查询阶段、Agent、工具及检查；核对 source 关联、最后活动、耗时、完整性缺口；分页/增量按 TaskLoop 公开游标继续。
2. 经 `/trace/export` 导出有界分页，再经 `/budgets` 核对当前与历史阶段的有效预算及来源。不得读内部 SQLite/JSONL、另造状态或用工具数推算模型迭代。
3. S2.A1 必须使用 research.md 中 #3 本次真实研究，不能以 fixture 代替六阶段实际记录；确定性测试另外验证未采集、非法游标、错误研究、会话失效和容量/保留状态转换。
4. S2.A2 保存两次既有 Vertex 失败的原证据索引；本次429、人工补网址、等待和失败逐项保留。真实取消须实际确认停止；停止未知与 Trace 不完整分别展示，不假称成功。
5. 记录 TaskLoop/Milkie/Inquiry SHA、实际模型/配置来源及运行耗时；未知用量不补0。原 `research-audit.ndjson` 历史不删，不将它当新 Trace 事实源。

配置/凭据未齐、gpt-6-astra/high有效设置未核实时保持 blocked；不复制全局登录，不启动第三轮SOTA质量实验。浏览器创建、引用、关闭、刷新见 research.md，停止/补资料见 reliability.md。
