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
- 格式（真实账本：份额 + 买入净值）：
  ```json
  {
    "holdings": {
      "招商": [
        {
          "code": "000171",
          "name": "易方达裕丰A",
          "shares": 4868.43,
          "cost_nav": 2.052,
          "fee": 50.0,
          "buy_date": "2026-05-08",
          "redeemable_date": "2026-05-08",
          "redeem_status": "redeemable"
        }
      ],
      "支付宝": []
    },
    "cash_flows": [
      { "date": "2026-05-27", "amount": 89571.0, "flow_type": "redeem",
        "code": "420002", "note": "420002 全部赎回" }
    ]
  }
  ```
- `holdings` 是 **`{渠道 -> 持仓数组}` 的 map**：渠道为 key，加载时注入到每笔的
  `channel`（`#[serde(skip)]` 运行时字段，**不要写在每笔 entry 内**）
- 每笔填 `shares`（份额）+ `cost_nav`（买入净值）；市值由 `shares × nav` 运行时推导，
  持有期收益 = `shares × 最新nav - (shares × cost_nav + fee)`
- **同基金同渠道分批买入（DCA）**：同一渠道数组内用不同 `buy_date` 区分多笔，各批独立
  保留成本/到期；`buy_date` 是 `position_daily` 的 lot 键，同渠道分批务必各填一个
- `fee`（申购手续费，元）/ `buy_date` / `redeemable_date` / `redeem_status` 可选；
  `fee` 计入成本基础（cost basis），**持有期收益会扣除手续费**
- `cash_flows`（顶层可选数组）：现金流水，`amount` 带符号（正=进账/赎回/分红，负=出账/申购）
- ⚠️ 旧扁平数组格式（`holdings: [...]` + 每笔写 `channel`）不再兼容：serde 解析直接报错
  （不静默回退，避免误算）
- 生成模板：`fund holdings --init`
- 加载入口：`fund_core::holdings::holdings()` /
  `portfolio_config() -> (Vec<Holding>, Vec<CashFlow>)`

### 持仓数据存储（SQLite，真实账本 5 表）
- DB 路径: `~/.fund-rs/portfolio.db`
- 规范化 schema（可连表查询）：
  - `funds` — 基金元数据（code / name / fund_type）
  - `nav_daily` — 基金每日净值，主键 `(date, code)`；与持仓无关，可 `backfill`
  - `position_daily` — 每日持仓明细快照，主键 `(date, code, channel, buy_date)`，
    按渠道 + 批次分笔，含 `shares` / `cost_nav` / `market_value`
  - `portfolio_daily` — 每日总览，主键 `date`：
    `total_market_value` / `total_cash` / `total_assets` / `total_cost` / `total_pnl`
  - `cash_flows` — 现金流水，UNIQUE 防重复入库（NULL code/note 存 '' 以便去重）
- 总资产 = 持仓市值 + 现金余额（`SUM(cash_flows.amount)` 截至该日），闭环
- 首次运行检测到旧 schema（旧 `portfolio_daily.holding` 或 `daily_returns*`）会
  备份为 `portfolio.db.legacy-<date>` 再重建，不裸删
- 模块: `crates/fund-core/src/db.rs`，提供 `save_snapshot()`（持仓+现金）/
  `save_nav()`（仅净值，backfill 用）/ `export_json()`（连表导出）
- `backfill` **只回填 `nav_daily`**（历史份额未知，不再用今天金额污染历史）

### F10 底层接口（基金本身持仓与行业配置）
- 模块: `crates/fund-core/src/f10.rs`
- 直连 `https://fundf10.eastmoney.com`，与统一 action_name 入口不同
- 返回为 `var apidata={ ... }` JS 赋值 + 嵌入 HTML 表格，已用纯 std 解析
- `get_top_stocks(code, year, month)` — 前十大股票，必须显式传季度末 `year/month`
- `get_active_industries(code)` — 行业配置（已过滤中证 GICS 双套分类）
- `latest_quarter_end(year, month)` — 推算最近已披露季度

### 实时估值接口（fundgz）
- 模块: `crates/fund-core/src/realtime.rs`
- 直连 `https://fundgz.1234567.com.cn/js/<code>.js`，返回 `jsonpgz({...});` JSONP 包裹，已剥壳解析
- `get_realtime_estimate(code)` → `RealtimeEstimate`（含 `prev_nav` / `est_nav` / `est_change_pct` / `est_time`），债基/股基/指数全类型覆盖
- ⚠️ **不要**用统一 API 的 `fundVarietieValuationDetail`(`get_fund_estimation`) 做单点估值：债基返回 `null`、股基返回盘中分时序列 `Datas`，与 `FundEstimation{Expansion.GZ}` 模型不匹配（`portfolio::fetch_rows` 内该调用实际一直被 `Err(_)` 吞掉，估值 footnote 行长期为空）

