//! 基金盘中实时估值（同花顺估值接口）。
//!
//! 与统一 `action_name` 入口不同：`fundVarietieValuationDetail` 对债基返回 `null`、
//! 对股基返回盘中分时序列，均不适合单点估值。原东方财富 `fundgz.1234567.com.cn`
//! 已整体 404（2026-10 起），现改为直连同花顺 `gz-fund.10jqka.com.cn` 分时估值接口，
//! 直接用 6 位基金代码查询，债基/股基/指数全类型覆盖。
//!
//! 返回体为 JS 赋值：
//! `vm_fd_<code>='<上一净值日>;<交易时段>|<估值日>~<上一日净值>~<分时点>;<分时点>...'`，
//! 分时点为 `HHMM,估算净值,上一日净值,<未用>`。取最后一个分时点作为当前估值。
//! 该接口不含基金名称，名称另走统一 API 的 `get_fund_brief`。

use anyhow::{anyhow, Context, Result};
use serde::Serialize;

use crate::api::Client;

const GZ_URL: &str =
    "https://gz-fund.10jqka.com.cn/?module=api&controller=index&action=chart&start=0930";

/// 基金盘中实时估值（涨跌幅相对上一交易日收盘净值）。
#[derive(Debug, Clone, Serialize)]
pub struct RealtimeEstimate {
    pub code: String,
    /// 基金简称；名称接口失败时为空字符串（不影响估值本身）。
    pub name: String,
    /// 上一交易日净值日期。
    pub prev_nav_date: String,
    /// 上一交易日单位净值。
    pub prev_nav: f64,
    /// 盘中估算净值（最新分时点）。
    pub est_nav: f64,
    /// 盘中估算涨跌幅 %，相对上一交易日，由 `est_nav / prev_nav - 1` 计算。
    pub est_change_pct: f64,
    /// 估值时间，`YYYY-MM-DD HH:MM`。
    pub est_time: String,
}

/// 拉取单只基金的盘中实时估值。失败（代码无效 / 无估值 / 网络）返回明确错误，不静默回退。
pub fn get_realtime_estimate(code: &str) -> Result<RealtimeEstimate> {
    if code.trim().is_empty() {
        return Err(anyhow!("fund code must not be empty"));
    }
    let url = format!("{GZ_URL}&info=vm_fd_{code}");
    let body = http_get(&url)?;
    let mut est =
        parse_10jqka(code, &body).with_context(|| format!("基金 {code} 实时估值解析失败"))?;
    // Name is cosmetic: a failed lookup must not discard a valid estimate, but is reported.
    est.name = match Client::new().get_fund_brief(code) {
        Ok(brief) => brief.name,
        Err(e) => {
            eprintln!("warning: 基金 {code} 名称获取失败: {e:#}");
            String::new()
        }
    };
    Ok(est)
}

/// 解析同花顺估值返回体（name 留空，由调用方填充）。无效代码 / 货币基金 / 未开盘时
/// 估值段为空（`<日期>~~`）-> 返回明确错误，避免误算。纯函数以便单测（不发网络）。
fn parse_10jqka(code: &str, body: &str) -> Result<RealtimeEstimate> {
    let body = body.trim();
    let payload = match (body.find('\''), body.rfind('\'')) {
        (Some(open), Some(close)) if close > open => &body[open + 1..close],
        _ => return Err(anyhow!("估值返回格式异常: {body:.200}")),
    };
    let (header, data) =
        payload.split_once('|').ok_or_else(|| anyhow!("估值返回缺少 '|' 分隔: {payload:.200}"))?;
    let prev_nav_date = header.split(';').next().unwrap_or_default().to_string();

    let mut parts = data.splitn(3, '~');
    let est_date = parts.next().unwrap_or_default();
    let prev_nav_raw = parts.next().unwrap_or_default();
    let points = parts.next().unwrap_or_default();
    if prev_nav_raw.trim().is_empty() || points.trim().is_empty() {
        return Err(anyhow!("无实时估值数据（代码无效 / 货币基金 / 暂未开盘）"));
    }
    let prev_nav = parse_f64(prev_nav_raw, "prev_nav")?;
    if prev_nav <= 0.0 {
        return Err(anyhow!("上一日净值非法: {prev_nav}"));
    }

    let last = points
        .split(';')
        .rev()
        .find(|p| !p.trim().is_empty())
        .ok_or_else(|| anyhow!("无分时估值点"))?;
    let mut fields = last.split(',');
    let hhmm = fields.next().unwrap_or_default().trim();
    let est_nav = parse_f64(fields.next().unwrap_or_default(), "est_nav")?;
    if hhmm.len() != 4 || !hhmm.bytes().all(|b| b.is_ascii_digit()) {
        return Err(anyhow!("估值时间格式异常: {hhmm:?}"));
    }

    Ok(RealtimeEstimate {
        code: code.to_string(),
        name: String::new(),
        prev_nav_date,
        prev_nav,
        est_nav,
        est_change_pct: (est_nav / prev_nav - 1.0) * 100.0,
        est_time: format!("{est_date} {}:{}", &hhmm[..2], &hhmm[2..]),
    })
}

