# GetRich 代码审查落实单

**交给：** 软件工程工位

**来源：** 2026-09-12 全仓静态审查（`getrich-core` / `api` / `gui` / `cli` / web / 打包 / 文档）

**版本：** 0.1.0

**方法：** 对照源码确认根因；未做运行时 exploit。条目 ID 稳定，提交信息请引用（例：`fix(B1): quote upsert ts guard`）。

本文件是工单，不是设计讨论。按批次改、按验收关。交互筛选版见对话旁 Canvas「GetRich 代码审查」。

---

## 怎么用

1. **只按批次开工。** 不要从 P3 优化开始。同文件尽量并入同一 PR（尤其 `quotes.rs` 的 B1+R7）。
2. **每条必须有回归。** P0/P1 没有单测或手工验收步骤，不算完成。
3. **不要拆已经做对的部分：** WAL、空库空状态、token 日志 `mask_token`、上海固定 UTC+8、日 K 用日历日作 key、Pulse 禁止「实时北向」文案、SQL 绑定参数、ingest `compare_exchange` 幂等。
4. **密钥：** 不要把真实 `data.tushare.token` 写进仓库或示例。若 `dist/config.json` 已经带过 token，先轮换再发版。

**级别约定**

| 级 | 含义 | 默认本机工作台 |
|----|------|----------------|
| P0 | 正确性/数据会被静默写坏 | 本迭代必须合入 |
| P1 | 高优先缺陷或可利用面 | 本迭代必须合入 |
| P2 | 契约/体验/并发，默认场景不一定立刻炸 | 下一迭代 |
| P3 | 卫生、文档、小优化 | 顺手或 backlog |

绑定 `0.0.0.0` 时，B11 / R2 / R3 按 **P0** 处理。

---

## 批次（落实顺序）

### 批次 A — 行情正确性（阻塞发版）

| ID | 级 | 标题 | 主文件 |
|----|----|------|--------|
| B1 | P0 | 报价 upsert 无 ts 保护，日线覆盖盘中价 | `crates/getrich-core/src/db/quotes.rs` |
| R7 | P2 | COALESCE 造成新 last + 旧 OHLC | 同上，与 B1 一起改 |

**建议 PR：** `fix(B1,R7): do not let daily quotes clobber live rt_k`

### 批次 B — 密钥与发布面

| ID | 级 | 标题 | 主文件 |
|----|----|------|--------|
| B4 | P1 | env token 改自选时落盘 | `crates/getrich-core/src/service.rs` |
| R1 | P1 | pack 复制含 token 的 `config.json` | `packaging/pack-windows.ps1` |
| B11 | P1 | 无鉴权 + CORS `*` | `crates/getrich-api/src/http.rs` |
| R2 | P1 | `import.file` 任意路径 | `crates/getrich-core/src/invoke.rs` |
| R3 | P1 | 非回环绑定无警告/无拒绝 | CLI `--bind`、`docs/DEPLOY_API.md` |
| B10 | P1 | 资料区 HTML 注入 | `crates/getrich-api/web/app.js` |

**建议 PR：** `fix(B4,R1,B10,B11): secrets, XSS, localhost API surface`

### 批次 C — 配置 / 代码 / 水位 / 自选

| ID | 级 | 标题 | 主文件 |
|----|----|------|--------|
| B2 | P1 | `920xxx` 无后缀当成深市 | `crates/getrich-core/src/symbol.rs` |
| B3 | P1 | `settings.board` 内存不更新 | `crates/getrich-core/src/service.rs` |
| B5 | P1 | 空结果仍 `mark_success` | `crates/getrich-core/src/service.rs` |
| B9 | P1 | WATCH 分页被 merge 打乱 | `crates/getrich-core/src/service.rs` |
| O6 | P3 | 为 B1/B2/B4/B9 补回归（跟本批一起做） | `crates/getrich-core/src/**/tests` |

### 批次 D — 日历与 Pulse

| ID | 级 | 标题 |
|----|----|------|
| B6 | P1 | `trade_cal` 缺失时节假日仍轮询 |
| B7 | P1 | 非交易日 `trade_date` 用上一工作日 |
| B8 | P1 | Open 锚点首次写入永久冻结 |
| B15 | P2 | `last_open_day` 不取 max、空则假装今天 |
| B21 | P2 | 默认 `rt_k_codes` 漏 `8*.BJ,4*.BJ` |
| B24 | P2 | 指数未入库，Pulse 指数位空 |

