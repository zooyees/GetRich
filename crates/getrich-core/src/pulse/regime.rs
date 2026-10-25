use super::{IndexPrint, Regime, Tape};
use crate::config::PulseConfig;

pub fn classify(
    tape: &Tape,
    cfg: &PulseConfig,
    indices: &[IndexPrint],
    calendar_fallback: bool,
    quotes_before_open: bool,
    unlabeled_heavy: bool,
    has_sectors: bool,
) -> Regime {
    if tape.n_quoted == 0 || tape.n_quoted < cfg.min_quoted {
        return Regime {
            code: "unknown".into(),
            label: "样本不足".into(),
            confidence: "low".into(),
            reasons: vec![format!("有效报价 {} 只，低于阈值", tape.n_quoted)],
            subtypes: Vec::new(),
        };
    }

    let advance = tape.advance_ratio.unwrap_or(0.0);
    let median = tape.median_pct.unwrap_or(0.0);
    let vs20 = tape.amount_vs_20d;
    let index_pct = indices
        .iter()
        .find(|i| i.ts_code == "000001.SH")
        .map(|i| i.change_pct);

    if tape.n_limit_down_est >= cfg.panic_limit_down
        && advance < 0.35
        && vs20.map(|r| r >= cfg.amount_hot).unwrap_or(false)
    {
        return finish(
            "panic",
            "恐慌抛售",
            vec![format!(
                "跌停估算 {} 家，上涨占比 {:.0}%，额为 20 日 ×{:.1}",
                tape.n_limit_down_est,
                advance * 100.0,
                vs20.unwrap_or(0.0)
            )],
            Vec::new(),
            calendar_fallback,
            quotes_before_open,
            unlabeled_heavy,
            has_sectors,
            vs20.is_none(),
            indices.is_empty(),
        );
    }

    let abs_ref = index_pct.unwrap_or(median).abs();
    let crowded = tape.top5_amount_share.unwrap_or(0.0) >= cfg.theme_top5_share;
    let ladder = tape.max_limit_times.unwrap_or(0) >= cfg.theme_height;
    if (ladder || crowded) && abs_ref < 0.8 {
        let mut subtypes = Vec::new();
        if ladder {
            subtypes.push("ladder".into());
        }
        if crowded {
            subtypes.push("crowded".into());
        }
        return finish(
            "theme",
            "题材结构市",
            vec![format!(
                "连板高度 {} / 前5行业额占比 {:.0}%，指数波动不大",
                tape.max_limit_times.unwrap_or(0),
                tape.top5_amount_share.unwrap_or(0.0) * 100.0
            )],
            subtypes,
            calendar_fallback,
            quotes_before_open,
            unlabeled_heavy,
            has_sectors,
            vs20.is_none(),
            indices.is_empty(),
        );
    }

    let hollow = if let Some(ip) = index_pct {
        (ip - median).abs() > cfg.hollow_gap_pct
    } else if let Some(aw) = tape.amount_weighted_pct {
        (aw - median).abs() > cfg.hollow_gap_pct_no_index
    } else {
        false
    };
    if hollow {
        let idx_s = index_pct
            .map(|p| format!("上证 {:+.1}%", p))
            .unwrap_or_else(|| "指数缺省".into());
        let gap = index_pct
            .map(|ip| (ip - median).abs())
            .or_else(|| tape.amount_weighted_pct.map(|aw| (aw - median).abs()))
            .unwrap_or(0.0);
        return finish(
            "hollow",
            "指数失真",
            vec![format!(
                "{}，个股中位 {:+.1}%，差 {:.1} 个百分点",
                idx_s, median, gap
            )],
            Vec::new(),
            calendar_fallback,
            quotes_before_open,
            unlabeled_heavy,
            has_sectors,
            vs20.is_none(),
            indices.is_empty(),
        );
    }

    if advance >= cfg.beta_advance_ratio && vs20.map(|r| r >= 1.00).unwrap_or(false) && median > 0.0
    {
        return finish(
            "beta_up",
            "普涨风险偏好",
            vec![format!(
                "上涨占比 {:.0}%，中位 {:+.1}%，额 ×{:.1}",
                advance * 100.0,
                median,
                vs20.unwrap_or(0.0)
            )],
            Vec::new(),
            calendar_fallback,
            quotes_before_open,
            unlabeled_heavy,
            has_sectors,
            vs20.is_none(),
            indices.is_empty(),
        );
    }

    if advance <= cfg.beta_down_advance_ratio
        && median < 0.0
        && vs20.map(|r| r >= 0.90).unwrap_or(false)
    {
        return finish(
            "beta_down",
            "普跌风险规避",
            vec![format!(
                "上涨占比 {:.0}%，中位 {:+.1}%",
                advance * 100.0,
                median
            )],
            Vec::new(),
            calendar_fallback,
            quotes_before_open,
            unlabeled_heavy,
            has_sectors,
            vs20.is_none(),
            indices.is_empty(),
        );
    }

    let ref_pct = index_pct.unwrap_or(median);
    if ref_pct.abs() < cfg.chop_abs_pct && vs20.map(|r| r < cfg.amount_chop).unwrap_or(false) {
        return finish(
            "chop",
            "缩量震荡",
            vec![format!(
                "波动 {:+.1}%，额为 20 日 ×{:.1}",
                ref_pct,
                vs20.unwrap_or(0.0)
            )],
            Vec::new(),
            calendar_fallback,
            quotes_before_open,
            unlabeled_heavy,
            has_sectors,
            vs20.is_none(),
            indices.is_empty(),
        );
    }

    finish(
        "rotation",
        "结构性轮动",
        Vec::new(),
        Vec::new(),
        calendar_fallback,
        quotes_before_open,
        unlabeled_heavy,
        has_sectors,
        vs20.is_none(),
        indices.is_empty(),
    )
}

#[allow(clippy::too_many_arguments)]
fn finish(
    code: &str,
    label: &str,
    mut reasons: Vec<String>,
    subtypes: Vec<String>,
    calendar_fallback: bool,
    quotes_before_open: bool,
    unlabeled_heavy: bool,
    has_sectors: bool,
    no_20d: bool,
    no_index: bool,
) -> Regime {
    reasons.truncate(6);
    let confidence = if calendar_fallback || quotes_before_open || no_20d {
        "low"
    } else if unlabeled_heavy || (no_index && !has_sectors) {
        "medium"
    } else if has_sectors {
        "high"
    } else {
        "medium"
    };
    Regime {
        code: code.into(),
        label: label.into(),
        confidence: confidence.into(),
        reasons,
        subtypes,
    }
}

pub fn rotation_reason(sectors: &[super::SectorMove]) -> Option<String> {
    let real: Vec<_> = sectors.iter().filter(|s| s.name != "未分类").collect();
    let ups: Vec<_> = real
        .iter()
        .filter(|s| s.direction == "up")
        .take(2)
        .collect();
    let down = real.iter().find(|s| s.direction == "down");
    if ups.is_empty() {
        return None;
    }
    let t1 = ups[0].name.as_str();
    let t2 = ups.get(1).map(|s| s.name.as_str());
    let b1 = down.map(|s| s.name.as_str());
    Some(match (t2, b1) {
        (Some(t2), Some(b1)) => format!("主线：{t1}、{t2}；拖累：{b1}"),
        (None, Some(b1)) => format!("主线：{t1}；拖累：{b1}"),
        (Some(t2), None) => format!("主线：{t1}、{t2}"),
        (None, None) => format!("主线：{t1}"),
    })
}
