use super::*;
use crate::controller::Payload;
use serde_json::{json, Value};

impl GetRichApp {
    pub(super) fn read_pulse(&mut self) {
        if let Some(snapshot) = self.core.realtime.snapshot() {
            self.pulse = snapshot.pulse.clone();
        }
    }
    pub(super) fn reload_stats(&mut self) {
        if let Err(e) = self.controller.submit("read:stats", |core| {
            core.stats().map(Payload::Stats).map_err(|e| e.to_string())
        }) {
            self.error = Some(e);
        }
    }
    pub(super) fn reload_list(&mut self) {
        self.detail_pinned = false;
        self.page_offset = 0;
        self.reload_board(true);
    }
    pub(super) fn reload_board(&mut self, open: bool) {
        let (board, query, sort, industry, filter, offset) = (
            self.board.clone(),
            self.query.clone(),
            self.sort.clone(),
            self.industry.clone(),
            self.change_filter.clone(),
            self.page_offset,
        );
        self.loading = true;
        if let Err(e) = self.controller.submit("read:board", move |core| {
            core.quote_board_filtered(
                Some(&board),
                Some(&query),
                Some(&sort),
                200,
                offset,
                Some(&industry),
                Some(&filter),
            )
            .map(|p| Payload::Board(p, open))
            .map_err(|e| e.to_string())
        }) {
            self.error = Some(e);
            self.loading = false;
        }
    }
    pub(super) fn refresh_from_db(&mut self) {
        self.reload_board(false);
    }
    pub(super) fn read_settings(&mut self) {
        if let Err(e) = self.controller.submit("read:settings", |core| {
            core.settings_view()
                .map(Payload::Settings)
                .map_err(|e| e.to_string())
        }) {
            self.error = Some(e);
        }
    }
    pub(super) fn submit_action(&mut self, purpose: &str, method: &str, params: Value) {
        let purpose = purpose.to_string();
        let method = method.to_string();
        self.import_msg = "操作已提交…".into();
        self.loading = true;
        if let Err(e) = self
            .controller
            .submit(&format!("action:{purpose}"), move |core| {
                getrich_core::invoke::invoke(core, &method, params)
                    .map(|value| Payload::Action(purpose, value))
                    .map_err(|e| e.to_string())
            })
        {
            self.error = Some(e);
        }
    }
    pub(super) fn submit_job(&mut self, kind: &str, params: Value) {
        self.submit_action("job", "job.submit", json!({"kind":kind,"params":params}));
    }
    pub(super) fn select_import(&mut self) {
        self.loading = true;
        if let Err(error) = self.controller.submit("action:file", |_| {
            let Some(path) = crate::file_dialog::select_import()? else {
                return Ok(Payload::Action("file-cancel".into(), Value::Null));
            };
            let bundle =
                getrich_core::import::load_file(&path, getrich_core::import::ImportKind::Bundle)
                    .map_err(|e| e.to_string())?;
            Ok(Payload::Action(
                "file".into(),
                json!({"path":path,"data":bundle}),
            ))
        }) {
            self.error = Some(error);
            self.loading = false;
        }
    }
    pub(super) fn navigate_stock(&mut self, symbol: &str, market: &str) {
        self.detail_pinned = true;
        self.screen = Screen::Detail;
        self.stock.tab = 0;
        self.stock.intraday = false;
        self.market.inspector_open = false;
        self.market.controls_open = false;
        self.market.sync_details_open = false;
        self.open_stock(symbol, market);
    }
    pub(super) fn apply_board(&mut self, page: QuoteBoardPage, open: bool) {
        self.loading = false;
        self.stats = Some(page.stats.clone());
        let old = self
            .pending_key
            .as_ref()
            .or_else(|| self.detail.as_ref().map(|d| &d.listing.key));
        self.selected = old
            .and_then(|key| {
                page.rows
                    .iter()
                    .position(|r| r.symbol == key.symbol && r.market == key.market.as_str())
            })
            .or(if open && !self.detail_pinned && !page.rows.is_empty() {
                Some(0)
            } else {
                None
            });
        let next = self
            .selected
            .and_then(|i| page.rows.get(i))
            .map(|r| (r.symbol.clone(), r.market.clone()));
        self.page = Some(page);
        if self.detail_pinned {
            return;
        }
        if let Some((symbol, market)) = next {
            self.open_stock(&symbol, &market);
        } else {
            self.detail = None;
            self.bars = None;
            self.pending_key = None;
        }
    }
    pub(super) fn open_stock(&mut self, symbol: &str, market: &str) {
        let Ok(key) = self.core.parse_key(symbol, Some(market)) else {
            return;
        };
        let same = self.detail.as_ref().is_some_and(|d| d.listing.key == key)
            && self
                .bars
                .as_ref()
                .is_some_and(|b| b.timeframe == self.tf.as_str());
        let research_key = format!("{}:{}", key.market.as_str(), key.symbol);
        if self.stock.key != research_key {
            self.stock.key = research_key;
            self.stock.data = Value::Null;
        }
        self.pending_key = Some(key.clone());
        self.detail_loading = !same;
        if !same {
            self.detail = None;
            self.bars = None;
            self.kline_span = 0;
            self.kline_end = 0;
            self.kline_hover = None;
        }
        if matches!(self.screen, Screen::Quotes | Screen::Detail) {
            self.screen = Screen::Detail;
        }
        let tf = self.tf;
        if let Err(e) = self.controller.submit("read:stock", move |core| {
            let detail = core.show_stock(&key).map_err(|e| e.to_string())?;
            let bars = core
                .list_bars(&key, tf, None, None, 20000)
                .map_err(|e| e.to_string())?;
            if let Err(error) = core.record_view(&key.symbol, Some(key.market.as_str())) {
                tracing::debug!(%error,"保存近期查看失败");
            }
            Ok(Payload::Stock(Box::new(detail), bars))
        }) {
            self.error = Some(e);
            self.detail_loading = false;
        }
    }
    pub(super) fn reload_bars(&mut self) {
        if let Some(key) = self.pending_key.clone() {
            self.open_stock(&key.symbol, key.market.as_str());
        }
    }
    pub(super) fn apply_watch_toggle(&mut self, symbol: &str, market: &str, name: Option<String>) {
        let watched = self
            .detail
            .as_ref()
            .filter(|d| d.quote.symbol == symbol && d.quote.market == market)
            .map(|d| d.watched)
            .unwrap_or_else(|| {
                self.settings_cache["watchlist"]
                    .as_array()
                    .is_some_and(|list| {
                        list.iter().any(|w| {
                            w["symbol"].as_str() == Some(symbol)
                                && w["market"].as_str() == Some(market)
                        })
                    })
            });
        self.submit_action(
            "watch",
            "watch.set",
            json!({"symbol":symbol,"market":market,"name":name,"watched":!watched}),
        );
    }
    pub(super) fn tick_background(&mut self) {
        self.maybe_start_map_sync();
        self.tick_research();
        self.tick_watch_search();
        if self
            .settings_tick
            .is_some_and(|t| t.elapsed() < Duration::from_secs(2))
        {
            return;
        }
        self.settings_tick = Some(Instant::now());
        if let Err(e) = self.controller.submit("read:jobs", |core| {
            core.jobs_list()
                .map(Payload::Jobs)
                .map_err(|e| e.to_string())
        }) {
            self.error = Some(e);
        }
        if self.settings_cache.as_object().is_none_or(|v| v.is_empty()) {
            self.read_settings();
        }
    }
    pub(super) fn apply_io(&mut self) {
        for reply in self.controller.drain() {
            match reply.result {
                Err(error) => {
                    if let Some(tag) = reply
                        .tag
                        .strip_prefix("action:")
                        .filter(|s| s.starts_with("research:"))
                    {
                        self.stock.pending.remove(tag);
                        self.stock.errors.insert(tag.into(), error.clone());
                    }
                    if reply.tag == "read:watch-search" {
                        self.watch_search.loading = false;
                        self.watch_search.error = Some(error);
                        continue;
                    }
                    if reply.tag == "action:map-sync" {
                        self.market.sync_pending = false;
                        self.market.error = Some(error.clone());
                    }
                    if reply.tag == "read:map" {
                        self.market.error = Some(error.clone());
                        self.market.loading = false;
                    }
                    self.error = Some(error);
                    self.loading = false;
                    self.detail_loading = false;
                }
                Ok(Payload::Map(map)) => {
                    self.market.loading = false;
                    let q = &map.query;
                    let current = &self.market.query;
                    if q.board == current.board
                        && q.metric == current.metric
                        && q.industry == current.industry
                        && q.watch_only == current.watch_only
                    {
                        self.error = None;
                        self.market.error = None;
                        self.market.data = Some(Arc::new(*map));
                    }
                }
                Ok(Payload::Stats(stats)) => self.stats = Some(stats),
                Ok(Payload::Board(page, open)) => self.apply_board(page, open),
                Ok(Payload::WatchSearch(query, market, page)) => {
                    if query == self.watch_search.query.trim() && market == self.watch_search.market
                    {
                        self.watch_search.loading = false;
                        self.watch_search.rows = page.rows;
                        self.watch_search.matched = page.matched;
                        self.watch_search.error = None;
                    }
                }
                Ok(Payload::Research(key, value)) => {
                    if self.pending_key.as_ref() == Some(&key) {
                        self.stock.key = format!("{}:{}", key.market.as_str(), key.symbol);
                        self.stock.data = value;
                    }
                }
                Ok(Payload::Stock(detail, bars)) => {
                    if self.pending_key.as_ref() == Some(&detail.listing.key)
                        && bars.timeframe == self.tf.as_str()
                    {
                        self.detail_loading = false;
                        self.detail = Some(*detail);
                        self.bars = Some(bars);
                        self.error = None;
                        self.read_research();
                    }
                }
                Ok(Payload::Settings(value)) => {
                    if self.provider_token.is_empty() {
                        self.provider_user =
                            value["provider"]["user_id"].as_str().unwrap_or("").into();
                        self.provider_enabled =
                            value["provider"]["enabled"].as_bool().unwrap_or(false);
                        self.provider_poll = value["provider"]["poll_ms"].as_u64().unwrap_or(5000);
                    }
                    if let Ok(p) = serde_json::from_value(value["preferences"].clone()) {
                        self.preferences = p;
                    }
                    self.settings_default_board =
                        value["default_board"].as_str().unwrap_or("ALL").into();
                    self.settings_cache = value;
                }
                Ok(Payload::Jobs(value)) => {
                    let signature = value["jobs"]
                        .as_array()
                        .map(|jobs| {
                            jobs.iter()
                                .map(|j| format!("{}:{}", j["id"], j["status"]))
                                .collect::<Vec<_>>()
                                .join(",")
                        })
                        .unwrap_or_default();
                    if signature != self.job_signature {
                        self.job_signature = signature;
                        self.reload_stats();
                        self.read_research();
                        self.reload_bars();
                        if self.screen == Screen::Pulse {
                            self.reload_map();
                        }
                        if matches!(self.screen, Screen::Quotes | Screen::Detail) {
                            self.reload_board(false);
                        }
                    }
                    if let Some(id) = self.market.sync_job.as_ref().and_then(|j| j["id"].as_i64()) {
                        if let Some(job) = value["jobs"]
                            .as_array()
                            .and_then(|jobs| jobs.iter().find(|j| j["id"].as_i64() == Some(id)))
                        {
                            self.market.sync_job = Some(job.clone());
                        }
                    }
                    self.jobs_cache = value;
                }
                Ok(Payload::Action(purpose, value)) => {
                    self.loading = false;
                    self.error = None;
                    if purpose.starts_with("research:") {
                        self.stock.pending.remove(&purpose);
                        if let Some(id) = value["id"].as_i64() {
                            self.stock.jobs.insert(purpose.clone(), id);
                        }
                        self.read_research();
                        continue;
                    }
                    match purpose.as_str() {
                        "map-sync" => {
                            self.market.sync_pending = false;
                            self.market.sync_job = Some(value);
                            self.import_msg = "正在后台同步星图行情，完成后自动更新。".into();
                        }
                        "file" => {
                            self.import_path = value["path"].as_str().unwrap_or("").into();
                            self.import_data = Some(value["data"].clone());
                            self.preview = None;
                            self.submit_action(
                                "preview",
                                "import.preview",
                                json!({"data":value["data"]}),
                            );
                        }
                        "file-cancel" => {}
                        "preview" => {
                            self.preview = Some(value);
                            self.import_msg = "校验完成，请检查预览后应用。".into();
                        }
                        "job" => {
                            self.import_msg =
                                format!("任务 #{} 已提交，可在数据页查看进度", value["id"])
                        }
                        "settings" => {
                            self.provider_token.clear();
                            if let Ok(p) = serde_json::from_value(value["preferences"].clone()) {
                                self.preferences = p;
                            }
                            self.settings_cache = value;
                            self.import_msg = "设置已保存。".into();
                        }
                        "watch" => {
                            if let Some(d) = self.detail.as_mut() {
                                if value["symbol"].as_str() == Some(d.quote.symbol.as_str())
                                    && value["market"].as_str() == Some(d.quote.market.as_str())
                                {
                                    d.watched = value["watched"].as_bool().unwrap_or(false);
                                }
                            }
                            self.read_settings();
                            if matches!(self.screen, Screen::Quotes | Screen::Detail) {
                                self.reload_board(false);
                            }
                            self.import_msg = "自选已保存。".into();
                        }
                        _ => self.import_msg = "操作完成。".into(),
                    }
                    if !["file-cancel", "file"].contains(&purpose.as_str()) {
                        self.ui_notice = Some((self.import_msg.clone(), Instant::now()));
                    }
                }
            }
        }
    }
}
