# GetRich C+E 部署（工作台内嵌 API）

架构对齐 WiParse：**长驻进程提供 localhost API + 行情页**；CLI 默认同进程读库，`--attach` 才连 API。

- 默认 `http://127.0.0.1:7878`
- 浏览器：`GET /` 主页（苹果极简）；`#/quotes` 同花顺行情列 / 明细 / K 线
- `GET /v1/health` `GET /v1/capabilities` `POST /v1/invoke` `GET /v1/events`
- REST：`GET /v1/quotes` `GET /v1/stocks/{market}/{symbol}` `GET /v1/stocks/{market}/{symbol}/bars` `GET /v1/stats`

```bash
cargo build --release -p getrich-api -p getrich-cli
./target/release/getrich-api
./target/release/getrich --attach stock list --pretty
```

绑定：`GETRICH_API_BIND=127.0.0.1:7879`

失败 invoke 为 HTTP 400/404 + `{ ok:false, error:{ code, message } }`。
