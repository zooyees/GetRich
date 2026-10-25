//! Auditable market heatmap. Geometry, membership and weights are shared by both clients.
use crate::{
    db,
    error::AppError,
    service::App,
    view::{quote_row_from_board, QuoteRow},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MapQuery {
    pub board: String,
    pub metric: String,
    pub industry: String,
    pub aspect: f64,
    pub watch_only: bool,
}
impl Default for MapQuery {
    fn default() -> Self {
        Self {
            board: "CN".into(),
            metric: "auto".into(),
            industry: String::new(),
            aspect: 1.6,
            watch_only: false,
        }
    }
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}
#[derive(Debug, Clone, Serialize)]
pub struct MapTile {
    pub key: String,
    pub quote: QuoteRow,
    pub industry: String,
    pub watched: bool,
    pub weight: f64,
    pub weight_as_of: Option<String>,
    pub weight_source: Option<String>,
    pub bounds: Bounds,
}
#[derive(Debug, Clone, Serialize)]
pub struct MapGroup {
    pub name: String,
    pub weight: f64,
    pub mean_pct: Option<f64>,
    pub bounds: Bounds,
    pub tiles: Vec<MapTile>,
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct MapCoverage {
    pub universe: usize,
    pub quoted: usize,
    pub displayed: usize,
    pub missing_quote: usize,
    pub other_date: usize,
    pub invalid: usize,
    pub missing_weight: usize,
    pub delayed: usize,
    pub unclassified: usize,
    pub up: usize,
    pub down: usize,
    pub flat: usize,
    pub unknown_change: usize,
}
#[derive(Debug, Clone, Serialize)]
pub struct MarketMap {
    pub schema_version: u32,
    pub query: MapQuery,
    pub computed_at: String,
    pub data_version: u64,
    pub trade_date: Option<String>,
    pub historical: bool,
    pub currency: String,
    pub metric: String,
    pub metric_label: String,
    pub classification: String,
    pub coverage: MapCoverage,
    pub notes: Vec<String>,
    pub groups: Vec<MapGroup>,
    pub indices: Vec<MapIndex>,
}
#[derive(Debug, Clone, Serialize)]
pub struct MapIndex {
    pub name: String,
    pub symbol: String,
    pub market: String,
    pub quote: Option<QuoteRow>,
}

#[derive(Clone)]
struct Weight {
    value: f64,
    date: Option<String>,
    source: Option<String>,
}
type Metadata = HashMap<(i64, String), Weight>;

impl App {
    pub fn market_map(&self, mut query: MapQuery) -> Result<MarketMap, AppError> {
        if ![
            "CN", "CN.SH", "CN.SZ", "CN.BJ", "STAR", "CHINEXT", "HK", "US",
        ]
        .contains(&query.board.as_str())
        {
            return Err(AppError::InvalidArg(
                "星图须选择单一币种市场：CN / CN.SH / CN.SZ / CN.BJ / STAR / CHINEXT / HK / US"
                    .into(),
            ));
        }
        if ![
            "auto",
            "float_market_cap",
            "market_cap",
            "turnover",
            "equal",
        ]
        .contains(&query.metric.as_str())
        {
            return Err(AppError::InvalidArg("不支持的星图面积指标".into()));
        }
        if !query.aspect.is_finite() {
            query.aspect = 1.6;
        }
        query.aspect = query.aspect.clamp(0.25, 5.0);
        let read = || -> Result<MarketMap, db::DbError> {
            let watch: HashSet<_> = self
                .watchlist()
                .into_iter()
                .map(|w| format!("{}:{}", w.market, w.symbol))
                .collect();
            let mut conn = self.db.connect()?;
            let tx = conn.transaction()?;
            let rows = db::list_quote_board(
                &tx,
                &db::QuoteBoardQuery {
                    scope: db::BoardScope::parse(&query.board, Vec::new()),
                    limit: 100_000,
                    ..Default::default()
                },
            )?;
            let mut metadata = Metadata::new();
            {
                let mut stmt=tx.prepare("SELECT instrument_id,key,value,source,as_of FROM instrument_metadata WHERE key IN ('float_market_cap','market_cap')")?;
                let entries = stmt.query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, Option<String>>(4)?,
                    ))
                })?;
                for entry in entries {
                    let (id, key, value, source, date) = entry?;
                    if let Some(value) = value
                        .and_then(|v| v.parse::<f64>().ok())
                        .filter(|v| v.is_finite() && *v > 0.0)
                    {
                        metadata.insert(
                            (id, key),
                            Weight {
                                value,
                                date,
                                source,
                            },
                        );
                    }
                }
            }
            let version = db::latest_batch_version(&tx)?;
            let mut indices = Vec::new();
            if !["US", "HK"].contains(&query.board.as_str()) {
                for (symbol, market, name) in [
                    ("000001", "CN.SH", "上证指数"),
                    ("399001", "CN.SZ", "深证成指"),
                    ("399006", "CN.SZ", "创业板指"),
                    ("000688", "CN.SH", "科创50"),
                    ("000300", "CN.SH", "沪深300"),
                ] {
                    let key = crate::models::InstrumentKey::new(
                        symbol,
                        market.parse().map_err(db::DbError::Message)?,
                    );
                    let quote = if let Some(instrument) =
                        db::get_listing(&tx, &key)?.filter(|i| i.instrument_type == "index")
                    {
                        let quote = db::get_quote(&tx, instrument.id)?;
                        quote.map(|quote| {
                            quote_row_from_board(db::QuoteBoardRow {
                                instrument,
                                quote: Some(quote),
                            })
                        })
                    } else {
                        None
                    };
                    let quote = quote.filter(|q| {
                        valid_quote(
                            q,
                            "CNY",
                            &crate::session::iso_date(crate::session::shanghai_today()),
                        )
                    });
                    indices.push(MapIndex {
                        name: name.into(),
                        symbol: symbol.into(),
                        market: market.into(),
                        quote,
                    });
                }
            }
            let currency = match query.board.as_str() {
                "HK" => "HKD",
                "US" => "USD",
                _ => "CNY",
            };
            let today = crate::session::iso_date(crate::session::market_date_at(
                if currency == "USD" { "US" } else { "CN.SH" },
                Utc::now(),
            ));
            let mut coverage = MapCoverage::default();
            let mut candidates = Vec::new();
            for row in rows {
                if row.instrument.instrument_type != "equity" || row.instrument.status != "active" {
                    continue;
                }
                let industry = row
                    .instrument
                    .industry
                    .clone()
                    .filter(|v| !v.trim().is_empty())
                    .unwrap_or_else(|| "未分类".into());
                if !query.industry.is_empty() && industry != query.industry {
                    continue;
                }
                let key = format!(
                    "{}:{}",
                    row.instrument.key.market, row.instrument.key.symbol
                );
                if query.watch_only && !watch.contains(&key) {
                    continue;
                }
                coverage.universe += 1;
                let quote = quote_row_from_board(row);
                if !quote.has_quote {
                    coverage.missing_quote += 1;
                    continue;
                }
                if !valid_quote(&quote, currency, &today) {
                    coverage.invalid += 1;
                    continue;
                }
                candidates.push((key, industry, quote));
            }
            let date = candidates
                .iter()
                .filter_map(|(_, _, q)| q.trade_date.clone())
                .max();
            candidates.retain(|(_, _, q)| {
                let keep = q.trade_date == date;
                if !keep {
                    coverage.other_date += 1;
                }
                keep
            });
            coverage.quoted = candidates.len();
            let current_cap = |q: &QuoteRow, key: &str| {
                metadata
                    .get(&(q.instrument_id, key.into()))
                    .filter(|v| v.date == date)
            };
            let metric = if query.metric == "auto" {
                let n = candidates.len();
                if n > 0
                    && candidates
                        .iter()
                        .filter(|(_, _, q)| current_cap(q, "float_market_cap").is_some())
                        .count()
                        * 10
                        >= n * 9
                {
                    "float_market_cap"
                } else if n > 0
                    && candidates
                        .iter()
                        .filter(|(_, _, q)| positive(q.turnover).is_some())
                        .count()
                        * 10
                        >= n * 9
                {
                    "turnover"
                } else {
                    "equal"
                }
            } else {
                &query.metric
            }
            .to_string();
            let label = match metric.as_str() {
                "float_market_cap" => "流通市值",
                "market_cap" => "总市值",
                "turnover" => "成交额",
                _ => "等面积",
            }
            .to_string();
            let mut grouped = BTreeMap::<String, Vec<MapTile>>::new();
            let mut sources = HashSet::new();
            for (key, industry, quote) in candidates {
                let weight = match metric.as_str() {
                    "float_market_cap" | "market_cap" => current_cap(&quote, &metric).cloned(),
                    "turnover" => positive(quote.turnover).map(|value| Weight {
                        value,
                        date: quote.trade_date.clone(),
                        source: quote.source.clone(),
                    }),
                    _ => Some(Weight {
                        value: 1.0,
                        date: None,
                        source: None,
                    }),
                };
                let Some(weight) = weight else {
                    coverage.missing_weight += 1;
                    continue;
                };
                if quote.data_status == "报价延迟" {
                    coverage.delayed += 1;
                }
                match quote.change_pct {
                    Some(v) if v > 0.0 => coverage.up += 1,
                    Some(v) if v < 0.0 => coverage.down += 1,
                    Some(_) => coverage.flat += 1,
                    None => coverage.unknown_change += 1,
                }
                if industry == "未分类" {
                    coverage.unclassified += 1;
                }
                if let Some(source) = &quote.source {
                    sources.insert(source.clone());
                }
                grouped.entry(industry.clone()).or_default().push(MapTile {
                    watched: watch.contains(&key),
                    key,
                    industry,
                    quote,
                    weight: weight.value,
                    weight_as_of: weight.date,
                    weight_source: weight.source,
                    bounds: Bounds::default(),
                });
            }
            let mut groups: Vec<_> = grouped
                .into_iter()
                .map(|(name, mut tiles)| {
                    tiles.sort_by(|a, b| b.weight.total_cmp(&a.weight).then(a.key.cmp(&b.key)));
                    let changes: Vec<_> = tiles.iter().filter_map(|t| t.quote.change_pct).collect();
                    MapGroup {
                        name,
                        weight: tiles.iter().map(|t| t.weight).sum(),
                        mean_pct: (!changes.is_empty())
                            .then(|| changes.iter().sum::<f64>() / changes.len() as f64),
                        bounds: Bounds::default(),
                        tiles,
                    }
                })
                .collect();
            groups.sort_by(|a, b| b.weight.total_cmp(&a.weight).then(a.name.cmp(&b.name)));
            let group_bounds = layout(
                &groups.iter().map(|g| g.weight).collect::<Vec<_>>(),
                query.aspect,
            );
            for (group, bounds) in groups.iter_mut().zip(group_bounds) {
                group.bounds = bounds;
                // Geometry is local to each industry. Clients reserve a fixed-height industry header.
                let aspect =
                    (bounds.w * query.aspect / bounds.h.max(0.000001)).clamp(0.001, 1000.0);
                let positions = layout(
                    &group.tiles.iter().map(|t| t.weight).collect::<Vec<_>>(),
                    aspect,
                );
                for (tile, bounds) in group.tiles.iter_mut().zip(positions) {
                    tile.bounds = bounds;
                }
            }
            coverage.displayed = groups.iter().map(|g| g.tiles.len()).sum();
            let historical = date.as_deref() != Some(today.as_str());
            let mut notes=vec!["范围仅为本地已导入的在市股票，覆盖率不代表全市场完整率。行业沿用来源分类，非申万二级行业。".into(),"只显示同一交易日的有效报价；缺失涨跌幅以灰色表示，行业涨跌为有效成分的等权均值。".into()];
            if query.metric == "auto" {
                notes.push(format!(
                    "自动面积当前使用{label}；至少 90% 同日样本具备流通市值时优先使用市值。"
                ));
            }
            if coverage.missing_weight > 0 {
                notes.push(format!(
                    "{} 只缺少同日有效{label}，未参与面积计算；可切换等面积查看。",
                    coverage.missing_weight
                ));
            }
            if historical {
                notes.push("历史截面：当前颜色和价格不代表实时行情。".into());
            }
            if sources.iter().any(|s| {
                s.to_lowercase().contains("demo") || s.to_lowercase().contains("synthetic")
            }) {
                notes.push("包含演示数据，仅用于功能验证。".into());
            }
            if coverage.universe >= 100_000 {
                notes.push("触及读取上限，当前范围可能不完整。".into());
            }
            tx.commit()?;
            Ok(MarketMap {
                schema_version: 1,
                query,
                computed_at: Utc::now().to_rfc3339(),
                data_version: version,
                trade_date: date,
                historical,
                currency: currency.into(),
                metric,
                metric_label: label,
                classification: "来源行业分类".into(),
                coverage,
                notes,
                groups,
                indices,
            })
        };
        read().map_err(Into::into)
    }
}
fn positive(value: Option<f64>) -> Option<f64> {
    value.filter(|v| v.is_finite() && *v > 0.0)
}
fn valid_quote(q: &QuoteRow, currency: &str, today: &str) -> bool {
    let Some(last) = q.last else {
        return false;
    };
    let Ok(market) = q.market.parse() else {
        return false;
    };
    let Some(ts) =
        q.ts.as_deref()
            .and_then(|t| crate::session::parse_cn_ts(t).ok())
    else {
        return false;
    };
    let snapshot = crate::models::QuoteSnapshot {
        key: crate::models::InstrumentKey::new(&q.symbol, market),
        ts,
        last,
        open: q.open,
        high: q.high,
        low: q.low,
        prev_close: q.prev_close,
        volume: q.volume,
        turnover: q.turnover,
        change_pct: q.change_pct,
        currency: q.currency.clone(),
        source: q.source.clone().unwrap_or_default(),
    };
    positive(q.last).is_some()
        && crate::validation::quote(&snapshot).is_ok()
        && (q.source_kind == "daily"
            || q.ts
                .as_deref()
                .and_then(|t| crate::session::parse_cn_ts(t).ok())
                .is_some_and(|t| t <= Utc::now() + chrono::Duration::minutes(1)))
        && q.currency.as_deref() == Some(currency)
        && q.trade_date
            .as_deref()
            .is_some_and(|d| d <= today && chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok())
        && q.change_pct.is_none_or(|v| v.is_finite() && v >= -100.0)
}

