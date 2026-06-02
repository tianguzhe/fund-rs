#!/usr/bin/env python3
"""Generate dist/data/portfolio-daily.json — a SIMULATED daily P&L curve.

Backtests the CURRENT holdings (share counts from holdings.json) against the
historical NAVs stored in nav_daily, i.e. "what if I had always held today's
shares". It does NOT reconstruct real historical positions (016816 was sold,
420002 was redeemed, 000171 was bought across 6 DCA tranches), so the curve
reflects the RETURN SHAPE of the current portfolio under past NAVs — not the
real account's day-by-day P&L. The HTML surfaces this caveat prominently.

Returns are computed from acc_nav (cumulative NAV), NOT unit nav: bond funds
distribute income by cutting unit nav while acc_nav is unchanged, which would
otherwise show a fake single-day crash. This matches the project's return-basis
discipline (see memory: feedback_annualized_return_trap).
"""
import argparse
import json
import os
import sqlite3
import statistics
import sys
from collections import defaultdict

DB = os.path.expanduser("~/.fund-rs/portfolio.db")
TRADING_DAYS_PER_YEAR = 252


def load_shares(holdings_path):
    """Aggregate total shares per fund code (merging channels and DCA lots)."""
    with open(holdings_path, encoding="utf-8") as f:
        cfg = json.load(f)
    shares = defaultdict(float)
    names = {}
    for lots in cfg.get("holdings", {}).values():
        for lot in lots:
            shares[lot["code"]] += float(lot["shares"])
            names.setdefault(lot["code"], lot.get("name", lot["code"]))
    return dict(shares), names


def load_navs(codes):
    """date -> {code: (nav, acc_nav)}; acc_nav falls back to nav when NULL."""
    con = sqlite3.connect(DB)
    try:
        placeholders = ",".join("?" * len(codes))
        rows = con.execute(
            f"SELECT date, code, nav, acc_nav FROM nav_daily WHERE code IN ({placeholders})",
            list(codes),
        ).fetchall()
    finally:
        con.close()
    by_date = defaultdict(dict)
    for date, code, nav, acc in rows:
        by_date[date][code] = (nav, acc if acc is not None else nav)
    return by_date


def main():
    ap = argparse.ArgumentParser(description="Generate simulated portfolio daily-return JSON")
    ap.add_argument("--holdings", default=os.path.expanduser("~/.fund-rs/holdings.json"))
    ap.add_argument("--out", default="dist/data/portfolio-daily.json")
    # Timestamp is passed in (not read from the clock) so output is reproducible.
    ap.add_argument("--generated", default="", help="ISO timestamp recorded in meta")
    args = ap.parse_args()

    shares, names = load_shares(args.holdings)
    codes = sorted(shares)
    if not codes:
        sys.exit("no holdings found")
    by_date = load_navs(codes)

    # Intersection: keep only dates where EVERY fund has a NAV, so the combined
    # market value never jumps from a single fund's missing day.
    dates = sorted(d for d, m in by_date.items() if len(m) == len(codes))
    if len(dates) < 2:
        sys.exit("not enough overlapping trading days to build a series")

    acc0 = sum(by_date[dates[0]][c][1] * shares[c] for c in codes)
    series = []
    prev_acc = None
    for d in dates:
        m = by_date[d]
        mv = sum(m[c][0] * shares[c] for c in codes)   # nominal value (unit nav)
        acc = sum(m[c][1] * shares[c] for c in codes)  # return basis (cumulative nav)
        daily = (acc / prev_acc - 1) if prev_acc else 0.0
        series.append({
            "date": d,
            "market_value": round(mv, 2),
            "daily_return_pct": round(daily * 100, 4),
            "cum_return_pct": round((acc / acc0 - 1) * 100, 4),
        })
        prev_acc = acc

    # Stats exclude day 0 (its daily return is a placeholder 0).
    rets = [s["daily_return_pct"] for s in series[1:]]
    up = sum(1 for r in rets if r > 0)
    down = sum(1 for r in rets if r < 0)
    best = max(series[1:], key=lambda s: s["daily_return_pct"])
    worst = min(series[1:], key=lambda s: s["daily_return_pct"])
    vol = (
        statistics.stdev([r / 100 for r in rets]) * (TRADING_DAYS_PER_YEAR ** 0.5) * 100
        if len(rets) > 1 else 0.0
    )

    last, first = dates[-1], dates[0]
    funds = []
    for c in codes:
        nav_last, acc_last = by_date[last][c]
        _, acc_first = by_date[first][c]
        gain = (acc_last - acc_first) * shares[c]  # this fund's contribution to acc gain
        funds.append({
            "code": c,
            "name": names[c],
            "shares": round(shares[c], 2),
            "latest_nav": nav_last,
            "period_pct": round((acc_last / acc_first - 1) * 100, 2),
            "market_value": round(nav_last * shares[c], 2),
            "contribution_pct": round(gain / acc0 * 100, 4),
        })
    funds.sort(key=lambda x: -x["market_value"])

    out = {
        "meta": {
            "from": first,
            "to": last,
            "basis": "simulated_current_shares",
            "generated": args.generated,
            "funds": [{"code": f["code"], "name": f["name"], "shares": f["shares"]} for f in funds],
            "disclaimer": (
                "模拟回溯：基于当前持仓份额 × 历史净值，未还原历史调仓"
                "（016816 清仓 / 420002 赎回 / 000171 分批 DCA），≠ 真实账户每日盈亏。"
                "收益按累计净值(acc_nav)口径计算，规避债基分红日假摔。"
            ),
        },
        "series": series,
        "stats": {
            "trading_days": len(series),
            "cum_return_pct": series[-1]["cum_return_pct"],
            "best_day": {"date": best["date"], "pct": best["daily_return_pct"]},
            "worst_day": {"date": worst["date"], "pct": worst["daily_return_pct"]},
            "up_days": up,
            "down_days": down,
            "win_rate_pct": round(up / len(rets) * 100, 1) if rets else 0.0,
            "ann_volatility_pct": round(vol, 2),
            "current_market_value": series[-1]["market_value"],
        },
        "funds": funds,
    }

    os.makedirs(os.path.dirname(args.out), exist_ok=True)
    with open(args.out, "w", encoding="utf-8") as f:
        json.dump(out, f, ensure_ascii=False, indent=2)
    print(
        f"wrote {args.out}: {len(series)} days {first}~{last}, "
        f"cum {series[-1]['cum_return_pct']:.2f}%, "
        f"mv {series[-1]['market_value']:,.0f}"
    )


if __name__ == "__main__":
    main()
