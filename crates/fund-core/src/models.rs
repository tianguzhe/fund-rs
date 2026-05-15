use serde::{Deserialize, Serialize};

// ── API Response Wrappers ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ApiResponse<T> {
    #[serde(rename = "Datas")]
    pub datas: T,
    #[serde(rename = "ErrCode")]
    pub err_code: i32,
}

#[derive(Debug, Deserialize)]
pub struct BigDataApiResponse<T> {
    pub datas: T,
    #[serde(rename = "resultCode")]
    pub result_code: i32,
}

// ── Search Types ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct FundSearchResult {
    #[serde(rename = "CODE")]
    pub code: String,
    #[serde(rename = "NAME")]
    pub name: String,
    #[serde(rename = "FundType", default)]
    pub fund_type: String,
}

#[derive(Debug, Deserialize)]
struct FundBaseInfo {
    #[serde(rename = "FTYPE", default)]
    fund_type: String,
}

#[derive(Debug, Deserialize)]
pub struct FundSearchItem {
    #[serde(rename = "CODE")]
    code: String,
    #[serde(rename = "NAME")]
    name: String,
    #[serde(rename = "FundBaseInfo", default)]
    base_info: Option<FundBaseInfo>,
}

impl From<FundSearchItem> for FundSearchResult {
    fn from(item: FundSearchItem) -> Self {
        Self {
            code: item.code,
            name: item.name,
            fund_type: item.base_info.map(|b| b.fund_type).unwrap_or_default(),
        }
    }
}

// ── Fund Detail ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct FundDetail {
    #[serde(rename = "FCODE")]
    pub code: String,
    #[serde(rename = "SHORTNAME")]
    pub name: String,
    #[serde(rename = "FULLNAME", default)]
    pub full_name: String,
    #[serde(rename = "FTYPE")]
    pub fund_type: String,
    #[serde(rename = "ESTABDATE")]
    pub estab_date: String,
    #[serde(rename = "JJGS")]
    pub company: String,
    #[serde(rename = "JJJL")]
    pub manager: String,
    #[serde(rename = "TGYH", default)]
    pub custodian: String,
    #[serde(rename = "ENDNAV", default)]
    pub scale: String,
    #[serde(rename = "RISKLEVEL", default)]
    pub risk_level: String,
    #[serde(rename = "MGREXP", default)]
    pub mgr_fee: String,
    #[serde(rename = "TRUSTEXP", default)]
    pub trust_fee: String,
    #[serde(rename = "SALESEXP", default)]
    pub sales_fee: String,
    /// 业绩比较基准描述（主动基金）
    #[serde(rename = "BENCH", default)]
    pub bench: String,
    /// 跟踪指数代码（指数/ETF 基金），用于选择 fundVPageAcc 的 INDEXCODE
    #[serde(rename = "INDEXCODE", default)]
    pub index_code: String,
}

// ── History / Net Value ────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct PeriodIncrease {
    pub title: String,
    pub return_rate: f64,
    pub avg: f64,
    pub hs300_return: f64,
    pub rank: i32,
    pub total: i32,
}

#[derive(Debug, Serialize)]
pub struct NetValuePoint {
    pub date: String,
    pub net_value: f64,
    pub acc_value: f64,
    pub growth: f64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RankHistoryPoint {
    #[serde(rename = "PDATE")]
    pub date: String,
    #[serde(rename = "QRANK")]
    pub rank: String,
    #[serde(rename = "QSC")]
    pub total: String,
}

impl RankHistoryPoint {
    pub fn rank_percentage(&self) -> Option<f64> {
        let rank: f64 = self.rank.parse().ok()?;
        let total: f64 = self.total.parse().ok()?;
        Some((rank / total) * 100.0)
    }
}

// ── Ranking ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct FundRank {
    #[serde(rename = "FCODE")]
    pub code: String,
    #[serde(rename = "SHORTNAME")]
    pub name: String,
    #[serde(rename = "DWJZ", default)]
    pub net_value: String,
    #[serde(rename = "LJJZ", default)]
    pub acc_value: String,
    #[serde(rename = "RZDF", default)]
    pub daily_change: String,
    #[serde(rename = "SYL_Z", default)]
    pub week_growth: String,
    #[serde(rename = "SYL_Y", default)]
    pub month_growth: String,
    #[serde(rename = "SYL_3Y", default)]
    pub three_month_growth: String,
    #[serde(rename = "SYL_6Y", default)]
    pub six_month_growth: String,
    #[serde(rename = "SYL_JN", default)]
    pub ytd_growth: String,
    #[serde(rename = "SYL_1N", default)]
    pub year_growth: String,
    #[serde(rename = "SYL_3N", default)]
    pub three_year_growth: String,
    /// 基金规模（元）
    #[serde(rename = "ENDNAV", default)]
    pub scale: String,
    #[serde(rename = "BFUNDTYPE", default)]
    pub fund_type_code: String,
}

