# Fund-rs 项目指南

## 项目概述
基金查询 CLI 工具，Rust 编写，调用天天基金 API。

## 项目结构（Cargo Workspace）
```
fund-rs/
├── Cargo.toml              # Workspace root
├── crates/
│   ├── fund-core/          # 共享库：API 客户端 + 模型 + DB + 持仓配置
│   └── fund-cli/           # CLI 二进制（bin name: fund）
└── README.md
```

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

### 依赖注意事项
- ⚠️ 避免使用 `ureq` + `rustls` - 在 macOS ARM 上与 `ring` 库有兼容问题
- ✅ 使用 `minreq` + `native-tls` 特性作为 HTTP 客户端

### API 设计模式
- 使用参数结构体（如 `FundRankParams`）而非多个独立参数
- HTTP 请求封装为泛型方法 `request<T: DeserializeOwned>`
- 错误处理使用 `anyhow::Context` 提供上下文信息

### 代码风格
- 数据模型使用 `serde` 的 `rename` 属性映射 API 字段
- 命令实现在 `crates/fund-cli/src/commands/` 目录，每个命令一个文件
- UI 显示逻辑在 `crates/fund-cli/src/ui/display.rs`，使用 `comfy-table` 渲染
- 共享逻辑在 `crates/fund-core/src/`，包含 API 客户端、模型、DB、持仓配置

### API 端点
- 基础 URL: `https://tiantian-fund-api.vercel.app/api/action`
- 参数格式: `?action_name=<action>&<params>`
- 主要接口: `fundMNRank`, `bigDataList`, `bigDataDetail`, `fundSearch`, `fundMNHisNetList`

### 持仓数据存储（SQLite）
- DB 路径: `~/.fund-rs/portfolio.db`
- 表名: `daily_returns`，主键 `(date, fund_code)`
- 模块: `crates/fund-core/src/db.rs`，提供 `save_records()` / `export_json()`
- 持仓硬编码在 `crates/fund-core/src/holdings.rs` 的 `holdings()` 函数，更新持仓需修改此处

### 命令列表

```bash
# 持仓
fund portfolio              # 查看持仓收益
fund portfolio --save       # 查看并保存到 SQLite
fund backfill --from 2026-03-01 --to 2026-04-30  # 补录历史

# 导出
fund export                 # 导出 portfolio_data.json
fund export -o data.json    # 指定输出路径

# 搜索 & 详情
fund search -k 天弘
fund info -c 420002         # 基金详情 + 阶段收益
fund trend -c 420002        # 基金详情
fund history -c 420002 -d 30 -l 10  # 历史净值

# 排行
fund rank                   # 默认排行
fund rank -t hh -n 20       # 混合型前20
fund rank -t zq             # 债券型
fund rank -t gp             # 股票型
fund rank-history -c 420002 -r 3y  # 排名历史

# 其他
fund theme -l 20            # 主题基金
fund bigdata                # 大数据选基
fund bigdata --detail 1     # 大数据详情

# 对比两只基金（输出 JSON 供网页展示）
fund compare --a 020602 --b 020156 -o compare_data.json

# 调试（任何命令加 --debug）
fund --debug info -c 420002
```

### 每日工作流
```bash
fund portfolio --save        # 拉取当日数据并写入 SQLite
fund export                  # 导出 JSON（可选）
fund backfill --from <date> --to <date>  # 补录历史日期范围
```

## 常见问题

### 编译错误
- 如遇 `ring` 库编译失败，检查是否使用了 `rustls` 特性
- 改用 `native-tls` 或 `minreq` 库

### API 调试
- 使用 `curl` 直接测试 API 响应格式
- 检查 `ErrCode` 或 `resultCode` 字段判断 API 是否成功
- 任何命令加 `--debug` 打印 HTTP 请求和响应
