use super::*;
use crate::controller::Payload;

pub(super) struct WatchSearch {
    pub query: String,
    pub market: String,
    pub rows: Vec<QuoteRow>,
    pub matched: usize,
    pub loading: bool,
    pub error: Option<String>,
    pub dirty: Option<Instant>,
    selected: Option<usize>,
}
impl Default for WatchSearch {
    fn default() -> Self {
        Self {
            query: String::new(),
            market: "ALL".into(),
            rows: Vec::new(),
            matched: 0,
            loading: false,
            error: None,
            dirty: None,
            selected: None,
        }
    }
}
impl GetRichApp {
    pub(super) fn tick_watch_search(&mut self) {
        if self.screen != Screen::Settings
            || self.settings_tab != 2
            || !self
                .watch_search
                .dirty
                .is_some_and(|t| t.elapsed() >= Duration::from_millis(180))
        {
            return;
        }
        self.watch_search.dirty = None;
        let query = self.watch_search.query.trim().to_string();
        let market = self.watch_search.market.clone();
        if query.is_empty() {
            self.watch_search.loading = false;
            return;
        }
        self.watch_search.loading = true;
        if let Err(error) = self.controller.submit("read:watch-search", move |core| {
            core.quote_board(Some(&market), Some(&query), Some("symbol"), 20, 0, None)
                .map(|page| Payload::WatchSearch(query, market, page))
                .map_err(|e| e.to_string())
        }) {
            self.watch_search.loading = false;
            self.watch_search.error = Some(error);
        }
    }
    fn invalidate_watch_search(&mut self) {
        self.watch_search.rows.clear();
        self.watch_search.matched = 0;
        self.watch_search.error = None;
        self.watch_search.selected = None;
        self.watch_search.loading = false;
        self.watch_search.dirty = Some(Instant::now());
    }
    pub(super) fn choose_watch_result(&mut self, index: usize) {
        if let Some(row) = self.watch_search.rows.get(index) {
            self.watch_symbol = row.symbol.clone();
            self.watch_market = row.market.clone();
            self.watch_name = row.name.clone().unwrap_or_default();
            self.watch_search.selected = Some(index);
        }
    }
    pub(super) fn watch_search_ui(&mut self, ui: &mut egui::Ui) {
        field_label(ui, "搜索股票名称或代码");
        let mut changed = false;
        let mut choose = None;
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 126.0).max(120.0);
            let response = ui.add_sized(
                [width, 38.0],
                egui::TextEdit::singleline(&mut self.watch_search.query)
                    .hint_text("如 贵州茅台 / 600519 / AAPL")
                    .margin(Vec2::new(10.0, 9.0)),
            );
            changed |= response.changed();
            if response.has_focus() || response.lost_focus() {
                if ui.input(|i| i.key_pressed(egui::Key::ArrowDown))
                    && !self.watch_search.rows.is_empty()
                {
                    self.watch_search.selected = Some(
                        (self.watch_search.selected.map(|i| i + 1).unwrap_or(0))
                            .min(self.watch_search.rows.len() - 1),
                    );
                }
                if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                    self.watch_search.selected =
                        Some(self.watch_search.selected.unwrap_or(0).saturating_sub(1));
                }
                if ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && !self.watch_search.rows.is_empty()
                {
                    choose = Some(self.watch_search.selected.unwrap_or(0));
                }
            }
            let previous = self.watch_search.market.clone();
            egui::ComboBox::from_id_salt("watch-search-market")
                .width(110.0)
                .selected_text(if previous == "ALL" {
                    "全部市场"
                } else {
                    super::management::market_label(&previous)
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.watch_search.market, "ALL".into(), "全部市场");
                    for market in ["CN.SH", "CN.SZ", "CN.BJ", "HK", "US"] {
                        ui.selectable_value(
                            &mut self.watch_search.market,
                            market.into(),
                            super::management::market_label(market),
                        );
                    }
                });
            changed |= previous != self.watch_search.market;
        });
        if changed {
            self.invalidate_watch_search();
        }
        if self.watch_search.query.trim().is_empty() {
            ui.colored_label(
                muted(),
                "从本地标的库搜索，选中结果会自动填写代码、市场和名称。也可在下方手动添加。",
            );
        } else if let Some(error) = &self.watch_search.error {
            ui.colored_label(
                Color32::from_rgb(219, 166, 78),
                format!("搜索失败：{error}，修改关键词可重试。"),
            );
        } else if self.watch_search.loading || self.watch_search.dirty.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.small("正在查找…");
            });
        } else if self.watch_search.rows.is_empty() {
            ui.colored_label(
                muted(),
                "未找到匹配标的。可更换名称或代码、切换市场，或先在数据中心同步股票列表。",
            );
        } else {
            ui.small(format!(
                "找到 {} 只，显示前 {} 只；↑↓ 选择，Enter 填入。",
                self.watch_search.matched,
                self.watch_search.rows.len()
            ));
            egui::ScrollArea::vertical()
                .id_salt("watch-search-results")
                .max_height(216.0)
                .show(ui, |ui| {
                    for (index, row) in self.watch_search.rows.iter().enumerate() {
                        let watched =
                            self.settings_cache["watchlist"]
                                .as_array()
                                .is_some_and(|list| {
                                    list.iter().any(|w| {
                                        w["symbol"].as_str() == Some(&row.symbol)
                                            && w["market"].as_str() == Some(&row.market)
                                    })
                                });
                        let label = format!(
                            "{}   {} · {}{}",
                            row.name.as_deref().unwrap_or(&row.symbol),
                            row.symbol,
                            super::management::market_label(&row.market),
                            if watched { " · 已自选" } else { "" }
                        );
                        if ui
                            .add_sized(
                                [ui.available_width(), 32.0],
                                egui::Button::new(&label)
                                    .selected(self.watch_search.selected == Some(index)),
                            )
                            .on_hover_text(&label)
                            .clicked()
                        {
                            choose = Some(index);
                        }
                    }
                });
        }
        if let Some(index) = choose {
            self.choose_watch_result(index);
        }
        ui.add_space(12.0);
    }
}
