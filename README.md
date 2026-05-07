# fund-rs

基金查询 & 持仓追踪工具集，Rust 编写，调用天天基金 API。

## 项目结构

```
fund-rs/
├── Cargo.toml              # Workspace root
├── crates/
│   ├── fund-core/          # 共享库：API 客户端 + 模型 + DB + 持仓配置
│   └── fund-cli/           # CLI 工具（bin: fund）
└── CLAUDE.md               # 开发指南
```

## CLI 命令

| 命令 | 说明 |
|------|------|
| `fund portfolio` | 显示持仓当日/当周/当月收益 |
| `fund portfolio --save` | 同上，并将数据保存到 SQLite |
| `fund backfill --from <date> --to <date>` | 补录历史日期范围数据 |
| `fund export [-o <file>]` | 导出历史数据为 JSON |
| `fund search -k <关键词>` | 搜索基金 |
| `fund info -c <代码>` | 基金详情 + 阶段收益 |
| `fund trend -c <代码>` | 基金详情 |
| `fund history -c <代码>` | 历史净值列表 |
| `fund rank` | 基金排行榜 |
| `fund rank-history -c <代码>` | 排名历史走势 |
| `fund theme` | 主题基金列表 |
| `fund bigdata` | 大数据选基 |
| `fund compare --a <代码> --b <代码>` | 对比两只基金，输出 JSON 供网页展示 |

## 安装

```bash
cargo build --release -p fund-cli
# 可执行文件: ./target/release/fund
```

## 每日工作流

```bash
fund portfolio --save        # 拉取当日数据并写入 SQLite
fund export                  # 导出 JSON（可选）
fund backfill --from 2026-03-01 --to 2026-03-31  # 补录历史
```

## 调试模式

任何命令加 `--debug` 或 `-d` 可打印 HTTP 请求和响应：

```bash
fund --debug info -c 420002
fund -d search -k 天弘
```

Debug 信息输出到 stderr，不影响 stdout 重定向。

## 数据存储

- SQLite: `~/.fund-rs/portfolio.db`
- 表: `daily_returns(date, fund_code, fund_name, holding, day_pct, day_amount, week_pct, week_amount, month_pct, month_amount)`
- 持仓硬编码在 `crates/fund-core/src/holdings.rs`，更新持仓需修改此处

## 技术栈

- `minreq` + `native-tls` — HTTP（macOS ARM 兼容，避免 `ring`/`rustls`）
- `rusqlite` — SQLite
- `clap` — CLI 框架
- `serde` / `serde_json` — JSON
- `comfy-table` / `owo-colors` — 终端渲染

## 注意事项

- 持仓变动需修改 `crates/fund-core/src/holdings.rs` 的 `holdings()` 函数
- API 基础 URL: `https://tiantian-fund-api.vercel.app/api/action`
