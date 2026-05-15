use anyhow::{Context, Result};
use serde::de::DeserializeOwned;

use crate::models::*;

const BASE_URL: &str = "https://tiantian-fund-api.vercel.app/api/action";

#[derive(Default)]
pub struct Client;

impl Client {
    pub fn new() -> Self {
        Self
    }

    fn request<T: DeserializeOwned>(&self, url: &str) -> Result<T> {
        let debug = std::env::var("FUND_DEBUG").is_ok();
        if debug {
            eprintln!("\n[DEBUG] curl -s '{}'", url);
        }

        let response = minreq::get(url).with_timeout(10).send().context("HTTP request failed")?;

        let body = response.as_str().context("Failed to read response body")?;

        if debug {
            let len = body.chars().take(500).count();
            let preview: String = body.chars().take(500).collect();
            eprintln!("[DEBUG] Response length: {} bytes", body.len());
            if body.len() <= 500 {
                eprintln!("[DEBUG] Response body: {}", body);
            } else {
                eprintln!("[DEBUG] Response body (first {} chars): {}...", len, preview);
            }
            eprintln!();
        }

        serde_json::from_str(body).context("JSON parse failed")
    }

    fn validate_non_empty(value: &str, field_name: &str) -> Result<()> {
        if value.is_empty() {
            anyhow::bail!("{} cannot be empty", field_name);
        }
        Ok(())
    }

    fn check_api_response<T>(response: ApiResponse<T>) -> Result<T> {
        if response.err_code != 0 {
            anyhow::bail!("API error code: {}", response.err_code);
        }
        Ok(response.datas)
    }

    fn check_bigdata<T>(response: BigDataApiResponse<T>) -> Result<T> {
        if response.result_code != 0 {
            anyhow::bail!("BigData API error code: {}", response.result_code);
        }
        Ok(response.datas)
    }

    fn build_url(action: &str, params: &[(&str, &str)]) -> String {
        let mut url = format!("{}?action_name={}", BASE_URL, action);
        for (key, value) in params {
            url.push_str(&format!("&{}={}", key, value));
        }
        url
    }

