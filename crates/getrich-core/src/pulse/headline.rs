use super::{FundFlow, IndexPrint, Regime, SectorMove, SessionInfo, Tape};
use crate::session::SessionPhase;

const FORBIDDEN: &[&str] = &[
    "看好",
    "看空",
    "建议",
    "抄底",
    "逃顶",
    "主力",
    "游资",
    "实时北向",
    "即将",
    "大概率",
];

pub fn headline(
    session: &SessionInfo,
    regime: &Regime,
    tape: &Tape,
    sectors: &[SectorMove],
    indices: &[IndexPrint],
    capital: &[FundFlow],
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if session.frozen {
        parts.push("截面冻结。".into());
    }
    if session.phase == SessionPhase::Pre {
        parts.push("基于上一交易日收盘截面。".into());
    }
    parts.push(format!("{} · {}。", session.phase.label_zh(), regime.label));

    if regime.code == "unknown" {
        parts.push(format!("有效报价 {} 只。", tape.n_quoted));
        return join_sentences(parts);
    }

    if let Some(m) = tape.median_pct {
        if let Some(idx) = indices.iter().find(|i| i.ts_code == "000001.SH") {
            parts.push(format!(
                "个股中位 {:+.1}%，{} {:+.1}%。",
                m, idx.name, idx.change_pct
            ));
        } else {
            parts.push(format!("个股中位 {:+.1}%。", m));
        }
    }

    if tape.advance_ratio.is_some() {
        parts.push(format!("上涨 {} / 下跌 {}。", tape.n_up, tape.n_down));
    }

    if tape.amount > 0.0 {
        let yi = tape.amount / 1.0e8;
        if let Some(r) = tape.amount_vs_20d {
            parts.push(format!("成交额 {yi:.1} 亿，为 20 日 ×{r:.1}。"));
        } else {
            parts.push(format!("成交额 {yi:.1} 亿。"));
        }
    }

    let ups: Vec<_> = sectors
        .iter()
        .filter(|s| s.name != "未分类" && s.direction == "up")
        .collect();
    let down = sectors
        .iter()
        .find(|s| s.name != "未分类" && s.direction == "down");
    if !ups.is_empty() {
        let t1 = &ups[0].name;
        let line = if ups.len() >= 2 {
            if let Some(b) = down {
                format!("{}、{} 领涨，{} 领跌。", t1, ups[1].name, b.name)
            } else {
                format!("{}、{} 领涨。", t1, ups[1].name)
            }
        } else if let Some(b) = down {
            format!("{} 领涨，{} 领跌。", t1, b.name)
        } else {
            format!("{} 领涨。", t1)
        };
        parts.push(line);
    }

    if let Some(c) = capital
        .iter()
        .find(|c| c.vintage == "t1" && c.value.is_some())
    {
        let v = c.value.unwrap();
        let dir = if v >= 0.0 { "净流入" } else { "净流出" };
        parts.push(format!(
            "{} 为 {} {} {:.1}{}（T+1）。",
            c.name,
            c.as_of.as_deref().unwrap_or(""),
            dir,
            v.abs(),
            c.unit
        ));
    }

    join_sentences(parts)
}

fn join_sentences(parts: Vec<String>) -> String {
    parts.concat()
}

pub fn contains_forbidden(text: &str) -> Option<&'static str> {
    FORBIDDEN.iter().copied().find(|w| text.contains(w))
}
