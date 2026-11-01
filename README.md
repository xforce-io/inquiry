# Inquiry

以证据推进判断的持续研究应用，基于 [TaskLoop](https://github.com/xforce-io/taskloop) 构建。

Inquiry 围绕研究问题组织资料、证据、判断、结论与报告，根据中间结果持续追问、求证和调整研究路径。

## 项目边界

- Inquiry 提供研究交互、研究 Skills、领域 Schema 与研究质量规则。
- TaskLoop 提供通用任务规划、路由、执行、评价、重规划及持久化机制。
- 实际推理与工具调用复用第三方 Agent Harness。

研究 Skills 首期随本仓库维护并保持独立包边界。TaskLoop 的具体依赖版本与接入方式待产品设计后确定，当前尚未接入运行时。

## 当前状态

项目处于设计阶段，尚未实现产品。当前范围、用户路径和接入契约以 Issue #1 的最小版本设计为准。

一期技术方向已确定为 TypeScript 前端、Rust 后端、TaskLoop Rust crate 直接依赖。最小闭环为上传文本/Markdown、提出问题、单次研究、查看带来源报告；资料不足时由用户补充后重新发起。详细边界与依赖契约见 [Issue #1 最小版本设计](docs/design/1-minimal-research-loop.md)。当前设计仍为 Draft，TaskLoop Rust crate 尚未交付，未完成运行时接入。

术语以 [名词表](docs/glossary.md) 为准。底座设计见 [TaskLoop RFC-0001](https://github.com/xforce-io/taskloop/blob/main/docs/RFC-0001-architecture.md)。
