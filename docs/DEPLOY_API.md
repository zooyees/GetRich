# GetRich C+E 部署（工作台内嵌 API）

架构对齐 WiParse：**长驻进程提供 localhost API + 行情页**；CLI 默认同进程读库，`--attach` 才连 API。

- 默认 `http://127.0.0.1:7878`
- 浏览器：`GET /` 主页（苹果极简）；`#/quotes` 同花顺行情列 / 明细 / K 线
- `GET /v1/events` NDJSON：`quote.updated` / `ingest.done`，空闲时 ping
- `GET /v1/tushare` 状态（不含 token）；`POST /v1/tushare/sync` 冷路径 listings+daily
- 盘中工人由 `data.tushare.enabled=true` 启动，交易时段拉 `rt_k`

```bash
cargo build --release -p getrich-api -p getrich-cli
./target/release/getrich-api
./target/release/getrich --attach stock list --pretty
```

绑定：`GETRICH_API_BIND=127.0.0.1:7879`。仅允许回环地址（`127.0.0.1` / `localhost` / `[::1]`）；无鉴权时禁止 `0.0.0.0`。写接口在 Origin 非本机时返回 403。

失败 invoke 为 HTTP 400/404/403 + `{ ok:false, error:{ code, message } }`。
