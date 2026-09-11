# GetRich

**[中文](#中文)** · **[English](#english)**

股票分析系统基座（Rust）：配置 / SQLite / 导入约定 / 同花顺风格行情展示。当前版本 **0.1.0**。架构对齐 [WiParse](https://github.com/zooyees/WiParse)（`*-core` + JSON CLI + localhost API + GUI 图标接线）。本阶段不含策略、信号或在线拉数。

Stock analysis foundation (Rust): config / SQLite / import contract / Tonghuashun-style quote board. **0.1.0**. No strategies, signals, or live scraping in this release.

---

## 中文

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
cargo run -p getrich-cli -- import bundle --file docs/examples/import.bundle.json
cargo run -p getrich-cli -- stock show --symbol 600519 --market CN.SH --pretty
```

字段约定：[`docs/IMPORT.md`](docs/IMPORT.md)。CLI：[`docs/CLI_REFERENCE.md`](docs/CLI_REFERENCE.md)。API：[`docs/DEPLOY_API.md`](docs/DEPLOY_API.md)。打包：[`docs/PACKAGING.md`](docs/PACKAGING.md)。

环境变量：`GETRICH_CONFIG`、`GETRICH_DB`、`GETRICH_DATA_ROOT`、`GETRICH_API_BIND`、`GETRICH_URL`。

### Windows exe

在 Windows 上：

```powershell
powershell -ExecutionPolicy Bypass -File packaging\pack-windows.ps1
# dist\GetRich.exe
```

图标：将官方 `D:\windlink\windlink\GetRich\icon\GetRich.ico` 覆盖仓库 `icon/GetRich.ico` 后重新打包。

---

## English

Foundation only: typed SQLite store, migrations, file import, quote board UI. You import data later. Empty DB is a first-class state. Home chrome is Apple-minimal; quote/detail/K-line columns follow Tonghuashun.

```bash
cargo run -p getrich-cli -- serve --bind 127.0.0.1:7878
cargo run -p getrich-gui
cargo run -p getrich-cli -- import bundle --file docs/examples/import.bundle.json
cargo run -p getrich-cli -- stock list --pretty
```

Import contract: [`docs/IMPORT.md`](docs/IMPORT.md). Packaging: [`docs/PACKAGING.md`](docs/PACKAGING.md).
