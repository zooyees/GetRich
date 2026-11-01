use super::*;
use crate::controller::Payload;
impl GetRichApp {
    pub(super) fn select_research_mode(&mut self, mode: &str) {
        if getrich_core::analysis::research_mode(Some(mode)).is_err()
            || self.preferences.research_mode == mode
        {
            return;
        }
        self.preferences.research_mode = mode.into();
        self.submit_action(
            "research-mode",
            "settings.save",
            json!({"preferences":{"research_mode":mode}}),
        );
        self.read_research();
    }
    pub(super) fn analysis_panel(&mut self, ui: &mut egui::Ui) {
        let Some(key) = self.pending_key.clone() else {
            return;
        };
        let mode = self.preferences.research_mode.clone();
        let id = format!("{}:{}:{}", key.market.as_str(), key.symbol, mode);
        let report = self.stock.data["analysis"].clone();
        ui.horizontal_wrapped(|ui| {
            for (id, label) in [
                ("intraday", "盘中短线"),
                ("swing", "日线波段"),
                ("value", "基本面长线"),
            ] {
                if ui.selectable_label(mode == id, label).clicked() {
                    self.select_research_mode(id);
                }
            }
            if ui
                .add_enabled(
                    !self.controller.is_pending("read:research"),
                    egui::Button::new("重算本地分析"),
                )
                .clicked()
            {
                self.read_research();
            }
            ui.small("本地计算 · 不触发服务器下载");
        });
        let section = self.research_section();
        self.research_status(ui, section);
        if report["symbol"].as_str() != Some(key.symbol.as_str())
            || report["market"].as_str() != Some(key.market.as_str())
            || report["mode"].as_str() != Some(self.preferences.research_mode.as_str())
            || (mode == "swing"
                && (if report["price_basis"] == "raw_unadjusted" {
                    "raw"
                } else {
                    report["price_basis"].as_str().unwrap_or("raw")
                }) != self.preferences.price_basis)
        {
            ui.label("正在读取本地研究数据…");
            return;
        }
        let workspace = &report["workspace"];
        if mode == "swing" {
            ui.small(format!(
                "{} — {} · {} 根日线 · 数据版本 {}",
                report["from"].as_str().unwrap_or("—"),
                report["as_of"].as_str().unwrap_or("—"),
                report["sample_count"],
                report["data_version"]
            ));
        } else {
            ui.small(format!(
                "研究截止 {} · 数据时间 {} · 样本 {}",
                report["evaluation_date"].as_str().unwrap_or("—"),
                workspace["as_of"].as_str().unwrap_or("—"),
                workspace["sample_count"]
            ));
            if mode == "value" {
                ui.small(format!(
                    "财报期 {} · 公告日 {}",
                    workspace["report_period"].as_str().unwrap_or("—"),
                    workspace["disclosed_at"].as_str().unwrap_or("—")
                ));
            }
        }
        let warnings = if mode == "swing" {
            &report["warnings"]
        } else {
            &workspace["warnings"]
        };
        for warning in warnings.as_array().into_iter().flatten() {
            ui.colored_label(muted(), warning.as_str().unwrap_or(""));
        }
        let owned: Vec<(String, String)> = if mode == "swing" {
            [
                ("5日收益 %", "return_5_pct"),
                ("20日收益 %", "return_20_pct"),
                ("60日收益 %", "return_60_pct"),
                ("RSI14", "rsi14"),
                ("MA20", "ma20"),
                ("MA60", "ma60"),
                ("MACD DIF", "macd_dif"),
                ("MACD DEA", "macd_dea"),
                ("MACD 柱", "macd_histogram"),
                ("ATR14", "atr14"),
                ("20日年化波动 %", "volatility20_annual_pct"),
                ("60根最大回撤 %", "max_drawdown60_pct"),
                ("相对前20日均量", "volume_ratio20"),
            ]
            .iter()
            .map(|(label, name)| {
                (
                    label.to_string(),
                    report["metrics"][*name]
                        .as_f64()
                        .map(|v| format!("{v:.2}"))
                        .unwrap_or("—".into()),
                )
            })
            .collect()
        } else {
            workspace["metrics"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|r| {
                    (
                        r["label"].as_str().unwrap_or("").to_string(),
                        r["value"]
                            .as_f64()
                            .map(|v| format!("{v:.2}"))
                            .unwrap_or("—".into()),
                    )
                })
                .collect()
        };
        let fields: Vec<_> = owned.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
        super::stock_research::metrics(ui, &fields);
        ui.collapsing("计算口径与数据来源", |ui| {
            ui.label(workspace["methodology"].as_str().unwrap_or(""));
            if mode == "swing" {
                ui.label(format!("来源 {}", report["sources"]));
            }
            if let Some(sources) = workspace["provenance"].as_object() {
                for (name, p) in sources {
                    ui.label(format!(
                        "{name} · {} · 更新 {} · 状态 {}",
                        p["source"].as_str().unwrap_or("—"),
                        p["updated_at"].as_str().unwrap_or("—"),
                        p["status"].as_str().unwrap_or("—")
                    ));
                    if let Some(e) = p["error"].as_str() {
                        ui.colored_label(muted(), e);
                    }
                }
            }
        });
        ui.separator();
        ui.heading("研究记录");
        ui.small("记录假设、风险、观察条件与复盘；点击保存写入本地，草稿按股票与研究方式分别保留在当前会话。");
        let mut draft = self
            .stock
            .drafts
            .get(&id)
            .cloned()
            .unwrap_or_else(|| report["note"].clone());
        let saving = self.controller.is_pending("action:note");
        let labels = match mode.as_str() {
            "intraday" => [
                "盘中结构与触发证据",
                "失效条件与流动性风险",
                "观察价位与仓位纪律",
                "盘中执行复盘",
            ],
            "value" => [
                "商业逻辑与竞争优势",
                "财务风险与反证",
                "估值假设与跟踪事项",
                "财报更新与假设复核",
            ],
            _ => [
                "趋势结构与研究证据",
                "波段失效条件与风险",
                "观察区间与执行计划",
                "波段复盘记录",
            ],
        };
        let note_fields: Vec<_> = ["thesis", "risks", "plan", "review"]
            .into_iter()
            .zip(labels)
            .collect();
        let columns = if ui.available_width() > 760.0 { 2 } else { 1 };
        ui.add_enabled_ui(!saving, |ui| {
            for row in note_fields.chunks(columns) {
                ui.columns(columns, |cols| {
                    for ((field, label), col) in row.iter().zip(cols) {
                        col.label(*label);
                        let mut text = draft[*field].as_str().unwrap_or("").to_string();
                        if col
                            .add(
                                egui::TextEdit::multiline(&mut text)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(3),
                            )
                            .changed()
                        {
                            draft[*field] = json!(text);
                            draft["_dirty"] = json!(true);
                        }
                    }
                });
            }
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!saving, primary_btn("保存研究记录"))
                .clicked()
            {
                let mut p = draft.clone();
                p["symbol"] = json!(key.symbol);
                p["market"] = json!(key.market.as_str());
                p["mode"] = json!(mode);
                p["basis"] = json!(if report["price_basis"] == "raw_unadjusted" {
                    "raw"
                } else {
                    report["price_basis"].as_str().unwrap_or("raw")
                });
                p["factor_source"] = report["adjustment"]["source"].clone();
                p["analysis_as_of"] = report["evaluation_date"].clone();
                let k = key.clone();
                if let Err(e) = self.controller.submit("action:note", move |core| {
                    core.save_research_note(&p)
                        .map(|v| Payload::Notebook(k, v))
                        .map_err(|e| e.to_string())
                }) {
                    self.error = Some(e);
                }
            }
            if ui.button("放弃草稿，载入已保存版本").clicked() {
                draft = report["note"].clone();
                self.read_research();
            }
            ui.small(format!(
                "版本 {}{}",
                draft["revision"].as_i64().unwrap_or(0),
                if draft["_dirty"] == true {
                    " · 未保存"
                } else {
                    ""
                }
            ));
        });
        self.stock.drafts.insert(id.clone(), draft);
        if ui
            .add_enabled(
                !self.controller.is_pending("read:note-history"),
                egui::Button::new("查看最近100个历史版本与分析快照"),
            )
            .clicked()
        {
            let k = key.clone();
            let p = json!({"symbol":key.symbol,"market":key.market.as_str(),"mode":mode});
            if let Err(e) = self.controller.submit("read:note-history", move |core| {
                core.research_notes(&p, true)
                    .map(|v| Payload::NotebookHistory(k, v))
                    .map_err(|e| e.to_string())
            }) {
                self.error = Some(e);
            }
        }
        if let Some(rows) = self
            .stock
            .histories
            .get(&id)
            .and_then(|v| v["rows"].as_array())
        {
            for row in rows {
                ui.collapsing(
                    format!(
                        "版本 {} · {}",
                        row["revision"],
                        row["updated_at"].as_str().unwrap_or("")
                    ),
                    |ui| {
                        for (field, label) in [
                            ("thesis", "研究假设"),
                            ("risks", "风险"),
                            ("plan", "观察计划"),
                            ("review", "复盘"),
                        ] {
                            ui.strong(label);
                            ui.label(row[field].as_str().unwrap_or(""));
                        }
                        ui.small(format!(
                            "保存时数据日期 {} · 批次 {} · 价格口径 {}",
                            row["analysis_snapshot"]["workspace"]["as_of"]
                                .as_str()
                                .or_else(|| row["analysis_snapshot"]["as_of"].as_str())
                                .unwrap_or("—"),
                            row["analysis_snapshot"]["data_version"],
                            row["analysis_snapshot"]["price_basis"]
                        ));
                        if let Some(metrics) =
                            row["analysis_snapshot"]["workspace"]["metrics"].as_array()
                        {
                            let owned: Vec<_> = metrics
                                .iter()
                                .map(|r| {
                                    (
                                        r["label"].as_str().unwrap_or(""),
                                        r["value"]
                                            .as_f64()
                                            .map(|v| format!("{v:.2}"))
                                            .unwrap_or("—".into()),
                                    )
                                })
                                .collect();
                            super::stock_research::metrics(ui, &owned);
                        }
                    },
                );
            }
        }
        ui.separator();
        ui.heading("本地条件选股");
        let (mut watch, mut min_change, mut billions) =
            self.stock.screen_params.unwrap_or((false, 2.0, 1.0));
        ui.horizontal_wrapped(|ui|{
            ui.checkbox(&mut watch,"仅A股自选");ui.label("最低涨幅");ui.add(egui::DragValue::new(&mut min_change).range(-100.0..=100.0).suffix("%"));
            ui.label("最低成交额");ui.add(egui::DragValue::new(&mut billions).range(0.0..=100000.0).suffix("亿元"));
            if ui.add_enabled(!self.controller.is_pending("read:screen"),egui::Button::new("运行条件选股")).clicked(){let p=json!({"board":if watch{"WATCH"}else{"CN"},"min_change_pct":min_change,"min_turnover":billions*1e8});if let Err(e)=self.controller.submit("read:screen",move|core|core.research_screen(&p).map(Payload::Screen).map_err(|e|e.to_string())){self.error=Some(e);}}
        });
        self.stock.screen_params = Some((watch, min_change, billions));
        ui.horizontal_wrapped(|ui| {
            for (label, p) in [
                ("活跃股：涨幅≥2% / 成交额≥1亿元", json!({})),
                (
                    "A股自选：涨幅≥0%",
                    json!({"board":"WATCH","min_change_pct":0,"min_turnover":0}),
                ),
            ] {
                if ui
                    .add_enabled(
                        !self.controller.is_pending("read:screen"),
                        egui::Button::new(label),
                    )
                    .clicked()
                {
                    if let Err(e) = self.controller.submit("read:screen", move |core| {
                        core.research_screen(&p)
                            .map(Payload::Screen)
                            .map_err(|e| e.to_string())
                    }) {
                        self.error = Some(e);
                    }
                }
            }
        });
        let screen = self.stock.screen.clone();
        if !screen.is_null() {
            ui.small(format!(
                "日期 {} · 可用 {} · 排除 {} · 匹配 {}（最多显示100）",
                screen["as_of"], screen["eligible"], screen["excluded"], screen["matched"]
            ));
        }
        if let Some(rows) = screen["rows"].as_array() {
            for row in rows {
                ui.horizontal(|ui| {
                    if ui
                        .link(format!(
                            "{} {}",
                            row["symbol"].as_str().unwrap_or(""),
                            row["name"].as_str().unwrap_or("")
                        ))
                        .clicked()
                    {
                        self.navigate_stock(
                            row["symbol"].as_str().unwrap_or(""),
                            row["market"].as_str().unwrap_or(""),
                        );
                        self.stock.tab = 4;
                        self.read_research();
                    }
                    ui.label(format!(
                        "涨幅 {}% · 成交额 {}",
                        row["change_pct"], row["turnover"]
                    ));
                });
            }
        }
    }
}
