//! F10 底层接口：基金本身持仓与行业配置。
//!
//! 直连 `fundf10.eastmoney.com`，与统一 `action_name` 入口不同。
//! 返回体不是标准 JSON，而是 `var apidata={ ... }` 形式的 JS 赋值，
//! 内部 `content` 字段为 HTML 表格。这里用纯 std 做最小化解析，
//! 避免引入 regex/scraper 依赖。

use anyhow::{Context, Result};
use serde::Serialize;

const F10_BASE: &str = "https://fundf10.eastmoney.com";

#[derive(Debug, Serialize, Clone)]
pub struct FeeRule {
    pub scope: String,
    pub rate: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct FeeRules {
    pub purchase: Vec<FeeRule>,
    pub redemption: Vec<FeeRule>,
}

#[derive(Debug, Serialize, Clone)]
pub struct StockHolding {
    pub stock_code: String,
    pub stock_name: String,
    /// 占净值比例 (%)
    pub ratio: f64,
    /// 持仓市值（万元）— 部分场景为 0（如新发基金）
    pub market_value_wan: f64,
}

#[derive(Debug, Serialize, Clone)]
pub struct TopStocksReport {
    /// 报告期，例如 "2026年1季度"
    pub period: String,
    /// 截止日期 YYYY-MM-DD
    pub end_date: String,
    pub stocks: Vec<StockHolding>,
}

#[derive(Debug, Serialize, Clone)]
pub struct IndustryAllocation {
    pub industry_code: String,
    pub industry_name: String,
    /// 最近一期占净值比例 (%)
    pub ratio: f64,
    /// 最近一期市值（万元）
    pub market_value_wan: f64,
    /// 截止日期
    pub end_date: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct BondHolding {
    pub bond_code: String,
    pub bond_name: String,
    /// 占净值比例 (%)
    pub ratio: f64,
    /// 持仓市值（万元）
    pub market_value_wan: f64,
}

#[derive(Debug, Serialize, Clone)]
pub struct TopBondsReport {
    /// 报告期，例如 "2026年1季度"
    pub period: String,
    /// 截止日期 YYYY-MM-DD
    pub end_date: String,
    pub bonds: Vec<BondHolding>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ScaleChangePoint {
    /// 报告期日期，例如 "2026-03-31"
    pub date: String,
    /// 期间申购（亿份）
    pub purchase_yi: f64,
    /// 期间赎回（亿份）
    pub redemption_yi: f64,
    /// 期末总份额（亿份）
    pub end_shares_yi: f64,
    /// 期末净资产（亿元）
    pub end_nav_yi: f64,
    /// 净资产变动率 (%)
    pub change_pct: f64,
}

#[derive(Debug, Serialize, Clone)]
pub struct HolderStructurePoint {
    /// 公告日期 YYYY-MM-DD
    pub announce_date: String,
    /// 机构持有比例 (%)
    pub institutional_pct: f64,
    /// 个人持有比例 (%)
    pub retail_pct: f64,
    /// 内部持有比例 (%)
    pub internal_pct: f64,
    /// 总份额（亿份）
    pub total_shares_yi: f64,
}

#[derive(Debug, Serialize, Clone)]
pub struct HoldingConstraints {
    /// 申购状态文本，例如 "开放申购"
    pub purchase_status: String,
    /// 赎回状态文本，例如 "开放赎回"
    pub redemption_status: String,
    /// 最短持有期（天）。"90 天持有" 这类硬约束被结构化到这里。
    /// None 表示页面未声明或无法识别（例如普通开放式无持有期限制）。
    pub min_holding_days: Option<u32>,
    /// 原始"基金特色"文案，作为正则识别失败时的兜底
    pub features: String,
}

fn http_get(url: &str) -> Result<String> {
    let debug = std::env::var("FUND_DEBUG").is_ok();
    if debug {
        eprintln!("\n[DEBUG] curl -s -H 'Referer: https://fundf10.eastmoney.com/' '{}'", url);
    }
    // Referer is required by some F10 endpoints to return real data instead of empty.
    let resp = minreq::get(url)
        .with_header("Referer", "https://fundf10.eastmoney.com/")
        .with_header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X) AppleWebKit/537.36")
        .with_timeout(10)
        .send()
        .context("F10 HTTP request failed")?;
    let body = resp.as_str().context("Failed to read F10 response")?.to_string();
    if debug {
        eprintln!("[DEBUG] F10 response length: {} bytes", body.len());
    }
    Ok(body)
}

/// Extract content between the first `field:"` and the matching closing `"`.
/// Eastmoney F10 HTML uses single quotes throughout, so we assume the value
/// payload contains no unescaped double quotes. Documented because if Eastmoney
/// ever switches quoting style, parsing silently returns wrong slices.
fn extract_str_field<'a>(body: &'a str, field: &str) -> Option<&'a str> {
    let needle = format!("{}:\"", field);
    let start = body.find(&needle)? + needle.len();
    let rest = &body[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// Strip nested HTML tags from a cell, collapse whitespace.
fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Naive HTML table parser: returns rows of cells. Only matches the `<tbody>` block
/// — `<thead>` is skipped to avoid leaking header rows into data.
fn parse_table_rows_from_section(section_html: &str) -> Vec<Vec<String>> {
    let tbody_start = match section_html.find("<tbody") {
        Some(i) => i,
        None => return Vec::new(),
    };
    let tbody_end = section_html[tbody_start..]
        .find("</tbody>")
        .map(|e| tbody_start + e)
        .unwrap_or(section_html.len());
    let body = &section_html[tbody_start..tbody_end];

    let mut rows = Vec::new();
    for tr_chunk in body.split("<tr").skip(1) {
        let after = match tr_chunk.find('>') {
            Some(i) => &tr_chunk[i + 1..],
            None => continue,
        };
        let row_end = after.find("</tr>").unwrap_or(after.len());
        let row_html = &after[..row_end];
        let mut cells = Vec::new();
        for td_chunk in row_html.split("<td").skip(1) {
            let inner_start = match td_chunk.find('>') {
                Some(i) => &td_chunk[i + 1..],
                None => continue,
            };
            let cell_end = inner_start.find("</td>").unwrap_or(inner_start.len());
            cells.push(strip_html(&inner_start[..cell_end]));
        }
        if !cells.is_empty() {
            rows.push(cells);
        }
    }
    rows
}

fn parse_table_rows(html: &str) -> Vec<Vec<String>> {
    parse_table_rows_from_section(html)
}

fn extract_box_section<'a>(html: &'a str, title: &str) -> Option<&'a str> {
    let marker = format!("<label class=\"left\">{}", title);
    let start = html.find(&marker)?;
    let rest = &html[start..];
    let end = rest.find("</div></div><div class=\"box").unwrap_or(rest.len());
    Some(&rest[..end])
}

