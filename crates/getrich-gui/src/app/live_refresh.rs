//! Revision-driven UI reads. Quotes stay independent of full history and research.
use super::*;
use crate::controller::Payload;

pub(super) struct LiveRefresh {
    seq: u64,
    quotes_dirty: bool,
    map_dirty: bool,
    bars_dirty: bool,
    bars_full: bool,
    bars_at: Option<Instant>,
}
impl Default for LiveRefresh {
    fn default() -> Self {
        Self {
            seq: 0,
            quotes_dirty: true,
            map_dirty: true,
            bars_dirty: false,
            bars_full: false,
            bars_at: None,
        }
    }
}
impl GetRichApp {
    pub(super) fn preserve_newer_quote(&self, detail: &mut StockDetail) {
        if let Some(current) = self
            .detail
            .as_ref()
            .filter(|d| d.listing.key == detail.listing.key)
        {
            let old = current
                .quote
                .ts
                .as_deref()
                .and_then(|s| session::parse_cn_ts(s).ok());
            let next = detail
                .quote
                .ts
                .as_deref()
                .and_then(|s| session::parse_cn_ts(s).ok());
            if old.zip(next).is_some_and(|(old, next)| old > next) {
                detail.quote = current.quote.clone();
            }
        }
    }
    pub(super) fn realtime_warning(&self) -> Option<String> {
        let status = &self.settings_cache["provider"]["realtime_status"];
        matches!(
            status["state"].as_str(),
            Some(
                "permission_denied"
                    | "rate_limited"
                    | "error"
                    | "disabled"
                    | "unconfigured"
                    | "outside_session"
                    | "closed_day"
            )
        )
        .then(|| {
            status["message"]
                .as_str()
                .unwrap_or("实时数据源异常")
                .to_string()
        })
    }
    pub(super) fn realtime_hint(&self) -> String {
        let provider = &self.settings_cache["provider"];
        let status = &provider["realtime_status"];
        format!(
            "{}\n最近实时成功：{}\n最近尝试：{}\n最近错误：{}",
            status["message"].as_str().unwrap_or("尚无实时状态"),
            status["last_success_at"]
                .as_str()
                .map(shanghai_ts)
                .unwrap_or_else(|| "尚无成功记录".into()),
            status["last_attempt_at"]
                .as_str()
                .map(shanghai_ts)
                .unwrap_or_else(|| "尚无记录".into()),
            provider["last_error"].as_str().unwrap_or("—")
        )
    }
    pub(super) fn observe_data_changes(&mut self) {
        let seq = self.core.events.seq();
        if seq == self.live.seq {
            return;
        }
        for event in self.core.events.replay_since(self.live.seq) {
            if matches!(
                event["type"].as_str(),
                Some("quote.updated" | "ingest.done" | "settings.changed")
            ) {
                self.live.quotes_dirty = true;
                self.live.map_dirty = true;
                self.live.bars_dirty = true;
                if event["type"] != "quote.updated" {
                    self.live.bars_full = true;
                }
            }
        }
        self.live.seq = seq;
    }
    pub(super) fn tick_quote_reads(&mut self) {
        if !matches!(self.screen, Screen::Quotes | Screen::Detail) {
            return;
        }
        let due = self.live.quotes_dirty
            || self
                .quote_tick
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(15));
        if due
            && !self.controller.is_pending("read:board")
            && !self.controller.is_pending("read:quote")
        {
            self.refresh_from_db();
            self.quote_tick = Some(Instant::now());
            self.live.quotes_dirty = false;
        }
        if self.live.bars_dirty
            && !self.controller.is_pending("read:stock")
            && self
                .live
                .bars_at
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(5))
        {
            self.refresh_current_bars(self.live.bars_full);
            self.live.bars_full = false;
            self.live.bars_at = Some(Instant::now());
            self.live.bars_dirty = false;
        }
    }
    pub(super) fn tick_map_reads(&mut self) {
        if self.screen != Screen::Pulse {
            return;
        }
        let due = self.live.map_dirty
            || self
                .pulse_tick
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(30));
        if due && !self.market.loading {
            self.read_pulse();
            self.reload_map();
            // The header must advance with the map even while the quote board is hidden.
            // health_stats reuses historical totals and reads the latest quote timestamp now.
            if let Err(error) = self.controller.submit_background("read:stats", |core| {
                core.health_stats()
                    .map(Payload::Stats)
                    .map_err(|e| e.to_string())
            }) {
                self.error = Some(error);
            }
            self.pulse_tick = Some(Instant::now());
            self.live.map_dirty = false;
        }
    }
    pub(super) fn refresh_current_quote(&mut self) {
        let Some(key) = self.pending_key.clone() else {
            return;
        };
        if let Err(error) = self
            .controller
            .submit_background("read:quote", move |core| {
                core.show_stock(&key)
                    .map(|d| Payload::Quote(Box::new(d)))
                    .map_err(|e| e.to_string())
            })
        {
            self.error = Some(error);
        }
    }
    pub(super) fn refresh_current_bars(&mut self, full: bool) {
        let Some(key) = self.pending_key.clone() else {
            return;
        };
        let tf = self.tf;
        let from = if full {
            None
        } else {
            self.bars
                .as_ref()
                .filter(|b| {
                    b.symbol == key.symbol
                        && b.market == key.market.as_str()
                        && b.timeframe == tf.as_str()
                })
                .and_then(|b| b.rows.last())
                .and_then(|bar| getrich_core::bar_refresh::tail_from(tf.as_str(), &bar.ts))
        };
        if let Err(error) = self
            .controller
            .submit_background("read:stock", move |core| {
                core.list_bars(&key, tf, from.as_deref(), None, 20000)
                    .map(|bars| Payload::Bars(key, bars, from))
                    .map_err(|e| e.to_string())
            })
        {
            self.error = Some(error);
        }
    }
}