/// Squarified treemap in a normalized rectangle; input order is preserved.
/// Scale before summation to keep even extreme financial weights finite.
pub fn layout(weights: &[f64], aspect: f64) -> Vec<Bounds> {
    let mut result = vec![Bounds::default(); weights.len()];
    let max = weights
        .iter()
        .copied()
        .filter(|v| v.is_finite() && *v > 0.0)
        .fold(0.0, f64::max);
    if max == 0.0 {
        return result;
    }
    let aspect = if aspect.is_finite() {
        aspect.clamp(0.001, 1000.0)
    } else {
        1.0
    };
    let scaled: Vec<f64> = weights
        .iter()
        .map(|v| {
            if v.is_finite() && *v > 0.0 {
                v / max
            } else {
                0.0
            }
        })
        .collect();
    let sum: f64 = scaled.iter().sum();
    let areas: Vec<(usize, f64)> = scaled
        .iter()
        .enumerate()
        .filter(|(_, v)| **v > 0.0)
        .map(|(i, v)| (i, v / sum * aspect))
        .collect();
    let mut remaining = Bounds {
        x: 0.0,
        y: 0.0,
        w: aspect,
        h: 1.0,
    };
    let mut start = 0;
    while start < areas.len() {
        let side = remaining.w.min(remaining.h).max(f64::MIN_POSITIVE);
        let mut end = start + 1;
        let mut total = areas[start].1;
        let mut min = total;
        let mut max = total;
        let score = |sum: f64, min: f64, max: f64| {
            (side * side * max / (sum * sum)).max(sum * sum / (side * side * min))
        };
        let mut worst = score(total, min, max);
        while end < areas.len() {
            let value = areas[end].1;
            let next = score(total + value, min.min(value), max.max(value));
            if next > worst {
                break;
            }
            total += value;
            min = min.min(value);
            max = max.max(value);
            worst = next;
            end += 1;
        }
        let vertical = remaining.w >= remaining.h;
        let thickness = if vertical {
            total / remaining.h
        } else {
            total / remaining.w
        };
        let mut offset = 0.0;
        for &(index, area) in &areas[start..end] {
            let length = area / thickness;
            let b = if vertical {
                Bounds {
                    x: remaining.x,
                    y: remaining.y + offset,
                    w: thickness,
                    h: length,
                }
            } else {
                Bounds {
                    x: remaining.x + offset,
                    y: remaining.y,
                    w: length,
                    h: thickness,
                }
            };
            result[index] = Bounds {
                x: b.x / aspect,
                w: b.w / aspect,
                ..b
            };
            offset += length;
        }
        if vertical {
            remaining.x += thickness;
            remaining.w = (remaining.w - thickness).max(0.0);
        } else {
            remaining.y += thickness;
            remaining.h = (remaining.h - thickness).max(0.0);
        }
        start = end;
    }
    result
}

