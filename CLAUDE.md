# Fund-rs 项目指南

## 项目概述
基金查询 CLI 工具，Rust 编写，调用天天基金 API。

## 项目结构（Cargo Workspace）
- `Cargo.toml` - workspace root
- `crates/fund-core/` - 共享库：API 客户端、模型、DB、持仓配置
- `crates/fund-cli/` - CLI 二进制（bin name: `fund`）

## 技术栈
- HTTP 客户端: `minreq` (使用 `native-tls` 特性)
- JSON 解析: `serde` + `serde_json`
- CLI 框架: `clap` (derive 模式)
- 表格渲染: `comfy-table`
- 错误处理: `anyhow`
- 本地存储: `rusqlite` (链接系统 SQLite，无需 bundled 特性)
- 颜色输出: `owo-colors`
- CJK 宽度: `unicode-width`

## 开发指南

### 构建和测试
- `cargo build --release -p fund-cli` - 编译 CLI release 版本
- `cargo check --workspace` - 检查所有 crate
- `./target/release/fund <command>` - 测试 CLI 命令
- `cargo test --all-features` - 运行测试
- `cargo fmt --all -- --check` - 检查代码格式（CI 要求）
- `cargo clippy --all-targets --all-features -- -D warnings` - Lint 检查（CI 要求）

**Rust 最低版本：1.75**（workspace Cargo.toml 中声明）

### 依赖注意事项
- ⚠️ 避免使用 `ureq` + `rustls` - 在 macOS ARM 上与 `ring` 库有兼容问题
- ✅ 使用 `minreq` + `native-tls` 特性作为 HTTP 客户端

### API 设计模式
- 使用参数结构体（如 `FundRankParams`）而非多个独立参数
- HTTP 请求封装为泛型方法 `request<T: DeserializeOwned>`
- 错误处理使用 `anyhow::Context` 提供上下文信息

### 代码风格
- 行宽上限：100 字符（rustfmt.toml 配置）
- 数据模型使用 `serde` 的 `rename` 属性映射 API 字段
- 命令实现在 `crates/fund-cli/src/commands/` 目录，每个命令一个文件
- UI 显示逻辑在 `crates/fund-cli/src/ui/display.rs`，使用 `comfy-table` 渲染
- 共享逻辑在 `crates/fund-core/src/`，包含 API 客户端、模型、DB、持仓配置

### API 端点
- 基础 URL: `https://tiantian-fund-api.vercel.app/api/action`
- 参数格式: `?action_name=<action>&<params>`
- 主要接口: `fundMNRank`, `bigDataList`, `bigDataDetail`, `fundSearch`, `fundMNHisNetList`

### 持仓配置（JSON 文件）
- 配置路径（按优先级）：
  1. `$FUND_HOLDINGS` 环境变量指向的文件
  2. 当前目录 `./holdings.json`
  3. `~/.fund-rs/holdings.json`（默认）
- 模块：`crates/fund-core/src/holdings_config.rs`
- 格式：
  ```json
  {
    "holdings": [
      {
        "code": "420002",
        "name": "天弘永利债A",
        "amount": 270000.0,
        "channel": "招商",
        "redeemable_date": "2026-02-11",
        "redeem_status": "redeemable"
      },
      {
        "code": "420002",
        "name": "天弘永利债A",
        "amount": 92119.0,
        "channel": "支付宝",
        "redeemable_date": "2026-05-15",
        "redeem_status": "redeemable"
      }
    ]
  }
  ```
- 支持同基金多条记录表达分笔持仓；当前 CLI 仍按每条记录的 `code` / `name` / `amount` 参与计算
- `channel`、`redeemable_date`、`redeem_status` 为可选字段，未提供时保持旧格式兼容
- 生成模板：`fund holdings --init`
- 加载入口：`fund_core::holdings::holdings() -> Result<Vec<Holding>>`

### 持仓数据存储（SQLite）
- DB 路径: `~/.fund-rs/portfolio.db`
- 表名: `daily_returns`，主键 `(date, fund_code)`
- 模块: `crates/fund-core/src/db.rs`，提供 `save_records()` / `export_json()`

### F10 底层接口（基金本身持仓与行业配置）
- 模块: `crates/fund-core/src/f10.rs`
- 直连 `https://fundf10.eastmoney.com`，与统一 action_name 入口不同
- 返回为 `var apidata={ ... }` JS 赋值 + 嵌入 HTML 表格，已用纯 std 解析
- `get_top_stocks(code, year, month)` — 前十大股票，必须显式传季度末 `year/month`
- `get_active_industries(code)` — 行业配置（已过滤中证 GICS 双套分类）
- `latest_quarter_end(year, month)` — 推算最近已披露季度

### 命令列表

