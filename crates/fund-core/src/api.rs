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
            let preview = &body[..body.len().min(500)];
            eprintln!("[DEBUG] Response length: {} bytes", body.len());
            if body.len() <= 500 {
                eprintln!("[DEBUG] Response body: {}", body);
            } else {
                eprintln!("[DEBUG] Response body (first 500 chars): {}...", preview);
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
}
