# 执行迁移与 Trace 关联（Issue #3 S2）

标准：`docs/design/3-milkie-trace-research-migration.md` v0.3 §11，S2.A1/S2.A2。本文件定义验收路径，不能把登记或确定性测试当完整产品验收通过。

1. 沿研究页的同一 ExecutionId，经本地会话保护的 `/api/research/{id}/trace` 查询阶段、Agent、工具及检查；核对 source 关联、最后活动、耗时、完整性缺口；分页/增量按 TaskLoop 公开游标继续。
2. 经 `/trace/export` 导出有界分页，再经 `/budgets` 核对当前与历史阶段的有效预算及来源。不得读内部 SQLite/JSONL、另造状态或用工具数推算模型迭代。
3. S2.A1 必须使用 research.md 中 #3 本次真实研究，不能以 fixture 代替六阶段实际记录；确定性测试另外验证未采集、非法游标、错误研究、会话失效和容量/保留状态转换。
4. S2.A2 保存两次既有 Vertex 失败的原证据索引；本次429、人工补网址、等待和失败逐项保留。真实取消须实际确认停止；停止未知与 Trace 不完整分别展示，不假称成功。
5. 记录 TaskLoop/Milkie/Inquiry SHA、实际模型/配置来源及运行耗时；未知用量不补0。原 `research-audit.ndjson` 历史不删，不将它当新 Trace 事实源。

配置/凭据未齐、gpt-6-astra/high有效设置未核实时保持 blocked；应用不复制全局登录；宿主环境准备须有本轮用户授权，不启动第三轮SOTA质量实验。浏览器创建、引用、关闭、刷新见 research.md，停止/补资料见 reliability.md。

## 配置与原生失败/停止边界

- `cargo test --locked --offline --test execution_config`：显式路径、已登记两档（`gpt-6.1-sol/high` 与 `gpt-6-astra/medium`）、无配置内容回显、未登记模型 `grok-4.7` 与未登记推理强度被拒绝且文件不改写、`codex_binary` 加 `max` 或 `low` 同样拒绝且不安装启动器、`medium` 仍由启动器注入 `high`、非法策略不装配；通过仅表示配置可用，不表示已登录。
- `cargo test --locked --offline --lib article::discovery::tests::initial_analysis_stops_paging_and_rejects_a_public_gap_without_a_search`：初次分析来源读取 8 次通过、第 9 次拒绝；其它节点不受该上限；有缺口且原生搜索回执为空时拒绝写入发现。不证明下一次真实研究已经搜到公开材料。
- `cargo test --locked --offline --test tier_binding`：真实研究 Playbook 经 `domain::bindings` 运行。分析与另一工作状态解析到两个不同的已登记模型 id；较低档状态在 `difficulty=hard` 时改为较高档，普通事实保持较低档；分析预算 80、另一工作状态 40；TaskLoop 流程定义拒绝难易字段和模型字段。

## 阶段交卷（Issue #19 S1–S3）

标准：沿用 `docs/design/2-research-web-mvp.md` 的预算不足规则。综合、成稿、评审和修订在登记时限前提交已写入结果；分析缺失时写 `decision=need_input` 且 evidence 为空；没有已接受草稿或不存在评审时失败，不编造引用或评审。`article-analysis` 的 max_run_ms 保持 450000，任务 `max_active_ms` 与 `max_wall_ms` 保持 1800000。

- `cargo test --locked --offline --lib article::revise::tests`：已写入分析不被时限失败替换；未写入分析的 evidence 长度为 0；评审和成稿缺失时不写产物；修订只原样沿用已接受草稿。综合分析打开 16 个不同来源后第 17 个拒绝，重复打开已读来源被拒绝且新来源仍通过，不带来源编号的索引不计入这 16 个；随后写入的分析在时限失败时仍保留；剩余时间进入 90000 毫秒交卷窗口时第一次来源读取即被拒绝；成稿连续 17 次来源读取都到达实际工具。
- `cargo test --locked --offline --lib article::tests::keyword_index_points_at_one_saved_paragraph_and_skips_the_rest`：关键词只命中包含该词的可用来源，摘录是该段落前缀。
- `cargo test --locked --offline --test article_goal source_index_gives_valid_example_and_historical_oversized_limits_remain_rejected`：真实 `inquiry.article_source` 在不带来源编号时按关键词返回 `r1s1` 的摘录，不存在的关键词返回空命中。

## 综合分析交卷（Issue #24 S1–S2）

标准：沿用阶段交卷规则。综合分析不得把 80 次工具预算耗在来源翻页上而不提交。`article-analysis` 的 max_run_ms 保持 450000，任务 `max_active_ms` 与 `max_wall_ms` 保持 1800000。上面的 `article::revise::tests` 覆盖 S1 与 S2。不把该测试当成下一次冻结题报告已经完成。
- `cargo test --locked --offline --test article_goal native_discovery_uses_round_receipts_and_fetches_without_http_search_fallback`：原生综合 finalize 窗口短于 450000，且该上限仍为 450000。
- 冻结 Vertex 原题的真实研究在合入候选上另跑，不把上述测试当成报告已完成。

## 模型档位（Issue #11 S1–S4，Issue #12 S1–S3）

标准：`docs/design/11-state-model-tiers/product.md` v0.1 的 S1.A1–S4.A1，以及 `docs/design/12-task-difficulty-tier/product.md` v0.1 的 S1.A1–S3.A1。上面两条测试是本文件对应这些验收的可执行路径。网页研究创建仍走文章编排；该路径不因 `difficulty` 改档。配置或登录缺失时，真实模型步骤记 blocked，不把测试通过写成已登录。
- `MILKIE_NODE=<固定Node> MILKIE_CLI=<固定Milkie构建> INQUIRY_NATIVE_CODEX_DIR=<Codex0.166.0所在目录> cargo test --locked --offline --test native_milkie -- --ignored --nocapture`：实际生产启动器/Sidecar/原生Codex连接本机401端点，核对high、失败元数据Trace、第二请求取消/隔离及SIGTERM退出。仅模拟凭据与拒绝端点，无真实模型成功；不得代替S1报告与S2同一真实研究关联。

## #7 现有登录与固定high接入

原生配置默认medium时，通过显式codex_binary生成任务私有启动器并强制high；使用reuse_native_login时必须指定既有config_dir/sessions真实目录。先跑配置路径/不改配置检查及native_milkie本地401/停止验证；再按运行前登记显式执行`INQUIRY_REAL_PROBE_DIR=<绝对证据目录> INQUIRY_REAL_PROBE_CONFIG=<显式JSON> cargo test --locked --test native_goal_probe -- --ignored --nocapture`，只允许一个真实Planner入口，既定小Run预算拒绝后续文章图。须核对原生执行completed及真实host工具提交，规划仅因max_runs不足被拒；不把预期的整体failed当研究成功，也不以此替代Web或文章语义验收。
