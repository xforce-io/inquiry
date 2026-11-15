# 难易改档技术设计

- Issue: [#12](https://github.com/xforce-io/inquiry/issues/12)
- L1: [product.md](product.md) v0.1 Draft
- 版本: v0.1
- 依据: [11 技术设计](../11-state-model-tiers/technical.md) v0.1
- 验收: S1.A1、S2.A1、S3.A1
- 功能文件: `.agents/skills/verify-inquiry/features/execution.md`

## 1. 设计依据与技术目标

L1 v0.1。Inquiry 按状态默认档与任务事实选择已登记档位 id。TaskLoop 不判断难易。

## 2. 现状与改动范围

[#11](https://github.com/xforce-io/inquiry/issues/11) 把工作状态绑到 `research.routine` 或 `research.higher`。TaskLoop `Invocation.inputs` 是实例原始输入。`Flow` 与 `EntryContract` 使用 `deny_unknown_fields`，没有模型字段。

改动：`tier::select` 在默认档为 routine 且 `inputs.difficulty == "hard"` 时返回 higher，否则返回默认档。`RoutingExecutor::execute` 调用 `select` 后委托对应内部执行器。`resources/research-input.schema.json` 增加可选 `difficulty`，取值 `ordinary` 或 `hard`。不改 TaskLoop。

## 3. 总体架构与关键路径

主路径：TaskLoop 按条目执行器 id 找到 `RoutingExecutor`。执行器读取 `invocation.entry.executor` 与 `invocation.inputs`，得到档位，再调用该档的内部执行器。

失败路径：执行器 id 没有默认档时，执行结果为失败，不猜测模型。流程定义若带上难易或模型字段，反序列化失败。

折衷：静态的条目执行器 id 不能单独表达「这道题更难」，所以选择留在宿主执行器里，而不是写进 TaskLoop 的 flow。

## 4. 数据与状态契约

任务事实是研究输入上的可选 `difficulty`。缺省与 `ordinary` 都是普通。只有 `hard` 升级。

选择结果是 `higher` 或 `routine`。模型 id 只从登记表读取。

研究输入 `additionalProperties` 仍为 false。旧输入不带 `difficulty` 时继续有效。

## 5. 接口与协作契约

`tier::select(executor_id, inputs) -> Option<&Tier>`。调用方只使用返回档的 id 与登记模型。

TaskLoop 流程 schema 不新增字段。文章 `question` schema 不新增 `difficulty`。

## 6. 运行与保障机制

预算、时限与 30 分钟上限不变。改档不改变 `inquiry.analyze` 的 80 次和其他工作状态的 40 次。

## 7. 迁移、发布与回滚

无数据迁移。不带 `difficulty` 的已有输入保持默认档。回滚是去掉可选字段和 `select` 的升级分支。

## 8. 测试与验证

| 验收 | 层级 | 机制 |
|---|---|---|
| S1.A1 S2.A1 S3.A1 | Integration | `tests/tier_binding.rs` 经 `domain::bindings` 取执行器并调用 `execute`；用真实 `Flow` 反序列化拒绝多余字段 |

功能文件：`.agents/skills/verify-inquiry/features/execution.md`。

## 9. 技术风险与开放问题

真实文章研究入口不读取 `difficulty`。本 Issue 的行为由研究 Playbook 绑定证明。
