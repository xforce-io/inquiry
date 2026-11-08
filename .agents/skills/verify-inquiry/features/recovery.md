# 搜索恢复与补查（Issue #5）

事实源：[Issue #5](https://github.com/xforce-io/inquiry/issues/5)、[L2 v0.1](../../../../docs/design/5-research-search-recovery.md)。固定候选、方法0.3.7及TaskLoop rev；不启动第四次质量实验。

| 子项 | 入口与操作 | 成功结果及证据 |
|---|---|---|
| S1.A1 | 创建页输入已封存问题、从无历史开始；核对原生输入 | 首次空历史合法，scope/plan仍必需；真实发现可提交；保存入口、输入与原生引用 |
| S1.A2 | 整理/分析提出缺口，返回来源发现 | 原生输入含授权缺口、历史候选查询和资料索引；按未补齐缺口补查；Integration两条回路及越权拒绝仍通过 |
| S2.A1 | 当前页面、受控429恢复及持续不可用；等待中刷新 | 显示服务等待、不出现补网址表单；有限恢复后继续或结束为服务不可用阶段成果；真实查询回执、请求时间与截图；不得冒充真实服务恢复 |
| S2.A2 | 搜索冷却/服务等待中停止；重复停止及来源版本冲突 | 取消后不再请求，保存内容仍可查；过期/冲突由公开接口确认；受控测试覆盖30秒总边界、共享冷却和取消 |
| S3.A1 | 当前专用Milkie/原生Harness，从真实创建页完成6阶段报告 | 至少1条引用打开保存段落并返回；刷新一致；同研究Trace query/export一致、真实模型与三个SHA可核对；旧失败原样保留 |

受控浏览器宿主：`INQUIRY_RECOVERY_TEST_DATA=<私有绝对目录> INQUIRY_RECOVERY_TEST_PORT=3425 cargo test --locked --lib search::tests::browser_recovery_host -- --ignored --nocapture`。仅cfg(test)注入HTTP脚本与确定性执行器，使用候选相同的Search/Router/Runtime/前端；输入`恢复`、`持续`、`冷却`分别触发相应故障。生产宿主没有此替换入口。

关联[研究闭环](research.md)、[来源](sources.md)、[执行](execution.md)、[可靠性](reliability.md)。验证数据落在`work/verify/5-<候选>/`，保留原始故障和每次人工介入。