### 批次 E — P2 契约 / 体验（下一迭代）

B12–B14、B16–B20、B22–B23、R4–R6、R8–R11、O1–O5。

### 批次 F — P3 backlog

R12、R13、O7。死配置（O4）要么接线要么从 `config.default.json` 删掉，不要留「假旋钮」。

---

## 工单明细

每条格式：根因 → 改法 → 验收。文件路径相对仓库根。

### B1 · P0 · 报价 upsert 无时间戳保护

**根因：** `crates/getrich-core/src/db/quotes.rs` `upsert_quote` 的 `ON CONFLICT(instrument_id) DO UPDATE` 无条件覆盖 `ts`/`last`。盘中路径 `ingest.rs` `rt_k` 与日线路径 `service.rs` `ingest_tushare` → `board_bundle`（`daily`）以及 `web/app.js` `loadDetail` 自动 `tushare.quote`（失败回落 `daily`）写同一张表。

**改法：**

1. SQL 增加 `WHERE excluded.ts >= quotes.ts`（字符串为 RFC3339，现有写入已是该格式；比较前确认同一时钟）。更稳：应用层比较 `DateTime<Utc>` 再决定是否 execute。
2. 日线/EOD 写入不得覆盖「同一交易日且已有带时刻的报价」。识别：`quotes.ts` 含空格或 `T` 且日历日等于今日上海日。
3. `tushare.quote` 在竞价/连续竞价窗口优先 `rt_k`；`rt_k` 失败时不要用 `daily` 盖盘中价（可只补 bars）。
4. 与 R7 一起：仅同一日历日才 `COALESCE` OHLC；跨日清空 `open/high/low/volume/turnover`。

**验收：**

- 单测：先写 `ts=当日 10:00 last=10`，再 upsert `ts=当日 date-only last=9`，库中仍为 10。
- 单测：先写昨日 15:00，再写今日 date-only，应更新为今日。
- 手工：交易时段打开详情，列表最新价不被日线收盘替换。
- `cargo test -p getrich-core` 全绿。

### R7 · P2 · 与 B1 同 PR

跨日半更新会导致「新 last + 旧 high」。跨日写入时 OHLC 以新行为准；同日 NULL 才 COALESCE。

### B4 · P1 · env token 落盘

**根因：** `service.rs` `write_config` 在磁盘/内存 token 为空时从 `tushare_cfg()`（含 `GETRICH_TUSHARE_TOKEN`）整包拷入再 `fs::write`。

**改法：** 落盘 `AppConfig` 时：若该 token 来自环境变量，写空字符串或保留文件原值，禁止回填。`watch.toggle` / `settings.board` 只改 `universe.*`。

**验收：** 仅设 env token、磁盘无 token → `watch.toggle` 后 `config.json` 仍无明文 token。单测可用临时目录 + env。

### R1 · P1 · 打包复制密钥

**改法：** `packaging/pack-windows.ps1` **禁止** `Copy-Item config.json`。只复制 `config.default.json`。`docs/PACKAGING.md` 写明：token 用环境变量或用户自己在 `dist` 建配置，发布物不含密钥。

**验收：** 仓库根存在含 token 的 `config.json` 时跑 pack，`dist/` 无该文件或 token 字段为空。若曾经发出过带 token 的 zip，轮换 Tushare token。

### B11 · P1 · CORS `*` + 无鉴权写接口

**改法（本机产品最小集）：**

1. 删除 `Access-Control-Allow-Origin: *`，或仅回显本页 Origin（工作台同源则浏览器不需要 CORS）。
2. 写接口（`POST /v1/invoke`、`tushare.sync`、`import.*`、`watch.toggle`）校验 `Origin`/`Host` 为 loopback。
3. 非 `127.0.0.1`/`::1`/`localhost` 绑定：拒绝启动，或要求 `GETRICH_API_TOKEN`（见 R3）。

**验收：** 从 `file://` 或其它 Origin 对 `http://127.0.0.1:7878/v1/invoke` 做 POST，应失败。本机工作台读写仍可用。

### R2 · P1 · `import.file` 路径沙箱

**改法：** 解析后的路径必须落在 `paths::app_root()`（或 `GETRICH_DATA_ROOT`）之下；拒绝 `..` 与绝对路径跳出根。CLI 若需导入任意路径，仅允许非 HTTP 的 CLI 入口，或显式 `--allow-any-path`。