/// Diverging colors used by both clients; missing values are distinct from zero.
pub fn tile_rgb(pct: Option<f64>, green_up: bool) -> [u8; 3] {
    let Some(pct) = pct.filter(|v| v.is_finite()) else {
        return [55, 65, 81];
    };
    let neutral = [46.0, 54.0, 68.0];
    let positive = if green_up {
        [18.0, 156.0, 110.0]
    } else {
        [233.0, 66.0, 83.0]
    };
    let negative = if green_up {
        [233.0, 66.0, 83.0]
    } else {
        [18.0, 156.0, 110.0]
    };
    let target = if pct >= 0.0 { positive } else { negative };
    let t = (pct.abs() / 5.0).clamp(0.0, 1.0).sqrt();
    std::array::from_fn(|i| (neutral[i] + (target[i] - neutral[i]) * t).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn area_and_bounds_at_market_scale() {
        let weights: Vec<_> = (1..=6001).rev().map(|i| (i * i) as f64).collect();
        for aspect in [0.3, 1.0, 2.8] {
            let rects = layout(&weights, aspect);
            let total: f64 = weights.iter().sum();
            for (rect, weight) in rects.iter().zip(&weights) {
                assert!(rect.w > 0.0 && rect.h > 0.0 && rect.x >= -1e-8 && rect.y >= -1e-8);
                assert!(rect.x + rect.w <= 1.0 + 1e-8 && rect.y + rect.h <= 1.0 + 1e-8);
                assert!((rect.w * rect.h - weight / total).abs() < 1e-8);
            }
            assert!((rects.iter().map(|r| r.w * r.h).sum::<f64>() - 1.0).abs() < 1e-8);
        }
    }
    #[test]
    fn no_overlaps_and_invalid_weights() {
        let rects = layout(&[9.0, 8.0, 5.0, 4.0, 2.0, 1.0, f64::NAN, -1.0, 0.0], 1.7);
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                assert!(
                    (a.x + a.w).min(b.x + b.w) - a.x.max(b.x) < 1e-8
                        || (a.y + a.h).min(b.y + b.h) - a.y.max(b.y) < 1e-8
                );
            }
        }
        assert_eq!(rects[6].w, 0.0);
        assert!(layout(&[f64::MAX, f64::MAX / 2.0], 1.0)
            .iter()
            .all(|r| r.w.is_finite()));
    }
    #[test]
    fn unknown_is_not_flat_and_color_preference_is_symmetric() {
        assert_ne!(tile_rgb(None, false), tile_rgb(Some(0.0), false));
        assert_eq!(tile_rgb(Some(4.0), false), tile_rgb(Some(-4.0), true));
    }
}
