---
name: verify-inquiry
description: 在本地真实 Inquiry 上验证研究、可靠性、质量实验及 Milkie 执行迁移路径。
---

# Inquiry 验证手册

## Launch

仓库根目录运行 `npm ci --prefix web`，`VITE_BUILD_SHA=$(git rev-parse HEAD) npm run build --prefix web`，`cargo build --locked`。Node 20.19+、Rust 1.91.1+、curl、Codex CLI 0.166.0、固定Milkie构建必须可用；登录由宿主按授权显式提供，默认测试专用目录，或显式reuse_native_login复用现有登录；应用不复制或自动发现全局凭据。当前实际研究固定 gpt-6.1-sol/high（2026-11-13用户指定）；历史Astra实验保留原版本，模型不回退。

Issue #3 使用README和`docs/examples/execution.json`中的显式执行配置；示例限额不是默认批准。`INQUIRY_EXECUTION_CONFIG`是绝对JSON路径，node/CLI/连接文件及专用config/session由宿主提供。原生config.toml须high且无profile覆盖；或显式codex_binary，由私有启动器对子进程覆盖high。复用既有登录须显式reuse_native_login=true，session_dir等于config_dir/sessions实际路径，框架存储仍隔离；不改全局配置、不复制认证。配置通过不代表账号/模型就绪。方法0.4.3当前Milkie构建固定2eb8d9aaaf0daa1efff8feabf06f6e9c874e8e3e（#283原生搜索候选，含已合入#282警告修复，无包发布）；执行配置显式native_search=true，原生模式不调用HTTP搜索回退，health.search_provider须为codex-native；原98b5ec1构建及两次警告误拒绝失败保留（execution-sidecar协议不变）；Codex与同版codex-code-mode-host均在任务专用PATH中，TaskLoop固定rev见Cargo.lock。无适用专用真实环境时保持blocked；宿主仅在用户明确授权后准备专用登录，不用本机401夹具当报告验收。

`INQUIRY_DATA_DIR`指向本次专用私有目录，`INQUIRY_PORT`默认3410，运行 `INQUIRY_EXECUTION_CONFIG=<绝对路径> cargo run --locked`。只监听127.0.0.1，不部署公网。默认system DNS；必要时宿主显式选择INQUIRY_DNS_MODE=cloudflare-doh，健康信息记录该模式。该模式的全部地址仍须通过公开地址边界，不忽略私有答案、不自动回退；真实采集前零模型验证网络。旧#2直接Codex启动方式只用于原历史证据的版本对照，不用于证明新链路。

## Doctor

1. 工作树干净，记录候选完整SHA、Cargo.lock摘要与 TaskLoop rev；正式候选构建后不可改方法包/资源。
2. `cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`、`npm test --prefix web`、前端 build 通过。确定性测试中的 synthetic 数据不能当模型/产品成果。
3. `/api/health` 的 build 必须等于候选，浏览器 `document.documentElement.dataset.build` 相同。只做读取确认，不用脚本直接创建以冒充界面操作。
4. 私有目录保留TaskLoop业务/Trace store、Milkie数据和原生会话位置（以显式session_dir为准，只操作本次Trace绑定的会话）；旧research-audit.ndjson原样保留，新链路不再另写原始工具日志。记录Milkie CLI摘要、三个仓库SHA、实际模型/推理配置、耗时和可得用量，未知不补零。
5. **首次真实研究前**按适用Issue封存输入和运行边界。#2质量实验仍按其L2 §11.2–11.7登记共同提示词、UTC时间、摘要、匿名随机映射和评分协议；#3同题迁移功能复验只封存原Vertex输入、候选/依赖和两次旧失败索引，不启动第三轮SOTA质量实验。没有相应登记不得生成报告。#7采用docs/experiments/7-quality-protocol-draft.md及其运行前登记：复杂Palantir Vertex原题、Q1–Q6全通过、一次修订、独立隔离效果评价、直接生成和同资料一次生成对照；不能复用#2旧分数作本票证明。

## Drive

读取 [功能地图](features/README.md)，按对应文件逐项操作。Web 使用 ego-browser 技能/真实浏览器，创建、点击、刷新、面板关闭通过实际界面完成；接口检查仅用来核对持久化，不代替驾驶。

仅#2原质量实验要求demo后3份无工具gpt-6-astra/max基线及两上下文盲评；原两轮FAIL保留，不由#3重跑。#3按其L2 v0.3的S1.A1/S1.A2/S2.A1/S2.A2执行同一真实研究及诊断/停止，固定prompt及人工介入全部保留。

浏览器异常先保留页面/截图再定位；修复后冻结新候选，核对影响和必要复验。不能改证据中的SHA冒充新跑。

## Evidence

证据存忽略的 `work/verify/{issue}-<run>/`：候选/环境/时间、健康版本、操作日志、截图、研究ID、公开接口快照、ArtifactRef及字节摘要、原始运行审计、质量报告/盲评/核查。验收行按适用Issue编号（#2 S1–S5；#3 S1/S2及四个A项；#7 S1–S5及16个A项）按 pass/fail/blocked/not_run 记录并链功能文件；完整必需项通过才能汇总pass。发布摘要只含可公开报告、短引文、URL和评分，不含会话cookie、凭据或第三方完整网页。

## Cleanup

停止本次宿主时发送正常中断，等待 runtime.shutdown。保留私有数据、日志、失败结果和实验登记，不删store或覆写输出。关闭本任务创建的浏览器空间；如用户接管，尊重其控制，不能另开空间绕过。未完成的研究明确记录取消/阻塞，不能宣称完成。无线上部署或发布后检查。

原生scope300秒/总Goal1800秒的当前候选仅有零模型验证；8114bd7旧180秒原题超时保留。累计模型入口28，原总46不足以覆盖完整剩余20入口，未获新48/研究45授权前不新开模型运行；原12600/10800秒及全部验收保持。
