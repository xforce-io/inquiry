# Inquiry

以证据推进判断的研究应用：发现来源、探活留存、整理、证据分析和报告，支持有界补查及用户补充网址。

TypeScript Web → 薄 Rust Web 宿主 → TaskLoop Rust crate。Inquiry 提供研究 Skill、Playbook/Schema、公开网页工具与领域质量检查；TaskLoop 封装执行、状态推进、产物接纳、隔离和故障处理。应用不读取框架内部表，不建设第二套执行状态机。

## 本地运行

需要 Rust 1.91.1+、Node 20.19+、curl、已登录的 Codex CLI **0.166.0**，账号能访问 `gpt-6-astra`。研究固定该模型/high，不做模型回退。TaskLoop 固定完整经审 SHA，Cargo.lock 已锁定。

```sh
npm ci --prefix web
VITE_BUILD_SHA=$(git rev-parse HEAD) npm run build --prefix web
cargo run --locked
```

打开 http://127.0.0.1:3410 。只监听本地，不是多用户托管服务。`INQUIRY_PORT` 可指定非零端口，`INQUIRY_DATA_DIR` 可指定私有工作目录，默认为 `data/`。取消请求与取消确认分开；退出使用正常中断并等待执行器停止。刷新原研究地址可恢复视图，不自动重新执行未知工作。

创建页填写问题，网址/范围可选；详情页显示实际阶段、已接纳资料、报告和限制。引用打开保存版本并定位段落，不以当前远端网页替代历史内容。资料不足可补网址；失败或停止保留已有内容。

公开 HTTP(S) 获取每页正文上限1MiB、最多5次重定向，逐跳检查DNS并固定目标IP，拒绝内网、凭据URL和非80/443端口；一期不支持PDF/OCR或登录网页。规范化节选会标记截断。网络实现依据 [curl connect-to](https://curl.se/docs/manpage.html#--connect-to) 与 [IANA 特殊地址注册表](https://www.iana.org/assignments/iana-ipv4-special-registry/)。网页是非可信文本，不能向Agent授予额外工具或权限。

方法0.3.6通过TaskLoop显式阶段策略限制工具调用：分析80次、其余40次，均保留最后2次给产物提交/修正；工具返回剩余额度，失败的已受理调用也计入。读取额度耗尽时仍须用已有证据提交并披露限制，不保证报告必然成功。总处理时长仍为30分钟，网页、产物字节及权限边界不变。

原始网页、运行目录和审计日志均为本地私有数据，不进入公开仓库。报告引用检查仅证明版本/定位/分析关系一致，内容质量仍需独立评价。`data/research-audit.ndjson` 记录实际执行尝试、可得用量和耗时，未知用量不补零。

## 验证与状态

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
npm run build --prefix web
```

确定性测试不是实际模型研究。产品仍在实现验证中，**真实研究闭环、质量对照、Web驾驶、独立审查和交付尚未完成**。质量实验遵循事先固定的协议，不宣称已优于SOTA；正式生成前必须封存输入与随机映射。

- [实现与验收 Issue #2](https://github.com/xforce-io/inquiry/issues/2)
- [唯一实现 L2 与实验协议](docs/design/2-research-web-mvp.md)
- [功能验证手册](.agents/skills/verify-inquiry/SKILL.md)
- [名词表](docs/glossary.md)
- [前置领域设计](docs/design/1-minimal-research-loop.md)
- [TaskLoop 已交付 PR #4](https://github.com/xforce-io/taskloop/pull/4)

## Milkie迁移进度（Issue #3）

当前分支已接入TaskLoop候选 `faae92602e175d7d876f45df20daed55fcfce3a1` 的公共预算接口，移除应用重复计数；阶段/检查时间继承原任务剩余，总处理上限仍为30分钟。依赖独立审查及双平台CI通过，但尚待创建PR所需人工批准，不称已合入版本。

本地会话内增加只读诊断入口：`GET /api/research/{id}/trace?limit=100`、`/trace/export?limit=100`、`/budgets`。Trace支持可选`run_id`及`after`游标，limit必须为1–1000；导出同样有界。未启用采集的实例明确返回disabled/不完整，不补造旧历史。入口沿用现有Host/Origin/session边界，不提供正文采集或独立监控页面。

Milkie生产装配、专用原生配置核对与真实模型/浏览器验证仍未完成；当前启动器仍为原Codex执行路径，不能把新增SDK和确定性测试称为已迁移研究。新链路必须保持gpt-6-astra/high；两次旧Vertex失败和#2质量实验FAIL保持。详细规格见[迁移设计](docs/design/3-milkie-trace-research-migration.md)，验收地图见[执行与Trace](.agents/skills/verify-inquiry/features/execution.md)。
