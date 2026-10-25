//! Management pages use bounded workspaces, consistent fields and a settings rail.
use super::*;
const SETTINGS_SECTIONS: &[(&str, &str)] = &[
    ("数据源连接", "账号与自动刷新"),
    ("行情与外观", "主题、默认板块与列"),
    ("自选管理", "添加、排序与导入"),
    ("存储与诊断", "本地文件与近期查看"),
];
impl GetRichApp {
    pub(super) fn ui_data(&mut self, ctx: &egui::Context) {
        page_canvas(ctx, "data-workspace", 1240.0, |ui| {
            page_title(
                ui,
                "WORKSPACE / 数据管理",
                "数据中心",
                "管理行情来源、导入文件与后台任务。",
            );
            if let Some(stats) = &self.stats {
                ui.columns(4, |cols| {
                    for (col, (label, value, note)) in cols.iter_mut().zip([
                        ("已导入标的", stats.listings.to_string(), "股票与指数"),
                        ("已存报价", stats.quotes.to_string(), "本地已保存"),
                        ("日线记录", stats.daily_bars.to_string(), "原始价格"),
                        (
                            "周线 / 月线",
                            format!("{} / {}", stats.weekly_bars, stats.monthly_bars),
                            "已存储记录",
                        ),
                    ]) {
                        card(col, 92.0, |ui| {
                            ui.colored_label(muted(), egui::RichText::new(label).size(12.0));
                            ui.label(egui::RichText::new(value).size(28.0).strong());
                            ui.colored_label(muted(), egui::RichText::new(note).size(11.0));
                        });
                    }
                });
            }
            ui.add_space(22.0);
            if ui.available_width() > 900.0 {
                design::paired_cards(ui, |left, right| {
                    self.data_import(left);
                    self.data_provider(right);
                });
            } else {
                card(ui, 0.0, |ui| self.data_import(ui));
                ui.add_space(20.0);
                card(ui, 0.0, |ui| self.data_provider(ui));
            }
            ui.add_space(22.0);
            card(ui, 160.0, |ui| self.data_jobs(ui));
        });
    }
    fn data_import(&mut self, ui: &mut egui::Ui) {
        card_title(
            ui,
            "导入本地文件",
            "预览校验结果后，再将有效记录写入本地库。",
        );
        field_label(ui, "文件路径");
        if text_field(
            ui,
            &mut self.import_path,
            "选择 JSON 文件，或输入数据目录内的路径",
            false,
        )
        .changed()
        {
            self.preview = None;
            self.import_data = None;
        }
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if action(ui, "选择文件", false).clicked() {
                self.select_import();
            }
            if action(ui, "使用演示文件", false).clicked() {
                self.import_path = "examples/demo.board.json".into();
                self.preview = None;
                self.import_data = None;
            }
            if ui
                .add_enabled(!self.loading, primary_btn("校验预览"))
                .clicked()
            {
                let params = if let Some(data) = &self.import_data {
                    json!({"data":data})
                } else {
                    json!({"path":self.import_path})
                };
                self.submit_action("preview", "import.preview", params);
            }
        });
        ui.add_space(12.0);
        ui.colored_label(
            muted(),
            egui::RichText::new(
                "支持 JSON 行情文件。预览不会修改数据库；重复记录会按时间与内容校验。",
            )
            .size(12.0),
        );
        ui.collapsing("文件格式说明",|ui|{
            ui.small("Bundle 可包含 listings（标的）、quotes（报价）与 bars（K 线）。相对路径从当前数据目录解析。");
        });
        if let Some(preview) = self.preview.clone() {
            ui.add_space(14.0);
            ui.separator();
            ui.add_space(4.0);
            let c = &preview["counts"];
            ui.strong("校验结果");
            ui.label(format!(
                "共 {} 条 · 新增 {} · 更新 {} · 不变 {}",
                c["requested"], c["inserted"], c["updated"], c["unchanged"]
            ));
            ui.colored_label(
                muted(),
                format!("陈旧 {} · 无效 {}", c["stale"], c["invalid"]),
            );
            if let Some(errors) = preview["errors"].as_array().filter(|e| !e.is_empty()) {
                ui.collapsing(format!("查看 {} 条异常", errors.len()), |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(160.0)
                        .show(ui, |ui| {
                            for error in errors {
                                ui.colored_label(
                                    up_color(),
                                    format!(
                                        "{} 第 {} 行 {}：{}",
                                        error["dataset"],
                                        error["row"],
                                        error["symbol"],
                                        error["message"]
                                    ),
                                );
                            }
                        });
                });
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        preview["can_apply"].as_bool().unwrap_or(false) && !self.loading,
                        primary_btn("应用有效记录"),
                    )
                    .clicked()
                {
                    let (kind, params) = if let Some(data) = &self.import_data {
                        (
                            "import.json",
                            json!({"data":data,"fingerprint":preview["fingerprint"]}),
                        )
                    } else {
                        (
                            "import.file",
                            json!({"path":self.import_path,"fingerprint":preview["fingerprint"]}),
                        )
                    };
                    self.submit_job(kind, params);
                    self.preview = None;
                }
            });
        }
    }
    fn data_provider(&mut self, ui: &mut egui::Ui) {
        card_title(ui, "同步行情数据", "按需更新报价、星图权重和大盘指数。");
        let configured = self.settings_cache["provider"]["configured"]
            .as_bool()
            .unwrap_or(false);
        ui.horizontal(|ui| {
            ui.strong("Tushare");
            status_badge(
                ui,
                if configured {
                    "已配置"
                } else {
                    "尚未连接"
                },
                configured,
            );
        });
        ui.add_space(8.0);
        for (title, note, kind, label) in [
            ("A 股行情", "股票列表与最新日线", "tushare.sync", "同步行情"),
            (
                "流通市值",
                "用于星图的同日面积权重",
                "tushare.valuations",
                "同步市值",
            ),
            (
                "大盘指数",
                "主要指数的收盘日线",
                "tushare.indices",
                "同步指数",
            ),
        ] {
            egui::Frame::none()
                .fill(secondary_surface())
                .rounding(7.0)
                .inner_margin(Margin::symmetric(12.0, 8.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(title).size(14.0));
                            ui.colored_label(muted(), egui::RichText::new(note).size(11.0));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_enabled(
                                    configured,
                                    egui::Button::new(label).min_size(Vec2::new(90.0, 34.0)),
                                )
                                .clicked()
                            {
                                self.submit_job(kind, json!({}));
                            }
                        });
                    });
                });
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if action(
                ui,
                if configured {
                    "管理数据源"
                } else {
                    "配置数据源"
                },
                !configured,
            )
            .clicked()
            {
                self.settings_tab = 0;
                self.read_settings();
                self.screen = Screen::Settings;
            }
            if ui.link("打开大盘星图 →").clicked() {
                self.screen = Screen::Pulse;
                self.reload_map();
            }
        });
    }
    fn data_jobs(&mut self, ui: &mut egui::Ui) {
        card_title(ui, "后台任务", "查看执行进度、校验异常与最终结果。");
        let jobs = self.jobs_cache["jobs"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if jobs.is_empty() {
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), 110.0),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    ui.add_space(12.0);
                    ui.label(egui::RichText::new("暂无后台任务").size(17.0));
                    ui.colored_label(muted(), "导入文件或同步数据后，进度与结果会显示在这里。");
                },
            );
            return;
        }
        for job in jobs {
            ui.separator();
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.strong(format!(
                    "#{}  {}",
                    job["id"],
                    job_title(job["kind"].as_str().unwrap_or(""))
                ));
                let status = job["status"].as_str().unwrap_or("");
                status_badge(ui, status_zh(status), status == "succeeded");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ["queued", "running"].contains(&status)
                        && job["phase"] != "正在原子写入"
                        && ui.button("取消任务").clicked()
                    {
                        self.submit_action("cancel", "job.cancel", json!({"id":job["id"]}));
                    }
                    if ["failed", "partial", "cancelled", "interrupted"].contains(&status)
                        && ui.button("重试任务").clicked()
                    {
                        self.submit_action("job", "job.retry", json!({"id":job["id"]}));
                    }
                });
            });
            ui.add(
                egui::ProgressBar::new(job["progress"].as_f64().unwrap_or(0.0) as f32 / 100.0)
                    .desired_height(6.0),
            );
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(muted(), status_zh(job["phase"].as_str().unwrap_or("")));
                ui.small(format!("{}%", job["progress"]));
            });
            if let Some(error) = job["error"].as_str() {
                ui.colored_label(up_color(), error);
            }
            if !job["result"].is_null() {
                ui.collapsing("完整结果", |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(
                                serde_json::to_string_pretty(&job["result"]).unwrap_or_default(),
                            )
                            .monospace()
                            .size(12.0),
                        )
                        .wrap(),
                    );
                });
            }
            if let Some(when) = job["finished_at"].as_str().or(job["created_at"].as_str()) {
                ui.colored_label(muted(), egui::RichText::new(shanghai_ts(when)).size(11.0));
            }
            ui.add_space(12.0);
        }
    }
    pub(super) fn ui_settings(&mut self, ctx: &egui::Context) {
        page_canvas(ctx, "settings-workspace", 1160.0, |ui| {
            page_title(
                ui,
                "PREFERENCES / 工作区偏好",
                "设置",
                "连接数据源，整理自选，建立适合你的研究工作区。",
            );
            if ui.available_width() > 900.0 {
                let width = ui.available_width();
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(190.0, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(190.0);
                            self.settings_navigation(ui, false);
                            ui.add_space(28.0);
                            ui.colored_label(
                                muted(),
                                egui::RichText::new("LOCAL WORKSPACE").size(10.0),
                            );
                            ui.colored_label(
                                muted(),
                                egui::RichText::new("设置保存在本机\n更改后请保存对应分区")
                                    .size(12.0),
                            );
                        },
                    );
                    ui.add_space(14.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(width - 214.0, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(width - 214.0);
                            self.settings_content(ui);
                        },
                    );
                });
            } else {
                ui.horizontal_wrapped(|ui| self.settings_navigation(ui, true));
                ui.add_space(12.0);
                self.settings_content(ui);
            }
        });
    }
    fn settings_navigation(&mut self, ui: &mut egui::Ui, compact: bool) {
        for (index, (title, note)) in SETTINGS_SECTIONS.iter().enumerate() {
            let selected = self.settings_tab == index;
            if compact {
                if chip(ui, title, selected).clicked() {
                    self.settings_tab = index;
                }
            } else {
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(190.0, 64.0), Sense::click());
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), *title)
                });
                if selected || response.hovered() {
                    ui.painter().rect_filled(rect, 8.0, selected_surface());
                }
                if selected {
                    ui.painter().rect_stroke(
                        rect,
                        8.0,
                        Stroke::new(1.0_f32, accent().gamma_multiply(0.35)),
                    );
                }
                ui.painter().text(
                    rect.min + Vec2::new(16.0, 16.0),
                    egui::Align2::LEFT_TOP,
                    *title,
                    egui::FontId::proportional(14.0),
                    if selected { accent() } else { text_color() },
                );
                ui.painter().text(
                    rect.min + Vec2::new(16.0, 38.0),
                    egui::Align2::LEFT_TOP,
                    *note,
                    egui::FontId::proportional(12.0),
                    muted(),
                );
                if response.clicked() {
                    self.settings_tab = index;
                }
            }
        }
    }
    fn settings_content(&mut self, ui: &mut egui::Ui) {
        card(ui, 470.0, |ui| match self.settings_tab {
            0 => self.settings_provider(ui),
            1 => self.settings_appearance(ui),
            2 => self.settings_watch(ui),
            _ => self.settings_storage(ui),
        });
    }
    fn settings_provider(&mut self, ui: &mut egui::Ui) {
        card_title(
            ui,
            "连接行情数据",
            "配置 Tushare，让行情与星图使用同一份本地数据。",
        );
        let configured = self.settings_cache["provider"]["configured"]
            .as_bool()
            .unwrap_or(false);
        ui.horizontal(|ui| {
            status_badge(
                ui,
                if configured {
                    "数据源已配置"
                } else {
                    "等待配置"
                },
                configured,
            );
            ui.colored_label(
                muted(),
                egui::RichText::new("支持沪、深、京 A 股").size(12.0),
            );
        });
        ui.add_space(12.0);
        ui.columns(2, |cols| {
            field_label(&mut cols[0], "用户 ID");
            text_field(
                &mut cols[0],
                &mut self.provider_user,
                "输入 Tushare 用户 ID",
                false,
            );
            field_label(&mut cols[1], "刷新间隔");
            cols[1].horizontal(|ui| {
                ui.add_sized(
                    [110.0, 38.0],
                    egui::DragValue::new(&mut self.provider_poll)
                        .range(1000..=300000)
                        .speed(100),
                );
                ui.colored_label(muted(), "毫秒");
            });
        });
        ui.add_space(10.0);
        field_label(ui, "访问 Token");
        let hint = self.settings_cache["provider"]["token_mask"]
            .as_str()
            .unwrap_or("输入 Token")
            .to_string();
        text_field(ui, &mut self.provider_token, &hint, true);
        ui.colored_label(
            muted(),
            egui::RichText::new("留空可保留已保存的 Token。环境变量中的凭据优先。").size(12.0),
        );
        ui.add_space(12.0);
        ui.checkbox(&mut self.provider_enabled, "交易时段自动刷新行情");
        ui.colored_label(
            muted(),
            egui::RichText::new("仅在数据源可用且处于交易时段时更新报价。").size(12.0),
        );
        ui.add_space(16.0);
        ui.separator();
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui|{
            if ui.add_enabled(!self.loading,primary_btn("保存数据源")).clicked(){
                let mut provider=json!({"user_id":self.provider_user,"enabled":self.provider_enabled,"poll_ms":self.provider_poll});
                if !self.provider_token.trim().is_empty(){provider["token"]=json!(self.provider_token);}
                self.submit_action("settings","settings.save",json!({"provider":provider}));
            }
            if ui.add_enabled(configured&&!self.loading,egui::Button::new("测试连接")).clicked(){self.submit_job("provider.test",json!({}));}
            if ui.add_enabled(configured,egui::Button::new("清除已保存 Token").frame(false)).clicked(){self.submit_action("settings","settings.save",json!({"provider":{"token":"","enabled":false}}));}
        });
        if let Some(error) = self.settings_cache["provider"]["last_error"].as_str() {
            ui.add_space(8.0);
            ui.colored_label(up_color(), error);
        }
        ui.add_space(12.0);
        ui.colored_label(
            muted(),
            egui::RichText::new("连接测试验证日历访问；行情、市值与指数权限由相应同步任务报告。")
                .size(12.0),
        );
    }
    fn settings_appearance(&mut self, ui: &mut egui::Ui) {
        card_title(ui, "行情与外观", "统一主题、涨跌配色与行情列表的展示方式。");
        ui.columns(2, |cols| {
            field_label(&mut cols[0], "界面主题");
            let label = match self.preferences.theme.as_str() {
                "light" => "浅色",
                "system" => "跟随系统",
                _ => "深色",
            };
            egui::ComboBox::from_id_salt("settings-theme")
                .width(cols[0].available_width())
                .selected_text(label)
                .show_ui(&mut cols[0], |ui| {
                    for (id, label) in [("light", "浅色"), ("dark", "深色"), ("system", "跟随系统")]
                    {
                        ui.selectable_value(&mut self.preferences.theme, id.into(), label);
                    }
                });
            field_label(&mut cols[1], "涨跌配色");
            egui::ComboBox::from_id_salt("settings-colors")
                .width(cols[1].available_width())
                .selected_text(if self.preferences.colors == "green_up" {
                    "绿涨红跌"
                } else {
                    "红涨绿跌"
                })
                .show_ui(&mut cols[1], |ui| {
                    ui.selectable_value(&mut self.preferences.colors, "red_up".into(), "红涨绿跌");
                    ui.selectable_value(
                        &mut self.preferences.colors,
                        "green_up".into(),
                        "绿涨红跌",
                    );
                });
        });
        ui.add_space(10.0);
        field_label(ui, "启动时默认板块");
        egui::ComboBox::from_id_salt("settings-default-board")
            .width(280.0)
            .selected_text(
                BOARDS
                    .iter()
                    .find(|(id, _)| *id == self.settings_default_board)
                    .map(|(_, label)| *label)
                    .unwrap_or("全部"),
            )
            .show_ui(ui, |ui| {
                for (id, label) in BOARDS {
                    ui.selectable_value(&mut self.settings_default_board, (*id).into(), *label);
                }
            });
        ui.add_space(10.0);
        field_label(ui, "行情列表宽度");
        ui.horizontal(|ui| {
            ui.add(
                egui::Slider::new(&mut self.preferences.list_width, 280.0..=900.0)
                    .show_value(false),
            );
            ui.add_sized(
                [64.0, 36.0],
                egui::DragValue::new(&mut self.preferences.list_width)
                    .range(280.0..=900.0)
                    .speed(10),
            );
            ui.colored_label(muted(), "px");
        });
        ui.add_space(6.0);
        field_label(ui, "工作区预设");
        ui.horizontal_wrapped(|ui| {
            for (label, board, width) in [("行情浏览", "ALL", 560.0), ("自选研究", "WATCH", 440.0)]
            {
                if ui.button(label).clicked() {
                    self.settings_default_board = board.into();
                    self.preferences.list_width = width;
                }
            }
        });
        ui.add_space(10.0);
        ui.collapsing(
            format!("显示列与排列顺序 · {} 列", self.preferences.columns.len()),
            |ui| self.settings_columns(ui),
        );
        ui.add_space(12.0);
        ui.separator();
        ui.add_space(6.0);
        if ui
            .add_enabled(!self.loading, primary_btn("保存外观"))
            .clicked()
        {
            self.submit_action(
                "settings",
                "settings.save",
                json!({"default_board":self.settings_default_board,"preferences":self.preferences}),
            );
        }
        ui.add_space(8.0);
        ui.colored_label(
            muted(),
            egui::RichText::new("K 线使用原始价格，周线、月线由日线合成。").size(12.0),
        );
    }
    fn settings_columns(&mut self, ui: &mut egui::Ui) {
        let mut order = self.preferences.columns.clone();
        for (id, _, _) in ALL_COLS {
            if !order.iter().any(|v| v == id) {
                order.push((*id).into());
            }
        }
        let mut move_column = None;
        for (index, id) in order.iter().enumerate() {
            let label = ALL_COLS
                .iter()
                .find(|(key, _, _)| key == id)
                .map(|(_, name, _)| *name)
                .unwrap_or(id);
            let mut enabled = self.preferences.columns.contains(id);
            ui.horizontal(|ui| {
                let fixed = ["symbol", "name"].contains(&id.as_str());
                if ui
                    .add_enabled(!fixed, egui::Checkbox::new(&mut enabled, label))
                    .changed()
                {
                    if enabled {
                        self.preferences.columns.push(id.clone());
                    } else {
                        self.preferences.columns.retain(|v| v != id);
                    }
                }
                if fixed {
                    ui.colored_label(muted(), egui::RichText::new("固定显示").size(11.0));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            index + 1 < order.len(),
                            egui::Button::new("下移").min_size(Vec2::splat(30.0)),
                        )
                        .on_hover_text("下移一列")
                        .clicked()
                    {
                        move_column = Some((index, index + 1));
                    }
                    if ui
                        .add_enabled(
                            index > 0,
                            egui::Button::new("上移").min_size(Vec2::splat(30.0)),
                        )
                        .on_hover_text("上移一列")
                        .clicked()
                    {
                        move_column = Some((index, index - 1));
                    }
                });
            });
        }
        if let Some((a, b)) = move_column {
            order.swap(a, b);
            self.preferences.columns = order
                .into_iter()
                .filter(|id| self.preferences.columns.contains(id))
                .collect();
        }
    }
    fn settings_watch(&mut self, ui: &mut egui::Ui) {
        card_title(
            ui,
            "自选管理",
            "将关注的股票收进同一个列表，支持跨市场管理。",
        );
        self.watch_search_ui(ui);
        ui.columns(3, |cols| {
            field_label(&mut cols[0], "股票代码");
            text_field(&mut cols[0], &mut self.watch_symbol, "如 600519", false);
            field_label(&mut cols[1], "交易市场");
            egui::ComboBox::from_id_salt("settings-watch-market")
                .width(cols[1].available_width())
                .selected_text(market_label(&self.watch_market))
                .show_ui(&mut cols[1], |ui| {
                    for market in ["CN.SH", "CN.SZ", "CN.BJ", "HK", "US"] {
                        ui.selectable_value(
                            &mut self.watch_market,
                            market.into(),
                            market_label(market),
                        );
                    }
                });
            field_label(&mut cols[2], "显示名称（可选）");
            text_field(&mut cols[2], &mut self.watch_name, "股票名称", false);
        });
        ui.add_space(4.0);
        if ui
            .add_enabled(
                !self.watch_symbol.trim().is_empty() && !self.loading,
                primary_btn("添加自选"),
            )
            .clicked()
        {
            self.submit_action("watch","watch.set",json!({"symbol":self.watch_symbol,"market":self.watch_market,"name":self.watch_name,"watched":true}));
        }
        ui.add_space(16.0);
        ui.separator();
        ui.add_space(4.0);
        let watch = self.settings_cache["watchlist"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if watch.is_empty() {
            ui.colored_label(muted(), "还没有自选股票，在上方输入代码即可添加。");
        }
        for (index, item) in watch.iter().enumerate() {
            egui::Frame::none().fill(if index%2==0{secondary_surface()}else{Color32::TRANSPARENT}).rounding(5.0).inner_margin(Margin::symmetric(10.0,4.0)).show(ui,|ui|{
                ui.set_width(ui.available_width());
                ui.horizontal(|ui|{
                    let width=(ui.available_width()-224.0).max(100.0);
                    ui.allocate_ui_with_layout(Vec2::new(width,42.0),egui::Layout::top_down(egui::Align::Min),|ui|{
                        ui.add(egui::Label::new(egui::RichText::new(item["name"].as_str().filter(|v|!v.is_empty()).unwrap_or_else(||item["symbol"].as_str().unwrap_or(""))).size(14.0)).truncate());
                        ui.colored_label(muted(),egui::RichText::new(format!("{} · {}",item["symbol"].as_str().unwrap_or(""),market_label(item["market"].as_str().unwrap_or("")))).size(11.0));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                        if ui.button("移除").clicked(){self.submit_action("watch","watch.set",json!({"symbol":item["symbol"],"market":item["market"],"watched":false}));}
                        if ui.add_enabled(index+1<watch.len(),egui::Button::new("下移")).clicked(){self.submit_action("watch","watch.move",json!({"symbol":item["symbol"],"market":item["market"],"direction":1}));}
                        if ui.add_enabled(index>0,egui::Button::new("上移")).clicked(){self.submit_action("watch","watch.move",json!({"symbol":item["symbol"],"market":item["market"],"direction":-1}));}
                    });
                });
            });
        }
        ui.add_space(12.0);
        ui.collapsing("批量导入与导出", |ui| {
            ui.colored_label(muted(), "粘贴自选 JSON 数组，可合并导入并自动去重。");
            ui.add_sized(
                [ui.available_width(), 110.0],
                egui::TextEdit::multiline(&mut self.watch_transfer)
                    .hint_text("[{\"symbol\":\"600519\",\"market\":\"CN.SH\"}]")
                    .font(egui::TextStyle::Monospace)
                    .margin(Vec2::splat(12.0)),
            );
            ui.horizontal_wrapped(|ui| {
                if ui.button("合并导入").clicked() {
                    match serde_json::from_str::<Value>(&self.watch_transfer) {
                        Ok(list) => {
                            self.submit_action("watch", "watch.import", json!({"watchlist":list}))
                        }
                        Err(error) => self.error = Some(format!("自选 JSON 格式有误：{error}")),
                    }
                }
                if ui.button("复制自选 JSON").clicked() {
                    ui.output_mut(|o| {
                        o.copied_text =
                            serde_json::to_string_pretty(&self.settings_cache["watchlist"])
                                .unwrap_or_default()
                    });
                }
            });
        });
    }
    fn settings_storage(&mut self, ui: &mut egui::Ui) {
        card_title(
            ui,
            "存储与诊断",
            "管理本地数据位置，查看近期记录和排障信息。",
        );
        for (key, label) in [
            ("data_root", "数据目录"),
            ("db_path", "数据库文件"),
            ("config_path", "配置文件"),
        ] {
            field_label(ui, label);
            let mut value = self.settings_cache[key]
                .as_str()
                .unwrap_or("读取中")
                .to_string();
            ui.horizontal(|ui| {
                ui.add_sized(
                    [(ui.available_width() - 78.0).max(100.0), 36.0],
                    egui::TextEdit::singleline(&mut value)
                        .interactive(false)
                        .font(egui::TextStyle::Monospace)
                        .margin(Vec2::new(10.0, 9.0)),
                );
                if ui.button("复制").clicked() {
                    ui.output_mut(|o| o.copied_text = value.clone());
                    self.ui_notice = Some((format!("{label}已复制"), Instant::now()));
                }
            });
            ui.add_space(6.0);
        }
        ui.add_space(8.0);
        if ui.button("复制诊断信息").clicked() {
            ui.output_mut(|o| {
                o.copied_text =
                    serde_json::to_string_pretty(&self.settings_cache).unwrap_or_default()
            });
            self.ui_notice = Some(("诊断信息已复制，凭据已屏蔽".into(), Instant::now()));
        }
        ui.add_space(12.0);
        ui.collapsing("近期查看", |ui| {
            let recent = self.settings_cache["preferences"]["recent"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if recent.is_empty() {
                ui.colored_label(muted(), "暂无近期记录。");
            }
            ui.horizontal_wrapped(|ui| {
                for item in recent {
                    if let Some(value) = item.as_str() {
                        if ui.button(value).clicked() {
                            if let Some((market, symbol)) = value.split_once(':') {
                                self.navigate_stock(symbol, market);
                            }
                        }
                    }
                }
            });
        });
        ui.collapsing("键盘快捷键", |ui| {
            ui.label(
                "↑ / ↓ 选股；1 / 2 / 3 切换 K 线周期；W 切换自选；/ 或 Ctrl+K 搜索；Esc 清除搜索。",
            );
        });
        ui.collapsing("调整数据位置", |ui| {
            ui.small(
                "通过 GETRICH_DATA_ROOT / GETRICH_DB 设置位置，重启后生效。迁移前请先备份数据库。",
            );
        });
    }
}
pub(super) fn market_label(market: &str) -> &str {
    match market {
        "CN.SH" => "上海 A 股",
        "CN.SZ" => "深圳 A 股",
        "CN.BJ" => "北证 A 股",
        "HK" => "港股",
        "US" => "美股",
        other => other,
    }
}
fn job_title(kind: &str) -> &str {
    match kind {
        "market.sync" => "刷新星图行情、市值与指数",
        "tushare.sync" => "同步 A 股行情",
        "tushare.valuations" => "同步流通市值",
        "tushare.indices" => "同步指数日线",
        "import.file" | "import.json" => "导入行情文件",
        "provider.test" => "数据源连接测试",
        "pulse.recompute" => "重算市场态势",
        "tushare.bars" => "补全历史日线",
        other => other,
    }
}
pub(super) fn status_zh(status: &str) -> &str {
    match status {
        "queued" => "等待执行",
        "running" => "执行中",
        "cancelling" => "正在取消",
        "cancelled" => "已取消",
        "succeeded" => "已完成",
        "partial" => "部分完成",
        "failed" => "失败",
        "interrupted" => "执行中断",
        other => other,
    }
}
