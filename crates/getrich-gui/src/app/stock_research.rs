use super::*;
use crate::controller::Payload;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(super) struct StockResearch {
    pub drafts: HashMap<String, Value>,
    pub histories: HashMap<String, Value>,
    pub screen: Value,
    pub screen_params: Option<(bool, f64, f64)>,
    pub tab: usize,
    pub intraday: bool,
    pub data: Value,
    pub key: String,
    pub attempted: HashSet<String>,
    pub pending: HashSet<String>,
    pub jobs: HashMap<String, i64>,
    pub errors: HashMap<String, String>,
    pub tick: Option<Instant>,
}

impl GetRichApp {
    pub(super) fn read_research(&mut self) {
        let Some(key) = self.pending_key.clone() else {
            return;
        };
        let analysis = self.stock.tab == 4;
        let mode = self.preferences.research_mode.clone();
        let basis = self.preferences.price_basis.clone();
        let source = self.preferences.adjustment_source.clone();
        if let Err(e) = self.controller.submit("read:research", move |core| {
            core.stock_research(&key)
                .and_then(|mut v| {
                    if analysis {
                        v["analysis"] = core.analysis_workspace_basis(
                            &key,
                            None,
                            Some(&mode),
                            &basis,
                            &source,
                        )?;
                    }
                    Ok(Payload::Research(key, v))
                })
                .map_err(|e| e.to_string())
        }) {
            self.error = Some(e);
        }
    }
    pub(super) fn research_section(&self) -> &'static str {
        match self.stock.tab {
            4 => match self.preferences.research_mode.as_str() {
                "intraday" => "intraday",
                "value" => "f10",
                _ => "history",
            },
            1 => "f10",
            2 => "funds",
            3 => "announcements",
            _ => {
                if self.stock.intraday {
                    "intraday"
                } else {
                    "history"
                }
            }
        }
    }
    pub(super) fn research_tag(&self, section: &str) -> String {
        format!("research:{}:{section}", self.stock.key)
    }
    pub(super) fn research_busy(&self, section: &str) -> bool {
        let tag = self.research_tag(section);
        self.stock.pending.contains(&tag)
            || self.stock.jobs.get(&tag).is_some_and(|id| {
                self.jobs_cache["jobs"]
                    .as_array()
                    .and_then(|jobs| jobs.iter().find(|j| j["id"].as_i64() == Some(*id)))
                    .is_none_or(|j| {
                        matches!(
                            j["status"].as_str(),
                            Some("queued" | "running" | "cancelling")
                        )
                    })
            })
    }
    pub(super) fn request_research(&mut self, section: &str, force: bool) {
        if section == "analysis" {
            return;
        }
        let Some(d) = self.detail.as_ref() else {
            return;
        };
        if !d.quote.market.starts_with("CN.")
            || !self.settings_cache["provider"]["configured"]
                .as_bool()
                .unwrap_or(false)
            || self.stock.key != format!("{}:{}", d.quote.market, d.quote.symbol)
            || self.research_busy(section)
        {
            return;
        }
        let tag = self.research_tag(section);
        let datasets: &[&str] = match section {
            "f10" => &["company", "financial", "valuation", "holders"],
            "funds" => &["funds"],
            "announcements" => &["announcements"],
            "intraday" => &["intraday"],
            _ => &["history"],
        };
        let needs = datasets.iter().any(|name| {
            self.stock.data["datasets"][*name]["needs_refresh"]
                .as_bool()
                .unwrap_or(false)
        });
        let live = matches!(section, "intraday" | "history")
            && self.stock.data["datasets"][section]["status"] == "ready";
        if !force && (!needs || (self.stock.attempted.contains(&tag) && !live)) {
            return;
        }
        let params = json!({"symbol":d.quote.symbol,"market":d.quote.market,"section":section});
        self.stock.attempted.insert(tag.clone());
        self.stock.pending.insert(tag.clone());
        self.stock.errors.remove(&tag);
        self.submit_action(
            &tag,
            "job.submit",
            json!({"kind":"stock.research","params":params}),
        );
    }
    pub(super) fn tick_research(&mut self) {
        if !matches!(self.screen, Screen::Quotes | Screen::Detail) || self.detail.is_none() {
            return;
        }
        if !self.controller.is_pending("read:research")
            && self
                .stock
                .tick
                .is_none_or(|t| t.elapsed() > Duration::from_secs(30))
        {
            self.stock.tick = Some(Instant::now());
            self.read_research();
        }
        self.request_research("history", false);
        let section = self.research_section();
        if section != "history" {
            self.request_research(section, false);
        }
    }
    pub(super) fn stock_tabs(&mut self, ui: &mut egui::Ui) {
        let narrow = ui.available_width() < 430.0;
        let width = ((ui.available_width() - 16.0) / 5.0).min(116.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.spacing_mut().interact_size.y = 26.0;
            ui.spacing_mut().button_padding = Vec2::new(6.0, 3.0);
            for (index, label) in [
                "行情走势",
                if narrow {
                    "F10 财务"
                } else {
                    "F10 · 公司财务"
                },
                "资金流向",
                "公司公告",
                "研究分析",
            ]
            .iter()
            .enumerate()
            {
                if super::detail_workspace::tab(ui, label, self.stock.tab == index, width).clicked()
                {
                    self.stock.tab = index;
                    if index == 4 {
                        self.read_research();
                    }
                }
            }
        });
    }
    pub(super) fn research_status(&mut self, ui: &mut egui::Ui, section: &str) {
        let busy = self.research_busy(section);
        ui.horizontal_wrapped(|ui| {
            if busy {
                ui.spinner();
                ui.label("正在后台获取，完成后自动更新…");
            }
            if ui
                .add_enabled(
                    !busy,
                    egui::Button::new(if section == "history" {
                        "更新完整历史"
                    } else {
                        "刷新数据"
                    }),
                )
                .clicked()
            {
                self.request_research(section, true);
            }
            if !self.settings_cache["provider"]["configured"]
                .as_bool()
                .unwrap_or(false)
            {
                ui.colored_label(muted(), "连接 Tushare 后可获取数据");
                if ui.link("配置数据源").clicked() {
                    self.screen = Screen::Settings;
                }
            }
            if let Some(job) = self
                .stock
                .jobs
                .get(&self.research_tag(section))
                .and_then(|id| {
                    self.jobs_cache["jobs"]
                        .as_array()
                        .and_then(|jobs| jobs.iter().find(|j| j["id"].as_i64() == Some(*id)))
                })
            {
                if let Some(e) = job["error"].as_str() {
                    ui.colored_label(Color32::from_rgb(219, 166, 78), e);
                }
            }
            if let Some(e) = self.stock.errors.get(&self.research_tag(section)) {
                ui.colored_label(Color32::from_rgb(219, 166, 78), e);
            }
        });
    }
    pub(super) fn dataset_note(&self, ui: &mut egui::Ui, name: &str) {
        let data = &self.stock.data["datasets"][name];
        if let Some(e) = data["error"].as_str() {
            ui.colored_label(Color32::from_rgb(219, 166, 78), e);
            if data["rows"].as_array().is_some_and(|r| !r.is_empty()) {
                ui.small("本次刷新失败，以下保留上次成功数据。");
            }
        }
        ui.label(
            egui::RichText::new(format!(
                "{}  ·  数据日期 {}  ·  获取 {}",
                data["source"].as_str().unwrap_or("Tushare"),
                detail_design::date(data["as_of"].as_str().unwrap_or("—")),
                data["updated_at"]
                    .as_str()
                    .map(shanghai_ts)
                    .unwrap_or_else(|| "尚未获取".into())
            ))
            .size(12.0)
            .color(muted()),
        );
    }
    pub(super) fn research_panel(&mut self, ui: &mut egui::Ui) {
        if self.stock.tab == 4 {
            self.analysis_panel(ui);
            return;
        }
        let section = self.research_section();
        self.research_status(ui, section);
        ui.add_space(4.0);
        if !self
            .detail
            .as_ref()
            .is_some_and(|d| d.quote.market.starts_with("CN."))
        {
            ui.label("当前资料源支持 A 股。港美股可查看本地导入的行情与历史日线。");
            return;
        }
        match self.stock.tab {
            1 => self.f10_panel(ui),
            2 => self.funds_panel(ui),
            3 => self.announcements_panel(ui),
            _ => {}
        }
    }
    fn f10_panel(&self, ui: &mut egui::Ui) {
        if ui.available_width() >= 1000.0 {
            ui.columns(2, |columns| {
                self.valuation_card(&mut columns[0]);
                self.financial_card(&mut columns[0]);
                self.company_card(&mut columns[1]);
                self.holders_card(&mut columns[1]);
            });
        } else {
            self.valuation_card(ui);
            self.financial_card(ui);
            self.company_card(ui);
            self.holders_card(ui);
        }
    }
    fn valuation_card(&self, ui: &mut egui::Ui) {
        let v = &self.stock.data["datasets"]["valuation"]["rows"][0];
        detail_design::section(ui, "估值与股本", "市值 / 元 · 股本 / 股", |ui| {
            metrics(
                ui,
                &[
                    ("总市值", money(number(v, "total_mv"))),
                    ("流通市值", money(number(v, "circ_mv"))),
                    ("市盈率 TTM", num(v, "pe_ttm")),
                    ("市净率", num(v, "pb")),
                    ("换手率 %", num(v, "turnover_rate")),
                    ("量比", num(v, "volume_ratio")),
                    ("股息率 TTM %", num(v, "dv_ttm")),
                    ("总股本", money(number(v, "total_share"))),
                ],
            );
            self.dataset_note(ui, "valuation");
        });
    }
    fn financial_card(&self, ui: &mut egui::Ui) {
        let f = &self.stock.data["datasets"]["financial"]["rows"][0];
        detail_design::section(
            ui,
            "财务指标",
            &format!(
                "报告期 {} · 公告 {}",
                detail_design::date(&value_text(&f["end_date"])),
                detail_design::date(&value_text(&f["ann_date"]))
            ),
            |ui| {
                metrics(
                    ui,
                    &[
                        ("每股收益", num(f, "eps")),
                        ("每股净资产", num(f, "bps")),
                        ("ROE %", num(f, "roe")),
                        ("毛利率 %", num(f, "grossprofit_margin")),
                        ("净利率 %", num(f, "netprofit_margin")),
                        ("资产负债率 %", num(f, "debt_to_assets")),
                        ("营收同比 %", num(f, "or_yoy")),
                        ("净利润同比 %", num(f, "netprofit_yoy")),
                    ],
                );
                self.dataset_note(ui, "financial");
            },
        );
    }
    fn company_card(&self, ui: &mut egui::Ui) {
        let c = &self.stock.data["datasets"]["company"]["rows"][0];
        detail_design::section(ui, "公司概况", "", |ui| {
            let mut rows = vec![];
            if let Some(d) = &self.detail {
                rows.push(("所属行业", d.listing.industry.clone().unwrap_or("—".into())));
                rows.push((
                    "上市日期",
                    detail_design::date(d.listing.listing_date.as_deref().unwrap_or("—")),
                ));
            }
            for (label, field) in [
                ("公司全称", "com_name"),
                ("法人代表", "chairman"),
                ("总经理", "manager"),
                ("董秘", "secretary"),
                ("员工人数", "employees"),
                ("办公地址", "office"),
            ] {
                rows.push((label, value_text(&c[field])));
            }
            detail_design::properties(ui, "company-profile", &rows);
            ui.collapsing("主营业务与公司介绍", |ui| {
                ui.label(value_text(&c["main_business"]));
                ui.add_space(6.0);
                ui.label(value_text(&c["introduction"]));
            });
            if let Some(url) = c["website"]
                .as_str()
                .filter(|u| getrich_core::research::safe_url(u))
            {
                ui.hyperlink_to("公司网站 ↗", url);
            }
            self.dataset_note(ui, "company");
        });
    }
    fn holders_card(&self, ui: &mut egui::Ui) {
        detail_design::section(
            ui,
            "前十大股东",
            "最新报告期 · 数量 / 股",
            |ui| {
                if let Some(rows) = self.stock.data["datasets"]["holders"]["rows"]
                    .as_array()
                    .filter(|r| !r.is_empty())
                {
                    let width = ui.available_width();
                    let amount_width = (width * 0.26).clamp(82.0, 150.0);
                    let ratio_width = 64.0;
                    let name_width = (width - amount_width - ratio_width - 24.0).max(70.0);
                    egui::Grid::new(("research_holders", width.round() as u32))
                        .num_columns(3)
                        .striped(true)
                        .spacing([12.0, 2.0])
                        .show(ui, |ui| {
                            for (w, label, numeric) in [
                                (name_width, "股东名称", false),
                                (amount_width, "持有股数", true),
                                (ratio_width, "占比 %", true),
                            ] {
                                detail_design::cell(ui, w, label, numeric, muted());
                            }
                            ui.end_row();
                            for r in rows {
                                detail_design::cell(
                                    ui,
                                    name_width,
                                    value_text(&r["holder_name"]),
                                    false,
                                    text_color(),
                                );
                                detail_design::cell(
                                    ui,
                                    amount_width,
                                    money(number(r, "hold_amount")),
                                    true,
                                    text_color(),
                                );
                                detail_design::cell(
                                    ui,
                                    ratio_width,
                                    num(r, "hold_ratio"),
                                    true,
                                    text_color(),
                                );
                                ui.end_row();
                            }
                        });
                } else {
                    detail_design::caption(ui, "暂无股东数据");
                }
                self.dataset_note(ui, "holders");
            },
        );
    }
    fn funds_panel(&self, ui: &mut egui::Ui) {
        let rows = self.stock.data["datasets"]["funds"]["rows"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        detail_design::section(
            ui,
            "资金流向",
            "金额 / 元 · 主动买卖成交分类",
            |ui| {
                if let Some(last) = rows.first() {
                    metrics(
                        ui,
                        &[
                            ("净流入（供应商口径）", money(number(last, "net_mf_amount"))),
                            ("大单净额", money(number(last, "large_net"))),
                            ("超大单净额", money(number(last, "extra_large_net"))),
                            ("近5日净流入", sum_money(&rows, 5)),
                            ("近20日净流入", sum_money(&rows, 20)),
                        ],
                    );
                } else {
                    detail_design::caption(ui, "暂无资金数据，刷新后显示最近60个交易日。");
                }
                self.dataset_note(ui, "funds");
                ui.collapsing("资金统计口径",|ui|{detail_design::caption(ui,"净流入采用供应商原字段，可能与分类差额之和不同。大单20–100万元，超大单≥100万元。");});
                if rows.is_empty() {
                    return;
                }
                ui.add_space(6.0);
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    let width = ui.available_width().max(660.0);
                    let value_width = ((width - 100.0 - 60.0) / 5.0).max(100.0);
                    egui::Grid::new(("funds_rows", width.round() as u32))
                        .num_columns(6)
                        .striped(true)
                        .spacing([12.0, 2.0])
                        .show(ui, |ui| {
                            for (i, label) in [
                                "交易日期",
                                "净流入",
                                "大单净额",
                                "超大单净额",
                                "大单买入",
                                "大单卖出",
                            ]
                            .iter()
                            .enumerate()
                            {
                                detail_design::cell(
                                    ui,
                                    if i == 0 { 100.0 } else { value_width },
                                    *label,
                                    i > 0,
                                    muted(),
                                );
                            }
                            ui.end_row();
                            for r in &rows {
                                detail_design::cell(
                                    ui,
                                    100.0,
                                    detail_design::date(&value_text(&r["trade_date"])),
                                    false,
                                    muted(),
                                );
                                for (i, field) in [
                                    "net_mf_amount",
                                    "large_net",
                                    "extra_large_net",
                                    "buy_lg_amount",
                                    "sell_lg_amount",
                                ]
                                .iter()
                                .enumerate()
                                {
                                    let n = number(r, field);
                                    let color = if i >= 3 || n.is_none_or(|v| v == 0.0) {
                                        text_color()
                                    } else if n.unwrap() > 0.0 {
                                        up_color()
                                    } else {
                                        down_color()
                                    };
                                    detail_design::cell(ui, value_width, money(n), true, color);
                                }
                                ui.end_row();
                            }
                        });
                });
            },
        );
    }
    fn announcements_panel(&self, ui: &mut egui::Ui) {
        detail_design::section(
            ui,
            "公司公告",
            "近一年披露记录 · 点击标题查看原文",
            |ui| {
                self.dataset_note(ui, "announcements");
                ui.hyperlink_to(
                    "巨潮资讯 · 官方公告查询 ↗",
                    "https://www.cninfo.com.cn/new/index",
                );
                ui.add_space(6.0);
                if let Some(rows) = self.stock.data["datasets"]["announcements"]["rows"]
                    .as_array()
                    .filter(|r| !r.is_empty())
                {
                    for r in rows {
                        ui.horizontal_top(|ui| {
                            ui.add_sized(
                                [96.0, 24.0],
                                egui::Label::new(
                                    egui::RichText::new(detail_design::date(&value_text(
                                        &r["ann_date"],
                                    )))
                                    .size(12.0)
                                    .color(muted()),
                                ),
                            );
                            let width = ui.available_width();
                            ui.allocate_ui_with_layout(
                                Vec2::new(width, 0.0),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.set_width(width);
                                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                                    let title = value_text(&r["title"]);
                                    if let Some(url) = r["url"]
                                        .as_str()
                                        .filter(|u| getrich_core::research::safe_url(u))
                                    {
                                        ui.hyperlink_to(title, url);
                                    } else {
                                        ui.add(egui::Label::new(title).wrap());
                                    }
                                },
                            );
                        });
                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(4.0);
                    }
                } else {
                    detail_design::caption(
                        ui,
                        if self.stock.data["datasets"]["announcements"]["error"].is_null() {
                            "近一年未返回公告记录。"
                        } else {
                            "公告获取未成功，请核对权限后重试。"
                        },
                    );
                }
            },
        );
    }
    pub(super) fn intraday_chart(&mut self, ui: &mut egui::Ui) {
        self.research_status(ui, "intraday");
        self.dataset_note(ui, "intraday");
        let rows = self.stock.data["datasets"]["intraday"]["rows"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if rows.is_empty() {
            card(ui, 0.0, |ui| {
                ui.set_min_height(250.0);
                ui.vertical_centered(|ui| {
                    ui.add_space(85.0);
                    ui.heading("暂无分时数据");
                    ui.label("分时使用真实 1 分钟成交数据，需要 A 股分钟接口权限。");
                });
            });
            return;
        }
        let prev = self.stock.data["datasets"]["intraday"]["prev_close"]
            .as_f64()
            .filter(|v| *v > 0.0);
        let prices: Vec<f64> = rows
            .iter()
            .flat_map(|r| [number(r, "close"), number(r, "vwap")])
            .flatten()
            .chain(prev)
            .collect();
        let low = prices.iter().copied().fold(f64::INFINITY, f64::min);
        let high = prices.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let center = prev.unwrap_or((low + high) / 2.0);
        let delta = (high - center)
            .abs()
            .max((center - low).abs())
            .max(center * 0.005)
            * 1.08;
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 360.0), Sense::hover());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 8.0, surface());
        let plot = Rect::from_min_max(
            rect.min + Vec2::new(64.0, 24.0),
            rect.max - Vec2::new(64.0, 82.0),
        );
        let volume = Rect::from_min_max(
            Pos2::new(plot.left(), plot.bottom() + 10.0),
            Pos2::new(plot.right(), rect.bottom() - 24.0),
        );
        let xy = |x: f64, y: f64| {
            Pos2::new(
                plot.left() + plot.width() * (x / 240.0) as f32,
                plot.bottom() - plot.height() * ((y - (center - delta)) / (2.0 * delta)) as f32,
            )
        };
        for i in 0..5 {
            let value = center + delta * (1.0 - i as f64 / 2.0);
            let y = xy(0.0, value).y;
            p.line_segment(
                [Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
                Stroke::new(0.5_f32, design::border()),
            );
            p.text(
                Pos2::new(plot.left() - 8.0, y),
                egui::Align2::RIGHT_CENTER,
                format!("{value:.2}"),
                egui::FontId::proportional(12.0),
                muted(),
            );
            if let Some(prev) = prev {
                p.text(
                    Pos2::new(plot.right() + 8.0, y),
                    egui::Align2::LEFT_CENTER,
                    format!("{:+.2}%", (value / prev - 1.0) * 100.0),
                    egui::FontId::proportional(12.0),
                    if value >= prev {
                        up_color()
                    } else {
                        down_color()
                    },
                );
            }
        }
        for (x, label) in [
            (0.0, "09:30"),
            (60.0, "10:30"),
            (120.0, "11:30 / 13:00"),
            (180.0, "14:00"),
            (240.0, "15:00"),
        ] {
            let pos = xy(x, center);
            p.text(
                Pos2::new(pos.x, rect.bottom() - 8.0),
                egui::Align2::CENTER_BOTTOM,
                label,
                egui::FontId::proportional(12.0),
                muted(),
            );
        }
        for (field, color) in [
            ("close", accent()),
            ("vwap", Color32::from_rgb(215, 165, 65)),
        ] {
            let mut previous: Option<(f64, Pos2)> = None;
            for r in &rows {
                if let (Some(x), Some(y)) = (number(r, "minute"), number(r, field)) {
                    let pos = xy(x, y);
                    if let Some((old_x, old_pos)) = previous {
                        if x - old_x <= 2.0 {
                            p.line_segment([old_pos, pos], Stroke::new(1.5_f32, color));
                        }
                    }
                    previous = Some((x, pos));
                }
            }
        }
        let maxvol = rows
            .iter()
            .filter_map(|r| number(r, "volume"))
            .fold(1.0, f64::max);
        for r in &rows {
            if let (Some(x), Some(vol)) = (number(r, "minute"), number(r, "volume")) {
                let x = xy(x, center).x;
                let h = volume.height() * (vol / maxvol) as f32;
                p.line_segment(
                    [
                        Pos2::new(x, volume.bottom()),
                        Pos2::new(x, volume.bottom() - h),
                    ],
                    Stroke::new(
                        (plot.width() / 300.0).clamp(1.0, 3.0),
                        accent().gamma_multiply(0.55),
                    ),
                );
            }
        }
        if let Some(pos) = response.hover_pos().filter(|p| plot.contains(*p)) {
            let minute = ((pos.x - plot.left()) / plot.width() * 240.0) as f64;
            if let Some(row) = rows.iter().min_by(|a, b| {
                (number(a, "minute").unwrap_or(0.0) - minute)
                    .abs()
                    .total_cmp(&(number(b, "minute").unwrap_or(0.0) - minute).abs())
            }) {
                let x = xy(number(row, "minute").unwrap_or(0.0), center).x;
                p.line_segment(
                    [Pos2::new(x, plot.top()), Pos2::new(x, volume.bottom())],
                    Stroke::new(0.5_f32, muted()),
                );
                response.on_hover_text(format!(
                    "{}\n价格 {}  均价 {}\n成交量 {} 股  成交额 {}",
                    value_text(&row["time"]),
                    num(row, "close"),
                    num(row, "vwap"),
                    money(number(row, "volume")),
                    money(number(row, "amount"))
                ));
            }
        }
        ui.small("蓝线：成交价  ·  金线：成交量加权均价  ·  下方：分钟成交量；缺失分钟保留断点，不插值。");
    }
}
fn number(v: &Value, field: &str) -> Option<f64> {
    v[field]
        .as_f64()
        .or_else(|| v[field].as_str()?.parse().ok())
}
fn value_text(v: &Value) -> String {
    match v {
        Value::Null => "—".into(),
        Value::String(s) => s.clone(),
        _ => v.to_string(),
    }
}
fn num(v: &Value, f: &str) -> String {
    number(v, f)
        .map(|n| format!("{n:.2}"))
        .unwrap_or_else(|| "—".into())
}
fn money(n: Option<f64>) -> String {
    n.map(|n| {
        if n.abs() >= 1e8 {
            format!("{:.2} 亿", n / 1e8)
        } else if n.abs() >= 1e4 {
            format!("{:.2} 万", n / 1e4)
        } else {
            format!("{n:.2}")
        }
    })
    .unwrap_or_else(|| "—".into())
}
fn sum_money(rows: &[Value], count: usize) -> String {
    if rows.len() < count {
        return format!("不足 {count} 个交易日");
    }
    let values: Option<Vec<f64>> = rows
        .iter()
        .take(count)
        .map(|r| number(r, "net_mf_amount"))
        .collect();
    money(values.map(|v| v.iter().sum()))
}
pub(super) fn metrics(ui: &mut egui::Ui, fields: &[(&str, String)]) {
    detail_design::metrics(ui, fields);
}
