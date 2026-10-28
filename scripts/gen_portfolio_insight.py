# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
"""Generate dist/data/portfolio-insight.json — REAL-ledger portfolio analytics.

Unlike gen_portfolio_daily.py (which backtests today's shares), this reads the
actual per-day positions saved by `fund portfolio --save` (position_daily), so
redemptions, DCA tranches and new buys are reflected exactly as they happened.

Daily P&L of a fund on day t = shares held at close of t-1 × (acc_nav_t − acc_nav_{t-1}).
acc_nav (cumulative NAV) is used so income distributions are not mistaken for
losses. Daily portfolio return = Σ P&L / market value at t-1, chained into a
time-weighted index — this neutralises cash in/out, unlike total_pnl deltas
which jump whenever a position is redeemed (its realised gain leaves the book).

Usage: uv run scripts/gen_portfolio_insight.py [-o dist/data/portfolio-insight.json]
"""
import argparse
import json
import math
import os
import sqlite3
import statistics
from collections import defaultdict
from datetime import date

DB = os.path.expanduser("~/.fund-rs/portfolio.db")
TRADING_DAYS_PER_YEAR = 252


def max_drawdown(index):
    """Return (max_dd_pct, peak_idx, trough_idx) for a value series."""
    peak, peak_i, best = index[0], 0, (0.0, 0, 0)
    for i, v in enumerate(index):
        if v > peak:
            peak, peak_i = v, i
        dd = v / peak - 1
        if dd < best[0]:
            best = (dd, peak_i, i)
    return best[0] * 100, best[1], best[2]


