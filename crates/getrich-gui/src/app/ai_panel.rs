use super::*;
impl GetRichApp {
    pub(super) fn settings_ai(&mut self, ui: &mut egui::Ui) {
        card_title(
            ui,
            "AI 分析模型",
            "使用兼容 Chat Completions 的模型接口；Tushare 继续负责市场数据。",
        );
        let mut cfg = self
            .stock
            .ai_config
            .clone()
            .unwrap_or_else(|| self.settings_cache["ai"].clone());
        let mut enabled = cfg["enabled"].as_bool().unwrap_or(false);
        ui.checkbox(&mut enabled, "启用 AI 分析");
        cfg["enabled"] = json!(enabled);
        for (field, label, hint) in [
            ("base_url", "接口基础地址", "https://api.deepseek.com/v1"),
            ("model", "模型名称", "填写服务商提供的模型 ID"),
        ] {
            field_label(ui, label);
            let mut v = cfg[field].as_str().unwrap_or("").to_string();
            text_field(ui, &mut v, hint, false);
            cfg[field] = json!(v);
        }
        field_label(ui, "API Key（留空保留）");
        let configured = self.settings_cache["ai"]["key_configured"] == true;
        text_field(
            ui,
            &mut self.stock.ai_key,
            if configured {
                "已配置；输入新值可替换"
            } else {
                "输入模型服务 API Key"
            },
            true,
        );
        ui.small("凭据仅存本机配置；支持 GETRICH_AI_API_KEY 环境变量，接口与诊断不返回密钥。");
        ui.small("点击生成时，仅发送当前股票的公开行情、财务、资金与公告摘要；不会发送个人备注或持仓。模型服务可能计费。");
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!self.loading, primary_btn("保存 AI 配置"))
                .clicked()
            {
                let mut p = cfg.clone();
                if !self.stock.ai_key.trim().is_empty() {
                    p["api_key"] = json!(self.stock.ai_key.trim());
                }
                self.submit_action("ai-settings", "settings.save", json!({"ai":p}));
            }
            if configured && ui.button("清除本地密钥并停用").clicked() {
                self.submit_action(
                    "ai-settings",
                    "settings.save",
                    json!({"ai":{"api_key":"","enabled":false}}),
                );
            }
        });
        self.stock.ai_config = Some(cfg);
    }
    pub(super) fn ai_panel(&mut self, ui: &mut egui::Ui) {
        let Some(key) = self.pending_key.clone() else {
            return;
        };
        let mut data = self.stock.data["ai"].clone();
        let basis = if self.preferences.research_mode == "swing" {
            self.preferences.price_basis.as_str()
        } else {
            "raw"
        };
        if !data["latest"].is_null()
            && (data["latest"]["symbol"] != key.symbol
                || data["latest"]["market"] != key.market.as_str()
                || data["latest"]["mode"] != self.preferences.research_mode
                || data["latest"]["basis"] != basis)
        {
            data["latest"] = Value::Null;
            data["history"] = json!([]);
        }

        let settings = &data["settings"];
        let latest = &data["latest"];
        let running = latest["status"] == "running";
        detail_design::frame().show(ui,|ui|{
            ui.horizontal_wrapped(|ui|{
                detail_design::heading(ui,"AI 个股分析","");
                for (mode,label) in [("intraday","盘中短线"),("swing","日线波段"),("value","基本面长线")]{if ui.selectable_label(self.preferences.research_mode==mode,label).clicked(){self.select_research_mode(mode);}}
                if ui.add_enabled(settings["configured"]==true&&!running&&!self.loading,primary_btn(if running{"AI 正在分析…"}else{"生成 / 更新 AI 分析"})).clicked(){self.submit_action("ai-generate","ai.analysis.generate",json!({"symbol":key.symbol,"market":key.market.as_str(),"mode":self.preferences.research_mode,"basis":self.preferences.price_basis,"factor_source":self.preferences.adjustment_source}));}
                if ui.button("模型设置").clicked(){self.settings_tab=4;self.screen=Screen::Settings;self.read_settings();}
            });
            if settings["configured"]!=true {ui.label("尚未配置 AI 模型。完成设置后，由模型自动解读本地数据并生成报告。");}
            ui.small("手动触发模型分析；页面刷新只读取本地报告。输入不包含个人备注或持仓。");
            if running{ui.horizontal(|ui|{ui.spinner();ui.label("后台分析中，可继续浏览其他页面；完成后自动保存并显示。");});}
            if let Some(e)=latest["error"].as_str(){ui.colored_label(up_color(),e);}
            if latest["model_changed"]==true{ui.colored_label(muted(),"此报告由先前的模型配置生成，请按需更新。");}
            if latest["data_changed"]==true{ui.colored_label(muted(),"本地数据已有变化，当前显示的是生成时快照；可点击更新分析。");}
            if latest["report"].is_object(){Self::ai_report(ui,latest);}
            if let Some(history)=data["history"].as_array().filter(|r|r.len()>1){ui.collapsing("最近分析记录",|ui|{for r in history.iter().skip(1){ui.collapsing(format!("#{} · {} · {}",r["id"],r["created_at"].as_str().unwrap_or(""),r["status"].as_str().unwrap_or("")),|ui|{if r["report"].is_object(){Self::ai_report(ui,r);}else{ui.label(r["error"].as_str().unwrap_or("未完成"));}});}});}
        });
        ui.add_space(8.0);
    }
    fn ai_report(ui: &mut egui::Ui, r: &Value) {
        ui.small(format!(
            "{} · 生成 {} · 数据快照 #{}",
            r["model"].as_str().unwrap_or("—"),
            r["finished_at"].as_str().unwrap_or("—"),
            r["id"]
        ));
        ui.label(
            egui::RichText::new(r["report"]["summary"].as_str().unwrap_or(""))
                .size(15.0)
                .strong(),
        );
        let columns = if ui.available_width() >= 950.0 { 2 } else { 1 };
        for row in getrich_core::ai::SECTIONS.chunks(columns) {
            ui.columns(columns, |cols| {
                for ((id, title), col) in row.iter().zip(cols) {
                    if let Some(s) = r["report"]["sections"]
                        .as_array()
                        .and_then(|rows| rows.iter().find(|s| s["key"] == *id))
                    {
                        col.add_space(10.0);
                        col.separator();
                        col.strong(*title);
                        col.label(s["text"].as_str().unwrap_or(""));
                        col.small(format!(
                            "依据 {}",
                            s["evidence"]
                                .as_array()
                                .map(|v| v
                                    .iter()
                                    .filter_map(Value::as_str)
                                    .collect::<Vec<_>>()
                                    .join("、"))
                                .unwrap_or_default()
                        ));
                    }
                }
            });
        }
        ui.small("以上为模型解读，引用可追溯到生成时输入；引用存在不等于推断已被验证。数据更新不会自动重写旧报告。");
        if let Some(rows) = r["input"]["evidence"].as_array() {
            ui.collapsing("查看生成时的数据依据", |ui| {
                for e in rows {
                    ui.collapsing(
                        format!(
                            "{} · {}",
                            e["id"].as_str().unwrap_or(""),
                            e["kind"].as_str().unwrap_or("")
                        ),
                        |ui| {
                            ui.label(serde_json::to_string_pretty(e).unwrap_or_default());
                        },
                    );
                }
            });
        }
    }
}