#[derive(Debug)]
pub struct FundRankParams {
    pub fund_type: String,
    pub sort_column: String,
    pub sort: String,
    pub page_index: usize,
    pub page_size: usize,
    pub cltype: Option<String>,
    pub buy: Option<String>,
    pub discount: Option<String>,
    pub risk_level: Option<String>,
    pub estab_date: Option<String>,
}

impl Default for FundRankParams {
    fn default() -> Self {
        Self {
            fund_type: "all".to_string(),
            sort_column: "DWJZ".to_string(),
            sort: "desc".to_string(),
            page_index: 1,
            page_size: 20,
            cltype: None,
            buy: None,
            discount: None,
            risk_level: None,
            estab_date: None,
        }
    }
}

// ── Big Data ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct BigDataItem {
    #[serde(rename = "Title")]
    pub title: String,
    #[serde(rename = "FundCode")]
    pub fund_code: String,
    #[serde(rename = "FundName")]
    pub fund_name: String,
    #[serde(rename = "SYL")]
    pub return_rate: String,
    #[serde(rename = "PeriodText")]
    pub period: String,
}

#[derive(Debug, Deserialize)]
pub struct BigDataDetailItem {
    #[serde(rename = "FCODE")]
    pub code: String,
    #[serde(rename = "SHORTNAME")]
    pub name: String,
    #[serde(rename = "SYL")]
    pub return_rate: String,
    #[serde(rename = "DWJZ", default)]
    pub net_value: String,
    #[serde(rename = "LJJZ", default)]
    pub acc_value: String,
}

// ── Fund Manager ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct FundManager {
    #[serde(rename = "MGRID")]
    pub manager_id: String,
    #[serde(rename = "MGRNAME")]
    pub manager_name: String,
    #[serde(rename = "DAYS", default)]
    pub days: String,
    #[serde(rename = "FEMPDATE", default)]
    pub start_date: String,
    #[serde(rename = "PENAVGROWTH", default)]
    pub return_since_start: String,
    #[serde(rename = "ISINOFFICE", default)]
    pub is_in_office: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ManagerPerformance {
    #[serde(rename = "MAXRETRA_1", default)]
    pub max_drawdown_1y: String,
    #[serde(rename = "MAXRETRA_3", default)]
    pub max_drawdown_3y: String,
    #[serde(rename = "SHARP_1", default)]
    pub sharpe_1y: String,
    #[serde(rename = "SHARP_3", default)]
    pub sharpe_3y: String,
    #[serde(rename = "STDDEV_1", default)]
    pub volatility_1y: String,
    #[serde(rename = "STDDEV_3", default)]
    pub volatility_3y: String,
    #[serde(rename = "WIN_1", default)]
    pub win_rate_1y: String,
    #[serde(rename = "WIN_3", default)]
    pub win_rate_3y: String,
}

// ── NAV Trend (for drawdown/volatility/Sharpe calculation) ────────────

#[derive(Debug, Serialize)]
pub struct NavTrendPoint {
    pub date: String,
    pub nav: f64,
    pub acc_nav: f64,
    pub daily_return: f64,
}

// ── Accumulated Return vs Benchmark ──────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AccumulatedReturn {
    pub date: String,
    pub fund_return: f64,
    pub index_return: f64,
    pub category_return: f64,
    pub bench_return: f64,
}

// ── Fund Rating ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct FundRating {
    #[serde(rename = "RDATE")]
    pub date: String,
    #[serde(rename = "ZSPJ", default)]
    pub zs_rating: String,
    #[serde(rename = "SZPJ3", default)]
    pub sz_rating: String,
    #[serde(rename = "JAPJ", default)]
    pub ja_rating: String,
}

