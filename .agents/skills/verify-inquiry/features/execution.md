# 执行迁移与 Trace 关联（Issue #3 S2）

标准：`docs/design/3-milkie-trace-research-migration.md` v0.3 §11，S2.A1/S2.A2。本文件定义验收路径，不能把登记或确定性测试当完整产品验收通过。

1. 沿研究页的同一 ExecutionId，经本地会话保护的 `/api/research/{id}/trace` 查询阶段、Agent、工具及检查；核对 source 关联、最后活动、耗时、完整性缺口；分页/增量按 TaskLoop 公开游标继续。
2. 经 `/trace/export` 导出有界分页，再经 `/budgets` 核对当前与历史阶段的有效预算及来源。不得读内部 SQLite/JSONL、另造状态或用工具数推算模型迭代。
3. S2.A1 必须使用 research.md 中 #3 本次真实研究，不能以 fixture 代替六阶段实际记录；确定性测试另外验证未采集、非法游标、错误研究、会话失效和容量/保留状态转换。
4. S2.A2 保存两次既有 Vertex 失败的原证据索引；本次429、人工补网址、等待和失败逐项保留。真实取消须实际确认停止；停止未知与 Trace 不完整分别展示，不假称成功。
5. 记录 TaskLoop/Milkie/Inquiry SHA、实际模型/配置来源及运行耗时；未知用量不补0。原 `research-audit.ndjson` 历史不删，不将它当新 Trace 事实源。

配置/凭据未齐、gpt-6-astra/high有效设置未核实时保持 blocked；应用不复制全局登录；宿主环境准备须有本轮用户授权，不启动第三轮SOTA质量实验。浏览器创建、引用、关闭、刷新见 research.md，停止/补资料见 reliability.md。

## 配置与原生失败/停止边界

- `cargo test --locked --offline --test execution_config`：显式路径/原生high/固定模型、无配置内容回显、非法策略不装配；通过仅表示配置可用，不表示已登录。
- `MILKIE_NODE=<固定Node> MILKIE_CLI=<固定Milkie构建> INQUIRY_NATIVE_CODEX_DIR=<Codex0.166.0所在目录> cargo test --locked --offline --test native_milkie -- --ignored --nocapture`：实际生产启动器/Sidecar/原生Codex连接本机401端点，核对high、失败元数据Trace、第二请求取消/隔离及SIGTERM退出。仅模拟凭据与拒绝端点，无真实模型成功；不得代替S1报告与S2同一真实研究关联。

## #7 现有登录与固定high接入

原生配置默认medium时，通过显式codex_binary生成任务私有启动器并强制high；使用reuse_native_login时必须指定既有config_dir/sessions真实目录。先跑配置路径/不改配置检查及native_milkie本地401/停止验证；再按运行前登记显式执行`INQUIRY_REAL_PROBE_DIR=<绝对证据目录> INQUIRY_REAL_PROBE_CONFIG=<显式JSON> cargo test --locked --test native_goal_probe -- --ignored --nocapture`，只允许一个真实Planner入口，既定小Run预算拒绝后续文章图。须核对原生执行completed及真实host工具提交，规划仅因max_runs不足被拒；不把预期的整体failed当研究成功，也不以此替代Web或文章语义验收。
