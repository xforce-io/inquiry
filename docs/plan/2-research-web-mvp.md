# Inquiry Issue #2 实现与验证计划

模式end-to-end；沿用Issue #1 L1/L2 v0.5.0与本票L2 v0.1.3、用户对Rust后端/TaskLoop crate直连/TypeScript Web及真实质量与computer use的明确要求。how: skip无现成机制可讲：仓库只有设计资产，无既有应用；设计Python见证不代替产品实现。按用户AGENTS保留单文件13节L2路径，不采用Skill的嵌套文档默认路径。

TaskLoop已交付：PR #4，main合入9197afac45be43992cd72ef6e0549d92175d3d49，实际审查/运行/双平台CI候选1c69bcf8a1bc0a7d9a4564bbe93f8b13d7317d04。Cargo仅固定此完整SHA与lock；不依赖本地路径、私有表或调度。其8轮独立审查历史不重置；本票是不同应用的实际实现，不借新票绕过旧门。

顺序：生产Skill/领域Schema和有界网络工具→领域gate及公开API宿主→两页Web→确定性边界与功能地图→冻结候选真实研究/质量对照/CUA→独立审查→CI/人工门/PR/合入。先补可判定的负例，再运行昂贵真实模型。代码和研究方法改变要重验受影响项，不把旧证据换SHA。

研究工具只读取公开HTTP(S)资料，逐跳DNS/IP校验并把实际连接固定到已验IP，代理也不得自行重解析目标。网络读取有大小/时间界限；源网页不可信，只提供安全文本，不执行脚本。公开来源与依据：curl官方manpage（connect-to/proxytunnel/suppress-connect-headers）和IANA特殊地址注册表。所有原始网页仅私有留存，不提交公开仓库。

用户路径只做创建与详情两类页面；研究中展示真实阶段/已接纳资料，报告引用打开保存版本面板；等待补网址、取消中/取消完成、阻塞/失败/部分结果/查询错误区分。Host/Origin/会话从可信宿主构造Caller，不接受HTTP自报身份，不建应用执行状态表。

在首份实际研究报告生成前，必须按L2§11封存共同提示词、问题、时间、随机映射与评估协议；先研究后3份Astra/max无工具直接基线，两个独立盲评。原样保留所有失败，不改15分差距/85分/12引用8一手来源条件，不把测试或自评冒充质量通过。

开发中同步建立.agents/skills/verify-inquiry及features映射。S1–S5当前全部not_run；没有真实Web驾驶和质量实验不能交付或先开PR。没有公网部署授权，不部署。

首轮a57e741已实际从Web运行，scope成功，discover47次调用后缺候选失败，未产生报告；调查原文与单独诊断保留在work/verify/2-first。诊断同输入/原已接纳plan、原方法，实际36次搜索才交候选，预设不允许capture/report。修复只更换实际搜索结果生产者、明确最多8次搜索后的收尾、逐次调用审计与中文失败反馈。共同问题/截止/映射/评分不变。新增production search live检查不调用模型且不产生研究报告。
