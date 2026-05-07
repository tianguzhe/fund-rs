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
    #[serde(rename = "DWJZ")]
    pub net_value: String,
    #[serde(rename = "LJJZ")]
    pub acc_value: String,
    #[serde(rename = "SYL_Z")]
    pub week_growth: String,
    #[serde(rename = "SYL_Y")]
    pub month_growth: String,
    #[serde(rename = "SYL_1N")]
    pub year_growth: String,
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