**验收：** HTTP `import.file` `path=C:\Windows\win.ini`（或 `../../secret`）返回 `INVALID_ARG`。`docs/examples/import.bundle.json` 仍可导入。

### R3 · P1 · 非回环绑定

**改法：** 解析 bind，非 loopback 则 error（推荐）或强制 token。`docs/DEPLOY_API.md` 写一句：默认仅本机；禁止无鉴权 `0.0.0.0`。

### B10 · P1 · 资料区 XSS

**改法：** `gridItem` 对 `label`/`value` 走 `escapeHtml`。检查所有 `innerHTML`：`r.status`（runs 约 912 行）一并转义。

**验收：** listing.industry / metadata.value 为 `<img src=x onerror=alert(1)>` 时页面显示字面量，不执行脚本。列表行情名回归不回退（已 escape）。

### B2 · P1 · 北证号段

**改法：** `symbol.rs` `infer_market`：6 位且 `92`/`83`/`87`/`43` 前缀 → `CN.BJ`。无把握不要默认 SZ。

**验收：** `parse_instrument("920002", None)` → `CN.BJ`；`830001` → `CN.BJ`；`000001` 仍 `CN.SZ`；`600519` 仍 `CN.SH`。带 `.BJ` 后缀回归保持。

### B3 · P1 · 默认板块内存陈旧

**改法：** `App.cfg` 改为 `Mutex<AppConfig>`（或至少 `default_board` 可写）。`set_default_board` 写盘成功后更新内存。`watch.list` 读新值。可与 R8/R9 一并收口配置源。

**验收：** 调 `settings.board` 后再 `watch.list`，`default_board` 为新值，无需重启。

### B5 · P1 · 假成功水位

**改法：** `ingest_tushare` 仅当 `r.status == "ok"` 且 `quotes_upserted > 0`（listings 同理）时 `mark_success`；否则 `mark_error` 或跳过。不要在 import 失败后仍标 quotes/bars.1d 成功。

**验收：** 模拟 `board_bundle` 空 quotes 后，`freshness` 该行不是 ok。

### B9 · P1 · 自选分页

**改法：** `BoardScope::Watch` 先取全量（或 SQL IN 全部自选）、`merge_watch_rows`，再按 `limit`/`offset` 切片。`matched` = merge 后总数；`truncated` = matched > 返回行数。

**验收：** 自选 5 只、`limit=2, offset=2` 返回第 3–4 只；缺 listing 的占位行计入 matched。扩展现有 `watch_board_lists_every_starred_symbol`。

### B6 · P1 · 节假日轮询

**改法：** `ingest.rs`：`last_open` 未知时不要当作开市（跳过或 ≥30min 才打一次 `trade_cal`）。`last_open != today` 必须跳过 `rt_k`。

**验收：** 代码路径：`last_open=None` 不进入 `poll_once`。有日历且为假日则不请求 `rt_k`。

### B7 · P1 · Pulse trade_date

**改法：** `trading=false` 时 `session.trade_date` = vendor `trade_cal` 的 last open（已有 `yyyymmdd`/`iso`），不要 `previous_weekday`。

**验收：** 周三且日历 last open 为上周五，Pulse `trade_date` 为上周五。

### B8 · P1 · Open 锚点冻结

**改法：** `persist_anchor` 对 `Open` 不要 `INSERT OR IGNORE` 后永不更新。可选：Open 阶段仅当 `tape.n_quoted >= min_quoted` 才写入且允许覆盖至 10:00；Lunch/Post 才固化。或 Open 不写 anchor。

**验收：** 9:31 残缺样本后，9:50 全市场样本能更新 open 锚点（若产品选择允许覆盖）。Lunch 读取的是较完整截面。

### B15 · P2 · last_open_day

对 `cal_date` 取字典序最大；`items` 空则 `Err`，禁止返回 `end`（今天）。

### B21 · P2 · rt_k 号段

`default_rt_k_codes` 改为含 `8*.BJ,4*.BJ`。已有用户 `config.json` 不会自动升级，在 `config.default.json` 改默认并在文档说明。

### B24 · P2 · 指数

`listings_bundle` / 热写补 `INDEX_KEYS` 对应指数（上证等），或 Pulse 文案明确「指数未接入」、不要假装有指数涨跌。

---