fn parse_fee_rules(section_html: &str) -> Vec<FeeRule> {
    parse_table_rows_from_section(section_html)
        .into_iter()
        .filter_map(|row| {
            if row.len() < 2 {
                return None;
            }
            Some(FeeRule { scope: row[0].clone(), rate: row[1].clone() })
        })
        .collect()
}

fn parse_pct(s: &str) -> f64 {
    s.trim().trim_end_matches('%').replace(',', "").parse().unwrap_or(0.0)
}

fn parse_num(s: &str) -> f64 {
    s.trim().replace(',', "").parse().unwrap_or(0.0)
}

/// Get top-10 stock holdings for a fund at the given quarter-end.
///
/// `year`/`month` must be a true quarter-end (e.g. 2026/03). Passing empty values
/// risks empty data — Eastmoney returns `<tbody></tbody>` for some funds when the
/// quarter is unspecified. See `references/tian.md` "jjcc 特别规则".
pub fn get_top_stocks(code: &str, year: u32, month: u32) -> Result<TopStocksReport> {
    let url = format!(
        "{}/FundArchivesDatas.aspx?type=jjcc&code={}&topline=10&year={}&month={:02}&rt=0",
        F10_BASE, code, year, month
    );
    let body = http_get(&url)?;

    let content = extract_str_field(&body, "content").unwrap_or("");
    let period = extract_period(&body).unwrap_or_default();
    let end_date = extract_end_date(content).unwrap_or_default();
    let rows = parse_table_rows(content);

    let mut stocks = Vec::new();
    for row in rows {
        // jjcc tbody columns are stable across quarters:
        // 0 序号 / 1 股票代码 / 2 股票名称 / 3 最新价 / 4 涨跌幅 /
        // 5 相关资讯 / 6 占净值比例 / 7 持股数 / 8 持仓市值
        if row.len() < 9 {
            continue;
        }
        stocks.push(StockHolding {
            stock_code: row[1].clone(),
            stock_name: row[2].clone(),
            ratio: parse_pct(&row[6]),
            market_value_wan: parse_num(&row[8]),
        });
    }

    Ok(TopStocksReport { period, end_date, stocks })
}

