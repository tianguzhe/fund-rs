mod commands;
mod ui;

use anyhow::Result;
use clap::{Parser, Subcommand};
use fund_core::api::Client;
use fund_core::models::FundRankParams;

#[derive(Parser)]
#[command(name = "fund", version, about = "Fund Query Tool")]
struct Cli {
    /// Enable debug mode to print curl commands
    #[arg(short, long, global = true)]
    debug: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Portfolio {
        /// 保存今日数据到 SQLite
        #[arg(long)]
        save: bool,
    },
    /// 补录历史数据到 SQLite（按日期范围）
    Backfill {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
    },
    /// 导出历史数据为 JSON（供 H5 图表使用）
    Export {
        #[arg(short, long, default_value = "dist/data/portfolio.json")]
        output: std::path::PathBuf,
    },
    Search {
        #[arg(short, long)]
        keyword: String,
    },
    Info {
        #[arg(short, long)]
        code: String,
    },
    Trend {
        #[arg(short, long)]
        code: String,
    },
    History {
        #[arg(short, long)]
        code: String,
        #[arg(short, long, default_value = "30")]
        days: i32,
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },
    Theme {
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },
    Bigdata {
        #[arg(short, long, default_value = "0")]
        category: i32,
        #[arg(long)]
        detail: Option<String>,
    },
    Rank {
        /// 基金类型短码：all / zq(债券) / hh(混合) / gp(股票) / zs(指数) / qdii / hb(货币)
        #[arg(short = 't', long, default_value = "all")]
        fund_type: String,
        #[arg(short = 'n', long, default_value = "20")]
        size: usize,
        #[arg(long, default_value = "DWJZ")]
        sort_column: String,
        #[arg(long, default_value = "desc")]
        sort: String,
        #[arg(long)]
        cltype: Option<String>,
        #[arg(long)]
        buy: Option<String>,
        #[arg(long)]
        discount: Option<String>,
        #[arg(long)]
        risk_level: Option<String>,
        #[arg(long)]
        estab_date: Option<String>,
    },
    RankHistory {
        #[arg(short, long)]
        code: String,
        #[arg(short, long, default_value = "3y")]
        range: String,
    },
    /// 深度分析基金（详情+阶段收益+风险指标+经理评价+综合评分）
    Analyze {
        #[arg(short, long)]
        code: String,
        /// 输出 JSON 格式
        #[arg(long)]
        json: bool,
        /// 写入指定路径而非 stdout（仅与 --json 配合使用），用于喂给 dist/fund-analysis.html
        #[arg(short = 'o', long)]
        output: Option<std::path::PathBuf>,
    },
    /// 对比两只基金，输出 JSON 供网页展示
    Compare {
        #[arg(long)]
        a: String,
        #[arg(long)]
        b: String,
        #[arg(short, long, default_value = "dist/data/compare.json")]
        output: std::path::PathBuf,
    },
    /// 组合穿透分析（资产配置 + 底层股票 + 行业暴露）
    Holdings {
        /// 生成持仓配置模板（~/.fund-rs/holdings.json）后退出
        #[arg(long)]
        init: bool,
        /// 显示底层股票的前 N 只
        #[arg(short, long, default_value = "15")]
        top: usize,
        /// 输出 JSON 格式
        #[arg(long)]
        json: bool,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    if cli.debug {
        std::env::set_var("FUND_DEBUG", "1");
    }

    let client = Client::new();

    match cli.command {
        Commands::Portfolio { save } => commands::portfolio::run(&client, save),
        Commands::Backfill { from, to } => commands::backfill::run(&client, &from, &to),
        Commands::Export { output } => commands::export::run(&output),
        Commands::Search { keyword } => commands::search::run(&client, &keyword),
        Commands::Info { code } => commands::info::run(&client, &code),
        Commands::Trend { code } => commands::trend::run(&client, &code),
        Commands::History { code, days, limit } => {
            commands::history::run(&client, &code, days, limit)
        }
        Commands::Theme { limit } => commands::theme::run(&client, limit),
        Commands::Bigdata { category, detail } => {
            if let Some(cltype) = detail {
                commands::bigdata::run_detail(&client, &cltype)
            } else {
                commands::bigdata::run_list(&client, category)
            }
        }
        Commands::Rank {
            fund_type,
            size,
            sort_column,
            sort,
            cltype,
            buy,
            discount,
            risk_level,
            estab_date,
        } => {
            let params = FundRankParams {
                fund_type,
                sort_column,
                sort,
                page_index: 1,
                page_size: size,
                cltype,
                buy,
                discount,
                risk_level,
                estab_date,
            };
            commands::rank::run(&client, params)
        }
        Commands::RankHistory { code, range } => {
            commands::rank_history::run(&client, &code, &range)
        }
        Commands::Compare { a, b, output } => commands::compare::run(&a, &b, &output),
        Commands::Analyze { code, json, output } => {
            commands::analyze::run(&client, &code, json, output.as_deref())
        }
        Commands::Holdings { init, top, json } => {
            if init {
                commands::holdings::init(false)
            } else {
                commands::holdings::run(&client, top, json)
            }
        }
    }
}
