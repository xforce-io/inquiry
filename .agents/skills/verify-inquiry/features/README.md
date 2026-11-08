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