pub fn get_fee_rules(code: &str) -> Result<FeeRules> {
    let url = format!("{}/jjfl_{}.html", F10_BASE, code);
    let body = http_get(&url)?;

    let purchase = extract_box_section(&body, "申购费率").map(parse_fee_rules).unwrap_or_default();
    let redemption =
        extract_box_section(&body, "赎回费率").map(parse_fee_rules).unwrap_or_default();

    Ok(FeeRules { purchase, redemption })
}

/// Most recent quarter-end whose holdings are likely published.
///
/// Fund managers report quarterly holdings ~15 working days after quarter-end;
/// the monthly cutoff used here is conservative and trades a small chance of
/// stale data for guaranteed-non-empty responses. Map:
/// - Jan/Feb       → last year Q3 (Sep)
/// - Mar/Apr       → last year Q4 (Dec)
/// - May/Jun/Jul   → current Q1 (Mar)
/// - Aug/Sep/Oct   → current Q2 (Jun)
/// - Nov/Dec       → current Q3 (Sep)
pub fn latest_quarter_end(year: u32, month: u32) -> (u32, u32) {
    match month {
        1 | 2 => (year - 1, 9),
        3 | 4 => (year - 1, 12),
        5..=7 => (year, 3),
        8..=10 => (year, 6),
        _ => (year, 9),
    }
}

/// Extract the report period text (e.g. "2026年1季度").
/// Prefer the `quarter:"..."` field when present; otherwise scrape the segment
/// from the embedded table header — jjcc responses omit `quarter` entirely.
fn extract_period(body: &str) -> Option<String> {
    if let Some(q) = extract_str_field(body, "quarter") {
        return Some(q.to_string());
    }
    let content = extract_str_field(body, "content")?;
    if !content.contains("年") {
        return None;
    }
    content
        .split("&nbsp;&nbsp;")
        .find(|s| s.contains("季度"))
        .map(|s| strip_html(s).trim().to_string())
}

fn extract_end_date(content: &str) -> Option<String> {
    // `<font class='px12'>2026-03-31</font>` — look for first YYYY-MM-DD pattern.
    let bytes = content.as_bytes();
    let mut i = 0;
    while i + 10 <= bytes.len() {
        let slice = &bytes[i..i + 10];
        if slice[4] == b'-'
            && slice[7] == b'-'
            && slice[0..4].iter().all(|b| b.is_ascii_digit())
            && slice[5..7].iter().all(|b| b.is_ascii_digit())
            && slice[8..10].iter().all(|b| b.is_ascii_digit())
        {
            return Some(String::from_utf8_lossy(slice).to_string());
        }
        i += 1;
    }
    None
}

