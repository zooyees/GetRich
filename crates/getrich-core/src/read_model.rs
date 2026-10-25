//! Stable sorting shared by merged watchlists and read projections.
use crate::view::QuoteRow;
use std::cmp::Ordering;

pub fn sort_rows(rows: &mut [QuoteRow], sort: &str) {
    let ascending = sort.ends_with("_asc");
    rows.sort_by(|a, b| {
        let tie = || a.market.cmp(&b.market).then(a.symbol.cmp(&b.symbol));
        let field = sort.trim_end_matches("_asc");
        let numeric = match field {
            "last" => Some((a.last, b.last)),
            "change_amt" => Some((a.change, b.change)),
            "change_pct" => Some((a.change_pct, b.change_pct)),
            "turnover" => Some((a.turnover, b.turnover)),
            "volume" => Some((a.volume, b.volume)),
            "amplitude" => Some((a.amplitude, b.amplitude)),
            _ => None,
        };
        if let Some((left, right)) = numeric {
            return match (left, right) {
                (Some(x), Some(y)) => {
                    let cmp = x.total_cmp(&y);
                    (if ascending { cmp } else { cmp.reverse() }).then_with(tie)
                }
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                _ => tie(),
            };
        }
        match sort {
            "name" => a
                .name
                .as_deref()
                .unwrap_or(&a.symbol)
                .cmp(b.name.as_deref().unwrap_or(&b.symbol))
                .then_with(tie),
            "name_desc" => b
                .name
                .as_deref()
                .unwrap_or(&b.symbol)
                .cmp(a.name.as_deref().unwrap_or(&a.symbol))
                .then_with(tie),
            "symbol_desc" => tie().reverse(),
            _ => tie(),
        }
    });
}