### 命令列表

```bash
# 持仓
fund portfolio              # 查看持仓收益（市值/现金/持有期收益 + 资产配置）
fund portfolio --save       # 同上并保存快照（nav_daily/position_daily/portfolio_daily/cash_flows）
fund backfill --from <date> --to <date>  # 仅补录历史净值（nav_daily），不碰持仓/总览

# 穿透分析
fund holdings               # 默认 TOP 15 股票
fund holdings --top 30      # 显示 TOP 30 股票
fund holdings --json        # 输出 JSON
fund holdings --init        # 生成 holdings.json 模板

# 导出
fund export                 # 导出 portfolio JSON（连表：组合时间线 + 各基金净值序列 + 持有期收益 + 现金流水）

# 搜索 / 详情 / 分析
fund search -k 天弘
fund info -c 420002
fund history -c 420002 -d 30 -l 10
fund analyze -c 020262 [--json] [-o dist/data/fund-020262.json]
#   --json 输出 JSON；-o 写文件而非 stdout（用于喂 dist/fund-analysis.html）

# 实时估值
fund estimate -c 161725            # 单只盘中估值（估算净值/涨跌幅/估值时间）
fund estimate -c 000171,161725     # 多只逗号分隔
fund estimate                      # 省略 -c：读 holdings.json，按份额估算今日盈亏 + Total
fund estimate --json               # 输出 JSON（estimates[] + failed[]）

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

### 持仓收益输出规范（2026-06-02 更新）
`fund portfolio` 命令现已按以下格式输出（CLI 原生输出，无需 Claude 二次整理）：

**输出结构**：
1. **顶部**：总资产 + 现金（显示扣除的总手续费）
2. **按类型分组表格**（债券型/混合型/股票型等各自独立）
   - 列：代码 | 基金名称 | 市值(元) | 1日 | 1日盈亏 | 7日 | 7日盈亏 | 30日 | 30日盈亏 | 持有期收益
   - 同一基金代码的多笔持仓（不同渠道/不同 `buy_date` 批次）自动合并为一行
   - 按市值降序排列
3. **底部收益汇总表**：1日/7日/30日/持有期，含说明列

**收益口径**：
- **1日/7日/30日**：最近 N 天的滚动收益率（非自然日历周期）
- **持有期收益**：从买入日到今天的累计收益，已扣除手续费，公式 = `市值 - (份额 × 买入净值 + 手续费)`

**Claude 协作规范**：
- 用户问"今日收益/持仓收益"时，直接运行 `fund portfolio`（当天未入库则加 `--save`）
- CLI 输出需转换为 **Markdown 表格**格式呈现（按类型分组 + 收益汇总表），不直接粘贴 ASCII 框线
- 转换后可在末尾追加一两句点评（如"000171 今日领涨"）
  - `Mixed: 168,512 CNY, 18.61%`
  - `Cash: 89,571 CNY, 9.89%`
  - `Total assets: 905,701 CNY`（= 持仓市值 + 现金）
- **持有期收益（按需）**：用户问"赚了多少 / 真实盈亏 / 持有收益"时另出 `Holding-Period Return` 表，列为 `Market Value | Cost Basis | Hold P&L | Return%`，成本 = `shares × cost_nav + fee`，收益 = 市值 − 成本
- **格式示意**：主表 + `Total` 汇总表 + `Asset Allocation` 摘要（+ 按需持有期表）

## Claude 协作经验

### 上游 API 已知限制（重要）
- `fundMNRank` 接口的 `FundType` 参数**静默忽略**，过滤必须客户端做（`api.rs::get_fund_rank` 内已做循环分页 + BFUNDTYPE 客户端过滤）
- `fundMNRank` `pageSize` 上游硬 cap **30 行/页**，需循环 pageIndex 翻页
- BFUNDTYPE 数字代码：`001`=股票 / `002`=混合 / `003`=债券 / `004`=指数 / `006`=QDII / `007`=货币
- 多经理基金的 `fundMSNMangerInfo / PerEval / PosChar / ProContr` 常返回 null（**不是 bug**，是 API 限制）

### 收益口径（重要，2026-06-01 翻车教训）
- ⚠️ **禁止用 `risk_metrics.annualized_return` 当"年收益"报给用户或填收益对比表**：它是近 2 年（~500 交易日）日收益的**波动年化**，债基这种低波动品种上**虚高近 2 倍**（实证：016816 `annualized_return`=4.28% vs 真实近 1 年 1.93%；485119=5.86% vs 2.98%）
- 收益对比/汇报**一律用** `periods[].return_rate`（Last Year / Last 2 Years / Last 3 Months）+ `yearly_returns[].return_rate`，并带 `avg` 同类均值对照——这是用户 App 实际看到、实际拿到的
- `annualized_return` **只可**配 Sharpe/Calmar 做风险收益比参考，**绝不**单独当年收益展示
- 填任何含"年化/年收益"列的表前自检：这个数和 `periods` 近 1 年对得上吗？对不上就用 periods
- 教训：曾用虚高口径差点推荐用户把优质短债换成"看着收益更高"的 485119（真实收益其实不及现有持仓，却多扛久期风险），是赔本错误决策

### fund-deep-analyzer agent 调用注意
- 若 Agent 调用报"1m 上下文已经全量可用"错误，**直接 fallback 到主 Claude 自己执行**（`./target/release/fund analyze -c <code> --json > /tmp/fund_<code>.json` + jq 提取）
- 主 Claude 已加载 agent 模板时，可不通过 Agent 工具直接产出 10 节研究级报告

### 深度分析数据提取
- 标准 jq 字段提取脚本：见 `.claude/agents/fund-deep-analyzer.md` 的 Step 2
- 关键字段：`risk_metrics.max_drawdown_start_date / end_date`（已在 `scoring.rs` 暴露）+ `accumulated_return[0/250/750/1250/last]` 切片做真实基准对照

### 深度分析报告生成习惯
- 模板深化采用**审视 → 列差距 → 用户确认 → 补强**循环（避免一次过度设计）
- 报告产出后再生成 HTML 数据：`fund analyze -c <code> --json -o dist/data/fund-<code>.json`
- 若 CLI 数据缺失（如 `manager_info=null`），报告显式标注"⚠️ 数据缺失"，**不要伪造**

### dist/fund-analysis.html 架构
- 模板 + 数据分离：URL `?code=<6位>` 自动 fetch `./data/fund-<code>.json`；`?amount=` 控制金额化卡片基准
- 视觉风格：**白色 Editorial**（Spectral display + Newsreader body + IBM Plex Mono numerals）
- 样式入口：`dist/fund-analysis.css`（HTML 只保留 stylesheet 引用，后续视觉修改优先改 CSS）
- chart 颜色硬编码点：`renderAccChart` 的 `series` 数组 / legend 的 ldot inline style / `renderRadar` 的 stroke/fill —— 改色调需同步这 3 处
- builder 函数约 25 个，新增需同步 `init()` 数组 + 任何用到的 class 都要在 `dist/fund-analysis.css` 定义（否则静默不显示）

### UI 渲染验证（headless）
```bash
# 启 server
cd dist && python3 -m http.server 9876 &
# 全页截图（含动画中间帧）
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless --disable-gpu \
  --window-size=1400,5000 --hide-scrollbars --virtual-time-budget=6000 \
  --screenshot=/tmp/snap.png "http://localhost:9876/fund-analysis.html?code=000171&amount=150000"
```
- 校验 JS 语法（重写 chart 后必跑）：`awk '/<script>/{f=1;next} /<\/script>/{f=0} f' dist/fund-analysis.html > /tmp/c.js && node --check /tmp/c.js`
- 验证所有 class 已定义：`comm -23 <(grep -oE 'class="[^"]+"' dist/fund-analysis.html | tr ' ' '\n' | grep -oE '^[a-z][a-z0-9_-]+$' | sort -u) <(grep -oE '\.[a-z][a-z0-9_-]+' dist/fund-analysis.html | sed 's/^\.//' | sort -u)`

## 常见问题

### 编译错误
- 如遇 `ring` 库编译失败，检查是否使用了 `rustls` 特性
- 改用 `native-tls` 或 `minreq` 库

### API 调试
- 使用 `curl` 直接测试 API 响应格式
- 检查 `ErrCode` 或 `resultCode` 字段判断 API 是否成功
- 任何命令加 `--debug` 打印 HTTP 请求和响应
