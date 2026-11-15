# 模型档位技术设计

- Issue: [#11](https://github.com/xforce-io/inquiry/issues/11)
- L1: [product.md](product.md) v0.1 Draft
- 版本: v0.1
- 验收: S1.A1、S2.A1、S3.A1、S4.A1
- 功能文件: `.agents/skills/verify-inquiry/features/execution.md`

## 1. 设计依据与技术目标

依据 L1 v0.1。加载接受登记集合，拒绝未登记模型或推理强度，不改写连接文件与专用原生配置。Playbook 工作状态绑定到不同档位。预算表保持独立。

非目标见 L1。不把难易判断放进 TaskLoop。

## 2. 现状与改动范围

现状：`ExecutionSetup::load` 只接受 `gpt-6.1-sol`，无 `codex_binary` 时原生配置必须是 `high`；有 `codex_binary` 时私有启动器注入 `high` 且不改配置。`resources/research.bundle.json` 的工作条目原先都使用 `research.agent`。`domain::bindings` 只注册这一个执行器。`budget::runtime_options` 已把 `inquiry.analyze` 设为 80、其余研究工作状态设为 40。文章编排仍注册一个 `article.agent`。

改动：`src/tier.rs` 持有登记表。`src/execution.rs` 按登记表准入。研究 Playbook 把分析改为 `research.higher`，其余工作状态改为 `research.routine`。`domain::bindings` 按条目里的执行器 id 注册宿主传入的执行器。

## 3. 总体架构与关键路径

档位表与选择是纯决定。模型与推理强度留在该档的连接和原生配置上。TaskLoop 仍按条目里的执行器 id 查找宿主执行器。

主路径：加载读配置 → 成员检查命中登记档 → 装配执行器，文件不写回。Playbook 运行时，`research.routine` 解析到较低档模型 id，`research.higher` 解析到较高档模型 id。

失败路径：未登记模型在连接检查拒绝；较高档上未登记推理强度在原生配置检查拒绝；`profile` 始终拒绝。错误使用固定句子，不回显配置正文。

折衷：较高档加 `codex_binary` 时仍视为 `high`，以保留现有「文件写 medium、启动器注入 high、文件不改」的行为。较低档没有这条覆盖，文件中的推理强度必须就是 `medium`。

## 4. 数据与状态契约

登记档：

| id | 模型 | 推理强度 | 默认执行器 id |
|---|---|---|---|
| higher | gpt-6.1-sol | high | research.higher |
| routine | gpt-6-astra | medium | research.routine |

连接文件仍是 contract 1、`agent-cli`、`codex` 加模型 id。不新增执行配置字段。一次加载一档。

不变量：加载返回后，该次读取的连接文件与 `config.toml` 字节不变。预算数字不读档位表。

## 5. 接口与协作契约

`ExecutionSetup::load` 成功表示该档可装配，不表示已登录或模型调用成功。

`tier::RoutingExecutor` 实现 `Executor`。它读取条目上的执行器 id，调用 `tier::select`，再委托对应内部执行器。`select` 在本 Issue 的默认事实上返回该执行器的默认档。返回值只有已登记档位。

TaskLoop `Flow` 与 `EntryContract` 保持 `deny_unknown_fields`，没有难易字段，也没有模型字段。

## 6. 运行与保障机制

工具调用预算仍由 `src/budget.rs` 决定：分析 80，其余 40，提交保留 2。不改阶段时限与 30 分钟上限。

原生启动器只注入 `high` 或 `medium`。较高档注入 `high`。启动器写在私有数据目录，不改专用原生配置。

## 7. 迁移、发布与回滚

旧的单档较高配置（`gpt-6.1-sol` 与 `high`，或较高档加 `codex_binary`）继续可加载。只准备了较高档的宿主不必新增文件。

回滚是恢复本变更前的加载条件与 Playbook 执行器 id。不迁移历史执行记录。

## 8. 测试与验证

| 验收 | 层级 | 机制 |
|---|---|---|
| S1.A1 S3.A1 | Integration | `tests/execution_config.rs` 调用 `ExecutionSetup::load` |
| S2.A1 S4.A1 | Integration | `tests/tier_binding.rs` 装载真实 Playbook 与 `domain::bindings`，经绑定执行器调用 `execute` |
| 既有较高档与启动器 | Integration | 原 `execution_config` 用例保持通过 |

功能文件：`.agents/skills/verify-inquiry/features/execution.md`。

## 9. 技术风险与开放问题

较低档模型是否被当前 Codex 接受，加载测试不能证明。真实研究仍走文章编排的单个执行器；那条路径不在本设计里拆成两档进程。
