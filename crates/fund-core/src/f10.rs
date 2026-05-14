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
fn parse_table_rows(html: &str) -> Vec<Vec<String>> {
    let tbody_start = match html.find("<tbody") {
        Some(i) => i,
        None => return Vec::new(),
    };
    let tbody_end =
        html[tbody_start..].find("</tbody>").map(|e| tbody_start + e).unwrap_or(html.len());
    let body = &html[tbody_start..tbody_end];

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
