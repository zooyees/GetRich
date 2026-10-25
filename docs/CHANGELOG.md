# Changelog

本记录按交付版本整理，最新版本为 **0.2.3**。历史评估文档保留当时结论，当前实现与验证结果以对应发布说明为准。

| 版本 | 主要变更 | 详细记录 |
| --- | --- | --- |
| 0.2.3 | 星图底栏、详情跳转、自选搜索 | [发布说明](RELEASE_0.2.3.md) |
| 0.2.2 | 星图采集刷新、完整历史、分时、F10、资金与公告 | [发布说明](RELEASE_0.2.2.md) |
| 0.2.1 | 桌面布局、设置分区、表单和空状态 | [UI 修正与验收](UI_CORRECTIONS_0.2.1.md) |
| 0.2.0 | 行情工作台重构、行业星图、数据质量与后台任务 | [开发与验收](WORKSTATION_IMPLEMENTATION_2026-10-25.md)、[星图设计与实现](UI_MARKET_ATLAS_2026-10-25.md) |
| 0.1.0 | Rust 工作区、SQLite、文件导入、CLI、API 与桌面基座 | 本文历史记录 |

## 0.2.3 · 2026-10-25

- 星图同步状态和日期、覆盖口径移到底边栏右侧，释放画布上方空间。
- 修正速览打开的目标股票被列表后台刷新清空，保留市场与代码直至主动换股。
- 自选管理增加名称、代码、市场搜索和键盘选择，自动填写添加表单，隔离过期结果。
- 桌面与 Web 同步修复；搜索基于本地标的库，缺少标的时提示同步股票列表。
- 验证：116 项 Rust 测试、21 项 Web 回归、严格 Clippy、格式检查，以及发布版 8 个接口验收场景通过；另用独立数据库验证实际 Windows 窗口的底栏、跳转和搜索保存。
- 发布包：`GetRich-Windows-0.2.3-20261025.zip`，10,246,836 字节；SHA-256：`7BA8753BF99BB8606CBDD09B5BB7E0AC9FC67B7D7D47E935D7034185787F701F`。已验证 ZIP 完整性、文件一致性和内嵌页面与源码一致。
- 数据库继续使用 schema 6；升级前关闭旧程序、备份配置和数据库，再复制到新版目录。运行时配置、凭据、数据库及打包产物保存在本地，不纳入源码提交。
- 详见 [0.2.3 发布说明](RELEASE_0.2.3.md)。

## 0.2.2 · 2026-10-25

- 星图使用全宽画布和收起式筛选；刷新改为后台采集行情、同日市值与指数，支持空库初始化。
- 修正单根快照日线阻止历史补全，自动加载完整历史并刷新图表；增加真实分时成交价、均价与成交量。
- 增加 F10、资金流向与公告资料页，独立缓存、来源日期、权限提示及手动重试。
- 修正多实例市场快照读取旧版本问题，见 [0.2.2 发布说明](RELEASE_0.2.2.md)。

## 0.2.1 · 2026-10-25

- 修正桌面布局：居中限宽工作区、四分区设置、等高数据操作卡、整行行情选中与中文排序操作。
- 统一字号、表单高度、导航选中样式、控件对比度与短时反馈；重新设计空库引导及未选股状态。
- 补充真实桌面窗口验收及深浅主题、四尺寸布局回归，见 [桌面 UI 修正说明](UI_CORRECTIONS_0.2.1.md)。

## 0.2.0 · 2026-10-25

- 重设计两端全部工作区；市场态势升级为行业大盘星图，支持市值 / 成交额 / 等面积、缩放、下钻、自选高亮与来源速览，详见 [界面与星图说明](UI_MARKET_ATLAS_2026-10-25.md)。
- 新增同日市值、指数日线同步任务和 market.map 共享接口；缺失权重、历史截面、币种与来源日期明确展示。
- 根据全页面评估修复统一行情、逐标的数据质量、非法输入、短 K 线缩放、周月聚合更新及跨进程设置覆盖。
- 桌面读取异步化；导入预览、后台任务、进度、取消、重试、原生文件选择。
- 两端分页、筛选、选择一致性、图表坐标、窄屏布局及可持久化外观、自选和近期查看。
- 独立事件/控制请求预算、服务实例身份、可靠事件恢复、快照版本和启动诊断。
- 新增回归与 Windows/Linux CI；详见 [开发与验收说明](WORKSTATION_IMPLEMENTATION_2026-10-25.md)。

## 0.1.0

- 自选未入库标的（如 US:MSFT）可打开占位详情并取消星标，不再报 `not found`；设置「移除」立即刷新行情板
- 审查落实（`docs/CODE_REVIEW.md`）：B1/R7 日线不盖盘中价且跨日不 COALESCE；B2 北证 92/83/87/43；B3 默认板块内存；B4/R1 env token 不落盘、pack 不复制 `config.json`；B5 空结果不标成功；B6 无日历不轮询；B7 Pulse 非交易日用 last open；B8 Open 锚点可覆盖；B9 自选先 merge 再分页；B10 XSS；B11/R3 回环 CORS/绑定；R2 import 沙箱
- 自选列表按行分配高度，不再只画出最后一只
- 日 K 不足时后台拉 Tushare 历史，K 线按 80 槽右对齐，避免单根撑满
- WiParse-style workspace: `getrich-core` / `getrich-cli` / `getrich-api`
- SQLite foundation: listings, quotes, bars, metadata, ingest_runs, data_freshness, migrations
- File import contract (JSON/CSV), idempotent upserts
- TuShare: EOD `tushare.sync` (listings+daily) plus optional in-session `rt_k` worker (`data.tushare.enabled`)
- Tonghuashun-style workstation: quote board, detail, K-line, empty/loading/error
- Apple-minimal home / shell chrome (web + egui); quote columns stay Tonghuashun
- JSON CLI + `/v1/invoke` + REST read path with DB busy retries
- Windows pack (`packaging/pack-windows.ps1` → `dist/GetRich.exe`) and WiParse-style `GetRich.ico` wiring
