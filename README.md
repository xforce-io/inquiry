# Inquiry

以证据推进判断的研究应用：发现来源、探活留存、整理、证据分析和报告，支持有界补查及用户补充网址。

- TypeScript Web + 薄 Rust Web 宿主，直接依赖 TaskLoop Rust crate。
- Inquiry 拥有研究 Skill、业务 Playbook/Schema、工具获取事实与研究质量 gate。
- TaskLoop 封装执行、状态、产物接受、隔离与故障处理；Inquiry 不读取其表、控制 Worker 或另建执行状态机。
- 一期一研究对应一个公开 ExecutionId，从原始输入和已接受产物构建展示，不增加独立业务状态数据库。

设计入口：[Issue #1](https://github.com/xforce-io/inquiry/issues/1)、[L2 v0.5.0](docs/design/1-minimal-research-loop.md)、[领域资源](docs/design/1-minimal-research-loop/contract/)、[设计复核](docs/design/1-minimal-research-loop/review.md)。通用契约仅认 [TaskLoop Issue #2](https://github.com/xforce-io/taskloop/issues/2)，不在本仓库复制。

当前只有 Draft 设计与领域见证，无产品实现或已接入的 TaskLoop crate。真实 Rust/Harness、页面和研究质量验收待执行；设计提交不可当作可用依赖版本。术语见[名词表](docs/glossary.md)。
