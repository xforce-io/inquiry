---
name: verify-inquiry
description: 在本地真实 Inquiry 上验证 Issue #2 的研究、可靠性、质量实验及 Web 路径。
---

# Inquiry 验证手册

## Launch

仓库根目录运行 `npm ci --prefix web`，`VITE_BUILD_SHA=$(git rev-parse HEAD) npm run build --prefix web`，`cargo build --locked`。Node 20.19+、Rust 1.91.1+、curl、已登录的 Codex CLI 0.166.0 必须可用；不复制登录凭据。实际研究固定 gpt-6-astra/high，模型不回退。`INQUIRY_DATA_DIR` 指向本次专用私有目录，`INQUIRY_PORT` 默认3410，运行 `cargo run --locked`。只监听127.0.0.1，不部署公网。

## Doctor

1. 工作树干净，记录候选完整SHA、Cargo.lock摘要与 TaskLoop rev；正式候选构建后不可改方法包/资源。
2. `cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`、前端 build 通过。确定性测试中的 synthetic 数据不能当模型/产品成果。
3. `/api/health` 的 build 必须等于候选，浏览器 `document.documentElement.dataset.build` 相同。只做读取确认，不用脚本直接创建以冒充界面操作。
4. 私有目录保留 TaskLoop store 和 research-audit.ndjson；原始网页不进入Git。记录实际模型、耗时、可得用量，未知不补零。
5. **首次真实研究前**按 L2 §11.2–11.7 封存共同提示词、UTC时间、摘要、匿名随机映射及评分协议。没有登记就不得生成任何真实报告。

## Drive

读取 [功能地图](features/README.md)，按对应文件逐项操作。Web 使用 ego-browser 技能/真实浏览器，创建、点击、刷新、面板关闭通过实际界面完成；接口检查仅用来核对持久化，不代替驾驶。

正式demo先创建并生成第一报告，再生成3份无工具 gpt-6-astra/max基线，最后两个新上下文盲评并审计。质量未达标如实fail，不换题/阈值/弱基线；保留每次失败和候选。

浏览器异常先保留页面/截图再定位；修复后冻结新候选，核对影响和必要复验。不能改证据中的SHA冒充新跑。

## Evidence

证据仅存忽略的 `work/verify/2-<run>/`：候选/环境/时间、健康版本、操作日志、截图、研究ID、公开接口快照、ArtifactRef及字节摘要、原始运行审计、质量报告/盲评/核查。验收行 S1–S5 按 pass/fail/blocked/not_run 记录并链功能文件；完整必需项通过才能汇总pass。发布摘要只含可公开报告、短引文、URL和评分，不含会话cookie、凭据或第三方完整网页。

## Cleanup

停止本次宿主时发送正常中断，等待 runtime.shutdown。保留私有数据、日志、失败结果和实验登记，不删store或覆写输出。关闭本任务创建的浏览器空间；如用户接管，尊重其控制，不能另开空间绕过。未完成的研究明确记录取消/阻塞，不能宣称完成。无线上部署或发布后检查。