def pearson(a, b):
    ma, mb = statistics.fmean(a), statistics.fmean(b)
    cov = sum((x - ma) * (y - mb) for x, y in zip(a, b))
    va = math.sqrt(sum((x - ma) ** 2 for x in a))
    vb = math.sqrt(sum((y - mb) ** 2 for y in b))
    return cov / (va * vb) if va and vb else 0.0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("-o", "--output", default="dist/data/portfolio-insight.json")
    args = ap.parse_args()

    con = sqlite3.connect(DB)
    try:
        funds = {
            code: {"name": name, "fund_type": ftype or ""}
            for code, name, ftype in con.execute("SELECT code, name, fund_type FROM funds")
        }
        pf_rows = con.execute(
            "SELECT date, total_market_value, total_cash, total_assets, total_cost, total_pnl "
            "FROM portfolio_daily ORDER BY date"
        ).fetchall()
        nav = defaultdict(dict)  # code -> date -> (nav, acc_nav)
        for d, code, n, acc in con.execute("SELECT date, code, nav, acc_nav FROM nav_daily"):
            nav[code][d] = (n, acc if acc is not None else n)
        pos = defaultdict(lambda: defaultdict(lambda: [0.0, 0.0]))  # date -> code -> [shares, mv]
        lots_today = []
        last_date = pf_rows[-1][0]
        for d, code, ch, bd, sh, cnav, mv in con.execute(
            "SELECT date, code, channel, buy_date, shares, cost_nav, market_value "
            "FROM position_daily"
        ):
            pos[d][code][0] += sh
            pos[d][code][1] += mv
            if d == last_date:
                lots_today.append(
                    {"code": code, "channel": ch, "buy_date": bd, "shares": sh,
                     "cost_nav": cnav, "market_value": mv}
                )
    finally:
        con.close()

    dates = [r[0] for r in pf_rows]
    assert len(dates) >= 2, "need at least two saved snapshots"
    codes = sorted({c for d in dates for c in pos[d]})

    # --- daily P&L attribution (real shares × Δacc_nav) ---
    daily_pnl = {c: [0.0] * len(dates) for c in codes}
    port_ret = [0.0]
    for i in range(1, len(dates)):
        prev, cur = dates[i - 1], dates[i]
        prev_mv = sum(v[1] for v in pos[prev].values())
        tot = 0.0
        for c, (sh, _) in pos[prev].items():
            a, b = nav[c].get(prev), nav[c].get(cur)
            if a and b:
                p = sh * (b[1] - a[1])
                daily_pnl[c][i] = p
                tot += p
        port_ret.append(tot / prev_mv if prev_mv else 0.0)

    idx = [100.0]
    for r in port_ret[1:]:
        idx.append(idx[-1] * (1 + r))
    peak, dd_series = idx[0], []
    for v in idx:
        peak = max(peak, v)
        dd_series.append((v / peak - 1) * 100)
    mdd, pk, tr = max_drawdown(idx)

    total_daily = [sum(daily_pnl[c][i] for c in codes) for i in range(len(dates))]
    rets = port_ret[1:]
    vol = statistics.stdev(rets) * math.sqrt(TRADING_DAYS_PER_YEAR) * 100
    span_days = (date.fromisoformat(dates[-1]) - date.fromisoformat(dates[0])).days
    twr = (idx[-1] / 100 - 1) * 100
    ann = ((idx[-1] / 100) ** (365 / span_days) - 1) * 100 if span_days else 0.0

    # --- monthly returns: portfolio (chained) + per fund (acc_nav month-end to month-end) ---
    months = sorted({d[:7] for d in dates})
    monthly_port = []
    for m in months:
        g = 1.0
        for d, r in zip(dates[1:], rets):
            if d[:7] == m:
                g *= 1 + r
        monthly_port.append((g - 1) * 100)
    monthly_pnl = {
        c: [sum(p for d, p in zip(dates, daily_pnl[c]) if d[:7] == m) for m in months]
        for c in codes
    }

    # --- per-fund NAV analytics over full nav history (acc_nav based) ---
    nav_dates = sorted({d for c in codes for d in nav[c]})
    fund_stats, nav_index = {}, {}
    for c in codes:
        ds = [d for d in nav_dates if d in nav[c]]
        acc = [nav[c][d][1] for d in ds]
        r = [acc[i] / acc[i - 1] - 1 for i in range(1, len(acc))]
        fdd, fpk, ftr = max_drawdown(acc)
        span = (date.fromisoformat(ds[-1]) - date.fromisoformat(ds[0])).days
        fund_stats[c] = {
            "from": ds[0], "to": ds[-1], "days": len(ds),
            "return_pct": (acc[-1] / acc[0] - 1) * 100,
            "ann_vol_pct": statistics.stdev(r) * math.sqrt(TRADING_DAYS_PER_YEAR) * 100,
            "max_dd_pct": fdd, "dd_peak": ds[fpk], "dd_trough": ds[ftr],
            "up_ratio": sum(1 for x in r if x > 0) / len(r) * 100,
            "span_days": span,
        }
        nav_index[c] = {"dates": ds, "acc": acc, "nav": [nav[c][d][0] for d in ds]}

    # correlation of daily acc_nav returns over dates where both funds have data
    corr = {}
    for a in codes:
        for b in codes:
            common = [d for d in nav_dates if d in nav[a] and d in nav[b]]
            if len(common) < 20:
                continue
            ra = [nav[a][common[i]][1] / nav[a][common[i - 1]][1] - 1
                  for i in range(1, len(common))]
            rb = [nav[b][common[i]][1] / nav[b][common[i - 1]][1] - 1
                  for i in range(1, len(common))]
            corr[f"{a}|{b}"] = {"r": pearson(ra, rb), "n": len(ra)}

    held_now = sorted(pos[last_date], key=lambda c: -pos[last_date][c][1])
    for lot in lots_today:
        n = nav[lot["code"]][last_date][0]
        cost = lot["shares"] * lot["cost_nav"]
        lot["nav"] = n
        lot["return_pct"] = (n / lot["cost_nav"] - 1) * 100
        lot["pnl_ex_fee"] = lot["market_value"] - cost

    out = {
        "generated_at": dates[-1],
        "dates": dates,
        "codes": codes,
        "held_now": held_now,
        "funds": funds,
        "portfolio": {
            "total_assets": [r[3] for r in pf_rows],
            "total_cost": [r[4] for r in pf_rows],
            "total_pnl": [r[5] for r in pf_rows],
            "daily_return_pct": [r * 100 for r in port_ret],
            "daily_pnl": total_daily,
            "index": idx,
            "drawdown_pct": dd_series,
        },
        "stats": {
            "twr_pct": twr, "ann_return_pct": ann, "ann_vol_pct": vol,
            "max_dd_pct": mdd, "dd_peak": dates[pk], "dd_trough": dates[tr],
            "up_days": sum(1 for r in rets if r > 0),
            "down_days": sum(1 for r in rets if r < 0),
            "best_day": max(zip(total_daily, dates)),
            "worst_day": min(zip(total_daily, dates)),
            "span_days": span_days,
        },
        "fund_daily_pnl": daily_pnl,
        "fund_mv": {c: [pos[d][c][1] if c in pos[d] else 0.0 for d in dates] for c in codes},
        "months": months,
        "monthly_port_pct": monthly_port,
        "monthly_pnl": monthly_pnl,
        "fund_stats": fund_stats,
        "nav_index": nav_index,
        "corr": corr,
        "lots": lots_today,
    }
    os.makedirs(os.path.dirname(args.output), exist_ok=True)
    with open(args.output, "w", encoding="utf-8") as f:
        json.dump(out, f, ensure_ascii=False)
    print(f"✓ wrote {args.output}: {len(dates)} days, {len(codes)} funds, {len(lots_today)} lots")


if __name__ == "__main__":
    main()
