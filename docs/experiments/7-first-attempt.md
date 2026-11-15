# #7 首批真实研究：认证阻塞

2026-11-13执行；依据[设计v0.2](../design/7-deep-article-quality-loop.md)和[协议v0.3](7-quality-protocol-draft.md)。源码候选`35a8dce4da2c5e35ddb728b9a8842a44d5dc4d78`，TaskLoop `daf0fbaae905be555b543063a4e0d477ea522be8`，Milkie `98b5ec1c02abbc9612356387d982d6e96b272a21`，原生Codex0.166.0，目标模型gpt-6-astra/high。

首批从真实创建页提交协议中的完整Palantir Foundry Vertex问题、范围及空种子网址，浏览器与健康版本相同，受理一项研究`goal-57388953-5215-471e-8926-e023f0321139`。网络请求记录核对三个语义输入与登记一致。界面生成自己的幂等键；这一请求身份差异在提交前另附说明，没有修改问题、范围或来源权限。

| 项目 | 实际结果 |
|---|---|
| 事前登记SHA-256 | `83c41bff46d0611aeec3dc7e002973e57078d07028df47635efdfdfabda83d30` |
| 匿名映射承诺SHA-256 | `7f31a13a283ef9077501413bb5d320b5e8b40a8d65cfae64b1b934a222232e47`；尚未进行匿名评价或解盲 |
| 规划入口 | 3次既定尝试，均未成功；没有工具调用或接纳产物 |
| 原生原因 | 3条execution.finished均为`auth_failed`，均确认原生停止 |
| 框架终点 | `failed` / `planning_exhausted`，不是`not_met`或`achieved` |
| 资源 | Run记录累计73.146秒；公开预算计量73.185秒，含框架处理。工具0次；模型实际用量未知，不填0 |
| 内容成果 | 没有初稿、修订稿或语义评价，不能判断方法优劣 |
| 验收结论 | 真实文章/效果必需项blocked；不启动直接生成、同资料或效果盲评，不关闭#7 |

本地`login status`仍显示已登录，只说明本地登录记录存在；实际远端认证拒绝，两者不互相替代。已请求用户在此前授权的专用目录重新登录；不输出、复制凭据，不切换模型，不自动新建研究刷结果。恢复后须先核对累计预算和受影响项，再登记新批；本次失败保留。

证据在本工作树忽略目录`work/verify/7-35a8dce/`：registration.json、registration-ui-identity-addendum.json、ui-submission.json、final.json、attempt-result.json、diagnostic-trace.json、diagnostic-budgets.json、failed-page.txt/png。源码冻结后fmt、严格Clippy、46项测试、前端构建、原生401/停止与真实网络探针均通过；最后两项只证明各自协议/获取路径，不代表真实账号或文章通过。原#2的FAIL/BLOCKED保持不变。
