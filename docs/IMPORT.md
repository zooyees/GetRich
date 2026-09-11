# GetRich 导入约定

后续由你导入股票库。本仓库**不抓取**行情。展示系统读取 SQLite 中已落地的 `listings` / `quotes` / `bars`；空库会显示明确空状态。

## 存储

- 文件：`getrich.db`（可用 `GETRICH_DB` 或 `config.system.db_name` 覆盖）
- 引擎：SQLite WAL + `schema_migrations`
- 幂等：listings `(symbol, market)`；quotes `instrument_id`；bars `(instrument_id, timeframe, ts)`

启动时自动 migrate。也可：

```bash
cargo run -p getrich-cli -- db migrate
cargo run -p getrich-cli -- db status
```

## 命令

```bash
getrich import bundle   --file path/to/data.json
getrich import listings --file listings.json   # 或 .csv
getrich import quotes   --file quotes.json
getrich import bars     --file bars.json
```

JSON 也可用 `POST /v1/invoke`：

```json
{ "method": "import.file", "params": { "path": "docs/examples/import.bundle.json", "kind": "bundle" } }
```

或 `import.json` + `params.data` 内嵌对象。

示例文件：[`docs/examples/import.bundle.json`](examples/import.bundle.json)（格式样本，不是内置行情）。

## Bundle JSON

```json
{
  "listings": [ { "symbol": "600519", "market": "CN.SH", "name": "贵州茅台", "currency": "CNY", "status": "active" } ],
  "quotes": [ { "symbol": "600519", "market": "CN.SH", "ts": "2024-01-03T07:00:00Z", "last": 1490, "open": 1488, "high": 1500, "low": 1480, "prev_close": 1488, "volume": 1100, "turnover": 1639000 } ],
  "bars": [ { "symbol": "600519", "market": "CN.SH", "timeframe": "1d", "ts": "2024-01-03", "open": 1488, "high": 1500, "low": 1480, "close": 1490, "volume": 1100 } ]
}
```

`listings` / `quotes` / `bars` 也可各自做成 JSON 数组文件。

### listings 字段

| 字段 | 必填 | 说明 |
|------|------|------|
| symbol | 是 | `AAPL` / `600519` / `00700` |
| market | 建议 | `US` `HK` `CN.SH` `CN.SZ` `CN.BJ`；可从代码推断 |
| name, name_en, exchange, currency | 否 | 展示用 |
| instrument_type | 否 | 默认 `equity` |
| status | 否 | 默认 `active` |
| isin, listing_date, sector, industry, lot_size | 否 | 元数据 |
| source, source_symbol | 否 | 来源标记 |
| metadata | 否 | `[{ "key", "value", "source", "as_of" }]` |

### quotes 字段（行情列表）

| 字段 | 必填 | 说明 |
|------|------|------|
| symbol, ts, last | 是 | `ts` 为 RFC3339 或 `YYYY-MM-DD` |
| market | 建议 | |
| open, high, low, prev_close | 否 | 今开/最高/最低/昨收 |
| volume, turnover | 否 | 成交量 / 成交额 |
| change_pct, currency, source | 否 | 涨跌幅可省略，由 last−昨收计算 |

没有 quotes 的股票仍出现在列表中，最新价显示为 `—`。

### bars 字段（K 线）

| 字段 | 必填 | 说明 |
|------|------|------|
| symbol, ts, open, high, low, close | 是 | `high >= low` |
| timeframe | 否 | `1d` `1w` `1mo` `1h` `5m`… 默认 `1d` |
| adj_close, volume, turnover, source | 否 | |

quotes/bars 若股票尚未在 listings 中，会自动插入一条最小 listings 行。

## CSV

首行表头，UTF-8。简单逗号分隔（字段内逗号请用双引号）。

listings：`symbol,market,name,exchange,currency,status,...`

quotes：`symbol,market,ts,last,open,high,low,prev_close,volume,turnover`

bars：`symbol,market,timeframe,ts,open,high,low,close,volume,turnover`

`ts` 也可用列名 `date`。

## 市场代码

| market | 含义 | 代码习惯 |
|--------|------|----------|
| US | 美股 | `AAPL` |
| HK | 港股 | `00700`（五位） |
| CN.SH | 上交所 | `600519`，科创板 `688xxx` |
| CN.SZ | 深交所 | `000001`，创业板 `300xxx` |
| CN.BJ | 北交所 | `8xxxxx` / `4xxxxx` |

## 展示派生列（不入库）

- 涨跌额 = last − prev_close
- 涨跌幅 = change_pct 或 (last−prev_close)/prev_close
- 振幅 = (high−low)/prev_close
- 方向：红涨绿跌（空为灰）
- 板块：科创板 / 创业板由代码推断，供行情页过滤

不要写入分析、策略或信号字段；那些不在本阶段范围。
