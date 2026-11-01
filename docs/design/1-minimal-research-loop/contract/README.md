# 基础契约设计样例与见证模型

详细语义的唯一事实源是[基础契约设计](../../1-minimal-research-loop.md)。此目录是它的机器可读实例和验证附件，不是可部署的 TaskLoop 实现。

| 文件 | 用途 |
|---|---|
| `skills/` | 标准 Skill 包样例，给 Agent 阅读，不自动安装或注册能力 |
| `bundle.schema.json` | EntryContract 与 Playbook 的结构约束 |
| `research.bundle.json` | 六阶段研究、补查回环、等待来源和三个终点 |
| `document-review.bundle.json` | 不同业务的状态/信号/检查器样例 |
| `registry.example.json` | 宿主注册映射；示例 CLI 路径不代表实际安装 |
| `submission.schema.json` | 受信适配器的候选产物提交格式 |
| `check-result.schema.json` | pass/fail/error、业务结果、证据和违规格式 |
| 其他 `*.schema.json` | 输入、阶段产物及信号的数据格式 |
| `verify_contract.py` | 契约结构、领域关系和离散状态模型检查 |
| `verification-result.json` | 最近一次运行的逐项结果与范围说明 |
| `trace.examples.json` | 成功、补查、等待/继续、取消的具体状态与产物轨迹 |
| `requirements.txt` | 设计验证辅助环境的锁定依赖，不是产品后端依赖 |

可复现运行（在仓库根目录，使用独立临时环境）：

```sh
python3 -m venv work/contract-check-env
work/contract-check-env/bin/python -m pip install -r docs/design/1-minimal-research-loop/contract/requirements.txt
work/contract-check-env/bin/python docs/design/1-minimal-research-loop/contract/verify_contract.py
```

测试使用固定字节与受信 fixture 回执，不联网访问 example.org。来源/引用交叉校验是真实程序检查；其余研究语义 gate 和文档审阅 gate 是显式 fixture。新增同 Skill 多实例的身份、回调/目录/回执、产物、信号、取消、预算、排队和恢复测试，以及两个实例的线程执行和最小 SQLite 表的双连接 CAS 竞争/控制行重开见证。单个模型依然按单写者串行修改，完整快照恢复是副本恢复，不证明生产 Checkpoint 事务、真实 Harness 会话/进程隔离、鉴权、网络隔离或模型语义质量。真实实现的必需验收仍见设计 §11.3。

执行本验证不改变 Rust 后端和 TaskLoop crate 的技术选型，也不启动产品实现或安装此目录中的 Skill。

一期公开输入仅支持问题、范围和 HTTP(S) 网址，上传/跨实例资料导入延后。run_epoch 是执行代数，不是分布式租约。模型中的 recheck 是后续扩展见证，不要求一期交付；检查器异常和存储读取错误必须保留原阶段并阻塞。