/// Industry list for a fund: `(industry_code, industry_name)` pairs.
///
/// Eastmoney's `hylx` mixes two taxonomies in one response: the CSRC (证监会)
/// alphabetic letters (A-Z) and the CSI (中证) two-digit GICS-style numerics
/// (10/15/20/.../60). They cover the same exposure twice, so summing both
/// double-counts allocation. We keep only the CSRC scheme — it's the canonical
/// one referenced across the F10 industry pages and matches typical user mental
/// model ("制造业"/"金融业") rather than abstract ("20工业"/"40金融").
pub fn get_industry_list(code: &str) -> Result<Vec<(String, String)>> {
    let url = format!("{}/F10DataApi.aspx?pt=1&type=hylx&code={}&rt=0", F10_BASE, code);
    let body = http_get(&url)?;
    // Parse `hylx:[["A","..."],["B","..."]]` — skip the synthetic "ZZZ"/"合计" row.
    let key = "hylx:[";
    let start = match body.find(key) {
        Some(i) => i + key.len(),
        None => return Ok(Vec::new()),
    };
    let end = match body[start..].rfind("]}") {
        Some(i) => start + i,
        None => return Ok(Vec::new()),
    };
    let raw = &body[start..end];

    let mut out = Vec::new();
    for pair_chunk in raw.split("[").skip(1) {
        let pair_end = match pair_chunk.find(']') {
            Some(i) => &pair_chunk[..i],
            None => continue,
        };
        // pair_end like: `"A","农、林、牧、渔业"`
        let parts: Vec<&str> = pair_end.split(',').collect();
        if parts.len() < 2 {
            continue;
        }
        let code = parts[0].trim().trim_matches('"').to_string();
        let name = parts[1..].join(",").trim().trim_matches('"').to_string();
        if code == "ZZZ" || name == "合计" {
            continue;
        }
        // Drop CSI numeric taxonomy to avoid double counting with CSRC letters.
        if code.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        out.push((code, name));
    }
    Ok(out)
}

/// Industry detail for the latest period. Returns 0% ratio if no data.
/// `industry_name` is left empty here — the hyxq endpoint only echoes the
/// numeric `hydm`, so callers (see `get_active_industries`) must backfill
/// the name from `get_industry_list`.
pub fn get_industry_detail(code: &str, hydm: &str) -> Result<IndustryAllocation> {
    let url =
        format!("{}/F10DataApi.aspx?pt=1&type=hyxq&code={}&hydm={}&rt=0", F10_BASE, code, hydm);
    let body = http_get(&url)?;
    let content = extract_str_field(&body, "content").unwrap_or("");
    let rows = parse_table_rows(content);

    let (ratio, market_value, end_date) = match rows.first() {
        Some(r) if r.len() >= 3 => (parse_pct(&r[1]), parse_num(&r[2]), r[0].clone()),
        _ => (0.0, 0.0, String::new()),
    };

    Ok(IndustryAllocation {
        industry_code: hydm.to_string(),
        industry_name: String::new(),
        ratio,
        market_value_wan: market_value,
        end_date,
    })
}

/// Convenience: fetch industries with non-zero exposure for a fund.
pub fn get_active_industries(code: &str) -> Result<Vec<IndustryAllocation>> {
    let list = get_industry_list(code)?;
    let mut out = Vec::with_capacity(list.len());
    for (hydm, name) in list {
        if let Ok(mut det) = get_industry_detail(code, &hydm) {
            if det.ratio > 0.0 {
                det.industry_name = name;
                out.push(det);
            }
        }
    }
    Ok(out)
}

/// Top bond holdings for a fund at the given quarter-end. Mirrors `get_top_stocks`
/// but reads the `zqcc` (债券持仓) endpoint, whose tbody has 5 columns:
/// 序号 / 债券代码 / 债券名称 / 占净值比例 / 持仓市值(万元).
pub fn get_top_bonds(code: &str, year: u32, month: u32) -> Result<TopBondsReport> {
    let url = format!(
        "{}/FundArchivesDatas.aspx?type=zqcc&code={}&year={}&month={:02}&rt=0",
        F10_BASE, code, year, month
    );
    let body = http_get(&url)?;

    let content = extract_str_field(&body, "content").unwrap_or("");
    let period = extract_period(&body).unwrap_or_default();
    let end_date = extract_end_date(content).unwrap_or_default();
    let rows = parse_table_rows(content);

    let mut bonds = Vec::new();
    for row in rows {
        if row.len() < 5 {
            continue;
        }
        bonds.push(BondHolding {
            bond_code: row[1].clone(),
            bond_name: row[2].clone(),
            ratio: parse_pct(&row[3]),
            market_value_wan: parse_num(&row[4]),
        });
    }

    Ok(TopBondsReport { period, end_date, bonds })
}