## 批次 E 摘要（下一迭代，仍须登记）

| ID | 级 | 做什么 | 验收要点 |
|----|----|--------|----------|
| B12 | P2 | `list_listings` 排除 `internal` | 水位行不出现在上市列表 |
| B13 | P2 | OHLC 收紧；无效 bar 计入 skipped | `high < close` 不入库且报告可见 |
| B14 | P2 | 热写区分 parse_error / skipped_missing | 坏时间戳不是 missing listing |
| B16 | P2 | GUI `open_stock` 设 `Screen::Detail` 或 Escape 看 `detail.is_some()` | 空搜索 Escape 能关详情 |
| B17 | P2 | `loadList`/`loadDetail`/`loadPulse` 加 `reqId` + AbortController | 快速切股不显示错股 K 线 |
| B18 | P2 | CLI attach 超时 ≥ Web sync（180s）或可配 | 慢 sync 不误报 |
| B19 | P2 | 实现 `since_seq` **或** 从 capabilities 删掉该 query | 契约不再撒谎 |
| B20 | P2 | capabilities 补 `/v1/watch` `/v1/runs` `POST /v1/tushare/bars`，bind 用真实值 | 与 `http.rs` 一致 |
| B22 | P2 | HTTP：NotFound 404，InvalidArg 400，其余 500 | DB 故障不再 400 |
| B23 | P2 | 改 `IMPORT.md`（已抓取）、`CLI_REFERENCE.md`（pulse / import tushare）、`DEPLOY_API.md`（GUI 内嵌 API） | 文档可照做 |
| R4 | P2 | EventHub 有界；限制 `/v1/events` 并发 | 慢订阅不把内存打满 |
| R5 | P2 | 尊重 `read_timeout_ms`；EOD 单独长超时 | 默认 15s 对非 sync 生效 |
| R6 | P2 | 未知 board → 400 而非 ALL | `board=FOO` 报错 |
| R8 | P2 | 自选写盘与 Mutex 同一临界区 | 并发 toggle 不丢 |
| R9 | P2 | 配置单源：热加载或写明仅启动读一次 | `tushare_cfg` 与 `self.cfg` 不再分叉 |
| R10 | P2 | 连接复用；接线或删除 `db_commit_interval_sec` | 配置无死字段 |
| R11 | P2 | 迁移以 `schema_migrations` 为准 | 抬高 `user_version` 不能跳过 |
| O1 | P2 | Pulse 聚合下推 SQL | `/v1/pulse` 全市场可接受延迟 |
| O2 | P2 | 缓存 listing id | 每轮 rt_k 不再全表 HashMap |
| O3 | P2 | 周月线单一来源 | 库存 W1 与 resample 不并存分叉 |
| O4 | P2 | `theme_height` / `max_limit_times` 实现或删除 | 配置即行为 |
| O5 | P2 | HTTP 线程池；sync 改后台 job | 一次 sync 不堵死其它请求 |

## 批次 F · P3

| ID | 做什么 |
|----|--------|
| R12 | 非 loopback 时 health 不返回 DB 绝对路径；`user_id` 脱敏 |
| R13 | LIKE 查询 ESCAPE `%` `_` |
| O7 | ST 用前缀/`*ST`，不用 `contains("ST")` |

---

## 不要做的事

- 不要为了「修 CORS」把工作台改成必须手动带复杂 header 才能用（本机同源应零摩擦）。
- 不要把 Tushare token 写进测试夹具或 `docs/examples`。
- 不要在 B1 里改用「永远禁止 daily 写 quotes」却不给收盘后更新路径——收盘后 daily 仍应能更新。
- 不要引入新的行情源或策略模块；本阶段仍是基座。

---

## 关单检查

批次 A+B+C+D 全部勾选后才算本审查闭环：

- [ ] `cargo test --workspace`
- [ ] `cargo clippy -p getrich-core -- -D warnings`（至少 core）
- [ ] 交易时段：实时价不被「同步」或打开详情打回昨日/今日收盘
- [ ] 无后缀 `920002` 进北证；自选分页正确
- [ ] 仅 env token 时 toggle 自选不写密钥；pack 产物无 token
- [ ] 资料区恶意字符串不执行；本机 UI 仍可用
- [ ] 文档与 `capabilities`、真实路由一致

关单时在 `docs/CHANGELOG.md` 记：审查落实批次与 ID 列表，不要只写「bugfix」。
