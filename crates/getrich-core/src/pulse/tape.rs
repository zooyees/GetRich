use super::limit;
use super::{Leader, SectorMove, Tape, WatchName, WatchPulse};
use crate::config::{PulseConfig, WatchItem};
use crate::db::PulseScanRow;
use crate::symbol::parse_instrument;

#[derive(Debug, Clone)]
pub struct Classified {
    pub row: PulseScanRow,
    pub pct: Option<f64>,
    pub halted: bool,
    pub limit_pct: Option<f64>,
}

pub fn classify(rows: Vec<PulseScanRow>, trade_date: &str, cfg: &PulseConfig) -> Vec<Classified> {
    rows.into_iter()
        .map(|row| {
            let pct = pct_of(&row);
            let halted = is_halted(&row, pct, cfg.flat_eps_pct);
            let limit_pct = limit::limit_pct(
                row.name.as_deref(),
                &row.market,
                &row.symbol,
                row.listing_date.as_deref(),
                trade_date,
            );
            Classified {
                row,
                pct,
                halted,
                limit_pct,
            }
        })
        .collect()
}

fn pct_of(row: &PulseScanRow) -> Option<f64> {
    if let Some(p) = row.change_pct {
        return Some(p);
    }
    let prev = row.prev_close?;
    if prev == 0.0 {
        return None;
    }
    Some((row.last - prev) / prev * 100.0)
}

fn is_halted(row: &PulseScanRow, pct: Option<f64>, eps: f64) -> bool {
    let vol0 = row.volume.unwrap_or(0.0) == 0.0;
    let last_eq = row
        .prev_close
        .map(|p| (row.last - p).abs() < 1e-9)
        .unwrap_or(false);
    let pct0 = pct.map(|p| p.abs() <= eps).unwrap_or(false);
    vol0 && last_eq && pct0
}

pub fn median(mut xs: Vec<f64>) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = xs.len();
    if n % 2 == 1 {
        Some(xs[n / 2])
    } else {
        Some((xs[n / 2 - 1] + xs[n / 2]) / 2.0)
    }
}

pub fn build_tape(
    classified: &[Classified],
    cfg: &PulseConfig,
    amount_vs_20d: Option<f64>,
) -> (Tape, Vec<SectorMove>, bool, bool) {
    let n_quoted = classified.len() as u32;
    let mut n_up = 0u32;
    let mut n_down = 0u32;
    let mut n_flat = 0u32;
    let mut n_limit_up = 0u32;
    let mut n_limit_down = 0u32;
    let mut pcts = Vec::new();
    let mut amount = 0.0;
    let mut up_amount = 0.0;
    let mut w_num = 0.0;
    let mut w_den = 0.0;
    let mut delist_note = false;
    let mut unlabeled = 0u32;

    for c in classified {
        if let Some(p) = c.pct {
            pcts.push(p);
            if p > cfg.flat_eps_pct {
                n_up += 1;
            } else if p < -cfg.flat_eps_pct {
                n_down += 1;
            } else {
                n_flat += 1;
            }
            let to = c.row.turnover.unwrap_or(0.0);
            amount += to;
            if p > 0.0 {
                up_amount += to;
            }
            w_num += p * to;
            w_den += to;
            if !c.halted {
                if let Some(lim) = c.limit_pct {
                    if p >= lim - cfg.limit_eps_pct {
                        n_limit_up += 1;
                    }
                    if p <= -(lim - cfg.limit_eps_pct) {
                        n_limit_down += 1;
                    }
                }
            }
        } else {
            n_flat += 1;
        }
        if c.row.industry.as_deref().unwrap_or("").trim().is_empty() {
            unlabeled += 1;
        }
        if limit::name_has_delist(c.row.name.as_deref()) {
            delist_note = true;
        }
    }

    let advance_ratio = if n_quoted == 0 {
        None
    } else {
        Some(n_up as f64 / n_quoted as f64)
    };
    let median_pct = median(pcts.clone());
    let mean_pct = if pcts.is_empty() {
        None
    } else {
        Some(pcts.iter().sum::<f64>() / pcts.len() as f64)
    };
    let amount_weighted_pct = if w_den > 0.0 {
        Some(w_num / w_den)
    } else {
        None
    };
    let up_amount_ratio = if amount > 0.0 {
        Some(up_amount / amount)
    } else {
        None
    };
    let amount_vs_20d = match amount_vs_20d {
        Some(m) if m > 0.0 => Some(amount / m),
        _ => None,
    };
    let unlabeled_share = if n_quoted == 0 {
        0.0
    } else {
        unlabeled as f64 / n_quoted as f64
    };

    let sectors = sector_moves(classified, amount, cfg);
    let top5_amount_share = top5_share(&sectors);

    let tape = Tape {
        universe: "CN".into(),
        n_quoted,
        n_up,
        n_down,
        n_flat,
        advance_ratio,
        median_pct,
        mean_pct,
        amount_weighted_pct,
        amount,
        amount_vs_20d,
        up_amount_ratio,
        n_limit_up_est: n_limit_up,
        n_limit_down_est: n_limit_down,
        top5_amount_share,
        max_limit_times: None,
    };
    (tape, sectors, unlabeled_share > 0.40, delist_note)
}