/// Historical scale changes (purchase / redemption / total shares / NAV per period).
///
/// Parses the embedded HTML table in `var gmbd_apidata={ content:"<table>...</table>" }`.
/// The same envelope also carries a `data:[...]` JSON array, but the field names there
/// (BZDM/CHANGE/ESEQID/...) drift across Eastmoney versions and the Vercel gateway
/// re-shapes them inconsistently, so the table is the stable source of truth.
///
/// Column layout (matches the visible page):
/// 0 日期 / 1 期间申购(亿份) / 2 期间赎回(亿份) /
/// 3 期末总份额(亿份) / 4 期末净资产(亿元) / 5 净资产变动率(%).
pub fn get_scale_changes(code: &str) -> Result<Vec<ScaleChangePoint>> {
    let url = format!("{}/FundArchivesDatas.aspx?type=gmbd&mode=&code={}&rt=0", F10_BASE, code);
    let body = http_get(&url)?;
    let content = extract_str_field(&body, "content").unwrap_or("");

    let mut out = Vec::new();
    for row in parse_table_rows(content) {
        if row.len() < 6 {
            continue;
        }
        out.push(ScaleChangePoint {
            date: row[0].clone(),
            purchase_yi: parse_num(&row[1]),
            redemption_yi: parse_num(&row[2]),
            end_shares_yi: parse_num(&row[3]),
            end_nav_yi: parse_num(&row[4]),
            change_pct: parse_pct(&row[5]),
        });
    }
    Ok(out)
}

/// Holder structure history: institutional vs retail vs internal share of total shares.
/// Source is the `cyrjg` endpoint whose tbody has 5 columns:
/// 公告日期 / 机构持有比例 / 个人持有比例 / 内部持有比例 / 总份额(亿份).
pub fn get_holder_structure(code: &str) -> Result<Vec<HolderStructurePoint>> {
    let url = format!("{}/FundArchivesDatas.aspx?type=cyrjg&code={}&rt=0", F10_BASE, code);
    let body = http_get(&url)?;
    let content = extract_str_field(&body, "content").unwrap_or("");

    let mut out = Vec::new();
    for row in parse_table_rows(content) {
        if row.len() < 5 {
            continue;
        }
        out.push(HolderStructurePoint {
            announce_date: row[0].clone(),
            institutional_pct: parse_pct(&row[1]),
            retail_pct: parse_pct(&row[2]),
            internal_pct: parse_pct(&row[3]),
            total_shares_yi: parse_num(&row[4]),
        });
    }
    Ok(out)
}

/// Parse the minimum holding period from a fund name or feature blurb.
///
/// Funds with a mandatory holding window encode it in the name in two common
/// patterns: prefix ("90天持有期") or suffix ("持有 6 个月" / "3年封闭运作").
/// Strategy: only run when text contains 持有 / 封闭, then scan every
/// `N(天|日|月|年)` occurrence and return the largest day count.
fn parse_min_holding_days(text: &str) -> Option<u32> {
    if !text.contains("持有") && !text.contains("封闭") {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    let mut best: Option<u32> = None;
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let mut j = i;
            let mut num = 0u32;
            while j < chars.len() && chars[j].is_ascii_digit() {
                num = num.saturating_mul(10).saturating_add((chars[j] as u32) - ('0' as u32));
                j += 1;
            }
            if j < chars.len() && num > 0 {
                let days = match chars[j] {
                    '天' | '日' => Some(num),
                    '月' => Some(num.saturating_mul(30)),
                    '年' => Some(num.saturating_mul(365)),
                    _ => None,
                };
                if let Some(d) = days {
                    best = Some(best.map_or(d, |p| p.max(d)));
                }
            }
            i = j;
            continue;
        }
        i += 1;
    }
    best
}

/// Detect holding-period constraints from a fund's name.
///
/// The F10 概况页 (jbgk) does not expose structured 申购/赎回状态 fields, so the
/// most reliable signal is the fund name itself — funds with mandatory holding
/// periods uniformly declare them in the short/full name (e.g. "90天持有期",
/// "3年封闭运作"). Pure function: no network, no fallible IO.
pub fn detect_holding_constraints(short_name: &str, full_name: &str) -> HoldingConstraints {
    // Prefer the full name (richer phrasing); short name covers the abbreviated case.
    let features =
        if full_name.is_empty() { short_name.to_string() } else { full_name.to_string() };
    let min_holding_days =
        parse_min_holding_days(&features).or_else(|| parse_min_holding_days(short_name));
    HoldingConstraints {
        purchase_status: String::new(),
        redemption_status: String::new(),
        min_holding_days,
        features,
    }
}