```bash
# 持仓
fund portfolio              # 查看持仓收益（含类型列 + 资产配置摘要）
fund portfolio --save       # 查看并保存到 SQLite
fund backfill --from <date> --to <date>  # 补录历史

# 穿透分析
fund holdings               # 默认 TOP 15 股票
fund holdings --top 30      # 显示 TOP 30 股票
fund holdings --json        # 输出 JSON
fund holdings --init        # 生成 holdings.json 模板

# 导出 / 对比
fund export                 # 导出 portfolio JSON
fund compare --a 020602 --b 020156 [-o out.json]  # 对比两只基金并输出 JSON

# 搜索 / 详情 / 分析
fund search -k 天弘
fund info -c 420002
fund history -c 420002 -d 30 -l 10
fund analyze -c 020262 [--json] [-o dist/data/fund-020262.json]
#   --json 输出 JSON；-o 写文件而非 stdout（用于喂 dist/fund-analysis.html）

# 排行 / 主题 / 大数据
fund rank [-t hh|zq|gp|zs|qdii|hb|all] [-n 20] [--sort-column SYL_1N|SYL_3N|DWJZ]
#   -t 客户端按 BFUNDTYPE 过滤（zq=003 债券 / hh=002 混合 / gp=001 股票 /
#   zs=004 指数 / qdii=006 / hb=007 货币）；上游 cap 30 行/页，CLI 自动翻 ≤20 页
#   债基排同类前 N 推荐 --sort-column SYL_3N（按 1Y 排序股基会挤掉债基）
fund rank-history -c 420002 -r 3y
fund theme -l 20
fund bigdata [--detail 1]

# 调试
fund --debug <command>
```

### 每日工作流
```bash
fund portfolio --save        # 拉取当日数据并写入 SQLite
fund export                  # 导出 JSON（可选）
fund backfill --from <date> --to <date>  # 补录历史日期范围
```

### 深度分析网页（fund-analysis.html）
- 模板路径：`dist/fund-analysis.html`（统一模板，无硬编码基金代码）
- 数据目录：`dist/data/fund-<6位代码>.json`，由 `fund analyze -c <CODE> --json -o dist/data/fund-<CODE>.json` 生成
- 访问方式：`dist/fund-analysis.html?code=000171` → 自动加载 `./data/fund-000171.json`
- 无 `?code=` 参数时回落到 `./data/fund-analysis.json`（旧链接兼容）
- 代码白名单：仅 6 位数字才接受，防止路径穿越
- 批量更新：循环跑 `fund analyze -c <CODE> --json -o dist/data/fund-<CODE>.json` 即可给每只基金更新数据

### 持仓收益输出规范（Claude 整理格式）
当用户要求查看"今日收益"、"持仓收益"时，运行 `fund portfolio` 后按以下格式整理输出：

- **不显示渠道列**：`channel` 字段仅 JSON 配置内部使用，输出时省略
- **同代码合并**：同一基金代码的多笔持仓（如不同渠道）合并为一行，金额求和，收益率取加权平均
- **默认输出**：使用英文表格，列为 `Code | Fund | Holding | Today | Today P&L | Week | Week P&L | Month | Month P&L`
- **列名约定**：基金名称列固定写作 `Fund`，不要使用 `基金` 或其他中文列名
- **基金名约定**：英文表格中基金名称也要统一翻成英文，不保留中文基金名
- **英文输出范围**：资产类型与区块标题也统一使用英文，如 `Bond / Mixed / Equity / Asset Allocation / Total`
- **英文表格口径**：`Holding` 直接显示真实持仓金额（不缩放、不除以 10），并按持仓降序排列
- **汇总表**：英输出后追加 `Total` 区块，列为 `Total (CNY) | Today | Today P&L | Week | Week P&L | Month | Month P&L`
- **资产配置摘要**：在底部追加英文摘要，使用 `Asset Allocation` 标题，并以 `Bond / Mixed / Equity` 等英文类型名输出金额与占比，示例：
  - `Bond: 739,673 CNY, 81.76%`
  - `Mixed: 165,000 CNY, 18.24%`
- **格式示意**：主表 + `Total` 汇总表 + `Asset Allocation` 摘要三段输出

### 基金对比风险收益散点图（ASCII）
多基金对比时，在数据表之后追加 ASCII 散点图，X 轴为最大回撤%，Y 轴为年化收益%，直观展示风险收益分布：

```
年化收益%
    ↑
 18% │                                          ● fund_code (label)
     │
 12% │
     │
  8% │      ★ current_holding (label)
     │  ● candidate_a ←── annotation
  7% │  ● current_holding (label)
  6% │          ● candidate_b ←── annotation
  5% │      ● candidate_c ←── annotation
  4% │
     └──────────────────────────────────→ 最大回撤%
         2%    5%    7%    9%    11%   17%
```

- **★** 标记用户当前持仓，**●** 标记候选基金
- 每个点右侧用 `←──` 追加一句话定位（如"收益最高"、"卡玛最优"）
- X/Y 轴刻度按实际数据范围调整，保持等距
- 优先放在对比报告的"综合对比"或"结论"章节之前

## 常见问题

### 编译错误
- 如遇 `ring` 库编译失败，检查是否使用了 `rustls` 特性
- 改用 `native-tls` 或 `minreq` 库

### API 调试
- 使用 `curl` 直接测试 API 响应格式
- 检查 `ErrCode` 或 `resultCode` 字段判断 API 是否成功
- 任何命令加 `--debug` 打印 HTTP 请求和响应
