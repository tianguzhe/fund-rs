//! 基金盘中实时估值（fundgz 接口）。
//!
//! 与统一 `action_name` 入口不同：`fundVarietieValuationDetail` 对债基返回 `null`、
//! 对股基返回盘中分时序列，均不适合单点估值。这里直连东方财富经典估值接口
//! `fundgz.1234567.com.cn/js/<code>.js`，一次返回完整字段，债基/股基/指数全类型覆盖。
//! 返回体为 `jsonpgz({ ... });` 的 JSONP 包裹，剥壳后是标准 JSON。

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

const GZ_BASE: &str = "https://fundgz.1234567.com.cn/js";

/// 基金盘中实时估值（涨跌幅相对上一交易日收盘净值）。
#[derive(Debug, Clone, Serialize)]
pub struct RealtimeEstimate {
    pub code: String,
    pub name: String,
    /// 上一交易日净值日期 (jzrq)。
    pub prev_nav_date: String,
    /// 上一交易日单位净值 (dwjz)。
    pub prev_nav: f64,
    /// 盘中估算净值 (gsz)。
    pub est_nav: f64,
    /// 盘中估算涨跌幅 %，相对上一交易日 (gszzl)。
    pub est_change_pct: f64,
    /// 估值时间 (gztime)。
    pub est_time: String,
}

/// 拉取单只基金的盘中实时估值。失败（代码无效 / 无估值 / 网络）返回明确错误，不静默回退。
pub fn get_realtime_estimate(code: &str) -> Result<RealtimeEstimate> {
    if code.trim().is_empty() {
        return Err(anyhow!("fund code must not be empty"));
    }
    let url = format!("{GZ_BASE}/{code}.js");
    let body = http_get(&url)?;
    parse_jsonpgz(&body).with_context(|| format!("基金 {code} 实时估值解析失败"))
}

/// fundgz 原始返回字段（全为字符串）。
#[derive(Deserialize)]
struct GzPayload {
    fundcode: String,
    name: String,
    jzrq: String,
    dwjz: String,
    gsz: String,
    gszzl: String,
    gztime: String,
}

/// 剥离 `jsonpgz( ... )` 包裹并解析。无效代码时 fundgz 返回空参数包裹（如 `jsonpgz();`），
/// 此时中间内容为空 -> 返回明确错误，避免误算。提取为纯函数以便单测（不发网络）。
fn parse_jsonpgz(body: &str) -> Result<RealtimeEstimate> {
    let body = body.trim();
    // Tolerate trailing semicolon and any wrapper name: take content between the
    // first '(' and the last ')'.
    let json = match (body.find('('), body.rfind(')')) {
        (Some(open), Some(close)) if close > open => body[open + 1..close].trim(),
        _ => "",
    };
    if json.is_empty() {
        return Err(anyhow!("无实时估值数据（代码无效 / 货币基金 / 暂未开盘）"));
    }

    let raw: GzPayload = serde_json::from_str(json)
        .with_context(|| format!("解析 fundgz 估值 JSON 失败: {json}"))?;

    Ok(RealtimeEstimate {
        code: raw.fundcode,
        name: raw.name,
        prev_nav_date: raw.jzrq,
        prev_nav: parse_f64(&raw.dwjz, "dwjz")?,
        est_nav: parse_f64(&raw.gsz, "gsz")?,
        est_change_pct: parse_f64(&raw.gszzl, "gszzl")?,
        est_time: raw.gztime,
    })
}

fn parse_f64(s: &str, field: &str) -> Result<f64> {
    s.trim().parse::<f64>().with_context(|| format!("估值字段 {field} 非法数值: {s:?}"))
}

/// fundgz 直连（仿 `f10::http_get`：带 UA/Referer/超时 + FUND_DEBUG 日志）。
fn http_get(url: &str) -> Result<String> {
    let debug = std::env::var("FUND_DEBUG").is_ok();
    if debug {
        eprintln!("\n[DEBUG] curl -s '{url}'");
    }
    let resp = minreq::get(url)
        .with_header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X) AppleWebKit/537.36")
        .with_header("Referer", "https://fund.eastmoney.com/")
        .with_timeout(10)
        .send()
        .context("fundgz HTTP request failed")?;
    let body = resp.as_str().context("Failed to read fundgz response")?.to_string();
    if debug {
        eprintln!("[DEBUG] fundgz response: {body}");
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_jsonpgz() {
        let body = r#"jsonpgz({"fundcode":"161725","name":"招商中证白酒指数(LOF)A","jzrq":"2026-05-29","dwjz":"0.5866","gsz":"0.5803","gszzl":"-1.07","gztime":"2026-06-01 13:53"});"#;
        let e = parse_jsonpgz(body).unwrap();
        assert_eq!(e.code, "161725");
        assert_eq!(e.name, "招商中证白酒指数(LOF)A");
        assert_eq!(e.prev_nav_date, "2026-05-29");
        assert!((e.prev_nav - 0.5866).abs() < 1e-9);
        assert!((e.est_nav - 0.5803).abs() < 1e-9);
        assert!((e.est_change_pct - (-1.07)).abs() < 1e-9);
        assert_eq!(e.est_time, "2026-06-01 13:53");
    }

    #[test]
    fn parses_without_trailing_semicolon() {
        let body = r#"jsonpgz({"fundcode":"1","name":"x","jzrq":"d","dwjz":"1.0","gsz":"1.1","gszzl":"10.0","gztime":"t"})"#;
        assert!(parse_jsonpgz(body).is_ok());
    }

    #[test]
    fn empty_wrapper_is_error() {
        // 无效代码时 fundgz 返回空参数包裹。
        assert!(parse_jsonpgz("jsonpgz();").is_err());
        assert!(parse_jsonpgz("jsonpgz()").is_err());
    }

    #[test]
    fn non_jsonp_body_is_error() {
        assert!(parse_jsonpgz("<html>404</html>").is_err());
    }

    #[test]
    fn invalid_numeric_field_is_error() {
        let body = r#"jsonpgz({"fundcode":"1","name":"x","jzrq":"d","dwjz":"N/A","gsz":"1","gszzl":"1","gztime":"t"});"#;
        assert!(parse_jsonpgz(body).is_err());
    }
}
