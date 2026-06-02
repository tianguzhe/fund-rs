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
| `fund portfolio` | 显示持仓收益（市值/现金/持有期收益 + 资产配置） |
| `fund portfolio --save` | 同上，并保存每日快照到 SQLite |
| `fund backfill --from <date> --to <date>` | 仅补录历史净值（nav_daily） |
| `fund export [-o <file>]` | 导出 portfolio JSON（连表：组合时间线 + 净值序列 + 现金流水） |
| `fund holdings --init` | 生成持仓配置模板 holdings.json |
| `fund search -k <关键词>` | 搜索基金 |
| `fund info -c <代码>` | 基金详情 + 阶段收益 |
| `fund trend -c <代码>` | 基金详情 |
| `fund history -c <代码>` | 历史净值列表 |
| `fund estimate -c <代码>` | 盘中实时估值（估算净值/涨跌幅）；省略 -c 读 holdings 估算今日盈亏 |
| `fund rank` | 基金排行榜 |
| `fund rank-history -c <代码>` | 排名历史走势 |
| `fund theme` | 主题基金列表 |
| `fund bigdata` | 大数据选基 |

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

- SQLite: `~/.fund-rs/portfolio.db`，真实账本 5 表：
  - `funds` 基金元数据 · `nav_daily` 每日净值 · `position_daily` 持仓明细（按渠道 + `buy_date` 批次分笔）
  - `portfolio_daily` 每日总览（总市值 + 现金 + 总资产 + 盈亏） · `cash_flows` 现金流水
- 持仓配置: JSON 文件 `~/.fund-rs/holdings.json`，`holdings` 为 `{渠道 -> 持仓数组}` map，每笔填 `shares` + `cost_nav`，市值由 `shares × nav` 推导
  - 生成模板: `fund holdings --init`；优先级 `$FUND_HOLDINGS` > `./holdings.json` > `~/.fund-rs/holdings.json`
  - 首次运行检测到旧库会自动备份为 `portfolio.db.legacy-<date>` 再重建

## 技术栈

- `minreq` + `native-tls` — HTTP（macOS ARM 兼容，避免 `ring`/`rustls`）
- `rusqlite` — SQLite
- `clap` — CLI 框架
- `serde` / `serde_json` — JSON
- `comfy-table` / `owo-colors` — 终端渲染

## 注意事项

- 持仓变动: 编辑 `~/.fund-rs/holdings.json`（不再硬编码），`fund holdings --init` 可生成模板
- 渠道是 `holdings` map 的 key，不写在每笔 entry 内（加载时注入）
- 同基金同渠道分批买入: 同一渠道数组内用不同 `buy_date` 区分，各批独立保留份额/成本
- API 基础 URL: `https://tiantian-fund-api.vercel.app/api/action`