fn parse_f64(s: &str, field: &str) -> Result<f64> {
    let v = s.trim().parse::<f64>().with_context(|| format!("估值字段 {field} 非法数值: {s:?}"))?;
    // f64::from_str accepts "NaN" / "inf"; reject them before they reach the P&L math.
    if !v.is_finite() {
        return Err(anyhow!("估值字段 {field} 非有限数值: {s:?}"));
    }
    Ok(v)
}

/// 同花顺估值直连（仿 `f10::http_get`：带 UA/超时 + FUND_DEBUG 日志）。
fn http_get(url: &str) -> Result<String> {
    let debug = std::env::var("FUND_DEBUG").is_ok();
    if debug {
        eprintln!("\n[DEBUG] curl -s '{url}'");
    }
    let resp = minreq::get(url)
        .with_header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X) AppleWebKit/537.36")
        .with_timeout(10)
        .send()
        .context("10jqka estimate HTTP request failed")?;
    let body = resp.as_str().context("Failed to read 10jqka estimate response")?.to_string();
    if debug {
        eprintln!("[DEBUG] 10jqka estimate response length: {} bytes", body.len());
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = "vm_fd_000171='2026-10-08;0930-1130,1300-1500|2026-10-09~2.040100000000~\
0930,2.03537,2.040100000000,0.000;1459,2.03350,2.040100000000,0.000;\
1500,2.03365,2.040100000000,0.000'";

    #[test]
    fn parses_last_point_as_estimate() {
        let e = parse_10jqka("000171", VALID).unwrap();
        assert_eq!(e.code, "000171");
        assert_eq!(e.prev_nav_date, "2026-10-08");
        assert!((e.prev_nav - 2.0401).abs() < 1e-9);
        assert!((e.est_nav - 2.03365).abs() < 1e-9);
        assert!((e.est_change_pct - (2.03365 / 2.0401 - 1.0) * 100.0).abs() < 1e-9);
        assert_eq!(e.est_time, "2026-10-09 15:00");
    }

    #[test]
    fn tolerates_trailing_semicolon() {
        let body = format!("{VALID};");
        assert!(parse_10jqka("000171", &body).is_ok());
    }

    #[test]
    fn empty_estimate_is_error() {
        // 无效代码 / 货币基金时估值段为空。
        let body = "vm_fd_999999='2026-10-08;0930-1130,1300-1500|2026-10-09~~'";
        assert!(parse_10jqka("999999", body).is_err());
    }

    #[test]
    fn non_js_body_is_error() {
        assert!(parse_10jqka("1", "<html>404</html>").is_err());
    }

    #[test]
    fn invalid_numeric_field_is_error() {
        let body = "vm_fd_1='2026-10-08;x|2026-10-09~N/A~0930,1.0,1.0,0.000'";
        assert!(parse_10jqka("1", body).is_err());
        let body = "vm_fd_1='2026-10-08;x|2026-10-09~1.0~0930,N/A,1.0,0.000'";
        assert!(parse_10jqka("1", body).is_err());
    }

    #[test]
    fn non_ascii_time_is_error_not_panic() {
        // 4 bytes but not 4 ASCII digits: byte slicing at 2 would panic on a char boundary.
        let body = "vm_fd_1='2026-10-08;x|2026-10-09~1.0~中a,1.0,1.0,0.000'";
        assert!(parse_10jqka("1", body).is_err());
    }

    #[test]
    fn non_finite_nav_is_error() {
        // Rust parses "NaN" / "inf" as valid f64; they must not reach the P&L math.
        let body = "vm_fd_1='2026-10-08;x|2026-10-09~NaN~0930,1.0,1.0,0.000'";
        assert!(parse_10jqka("1", body).is_err());
        let body = "vm_fd_1='2026-10-08;x|2026-10-09~1.0~0930,inf,1.0,0.000'";
        assert!(parse_10jqka("1", body).is_err());
    }
}
