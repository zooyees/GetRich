//! Compact, aligned chart controls shared by the stock detail workspace.
use super::*;

pub(super) fn ma_color(period: usize) -> Color32 {
    match period {
        5 => muted(),
        10 => accent(),
        20 => Color32::from_rgb(220, 142, 65),
        30 => Color32::from_rgb(185, 133, 237),
        120 => Color32::from_rgb(67, 196, 185),
        _ => Color32::from_rgb(218, 181, 71),
    }
}

pub(super) fn tab(ui: &mut egui::Ui, label: &str, selected: bool, width: f32) -> egui::Response {
    ui.add_sized(
        [width, 26.0],
        egui::Button::new(egui::RichText::new(label).size(12.0).color(if selected {
            accent()
        } else {
            muted()
        }))
        .fill(if selected {
            selected_surface()
        } else {
            Color32::TRANSPARENT
        })
        .stroke(Stroke::NONE)
        .rounding(5.0),
    )
}

impl GetRichApp {
    pub(super) fn stock_heading(&self, ui: &mut egui::Ui) -> bool {
        let detail = self.detail.as_ref().expect("stock detail");
        let q = &detail.quote;
        let name = q.name.as_deref().unwrap_or(&q.symbol);
        let source = format!(
            "报价 {} · {} · {}",
            q.trade_date.as_deref().unwrap_or("—"),
            q.data_status,
            q.source.as_deref().unwrap_or("来源未标明")
        );
        let quality = if q.source_kind == "daily" {
            "日线 · 日期精度"
        } else {
            "盘中快照"
        };
        let warning = self.realtime_warning();
        let hint = self.realtime_hint();
        let mut clicked = false;
        let draw = |ui: &mut egui::Ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            ui.spacing_mut().interact_size.y = 28.0;
            let width = ui
                .fonts(|fonts| {
                    fonts
                        .layout_no_wrap(name.into(), egui::FontId::proportional(22.0), text_color())
                        .size()
                        .x
                })
                .clamp(60.0, 220.0);
            ui.add_sized(
                [width, 28.0],
                egui::Label::new(egui::RichText::new(name).size(22.0).strong()).truncate(),
            )
            .on_hover_text(name);
            ui.label(
                egui::RichText::new(format!(
                    "{} · {} {}",
                    q.symbol,
                    q.board_label,
                    q.currency.as_deref().unwrap_or("")
                ))
                .size(11.0)
                .color(muted()),
            )
            .on_hover_text(format!("{source}\n{quality}"));
            ui.label(
                egui::RichText::new(if detail.empty.no_quote {
                    "暂无报价".into()
                } else {
                    fmt(q.last)
                })
                .size(24.0)
                .strong()
                .color(dir_color(&q.direction)),
            );
            ui.label(
                egui::RichText::new(format!("{}  {}", signed(q.change), pct(q.change_pct)))
                    .size(12.0)
                    .color(dir_color(&q.direction)),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                clicked = ui
                    .add_sized(
                        [72.0, 28.0],
                        egui::Button::new(if detail.watched {
                            "★ 自选"
                        } else {
                            "☆ 自选"
                        }),
                    )
                    .clicked();
                if ui.available_width() >= 220.0 {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!(
                                "{} · {}",
                                q.trade_date.as_deref().unwrap_or("—"),
                                warning.as_deref().unwrap_or(quality)
                            ))
                            .size(10.0)
                            .color(muted()),
                        )
                        .truncate(),
                    )
                    .on_hover_text(format!("{source}\n{hint}"));
                }
            });
        };
        if ui.available_width() < 650.0 {
            ui.horizontal_wrapped(draw);
        } else {
            ui.horizontal(draw);
        }
        clicked
    }

    pub(super) fn chart_toolbar(&mut self, ui: &mut egui::Ui) -> Option<Timeframe> {
        let mut next = None;
        let inline_tabs = ui.available_width() >= 1000.0;
        let inline_navigation = ui.available_width() >= 580.0;
        egui::Frame::none()
            .fill(secondary_surface())
            .stroke(Stroke::new(1.0_f32, design::border()))
            .rounding(6.0)
            .inner_margin(Vec2::new(8.0, 4.0))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 3.0);
                ui.spacing_mut().button_padding = Vec2::new(6.0, 3.0);
                ui.spacing_mut().interact_size.y = 28.0;
                if !inline_tabs {
                    self.stock_tabs(ui);
                }
                ui.horizontal(|ui| {
                    if inline_tabs {
                        self.stock_tabs(ui);
                    }
                    if self.stock.tab != 0 {
                        return;
                    }
                    egui::Frame::none()
                        .fill(background())
                        .stroke(Stroke::new(1.0_f32, design::border()))
                        .rounding(5.0)
                        .inner_margin(1.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                if tab(ui, "分时", self.stock.intraday, 40.0).clicked() {
                                    self.stock.intraday = true;
                                }
                                for (tf, label) in [
                                    (Timeframe::D1, "日K"),
                                    (Timeframe::W1, "周K"),
                                    (Timeframe::Mo1, "月K"),
                                ] {
                                    if tab(ui, label, !self.stock.intraday && self.tf == tf, 40.0)
                                        .clicked()
                                    {
                                        self.stock.intraday = false;
                                        next = Some(tf);
                                    }
                                }
                            });
                        });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if !self.stock.intraday {
                            let busy = self.research_busy("history");
                            let configured = self.settings_cache["provider"]["configured"]
                                .as_bool()
                                .unwrap_or(false);
                            let supported = self
                                .detail
                                .as_ref()
                                .is_some_and(|d| d.quote.market.starts_with("CN."));
                            if ui
                                .add_enabled_ui(!configured || (supported && !busy), |ui| {
                                    ui.add_sized(
                                        [110.0, 28.0],
                                        egui::Button::new(if !configured {
                                            "配置数据源"
                                        } else if busy {
                                            "更新中…"
                                        } else {
                                            "更新完整历史"
                                        }),
                                    )
                                })
                                .inner
                                .on_hover_text("后台补全上市以来的原始日线；更新时保留现有图表")
                                .clicked()
                            {
                                if configured {
                                    self.request_research("history", true);
                                } else {
                                    self.screen = Screen::Settings;
                                    self.settings_tab = 0;
                                    self.read_settings();
                                }
                            }
                            if inline_navigation {
                                ui.add_space(6.0);
                                self.chart_navigation(ui);
                            }
                        }
                    });
                });
                if self.stock.tab == 0 && !self.stock.intraday {
                    if !inline_navigation {
                        ui.horizontal(|ui| self.chart_navigation(ui));
                    }
                    self.chart_readout(ui);
                    self.ma_controls(ui);
                }
            });
        next
    }
    fn chart_navigation(&mut self, ui: &mut egui::Ui) {
        let enabled = self.bars.as_ref().is_some_and(|b| !b.rows.is_empty());
        // RTL rows use reversed widget order so the visible control order stays the same.
        let mut controls = vec![
            ("left", "←", "查看更早 K 线"),
            ("right", "→", "查看更晚 K 线"),
            ("in", "＋", "放大 K 线"),
            ("out", "－", "缩小 K 线"),
            ("reset", "最新", "回到最新 K 线"),
        ];
        if ui.layout().main_dir() == egui::Direction::RightToLeft {
            controls.reverse();
        }
        for (action, label, hint) in controls {
            if ui
                .add_enabled(
                    enabled,
                    egui::Button::new(label)
                        .min_size(Vec2::new(if action == "reset" { 48.0 } else { 28.0 }, 28.0)),
                )
                .on_hover_text(hint)
                .clicked()
            {
                self.move_chart(action);
            }
        }
    }
    fn move_chart(&mut self, action: &str) {
        let Some(bars) = self.bars.as_ref().filter(|b| !b.rows.is_empty()) else {
            return;
        };
        let (_, end, span) =
            crate::chart_model::viewport(bars.rows.len(), self.kline_span, self.kline_end);
        self.kline_hover = None;
        match action {
            "left" => self.kline_end = end.saturating_sub((span / 2).max(1)).max(span),
            "right" => {
                let next = (end + span / 2).min(bars.rows.len());
                self.kline_end = if next == bars.rows.len() { 0 } else { next };
            }
            "in" => self.kline_span = (span * 4 / 5).max(1),
            "out" => self.kline_span = (span * 5 / 4 + 1).min(bars.rows.len()),
            _ => {
                self.kline_span = 0;
                self.kline_end = 0;
            }
        }
    }
    fn chart_index(&self) -> Option<usize> {
        let b = self.bars.as_ref()?;
        let (start, end, _) =
            crate::chart_model::viewport(b.rows.len(), self.kline_span, self.kline_end);
        end.checked_sub(1).map(|last| {
            self.kline_hover
                .filter(|i| *i < end - start)
                .map(|i| start + i)
                .unwrap_or(last)
        })
    }
    fn chart_readout(&self, ui: &mut egui::Ui) {
        let range = self
            .bars
            .as_ref()
            .map(|b| {
                format!(
                    "{} — {} · {} 根 · 原始价格 / 未复权{}",
                    b.from.as_deref().unwrap_or("—"),
                    b.to.as_deref().unwrap_or("—"),
                    b.count,
                    if b.partial_last_period {
                        " · 当前周期未结束"
                    } else {
                        ""
                    }
                )
            })
            .unwrap_or_else(|| "尚无历史 K 线".into());
        let ohlc = self
            .chart_index()
            .and_then(|i| self.bars.as_ref()?.rows.get(i))
            .map(|b| {
                format!(
                    "{}  开 {:.2}  高 {:.2}  低 {:.2}  收 {:.2}",
                    b.ts, b.open, b.high, b.low, b.close
                )
            })
            .unwrap_or_else(|| "等待 K 线数据".into());
        let source = &self.stock.data["datasets"]["history"];
        let hint = format!(
            "{}\n{} · 数据日期 {} · 获取 {}",
            range,
            source["source"].as_str().unwrap_or("本地历史"),
            source["as_of"].as_str().unwrap_or("—"),
            source["updated_at"]
                .as_str()
                .map(shanghai_ts)
                .unwrap_or_else(|| "尚未获取".into())
        );
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(14.0, 2.0);
            ui.spacing_mut().interact_size.y = 16.0;
            ui.add(egui::Label::new(
                egui::RichText::new(&range).size(11.0).color(muted()),
            ))
            .on_hover_text(hint);
            ui.label(
                egui::RichText::new(&ohlc)
                    .monospace()
                    .size(12.0)
                    .color(text_color()),
            );
        });
    }
    fn ma_controls(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().interact_size.y = 22.0;
        let index = self.chart_index();
        let values: Vec<_> = getrich_core::indicators::MA_PERIODS
            .into_iter()
            .map(|period| {
                let value = index.and_then(|i| {
                    self.bars
                        .as_ref()?
                        .moving_averages
                        .iter()
                        .find(|ma| ma.period == period)?
                        .values
                        .get(i)
                        .copied()
                        .flatten()
                });
                (period, value)
            })
            .collect();
        let available = (ui.available_width() - 36.0).max(1.0);
        let cols = if available >= 1144.0 {
            6
        } else if available >= 568.0 {
            3
        } else if available >= 376.0 {
            2
        } else {
            1
        };
        let width = ((available - 8.0 * (cols - 1) as f32) / cols as f32).min(184.0);
        let mut changed = false;
        for (row_index, row) in values.chunks(cols).enumerate() {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.allocate_ui_with_layout(
                    Vec2::new(28.0, 24.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_width(28.0);
                        if row_index == 0 {
                            ui.label(egui::RichText::new("均线").size(11.0).color(muted()));
                        }
                    },
                );
                for (period, value) in row {
                    egui::Frame::none()
                        .inner_margin(Vec2::new(4.0, 1.0))
                        .show(ui, |ui| {
                            ui.set_width((width - 8.0).max(60.0));
                            ui.spacing_mut().interact_size.y = 22.0;
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.horizontal(|ui| {
                                let mut show = self.preferences.moving_averages.contains(period);
                                let color = if show { ma_color(*period) } else { muted() };
                                let label = match period {
                                    120 => "MA120 · 半年".into(),
                                    250 => "MA250 · 年线".into(),
                                    _ => format!("MA{period}"),
                                };
                                if ui
                                    .checkbox(
                                        &mut show,
                                        egui::RichText::new(label).size(12.0).color(color),
                                    )
                                    .on_hover_text(format!(
                                        "{period} 个交易日日均线；勾选状态会保存，— 表示日线不足。"
                                    ))
                                    .changed()
                                {
                                    self.preferences.moving_averages.retain(|p| p != period);
                                    if show {
                                        self.preferences.moving_averages.push(*period);
                                    }
                                    changed = true;
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            egui::RichText::new(fmt(*value))
                                                .monospace()
                                                .size(12.0)
                                                .color(color),
                                        );
                                    },
                                );
                            });
                        });
                }
            });
        }
        if changed {
            let params =
                json!({"preferences":{"moving_averages":self.preferences.moving_averages}});
            if let Err(error) = self
                .controller
                .submit("action:ma-preferences", move |core| {
                    getrich_core::invoke::invoke(core, "settings.save", params)
                        .map(|value| {
                            crate::controller::Payload::Action("ma-preferences".into(), value)
                        })
                        .map_err(|e| e.to_string())
                })
            {
                self.error = Some(error);
            }
        }
    }
    pub(super) fn chart_notice(&mut self, ui: &mut egui::Ui) {
        let data = self.stock.data["datasets"]["history"].clone();
        let tag = self.research_tag("history");
        let error = data["error"]
            .as_str()
            .map(str::to_string)
            .or_else(|| self.stock.errors.get(&tag).cloned())
            .or_else(|| {
                self.stock.jobs.get(&tag).and_then(|id| {
                    self.jobs_cache["jobs"]
                        .as_array()?
                        .iter()
                        .find(|job| job["id"].as_i64() == Some(*id))?["error"]
                        .as_str()
                        .map(str::to_string)
                })
            });
        if let Some(error) = error {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(error)
                        .size(12.0)
                        .color(Color32::from_rgb(219, 166, 78)),
                )
                .wrap(),
            );
        }
    }
}
