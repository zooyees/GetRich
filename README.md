# GetRich

**[中文](#中文)** · **[English](#english)**

股票分析系统基座（Rust）：配置 / SQLite / 导入约定 / 同花顺风格行情展示。当前版本 **0.4.0**。架构对齐 [WiParse](https://github.com/zooyees/WiParse)（`*-core` + JSON CLI + localhost API + GUI 图标接线）。提供短线/波段/长线研究、专业指标、版本记录、组合复盘与受约束的单证券策略验证。A 股可通过 TuShare `rt_k` 在交易时段刷新报价。

Stock analysis foundation (Rust): config / SQLite / import contract / Tonghuashun-style quote board. **0.4.0**. Research workspaces, auditable data, portfolio accounting and constrained single-security backtesting. A-share quotes can refresh in session via TuShare `rt_k`.

---

## 中文

0.4.0 新增“专业工作台”：专业数据版本与质量、原价/前后复权、指标副图、同业/多股比较、模板/研究池、预警、组合账本和冻结输入回测。严格时点资料不足时阻止运行；支持明确标注的历史研究模拟。见 [0.4.0发布说明](docs/RELEASE_0.4.0.md) 和 [操作手册](docs/PROFESSIONAL_WORKBENCH_GUIDE.md)。

0.3.1 提供盘中短线、日线波段、基本面长线三个研究工作区，独立笔记及分析证据，延续条件选股与数据库备份，见 [建设方案](docs/PROFESSIONAL_WORKSTATION_PLAN_2026-11-01.md) 和 [发布说明](docs/RELEASE_0.3.1.md)。0.2.11 新增 SQLite 持久化请求缓存、历史缺口增量下载和本地存储统计，见 [本版说明](docs/RELEASE_0.2.11.md)。0.2.10 修复交易时段自动刷新链路，隔离报价、历史和后台操作，增加 K 线尾部增量更新及星图绘制缓存，并明确提示实时接口权限状态，见 [本版说明](docs/RELEASE_0.2.10.md) 与 [刷新架构审查](docs/REFRESH_ARCHITECTURE_AUDIT_2026-10-28.md)。0.2.3 修正星图底栏、个股详情跳转并增加自选搜索，见 [本版说明](docs/RELEASE_0.2.3.md)。0.2.2 增加完整历史补全、分时、F10、资金与公告资料，星图使用全宽主画布并支持后台采集刷新，见 [本版说明](docs/RELEASE_0.2.2.md)。前版已修正桌面布局、设置分区与空状态；0.2.0 已按全页面评估改造行情口径、异步任务、页面交互与稳定性；两端界面重设计，市场态势升级为行业大盘星图，支持同日市值、成交额、等面积、缩放下钻与来源追溯。见 [开发与验收说明](docs/WORKSTATION_IMPLEMENTATION_2026-10-25.md)。

### 能做什么

| 模块 | 说明 |
|------|------|
| 软件框架 | 配置深合并、日志、错误码、SQLite 迁移、JSON invoke |
| 股票基座库 | listings / quotes / bars / metadata；幂等写入 |
| 导入 | 你后续灌库；见 [`docs/IMPORT.md`](docs/IMPORT.md) |
| 查看 | CLI `stock list/show/bars`；浏览器 / 桌面：主页 + 行情列表、详情、K 线 |
| 打包 | Windows `GetRich.exe`（见 [`docs/PACKAGING.md`](docs/PACKAGING.md)） |

空库可启动：列表显示「暂无行情数据」，不会假装有行情。

### 仓库结构

```
GetRich/
├── crates/getrich-core/   # 配置、路径、库、导入、读模型
├── crates/getrich-cli/    # JSON CLI（`getrich`）
├── crates/getrich-api/    # 工作台 + HTTP
├── crates/getrich-gui/    # 桌面窗口（WiParse 图标接线）
├── icon/GetRich.ico
├── packaging/             # pack-windows.ps1 / pack.sh
├── docs/
└── config.default.json
```

### 构建

需要 [Rust](https://rustup.rs/) 1.83+。

```bash
cargo build --workspace
cargo test --workspace
```

### 查看

```bash
# 迁移并看空库
cargo run -p getrich-cli -- db migrate
cargo run -p getrich-cli -- stock list --pretty

# 工作台（主页为苹果极简；行情页为同花顺列）
cargo run -p getrich-cli -- serve --bind 127.0.0.1:7878
# 浏览器 http://127.0.0.1:7878/     主页
#         http://127.0.0.1:7878/#/quotes

# 桌面窗口
cargo run -p getrich-gui
```

行情页：板块（全部 / 沪深京 / 沪市 / 深市 / 创业板 / 科创板 / 北证 / 港股 / 美股 / 自选）、搜索、红涨绿跌、日/周/月 K、空/加载/失败可重试。键盘：↑↓ Enter Esc `/`。

### 导入后再看

```bash
cargo run -p getrich-cli -- import bundle --file docs/examples/demo.board.json
cargo run -p getrich-cli -- stock show --symbol 600519 --market CN.SH --pretty
```

字段约定：[`docs/IMPORT.md`](docs/IMPORT.md)。CLI：[`docs/CLI_REFERENCE.md`](docs/CLI_REFERENCE.md)。API：[`docs/DEPLOY_API.md`](docs/DEPLOY_API.md)。打包：[`docs/PACKAGING.md`](docs/PACKAGING.md)。审查落实：[`docs/CODE_REVIEW.md`](docs/CODE_REVIEW.md)。

环境变量：`GETRICH_CONFIG`、`GETRICH_DB`、`GETRICH_DATA_ROOT`、`GETRICH_API_BIND`、`GETRICH_URL`、`GETRICH_TUSHARE_TOKEN`、`GETRICH_TUSHARE_USER`。盘中刷新：`config.json` 里 `data.tushare.enabled=true`（默认关闭）。

### Windows exe

在 Windows 上：

```powershell
powershell -ExecutionPolicy Bypass -File packaging\pack-windows.ps1
# dist\GetRich-Windows-0.4.0-YYYYMMDD.zip
```

图标：将官方 `D:\windlink\windlink\GetRich\icon\GetRich.ico` 覆盖仓库 `icon/GetRich.ico` 后重新打包。

---

## English

Foundation only: typed SQLite store, migrations, file import, quote board UI. You import data later. Empty DB is a first-class state. Home chrome is Apple-minimal; quote/detail/K-line columns follow Tonghuashun.

```bash
cargo run -p getrich-cli -- serve --bind 127.0.0.1:7878
cargo run -p getrich-gui
cargo run -p getrich-cli -- import bundle --file docs/examples/demo.board.json
cargo run -p getrich-cli -- stock list --pretty
```

Import contract: [`docs/IMPORT.md`](docs/IMPORT.md). Packaging: [`docs/PACKAGING.md`](docs/PACKAGING.md). Review handoff: [`docs/CODE_REVIEW.md`](docs/CODE_REVIEW.md).