fn sector_moves(
    classified: &[Classified],
    market_amount: f64,
    cfg: &PulseConfig,
) -> Vec<SectorMove> {
    use std::collections::HashMap;
    struct Agg {
        w_num: f64,
        w_den: f64,
        pcts: Vec<f64>,
        amount: f64,
        n: u32,
        leaders: Vec<(f64, String, String, String)>,
    }
    let mut map: HashMap<String, Agg> = HashMap::new();
    for c in classified {
        let name = c
            .row
            .industry
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("未分类")
            .to_string();
        let e = map.entry(name).or_insert(Agg {
            w_num: 0.0,
            w_den: 0.0,
            pcts: Vec::new(),
            amount: 0.0,
            n: 0,
            leaders: Vec::new(),
        });
        e.n += 1;
        let to = c.row.turnover.unwrap_or(0.0);
        e.amount += to;
        if let Some(p) = c.pct {
            e.pcts.push(p);
            e.w_num += p * to;
            e.w_den += to;
            e.leaders.push((
                p,
                c.row.symbol.clone(),
                c.row.market.clone(),
                c.row.name.clone().unwrap_or_else(|| c.row.symbol.clone()),
            ));
        }
    }
    let mut items: Vec<SectorMove> = map
        .into_iter()
        .map(|(name, mut agg)| {
            agg.leaders
                .sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            let (change_pct, unweighted) = if agg.w_den > 0.0 {
                (Some(agg.w_num / agg.w_den), false)
            } else {
                (median(agg.pcts.clone()), true)
            };
            let leaders: Vec<Leader> = agg
                .leaders
                .into_iter()
                .take(cfg.leader_n)
                .map(|(pct, symbol, market, name)| Leader {
                    symbol,
                    market,
                    name,
                    pct,
                })
                .collect();
            let direction = match change_pct {
                Some(p) if p > cfg.flat_eps_pct => "up",
                Some(p) if p < -cfg.flat_eps_pct => "down",
                _ => "flat",
            }
            .to_string();
            SectorMove {
                name,
                change_pct,
                median_pct: median(agg.pcts),
                amount: agg.amount,
                amount_share: if market_amount > 0.0 {
                    Some(agg.amount / market_amount)
                } else {
                    None
                },
                n: agg.n,
                leaders,
                rank: 0,
                rank_change: None,
                direction,
                unweighted,
            }
        })
        .collect();
    items.sort_by(|a, b| {
        b.change_pct
            .unwrap_or(f64::NEG_INFINITY)
            .partial_cmp(&a.change_pct.unwrap_or(f64::NEG_INFINITY))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for (i, s) in items.iter_mut().enumerate() {
        s.rank = (i + 1) as i32;
    }
    items
}

fn top5_share(sectors: &[SectorMove]) -> Option<f64> {
    let mut classified: Vec<&SectorMove> = sectors.iter().filter(|s| s.name != "未分类").collect();
    if classified.is_empty() {
        return None;
    }
    classified.sort_by(|a, b| {
        b.amount
            .partial_cmp(&a.amount)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let n = classified.len().min(5);
    Some(
        classified
            .iter()
            .take(n)
            .filter_map(|s| s.amount_share)
            .sum(),
    )
}

pub fn pick_sector_board(sectors: &[SectorMove], top_n: usize) -> Vec<SectorMove> {
    let real: Vec<SectorMove> = sectors
        .iter()
        .filter(|s| s.name != "未分类")
        .cloned()
        .collect();
    let mut up: Vec<SectorMove> = real
        .iter()
        .filter(|s| s.direction == "up")
        .cloned()
        .collect();
    let mut down: Vec<SectorMove> = real
        .iter()
        .filter(|s| s.direction == "down")
        .cloned()
        .collect();
    up.truncate(top_n);
    down.sort_by(|a, b| {
        a.change_pct
            .unwrap_or(0.0)
            .partial_cmp(&b.change_pct.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    down.truncate(top_n);
    up.extend(down);
    up
}

pub fn apply_rank_change(sectors: &mut [SectorMove], prev: &[(String, i32)]) {
    for s in sectors.iter_mut() {
        if let Some((_, rank)) = prev.iter().find(|(n, _)| n == &s.name) {
            s.rank_change = Some(s.rank - rank);
        }
    }
}

pub fn watch_pulse(
    classified: &[Classified],
    watch: &[WatchItem],
    market_median: Option<f64>,
    min_n: usize,
) -> WatchPulse {
    let mut quoted = Vec::new();
    for w in watch {
        let Ok(key) = parse_instrument(&w.symbol, Some(&w.market)) else {
            continue;
        };
        if !matches!(key.market.as_str(), "CN.SH" | "CN.SZ" | "CN.BJ") {
            continue;
        }
        if let Some(c) = classified
            .iter()
            .find(|c| c.row.symbol == key.symbol && c.row.market == key.market.as_str())
        {
            if let Some(pct) = c.pct {
                quoted.push(WatchName {
                    symbol: c.row.symbol.clone(),
                    market: c.row.market.clone(),
                    name: c
                        .row
                        .name
                        .clone()
                        .or(w.name.clone())
                        .unwrap_or_else(|| c.row.symbol.clone()),
                    pct,
                });
            }
        }
    }
    let n = quoted.len();
    if n < min_n {
        return WatchPulse {
            n: n as u32,
            vs_median_pct: None,
            leaders: Vec::new(),
            laggards: Vec::new(),
            note: Some("自选报价不足".into()),
        };
    }
    quoted.sort_by(|a, b| {
        b.pct
            .partial_cmp(&a.pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let med = median(quoted.iter().map(|q| q.pct).collect());
    let vs = match (med, market_median) {
        (Some(a), Some(b)) => Some(a - b),
        _ => None,
    };
    let leaders = quoted.iter().take(3).cloned().collect();
    let mut lag = quoted.clone();
    lag.sort_by(|a, b| {
        a.pct
            .partial_cmp(&b.pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    lag.truncate(3);
    WatchPulse {
        n: n as u32,
        vs_median_pct: vs,
        leaders,
        laggards: lag,
        note: None,
    }
}
