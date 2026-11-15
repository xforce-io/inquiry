# Inquiry 功能地图

事实源：[Issue #2](https://github.com/xforce-io/inquiry/issues/2) 与 [L2 v0.1.7](../../../../docs/design/2-research-web-mvp.md)。沿用 Issue #1 L1 v0.5.0；不补造验收编号。

| Story | 主要功能 | 必需路径及关联 |
|---|---|---|
| S1 | [研究闭环](research.md) | 6阶段、真实报告、引用面板、返回阅读；关联来源 |
| S2 | [来源与证据](sources.md) | 真实获取留存、三类负例、网络边界、缺口 |
| S3 | [可靠性](reliability.md) | 两研究隔离、刷新、同键重试、补来源、冲突/到期、停止 |
| S4 | [研究质量对照](quality.md) | 事前登记、真实报告和3基线、2独立盲评、事实核查、阈值 |
| S5 | [Web体验](web.md) | 两页面、七操作、空/错/成状态、无阻断使用问题 |

## Issue #3 迁移验收

事实源：[Issue #3](https://github.com/xforce-io/inquiry/issues/3)、[L2 v0.2](../../../../docs/design/3-milkie-trace-research-migration.md)。上述 #2 S4 失败及原协议保持，不在 #3 重新编号或宣称通过。

| Story | 主要功能 | 必需路径及关联 |
|---|---|---|
| #3 S1 | [研究闭环](research.md) | S1.A1/S1.A2：新链路固定Vertex真实六阶段报告、至少1条引用、关闭/刷新；关联来源/Web |
| #3 S2 | [执行与Trace](execution.md) | S2.A1/S2.A2：同研究公共Trace/预算查询及导出、真实停止、保留两次旧失败和新介入；关联可靠性 |
| #5 S1/S2/S3 | [搜索恢复与补查](recovery.md) | S1.A1/S1.A2首次与两条补查回路；S2.A1/S2.A2有界恢复、刷新/停止/冲突；S3.A1当前真实报告、引用与Trace |

## Issue #7 深度文章验收

事实源：[设计 v0.12](../../../../docs/design/7-deep-article-quality-loop.md) §11；产品基线沿用已确认v0.2，技术修订含用户指定Sol、显式复用既有登录/强制high、原生采集有界恢复及经用户明确授权的Codex原生搜索接入。2026-11-13用户确认产品基线。下列编号只属于#7，不覆盖历史#2/#3验收结论。

| Story | 主要功能 | 必需路径及关联 |
|---|---|---|
| #7 S1 | [研究闭环](research.md) | S1.A1/A2：复杂问题完整范围、空输入、对象歧义及澄清 |
| #7 S2 | [来源与证据](sources.md) | S2.A1/A2/A3：自主真实资料、判断回溯、冲突反证；关联 recovery.md 的失败/人工来源 |
| #7 S3 | [研究质量](quality.md) | S3.A1–A4：真实初稿/评价/修订/最终评价、真实语义负例、错误/版本、资源边界；关联 reliability.md |
| #7 S4 | [Web体验](web.md) | S4.A1–A4：创建/阅读/引用/刷新、五类结果、隔离/重放/读取失败；关联 reliability.md 与 execution.md |
| #7 S5 | [研究质量](quality.md) | S5.A1–A3：预登记复杂文章、独立效果核验、引用/负例、完整尝试与资源账目 |

## Issue #11 / #12 模型档位

事实源：[Issue #11](https://github.com/xforce-io/inquiry/issues/11)、[Issue #12](https://github.com/xforce-io/inquiry/issues/12)、[L1 v0.1](../../../../docs/design/11-state-model-tiers/product.md)、[L1 v0.1](../../../../docs/design/12-task-difficulty-tier/product.md)。不改 #2/#3/#7 的验收结论。

| Story | 主要功能 | 必需路径及关联 |
|---|---|---|
| #11 S1–S4 | [执行与Trace](execution.md) | S1.A1 两档加载且四份文件不变；S2.A1 两个工作状态两个已登记模型 id；S3.A1 未登记模型与推理强度拒绝且不含密钥；S4.A1 分析 80、其余 40 |
| #12 S1–S3 | [执行与Trace](execution.md) | S1.A1 一个较低档状态在 hard 时改为较高档；S2.A1 普通事实保持较低档；S3.A1 结果只是已登记档位，TaskLoop 流程 schema 无难易字段也无模型字段 |
| #19 S1–S3 | [执行与Trace](execution.md) | S1 已写入的综合分析在 450000 毫秒内保留并完成；S2 未写入时 evidence 为空的 need_input；S3 成稿与评审不编造缺失引用或评审。任务上限仍为 1800000 |
| #22 S1–S2 | [执行与Trace](execution.md) | S1 `codex_binary` 下 `max`/`low` 在原生配置检查拒绝且文件与启动器都不改写，`medium` 仍注入 `high`；S2 初次分析来源读取 8 次后拒绝并要求原生搜索，有缺口的空回执不写入发现 |
| #24 S1–S2 | [执行与Trace](execution.md) | S1 综合分析来源读取 16 次后第 17 次拒绝且保留已写入分析；S2 90000 毫秒交卷窗口内拒绝继续翻页，成稿 17 次读取不受该上限。阶段 450000 与任务 1800000 不变 |
| #26 S1–S2 | [执行与Trace](execution.md) | S1 未满 16 个不同来源时重复读取被拒绝，新来源通过，索引 3 次不计入；S2 第 17 个不同来源拒绝且保留已写入分析，成稿 17 次不受限。阶段 450000 与任务 1800000 不变 |
| #28 S1–S2 | [执行与Trace](execution.md) | S1 不带来源编号的关键词索引返回一条已保存段落前缀；S2 该索引不计入 16 个不同来源。阶段 450000 与任务 1800000 不变 |