    fn push_param<'a>(params: &mut Vec<(&'a str, String)>, key: &'a str, value: &Option<String>) {
        if let Some(v) = value {
            params.push((key, v.clone()));
        }
    }

    pub fn search_fund(&self, keyword: &str) -> Result<Vec<FundSearchResult>> {
        Self::validate_non_empty(keyword, "keyword")?;

        let encoded = urlencoding::encode(keyword);
        let url = Self::build_url("fundSearch", &[("m", "1"), ("key", &encoded)]);

        let response: ApiResponse<Vec<FundSearchItem>> = self.request(&url)?;
        Ok(response.datas.into_iter().map(FundSearchResult::from).collect())
    }

    pub fn search_manager(&self, keyword: &str) -> Result<Vec<ManagerSearchResult>> {
        Self::validate_non_empty(keyword, "keyword")?;
        let encoded = urlencoding::encode(keyword);
        let url = Self::build_url("fundSearch", &[("m", "7"), ("key", &encoded)]);
        let response: ApiResponse<Vec<ManagerSearchResult>> = self.request(&url)?;
        Ok(response.datas)
    }

    pub fn search_company_by_name(&self, keyword: &str) -> Result<Vec<CompanySearchResult>> {
        Self::validate_non_empty(keyword, "keyword")?;
        let encoded = urlencoding::encode(keyword);
        let url = Self::build_url("fundSearch", &[("m", "8"), ("key", &encoded)]);
        let response: ApiResponse<Vec<CompanySearchResult>> = self.request(&url)?;
        Ok(response.datas)
    }

    /// 基金简介，含跟踪指数代码（指数基金）
    pub fn get_fund_brief(&self, code: &str) -> Result<FundBrief> {
        Self::validate_non_empty(code, "fund code")?;
        let url = Self::build_url("fundMNStopWatch", &[("FCODE", code)]);
        Self::check_api_response(self.request(&url)?)
    }

    pub fn get_fund_estimate(&self, code: &str) -> Result<FundDetail> {
        Self::validate_non_empty(code, "fund code")?;
        let url = Self::build_url("fundMNDetailInformation", &[("FCODE", code)]);
        Self::check_api_response(self.request(&url)?)
    }

    pub fn get_net_value_history(&self, code: &str, days: i32) -> Result<Vec<NetValuePoint>> {
        let url = Self::build_url(
            "fundMNHisNetList",
            &[("FCODE", code), ("pageIndex", "1"), ("pagesize", &days.to_string())],
        );

        #[derive(serde::Deserialize)]
        struct HistoryItem {
            #[serde(rename = "FSRQ")]
            date: String,
            #[serde(rename = "DWJZ")]
            net_value: String,
            #[serde(rename = "LJJZ")]
            acc_value: String,
            #[serde(rename = "JZZZL")]
            growth: String,
        }

        let items: Vec<HistoryItem> = Self::check_api_response(self.request(&url)?)?;

        Ok(items
            .into_iter()
            .filter_map(|item| {
                Some(NetValuePoint {
                    date: item.date,
                    net_value: item.net_value.parse().ok()?,
                    acc_value: item.acc_value.parse().ok()?,
                    growth: item.growth.parse().ok()?,
                })
            })
            .collect())
    }

    pub fn get_period_increase(&self, code: &str) -> Result<Vec<PeriodIncrease>> {
        let url = Self::build_url("fundMNPeriodIncrease", &[("FCODE", code)]);

        #[derive(serde::Deserialize)]
        struct PeriodItem {
            title: String,
            syl: String,
            avg: String,
            hs300: String,
            rank: String,
            sc: String,
        }

        let title_map = [
            ("Z", "Last Week"),
            ("Y", "Last Month"),
            ("3Y", "Last 3 Months"),
            ("6Y", "Last 6 Months"),
            ("1N", "Last Year"),
            ("2N", "Last 2 Years"),
            ("3N", "Last 3 Years"),
            ("5N", "Last 5 Years"),
            ("JN", "Year to Date"),
            ("LN", "Since Inception"),
        ];

        let items: Vec<PeriodItem> = Self::check_api_response(self.request(&url)?)?;

        Ok(items
            .into_iter()
            .filter_map(|item| {
                let title = title_map
                    .iter()
                    .find(|(k, _)| *k == item.title)
                    .map(|(_, v)| v.to_string())
                    .unwrap_or(item.title);

                Some(PeriodIncrease {
                    title,
                    return_rate: item.syl.parse().ok()?,
                    avg: item.avg.parse().ok()?,
                    hs300_return: item.hs300.parse().ok()?,
                    rank: item.rank.parse().ok()?,
                    total: item.sc.parse().ok()?,
                })
            })
            .collect())
    }

    pub fn get_theme_list(&self) -> Result<Vec<FundTheme>> {
        let url = Self::build_url("fundMNSubjectList", &[]);
        Self::check_api_response(self.request(&url)?)
    }

    pub fn get_big_data_list(&self, category: i32) -> Result<Vec<BigDataItem>> {
        let url = Self::build_url("bigDataList", &[("ClCategory", &category.to_string())]);
        Self::check_bigdata(self.request(&url)?)
    }

    pub fn get_fund_rank(&self, params: &FundRankParams) -> Result<Vec<FundRank>> {
        let mut query_params: Vec<(&str, String)> = vec![
            ("FundType", params.fund_type.clone()),
            ("SortColumn", params.sort_column.clone()),
            ("Sort", params.sort.clone()),
            ("pageIndex", params.page_index.to_string()),
            ("pageSize", params.page_size.to_string()),
        ];

        Self::push_param(&mut query_params, "CLTYPE", &params.cltype);
        Self::push_param(&mut query_params, "BUY", &params.buy);
        Self::push_param(&mut query_params, "DISCOUNT", &params.discount);
        Self::push_param(&mut query_params, "RISKLEVEL", &params.risk_level);
        Self::push_param(&mut query_params, "ESTABDATE", &params.estab_date);

        let str_params: Vec<(&str, &str)> =
            query_params.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let url = Self::build_url("fundMNRank", &str_params);

        Self::check_api_response(self.request(&url)?)
    }

    pub fn get_big_data_detail(&self, cltype: &str) -> Result<Vec<BigDataDetailItem>> {
        let url = Self::build_url("bigDataDetail", &[("cltype", cltype)]);
        Self::check_bigdata(self.request(&url)?)
    }

    pub fn get_rank_history(&self, code: &str, range: &str) -> Result<Vec<RankHistoryPoint>> {
        Self::validate_non_empty(code, "fund code")?;
        let url = Self::build_url("fundRankDiagram", &[("FCODE", code), ("RANGE", range)]);
        Self::check_api_response(self.request(&url)?)
    }

    // ── Fund Manager ───────────────────────────────────────────────────

    pub fn get_fund_managers(&self, code: &str) -> Result<Vec<FundManager>> {
        Self::validate_non_empty(code, "fund code")?;
        let url = Self::build_url("fundMNMangerList", &[("FCODE", code)]);
        Self::check_api_response(self.request(&url)?)
    }

    pub fn get_manager_performance(&self, manager_id: &str) -> Result<ManagerPerformance> {
        Self::validate_non_empty(manager_id, "manager ID")?;
        let url = Self::build_url("fundMSNMangerPerEval", &[("mGRID", manager_id)]);
        Self::check_api_response(self.request(&url)?)
    }

    // ── NAV Trend (for risk metrics calculation) ───────────────────────

    pub fn get_nav_trend(
        &self,
        code: &str,
        range: &str,
        point_count: i32,
    ) -> Result<Vec<NavTrendPoint>> {
        Self::validate_non_empty(code, "fund code")?;
        let url = Self::build_url(
            "fundVPageDiagram",
            &[("FCODE", code), ("RANGE", range), ("POINTCOUNT", &point_count.to_string())],
        );

        #[derive(serde::Deserialize)]
        struct TrendItem {
            #[serde(rename = "FSRQ")]
            date: String,
            #[serde(rename = "DWJZ")]
            nav: String,
            #[serde(rename = "LJJZ")]
            acc_nav: String,
            #[serde(rename = "JZZZL", default)]
            daily_return: String,
        }

        // fundVPageDiagram uses "data" (lowercase), not the standard "Datas" wrapper
        #[derive(serde::Deserialize)]
        struct DiagramResponse {
            #[serde(rename = "data", default)]
            data: Vec<TrendItem>,
        }

        let response: DiagramResponse = self.request(&url)?;
        let items = response.data;

        Ok(items
            .into_iter()
            .filter_map(|item| {
                Some(NavTrendPoint {
                    date: item.date,
                    nav: item.nav.parse().ok()?,
                    acc_nav: item.acc_nav.parse().ok()?,
                    daily_return: item.daily_return.parse().unwrap_or(0.0),
                })
            })
            .collect())
    }

    // ── Accumulated Return vs Benchmark ────────────────────────────────

    pub fn get_accumulated_return(
        &self,
        code: &str,
        range: &str,
        index_code: &str,
    ) -> Result<Vec<AccumulatedReturn>> {
        Self::validate_non_empty(code, "fund code")?;
        let url = Self::build_url(
            "fundVPageAcc",
            &[("FCODE", code), ("RANGE", range), ("INDEXCODE", index_code)],
        );

        #[derive(serde::Deserialize)]
        struct AccItem {
            #[serde(rename = "PDATE")]
            date: String,
            #[serde(rename = "YIELD", default)]
            fund_return: Option<String>,
            #[serde(rename = "INDEXYIELD", default)]
            index_return: Option<String>,
            #[serde(rename = "FUNDTYPEYIELD", default)]
            category_return: Option<String>,
            #[serde(rename = "BENCHQUOTE")]
            bench_return: Option<String>,
        }

        // This API uses "data" field instead of "Datas"
        #[derive(serde::Deserialize)]
        struct AccResponse {
            #[serde(rename = "data", default)]
            data: Vec<AccItem>,
        }

        let response: AccResponse = self.request(&url)?;

        Ok(response
            .data
            .into_iter()
            .map(|item| AccumulatedReturn {
                date: item.date,
                fund_return: item.fund_return.unwrap_or_default().parse().unwrap_or(0.0),
                index_return: item.index_return.unwrap_or_default().parse().unwrap_or(0.0),
                category_return: item.category_return.unwrap_or_default().parse().unwrap_or(0.0),
                bench_return: item.bench_return.unwrap_or_default().parse().unwrap_or(0.0),
            })
            .collect())
    }

    // ── Fund Rating ────────────────────────────────────────────────────

    pub fn get_fund_rating(&self, code: &str) -> Result<Vec<FundRating>> {
        Self::validate_non_empty(code, "fund code")?;
        let url = Self::build_url(
            "fundGradeDetail",
            &[("FCODE", code), ("pageIndex", "1"), ("pageSize", "10")],
        );
        Self::check_api_response(self.request(&url)?)
    }

    // ── Yearly/Monthly Returns ─────────────────────────────────────────

    pub fn get_yearly_returns(&self, code: &str) -> Result<Vec<PeriodIncrease>> {
        self.get_period_increase_with_range(code, "n")
    }

    pub fn get_monthly_returns(&self, code: &str) -> Result<Vec<PeriodIncrease>> {
        self.get_period_increase_with_range(code, "y")
    }

    fn get_period_increase_with_range(
        &self,
        code: &str,
        range: &str,
    ) -> Result<Vec<PeriodIncrease>> {
        Self::validate_non_empty(code, "fund code")?;
        let url = Self::build_url("fundMNPeriodIncrease", &[("FCODE", code), ("RANGE", range)]);

        #[derive(serde::Deserialize)]
        struct PeriodItem {
            title: String,
            syl: String,
            avg: String,
            hs300: String,
            rank: String,
            sc: String,
        }

        let items: Vec<PeriodItem> = Self::check_api_response(self.request(&url)?)?;

        Ok(items
            .into_iter()
            .filter_map(|item| {
                Some(PeriodIncrease {
                    title: item.title,
                    return_rate: item.syl.parse().ok()?,
                    avg: item.avg.parse().ok()?,
                    hs300_return: item.hs300.parse().ok()?,
                    rank: item.rank.parse().ok()?,
                    total: item.sc.parse().ok()?,
                })
            })
            .collect())
    }

    // ── Manager Detail APIs ─────────────────────────────────────────────

    pub fn get_manager_info(&self, manager_id: &str) -> Result<ManagerInfo> {
        Self::validate_non_empty(manager_id, "manager ID")?;
        let url = Self::build_url("fundMSNMangerInfo", &[("FCODE", manager_id)]);
        Self::check_api_response(self.request(&url)?)
    }

    pub fn get_manager_acc(&self, manager_id: &str, range: &str) -> Result<Vec<ManagerAccPoint>> {
        Self::validate_non_empty(manager_id, "manager ID")?;
        let url = Self::build_url("fundMSNMangerAcc", &[("mGRID", manager_id), ("rANGE", range)]);
        Self::check_api_response(self.request(&url)?)
    }

    pub fn get_manager_rank(&self, manager_id: &str) -> Result<ManagerRankData> {
        Self::validate_non_empty(manager_id, "manager ID")?;
        let url = Self::build_url("fundMSNMangerPerRank", &[("mGRID", manager_id)]);
        Self::check_api_response(self.request(&url)?)
    }

    pub fn get_manager_holding_style(&self, manager_id: &str) -> Result<ManagerHoldingStyle> {
        Self::validate_non_empty(manager_id, "manager ID")?;
        let url = Self::build_url("fundMSNMangerPosMark", &[("mGRID", manager_id)]);
        Self::check_api_response(self.request(&url)?)
    }

    pub fn get_manager_holding_char(&self, manager_id: &str) -> Result<ManagerHoldingChar> {
        Self::validate_non_empty(manager_id, "manager ID")?;
        let url = Self::build_url("fundMSNMangerPosChar", &[("mGRID", manager_id)]);
        Self::check_api_response(self.request(&url)?)
    }

    pub fn get_manager_history_funds(&self, manager_id: &str) -> Result<Vec<ManagerHistoryFund>> {
        Self::validate_non_empty(manager_id, "manager ID")?;
        let url = Self::build_url("fundMSNMangerProContr", &[("mGRID", manager_id)]);
        Self::check_api_response(self.request(&url)?)
    }

    // ── Fund Estimation ─────────────────────────────────────────────────

    pub fn get_fund_estimation(&self, code: &str) -> Result<FundEstimation> {
        Self::validate_non_empty(code, "fund code")?;
        let url = Self::build_url("fundVarietieValuationDetail", &[("FCODE", code)]);

        #[derive(serde::Deserialize)]
        struct EstResponse {
            #[serde(rename = "Expansion")]
            expansion: FundEstimation,
        }

        let resp: EstResponse = Self::check_api_response(self.request(&url)?)?;
        Ok(resp.expansion)
    }

    // ── Fund Company ────────────────────────────────────────────────────

    pub fn get_fund_companies(&self) -> Result<Vec<FundCompany>> {
        let url = Self::build_url("fundCompanyBaseList", &[]);
        Self::check_api_response(self.request(&url)?)
    }

    pub fn get_company_info(&self, company_id: &str, action: &str) -> Result<serde_json::Value> {
        Self::validate_non_empty(company_id, "company ID")?;
        let url = Self::build_url("companyApi2", &[("cc", company_id), ("action", action)]);
        let response: ApiResponse<serde_json::Value> = self.request(&url)?;
        if response.err_code != 0 {
            anyhow::bail!("API error code: {}", response.err_code);
        }
        Ok(response.datas)
    }

    /// 公司基本档案（法定名称、成立时间、注册资本、管理规模等）
    pub fn get_company_archive(&self, company_id: &str) -> Result<CompanyArchive> {
        Self::validate_non_empty(company_id, "company ID")?;
        let url =
            Self::build_url("companyApi2", &[("cc", company_id), ("action", "companyarchives")]);
        Self::check_api_response(self.request(&url)?)
    }

    // ── Search By Name ──────────────────────────────────────────────────

    pub fn search_by_name(
        &self,
        keyword: &str,
        page: usize,
        size: usize,
    ) -> Result<SearchByNameResult> {
        Self::validate_non_empty(keyword, "keyword")?;
        let encoded = urlencoding::encode(keyword);
        let url = Self::build_url(
            "fundSearchInfoByName",
            &[
                ("key", &*encoded),
                ("orderType", "1"),
                ("pageindex", &page.to_string()),
                ("pagesize", &size.to_string()),
            ],
        );

        #[derive(serde::Deserialize)]
        struct SearchResponse {
            #[serde(rename = "totalCount", default)]
            total: usize,
            #[serde(rename = "data", default)]
            data: Vec<FundListItem>,
        }

        let resp: SearchResponse = self.request(&url)?;
        Ok(SearchByNameResult { total: resp.total, items: resp.data })
    }

    // ── Fund List by Letter/Type ────────────────────────────────────────

    pub fn get_fund_net_list(
        &self,
        fund_type: &str,
        letter: Option<&str>,
        sort_column: &str,
        page: usize,
        size: usize,
    ) -> Result<Vec<FundListItem>> {
        let page_str = page.to_string();
        let size_str = size.to_string();
        let mut params: Vec<(&str, &str)> = vec![
            ("fundtype", fund_type),
            ("SortColumn", sort_column),
            ("Sort", "desc"),
            ("pageIndex", &page_str),
            ("pagesize", &size_str),
        ];
        if let Some(l) = letter {
            params.push(("Letter", l));
        }
        let url = Self::build_url("fundNetList", &params);
        Self::check_api_response(self.request(&url)?)
    }

    pub fn get_fund_new_list(
        &self,
        fund_type: &str,
        sort_column: &str,
        page: usize,
        size: usize,
    ) -> Result<Vec<FundListItem>> {
        let url = Self::build_url(
            "fundMNNetNewList",
            &[
                ("fundtype", fund_type),
                ("SortColumn", sort_column),
                ("Sort", "desc"),
                ("pageIndex", &page.to_string()),
                ("pagesize", &size.to_string()),
            ],
        );
        Self::check_api_response(self.request(&url)?)
    }

    // ── HK Fund Rank ────────────────────────────────────────────────────

    pub fn get_hk_fund_rank(&self, params: &FundRankParams) -> Result<Vec<FundRank>> {
        let mut query_params: Vec<(&str, String)> = vec![
            ("FundType", params.fund_type.clone()),
            ("SortColumn", params.sort_column.clone()),
            ("Sort", params.sort.clone()),
            ("pageIndex", params.page_index.to_string()),
            ("pageSize", params.page_size.to_string()),
        ];

        Self::push_param(&mut query_params, "CLTYPE", &params.cltype);

        let str_params: Vec<(&str, &str)> =
            query_params.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let url = Self::build_url("fundMNHKRank", &str_params);

        Self::check_api_response(self.request(&url)?)
    }

    // ── Theme List/Focus ────────────────────────────────────────────────

    /// fundThemeList is currently unavailable (times out or returns HTML).
    #[deprecated(note = "fundThemeList endpoint is currently unavailable")]
    pub fn get_theme_hot_list(&self, _rank_item: &str, _category: &str) -> Result<serde_json::Value> {
        anyhow::bail!("fundThemeList 接口当前不可用（超时或返回 HTML）")
    }

    /// fundThemeFocusList is currently unavailable (times out or returns HTML).
    #[deprecated(note = "fundThemeFocusList endpoint is currently unavailable")]
    pub fn get_theme_focus_list(&self, _code: Option<&str>) -> Result<serde_json::Value> {
        anyhow::bail!("fundThemeFocusList 接口当前不可用（超时或返回 HTML）")
    }
}
