# 来源与证据（S2）

入口：详情页“研究资料”和引用面板；内部边界用确定性 Unit / Integration。

- 至少一条真实公开URL有发现依据、请求/最终URL、时间/处置、原始及规范化摘要，留存字节与事实相符；失败来源明确处置原因，不冒充可用。
- 核对 materials.fact 与受信工具事实、snapshot.fact、原始/规范化摘要，引用用外层ArtifactRef版本及从0开始的段落、精确短引文。
- 负例分别拒绝伪造获取回执、正文/规范化版本替换、未分析/悬空引用；不能仅靠Schema格式验收。
- 拒绝内网/环回/保留IP、凭据URL、非HTTP(S)、受限端口、重定向越权；每跳DNS并固定连接IP，代理不能重新决定目标。大小/时间/压缩边界保留失败原因。
- 页面只展示安全文本，报告不执行远程HTML/脚本；资料不足及缺口可见。

命令：`cargo test --locked`；实际网络获取和页面回溯另留真实证据。自动测试通过不代替真实来源链路。

## Issue #7 / S2.A1–A3

无人工种子网址的复杂正例必须真实搜索并获取来源；至少一条跨源关键判断及冲突/反证对应保存内容。新引用身份为资料包ArtifactRef.id+sha256、source_id、零基paragraph及精确quote；原文档案按encoding恢复原始字节（缺省identity，新写gzip），恢复长度和摘要与fact.body_sha256对应，规范化段落摘要独立核对。两轮来源标明搜索发现/用户提供，失败原因和截断保留；不得用搜索摘要当正文。获取失败、搜索不可用和用户补充按recovery.md分开取证，旧materials结构仅用于历史读取。


### v0.7 增量恢复验证

固定2.5MiB原文档案和1MiB单页边界；核对gzip无损恢复、长度/摘要/CRC损坏拒绝和旧identity读取。零模型真实采集探针须逐份恢复原文字节、核对规范化摘要，记录实际接纳数，不将压缩率当作内容质量。补查搜索不可用时，仅可在20份上限内重取前轮实际发现且允许重试的URL；新候选优先，fact.retry_of关联原资料包版本与source_id，旧失败记录保留，不伪造本轮搜索回执。

历史方法0.4.1及未启用native_search的HTTP模式按用户明确指示使用DuckDuckGo HTML。健康字段search_provider及每次回执provider必须为duckduckgo-html；实际query须返回相关公开链接，解码/l/?uddg后按原边界逐页获取；不执行Brave或其它备用请求。受控测试覆盖广告/导航剔除、重复uddg拒绝、不安全目标拒绝、解码去重/30条界限、冷却及取消，另由零模型生产采集探针核对真实搜索→正文→原文/规范化SHA。历史Brave证据保留其原版本，不能改标签冒充新来源。


### v0.9 / 方法0.4.2原生发现

显式native_search=true后health.search_provider=codex-native；范围及初次分析产生宿主保留discoveries，首轮/补查仅使用各自真实原生回执，无HTTP搜索回退。模型写保留槽位须拒绝；不可用、不完整、错归属、超实际查询预算不可冒充已发现。单个原生搜索聚合多个query时保留全部实际查询，用run/item归属而不伪造逐URL对应；未知上游请求数不填零。搜索片段不得进入保存正文或引用。

确定性Integration：`cargo test --locked --test article_goal native_discovery`验证两轮归属、零HTTP搜索及独立Fetch和既有来源重取；`cargo test --locked article::discovery`验证保留槽位、片段剔除及回执边界。实际公开抓取可显式指定INQUIRY_NATIVE_RECEIPT_FILE，运行原native_collection探针：该模式是既有真实回执的受控回放，零新增模型，不作为新自主研究；逐份原文字节与规范化SHA核对沿上文。新原题研究须另外真实创建及验收。
