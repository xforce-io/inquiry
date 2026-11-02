# Inquiry 研究资源与领域见证

唯一 L2：[最小研究闭环与对接](../../1-minimal-research-loop.md)。本目录不定义 TaskLoop 的通用协议或运行算法。

| 文件 | Inquiry 所有的内容 |
|---|---|
| skills/inquiry-deep-research/SKILL.md | Agent 的研究方法，独立包版本 0.2.0，不存任务状态 |
| research.bundle.json | 消费 TaskLoop 格式的六阶段业务定义，不是格式规范 |
| research-input / sources-added Schema | 初始问题/网址/范围与补来源 payload |
| plan / candidates / materials / catalogue / analysis / report Schema | 本业务阶段内容；analysis 中证据有明确定位与 ID 关联 |
| acquisition-fact.schema.json | 搜索/抓取产生的领域获取事实；执行归属由框架信封提供，不重复 run_id |
| verify_research_contract.py、requirements.txt、research-verification.json | 仅业务 Schema/内容关系的设计验证，不 import TaskLoop 内部状态机 |

research.bundle 引用的 task-input.schema.json 是 TaskLoop 保留内建格式。本目录不复制它；框架加载时提供。通用 Schema、原推进模型/轨迹及第二业务 fixture 已移到 [TaskLoop Issue #2](https://github.com/xforce-io/taskloop/issues/2)，研究 fixture 仅保留旧版冻结证据，不是生产研究资源的第二事实源。

领域验证使用锁定的 JSON Schema Python 校验器，仅为设计见证，不改变 Rust 技术选型、不安装 Skill、不启动真实模型/网页。真实 Rust/Harness、页面、来源解析和研究语义质量仍 not_run。
