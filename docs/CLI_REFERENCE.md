# GetRich CLI 参考

JSON CLI（`getrich`）。默认本进程打开 SQLite。`--attach` 转发到已启动的 `getrich-api`（`GETRICH_URL` / `--url` / `http://127.0.0.1:7878`）。

```bash
getrich --help
getrich stock --help
```

## 全局选项

| 选项 | 说明 |
|------|------|
| `--pretty` | 美化 JSON |
| `--quiet` / `-q` | 只输出 `data` 或 `error` |
| `--config <path>` | 配置文件（`GETRICH_CONFIG`） |
| `--attach` | 转发 API |
| `--url <url>` | API 根地址 |

成功 stdout / 失败 stderr exit 1：

```json
{ "ok": true, "cmd": "stock.list", "ts": "...", "data": { } }
```

## 命令树

```
getrich
├── version
├── serve [--bind]
├── api (health | capabilities | invoke)
├── db (migrate | status)
├── import (bundle | listings | quotes | bars) --file
└── stock (list | show | bars)
```

## 查看（空库也可用）

```bash
getrich db migrate
getrich db status
getrich stock list
getrich stock list --board 沪市 --query 茅台 --sort change_pct
getrich stock show --symbol 600519 --market CN.SH
getrich stock bars --symbol AAPL --market US --timeframe 1d --limit 200
```

`stock list` 在空库返回 `empty: true` 与中文说明，不会报错。

## 导入（你后续灌库）

见 [`IMPORT.md`](IMPORT.md)。

```bash
getrich import bundle --file docs/examples/import.bundle.json
```

## 工作台

```bash
getrich serve --bind 127.0.0.1:7878
# 浏览器打开 http://127.0.0.1:7878/          主页
#            http://127.0.0.1:7878/#/quotes  行情
```

或 `cargo run -p getrich-api` / `cargo run -p getrich-gui`。Windows 打包见 [`PACKAGING.md`](PACKAGING.md)。

环境变量：`GETRICH_CONFIG`、`GETRICH_DB`、`GETRICH_DATA_ROOT`、`GETRICH_API_BIND`、`GETRICH_URL`。