// ── Manager Detail ───────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct ManagerInfo {
    #[serde(rename = "MGRID")]
    pub manager_id: String,
    #[serde(rename = "MGRNAME")]
    pub manager_name: String,
    #[serde(rename = "RESUME", default)]
    pub resume: String,
    #[serde(rename = "TOTALDAYS", default)]
    pub total_days: String,
    #[serde(rename = "NETNAV", default)]
    pub net_nav: String,
    #[serde(rename = "FCOUNT", default)]
    pub fund_count: String,
    #[serde(rename = "PRENAME", default)]
    pub representative: String,
    #[serde(rename = "YIELDSE", default)]
    pub annual_return: String,
    #[serde(rename = "JJGS", default)]
    pub company: String,
    #[serde(rename = "FMAXEARN1", default)]
    pub max_earn: String,
    #[serde(rename = "FMAXRETRA1", default)]
    pub max_drawdown: String,
    #[serde(rename = "MGOLD", default)]
    pub score: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ManagerAccPoint {
    #[serde(rename = "PDATE")]
    pub date: String,
    #[serde(rename = "SYI", default)]
    pub manager_return: String,
    #[serde(rename = "AVGSYI", default)]
    pub avg_return: String,
    #[serde(rename = "INDEXSYI", default)]
    pub index_return: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ManagerRankData {
    #[serde(rename = "W", default)]
    pub week: String,
    #[serde(rename = "M", default)]
    pub month: String,
    #[serde(rename = "Q", default)]
    pub quarter: String,
    #[serde(rename = "HY", default)]
    pub half_year: String,
    #[serde(rename = "Y", default)]
    pub year: String,
    #[serde(rename = "TWY", default)]
    pub two_year: String,
    #[serde(rename = "TRY", default)]
    pub three_year: String,
    #[serde(rename = "FY", default)]
    pub five_year: String,
    #[serde(rename = "WRANK", default)]
    pub week_rank: String,
    #[serde(rename = "MRANK", default)]
    pub month_rank: String,
    #[serde(rename = "QRANK", default)]
    pub quarter_rank: String,
    #[serde(rename = "YRANK", default)]
    pub year_rank: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ManagerHoldingStyle {
    #[serde(rename = "Pos", default)]
    pub positions: Vec<StockPosition>,
    #[serde(rename = "PosDate", default)]
    pub pos_date: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct StockPosition {
    #[serde(rename = "GPDM")]
    pub code: String,
    #[serde(rename = "GPJC")]
    pub name: String,
    #[serde(rename = "JZBL", default)]
    pub ratio: String,
    #[serde(rename = "INDEXNAME", default)]
    pub industry: String,
    #[serde(rename = "PCTNVCHG", default)]
    pub change: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ManagerHoldingChar {
    #[serde(rename = "GPCW", default)]
    pub stock_position: String,
    #[serde(rename = "SDJZD", default)]
    pub top10_concentration: String,
    #[serde(rename = "DYHYZB", default)]
    pub top1_industry: String,
    #[serde(rename = "YCESL_3M", default)]
    pub monthly_excess_win: String,
    #[serde(rename = "HYJZD", default)]
    pub industry_concentration: String,
    #[serde(rename = "GPCWAVG", default)]
    pub stock_position_avg: String,
    #[serde(rename = "SDJZDAVG", default)]
    pub top10_concentration_avg: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ManagerHistoryFund {
    #[serde(rename = "FCODE")]
    pub code: String,
    #[serde(rename = "SHORTNAME")]
    pub name: String,
    #[serde(rename = "FEMPDATE", default)]
    pub start_date: String,
    #[serde(rename = "LEMPDATE", default)]
    pub end_date: String,
    #[serde(rename = "TOTALDAYS", default)]
    pub days: String,
    #[serde(rename = "PENAVGROWTH", default)]
    pub return_rate: String,
    #[serde(rename = "TLRANK", default)]
    pub rank: String,
    #[serde(rename = "TLSC", default)]
    pub total: String,
}

// ── Fund Estimation ──────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct FundEstimation {
    #[serde(rename = "GZ", default)]
    pub nav: String,
    #[serde(rename = "GSZZL", default)]
    pub change_pct: String,
    #[serde(rename = "GZTIME", default)]
    pub time: String,
    #[serde(rename = "SOURCERATE", default)]
    pub original_fee: String,
    #[serde(rename = "rate", default)]
    pub discount_fee: String,
    #[serde(rename = "BUY", default)]
    pub can_buy: String,
    #[serde(rename = "SGZT", default)]
    pub buy_status: String,
}

// ── Fund Company ─────────────────────────────────────────────────────

/// fundCompanyBaseList 返回字段（注意：此接口用 COMPANYCODE/SNAME，
/// 与 fundSearch m=8 的 JJGSID/JJGS 不同）
#[derive(Debug, Deserialize, Serialize)]
pub struct FundCompany {
    #[serde(rename = "COMPANYCODE")]
    pub id: String,
    #[serde(rename = "SNAME")]
    pub name: String,
    #[serde(rename = "ABBNAME", default)]
    pub abbr: String,
    #[serde(rename = "FUNDCOUNT", default)]
    pub fund_count: String,
    #[serde(rename = "JJRS", default)]
    pub manager_count: String,
    #[serde(rename = "ESTABDATE", default)]
    pub estab_date: String,
}

// ── Fund List Item ───────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct FundListItem {
    #[serde(rename = "FCODE", default)]
    pub code: String,
    #[serde(rename = "SHORTNAME", default)]
    pub name: String,
    #[serde(rename = "FTYPE", default)]
    pub fund_type: String,
    #[serde(rename = "DWJZ", default)]
    pub net_value: String,
    #[serde(rename = "LJJZ", default)]
    pub acc_value: String,
    #[serde(rename = "RZDF", default)]
    pub daily_change: String,
}

// ── Search By Name Result ────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SearchByNameResult {
    pub total: usize,
    pub items: Vec<FundListItem>,
}

// ── Themes ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct FundTheme {
    #[serde(rename = "INDEXCODE")]
    pub code: String,
    #[serde(rename = "INDEXNAME")]
    pub name: String,
    #[serde(rename = "TYPE")]
    pub theme_type: String,
}

// ── Fund Brief (fundMNStopWatch) ─────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct FundBrief {
    #[serde(rename = "FCODE")]
    pub code: String,
    #[serde(rename = "SHORTNAME")]
    pub name: String,
    #[serde(rename = "FTYPE")]
    pub fund_type: String,
    #[serde(rename = "ESTABDATE", default)]
    pub estab_date: String,
    /// 基金类型编码，区分主动/指数/ETF 等
    #[serde(rename = "BFUNDTYPE", default)]
    pub b_fund_type: String,
    /// 跟踪指数代码，仅指数基金有值
    #[serde(rename = "INDEXCODE", default)]
    pub index_code: String,
    /// 跟踪指数名称
    #[serde(rename = "INDEXNAME", default)]
    pub index_name: String,
}

// ── Manager Search ───────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct ManagerSearchResult {
    #[serde(rename = "MgrId")]
    pub manager_id: String,
    #[serde(rename = "MgrName")]
    pub manager_name: String,
    #[serde(rename = "JJGS", default)]
    pub company: String,
}

// ── Company Search ───────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct CompanySearchResult {
    #[serde(rename = "JJGSID")]
    pub company_id: String,
    #[serde(rename = "JJGS")]
    pub company_name: String,
    /// 旗下基金列表（简要）
    #[serde(rename = "QXJJ", default)]
    pub funds: Vec<CompanyFundBrief>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CompanyFundBrief {
    #[serde(rename = "FCODE", alias = "_id")]
    pub code: String,
    #[serde(rename = "SHORTNAME", default)]
    pub name: String,
}

// ── Company Archive (companyApi2 action=companyarchives) ─────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct CompanyArchive {
    /// 法定名称
    #[serde(rename = "FDMC", default)]
    pub full_name: String,
    /// 成立时间
    #[serde(rename = "CLRQ", default)]
    pub estab_date: String,
    /// 注册资本
    #[serde(rename = "ZCZB", default)]
    pub reg_capital: String,
    /// 法人代表
    #[serde(rename = "FRDB", default)]
    pub legal_rep: String,
    /// 总经理
    #[serde(rename = "Manager", default)]
    pub manager: String,
    /// 管理规模（元）
    #[serde(rename = "GLGM", default)]
    pub aum: String,
    /// 旗下基金总数
    #[serde(rename = "Count", default)]
    pub fund_count: String,
}
