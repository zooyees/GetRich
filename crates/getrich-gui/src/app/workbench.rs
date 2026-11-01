use super::*;
use crate::controller::Payload;
use std::collections::{HashMap, HashSet};
pub(super) struct WorkbenchState {
    pub catalog: Value,
    pub tool: String,
    pub group: String,
    pub forms: HashMap<String, Value>,
    pub results: HashMap<String, Value>,
    pub errors: HashMap<String, String>,
    pub jobs: HashMap<String, i64>,
    pub tick: Option<Instant>,
    pub badge_tick: Option<Instant>,
    pub unread: i64,
    pub last_alert: i64,
    pub hidden: HashSet<String>,
    pub selected: Option<(Vec<Value>, Value)>,
}
impl Default for WorkbenchState {
    fn default() -> Self {
        Self {
            catalog: Value::Null,
            tool: "overview".into(),
            group: "概览".into(),
            forms: HashMap::new(),
            results: HashMap::new(),
            errors: HashMap::new(),
            jobs: HashMap::new(),
            tick: None,
            badge_tick: None,
            unread: 0,
            last_alert: 0,
            hidden: HashSet::new(),
            selected: None,
        }
    }
}
fn display(v: &Value) -> String {
    match v {
        Value::Null => "—".into(),
        Value::String(s) => s.clone(),
        Value::Bool(v) => if *v { "是" } else { "否" }.into(),
        Value::Number(n) => n
            .as_i64()
            .map(|n| n.to_string())
            .unwrap_or_else(|| format!("{:.2}", n.as_f64().unwrap_or(0.0))),
        _ => v.to_string(),
    }
}
impl GetRichApp {
    pub(super) fn open_workbench(&mut self) {
        self.screen = Screen::Workbench;
        if self.workbench.catalog.is_null() {
            let _ = self.controller.submit("read:workbench:catalog", |core| {
                getrich_core::professional::execute(core, "catalog", &json!({}))
                    .map(|v| Payload::Workbench("catalog".into(), v))
                    .map_err(|e| e.to_string())
            });
        }
        if !self.workbench.results.contains_key("overview") {
            self.run_workbench("overview", json!({}));
        }
    }
    fn load_workbench_form(&mut self, request: &Value) {
        let id = request["tool"].as_str().unwrap_or("").to_string();
        self.workbench
            .forms
            .insert(id.clone(), request["values"].clone());
        if let Some(group) = self.workbench.catalog["tools"]
            .as_array()
            .and_then(|tools| tools.iter().find(|t| t["id"] == id))
            .and_then(|t| t["group"].as_str())
        {
            self.workbench.group = group.into();
        }
        self.workbench.tool = id;
    }
    pub(super) fn run_workbench(&mut self, tool: &str, values: Value) {
        let tool = tool.to_string();
        let tag = format!("action:workbench:{tool}");
        if self.controller.is_pending(&tag) {
            return;
        }
        self.workbench.errors.remove(&tool);
        let key = tool.clone();
        if let Err(e) = self.controller.submit(&tag, move |core| {
            getrich_core::professional::execute(core, &tool, &values)
                .map(|v| Payload::Workbench(key, v))
                .map_err(|e| e.to_string())
        }) {
            self.error = Some(e);
        }
    }
    pub(super) fn apply_workbench(&mut self, tool: String, value: Value) {
        if tool == "_badge" {
            self.workbench.unread = value["unread"].as_i64().unwrap_or(0);
            let latest = value["latest_id"].as_i64().unwrap_or(0);
            if latest > self.workbench.last_alert {
                self.ui_notice = Some((
                    format!(
                        "有 {} 条未读预警，可在专业工作台查看",
                        self.workbench.unread
                    ),
                    Instant::now(),
                ));
            }
            self.workbench.last_alert = self.workbench.last_alert.max(latest);
            return;
        }
        if tool == "catalog" {
            self.workbench.catalog = value;
            return;
        }
        if let Some(id) = value["job_id"].as_i64() {
            self.workbench.jobs.insert(tool.clone(), id);
        }
        if let Some(patch) = value["form_patch"].as_object() {
            if let Some(form) = self
                .workbench
                .forms
                .get_mut(&tool)
                .and_then(Value::as_object_mut)
            {
                for (k, v) in patch {
                    form.insert(k.clone(), v.clone());
                }
            }
        }
        if value["open_research"].is_object() {
            let request = &value["open_research"];
            let symbol = request["symbol"].as_str().unwrap_or("");
            let market = request["market"].as_str().unwrap_or("");
            let mode = request["mode"].as_str().unwrap_or("swing");
            let id = format!("{market}:{symbol}:{mode}");
            if self
                .stock
                .drafts
                .get(&id)
                .is_some_and(|v| v["_dirty"] == true)
            {
                self.workbench.errors.insert(
                    tool.clone(),
                    "该股票已有未保存草稿，请先保存或放弃，再应用模板".into(),
                );
            } else {
                self.stock.drafts.insert(id, request["draft"].clone());
                self.navigate_stock(symbol, market);
                self.preferences.research_mode = mode.into();
                self.stock.tab = 4;
                self.read_research();
            }
        }
        self.workbench.results.insert(tool, value);
    }
    pub(super) fn tick_workbench(&mut self) {
        if self
            .workbench
            .badge_tick
            .is_none_or(|t| t.elapsed() > Duration::from_secs(15))
        {
            self.workbench.badge_tick = Some(Instant::now());
            let _ = self
                .controller
                .submit_background("read:workbench-alert-badge", |core| {
                    getrich_core::professional::execute(core, "alerts.badge", &json!({}))
                        .map(|v| Payload::Workbench("_badge".into(), v))
                        .map_err(|e| e.to_string())
                });
        }
        if self.screen != Screen::Workbench
            || self
                .workbench
                .tick
                .is_some_and(|t| t.elapsed() < Duration::from_millis(1500))
        {
            return;
        }
        self.workbench.tick = Some(Instant::now());
        let jobs: Vec<_> = self
            .workbench
            .jobs
            .iter()
            .map(|(t, id)| (t.clone(), *id))
            .collect();
        for (tool, id) in jobs {
            let tag = format!("read:workbench-job:{id}");
            let _=self.controller.submit_background(&tag,move|core|{let job=core.job(id).map_err(|e|e.to_string())?;if matches!(job["status"].as_str(),Some("succeeded"|"partial")){let mut v=job["result"].clone();v["job_finished"]=json!(true);Ok(Payload::Workbench(tool,v))}else if matches!(job["status"].as_str(),Some("failed"|"cancelled"|"interrupted")){Ok(Payload::Workbench(tool,json!({"job_finished":true,"title":"任务未完成","notices":[job["error"].as_str().unwrap_or("任务已取消或中断")]})))}else{Ok(Payload::Workbench(format!("progress:{tool}"),json!({"phase":job["phase"],"progress":job["progress"]})))}});
        }
    }
    pub(super) fn ui_workbench(&mut self, ctx: &egui::Context) {
        page_canvas(ctx, "professional-workbench", 1600.0, |ui| {
            page_title(
                ui,
                "PROFESSIONAL / 专业研究",
                "专业工作台",
                "统一的数据、研究规则与可审查结果，保存在本地数据库。",
            );
            ui.horizontal_wrapped(|ui| {
                for group in ["概览", "数据", "分析", "研究", "预警", "组合", "回测"]
                {
                    if ui
                        .selectable_label(self.workbench.group == group, group)
                        .clicked()
                    {
                        self.workbench.group = group.into();
                        if let Some(tool) = self.workbench.catalog["tools"]
                            .as_array()
                            .and_then(|tools| tools.iter().find(|t| t["group"] == group))
                        {
                            self.workbench.tool = tool["id"].as_str().unwrap_or("overview").into();
                            self.workbench.selected = None;
                        }
                    }
                }
            });
            let tools = self.workbench.catalog["tools"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            ui.horizontal_wrapped(|ui| {
                for tool in tools.iter().filter(|t| t["group"] == self.workbench.group) {
                    let id = tool["id"].as_str().unwrap_or("");
                    if ui
                        .selectable_label(
                            self.workbench.tool == id,
                            tool["label"].as_str().unwrap_or(""),
                        )
                        .clicked()
                    {
                        self.workbench.tool = id.into();
                        self.workbench.selected = None;
                    }
                }
            });
            ui.add_space(10.0);
            let current = self.workbench.tool.clone();
            let schema = tools
                .iter()
                .find(|t| t["id"] == current)
                .cloned()
                .unwrap_or(json!({"label":"工作台概览","fields":[]}));
            let busy = self
                .controller
                .is_pending(&format!("action:workbench:{current}"))
                || self.workbench.jobs.contains_key(&current);
            let mut form = self
                .workbench
                .forms
                .get(&current)
                .cloned()
                .unwrap_or_else(|| {
                    let mut v = json!({});
                    for field in schema["fields"].as_array().into_iter().flatten() {
                        v[field["id"].as_str().unwrap_or("")] = field["default"].clone();
                    }
                    v
                });
            card(ui, 0.0, |ui| {
                ui.heading(schema["label"].as_str().unwrap_or("专业研究"));
                ui.label(schema["help"].as_str().unwrap_or(""));
                let fields: Vec<_> = schema["fields"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|f| f["kind"] != "hidden")
                    .cloned()
                    .collect();
                let cols = if ui.available_width() > 950.0 {
                    3
                } else if ui.available_width() > 520.0 {
                    2
                } else {
                    1
                };
                ui.add_enabled_ui(!busy, |ui| {
                    let small: Vec<_> = fields
                        .iter()
                        .filter(|f| !matches!(f["kind"].as_str(), Some("csv" | "multiline")))
                        .collect();
                    for chunk in small.chunks(cols) {
                        ui.columns(cols, |columns| {
                            for (field, col) in chunk.iter().zip(columns) {
                                draw_field(col, field, &mut form, &current);
                            }
                        });
                    }
                    for field in fields
                        .iter()
                        .filter(|f| matches!(f["kind"].as_str(), Some("csv" | "multiline")))
                    {
                        draw_field(ui, field, &mut form, &current);
                    }
                    if current == "data.import" {
                        let dataset = form["dataset"].as_str().unwrap_or("factors");
                        ui.small(format!(
                            "表头：{}",
                            self.workbench.catalog["csv_headers"][dataset]
                                .as_str()
                                .unwrap_or("")
                        ));
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            !busy,
                            primary_btn(if busy { "后台处理中…" } else { "执行" }),
                        )
                        .clicked()
                    {
                        self.run_workbench(&current, form.clone());
                    }
                    if fields.iter().any(|f| f["id"] == "symbol")
                        && !busy
                        && ui.button("使用当前个股").clicked()
                    {
                        if let Some(key) = &self.pending_key {
                            form["symbol"] = json!(key.symbol);
                            form["market"] = json!(key.market.as_str());
                        }
                    }
                    if fields.iter().any(|f| f["kind"] == "csv")
                        && ui
                            .add_enabled(!busy, egui::Button::new("选择CSV文件"))
                            .clicked()
                    {
                        let target = current.clone();
                        let tag = format!("action:workbench-file:{target}");
                        let _ = self.controller.submit(&tag, move |_| {
                            let Some(path) = crate::file_dialog::select_import()? else {
                                return Ok(Payload::Workbench(
                                    format!("file:{target}"),
                                    Value::Null,
                                ));
                            };
                            let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
                            if meta.len() > 4_000_000 {
                                return Err("CSV文件最多4MB".into());
                            }
                            let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
                            Ok(Payload::Workbench(
                                format!("file:{target}"),
                                json!({"csv":raw}),
                            ))
                        });
                    }
                });
            });
            self.workbench.forms.insert(current.clone(), form);
            if let Some(error) = self.workbench.errors.get(&current) {
                ui.colored_label(up_color(), error);
            }
            if let Some(progress) = self.workbench.results.get(&format!("progress:{current}")) {
                ui.label(format!(
                    "{} · {}%",
                    progress["phase"].as_str().unwrap_or("执行中"),
                    progress["progress"]
                ));
            }
            if let Some(result) = self.workbench.results.get(&current).cloned() {
                self.workbench_result(ui, &current, &result);
            }
        });
    }
    fn workbench_result(&mut self, ui: &mut egui::Ui, tool: &str, result: &Value) {
        ui.add_space(12.0);
        card(ui, 0.0, |ui| {
            ui.heading(result["title"].as_str().unwrap_or("执行结果"));
            if ui.button("复制完整结果与口径").clicked() {
                ui.output_mut(|o| {
                    o.copied_text = serde_json::to_string_pretty(result).unwrap_or_default()
                });
            }
            let owned: Vec<_> = result["summary"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|r| (r["label"].as_str().unwrap_or(""), display(&r["value"])))
                .collect();
            stock_research::metrics(ui, &owned);
            for notice in result["notices"].as_array().into_iter().flatten() {
                ui.colored_label(muted(), notice.as_str().unwrap_or(""));
            }
            if let Some(id) = result["run_id"].as_i64() {
                ui.small(format!(
                    "运行 {id} · 输入指纹 {}",
                    result["fingerprint"].as_str().unwrap_or("")
                ));
            }
            if let Some(series) = result["series"].as_array() {
                draw_series(ui, series, &mut self.workbench.hidden);
            }
            for (i, table) in result["tables"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                ui.add_space(12.0);
                ui.strong(table["title"].as_str().unwrap_or("结果"));
                let columns = table["columns"].as_array().cloned().unwrap_or_default();
                let rows = table["rows"].as_array().cloned().unwrap_or_default();
                if ui.button("复制此表为CSV").clicked() {
                    ui.output_mut(|o| o.copied_text = export_csv(&columns, &rows));
                }
                egui::ScrollArea::both()
                    .id_salt((tool, i))
                    .max_height(340.0)
                    .show(ui, |ui| {
                        egui::Grid::new(("professional-table", tool, i))
                            .striped(true)
                            .spacing([8.0, 6.0])
                            .show(ui, |ui| {
                                for col in &columns {
                                    ui.add_sized(
                                        [120.0, 22.0],
                                        egui::Label::new(
                                            egui::RichText::new(
                                                col["label"].as_str().unwrap_or(""),
                                            )
                                            .strong(),
                                        )
                                        .truncate(),
                                    );
                                }
                                ui.label("操作");
                                ui.end_row();
                                for row in rows.iter().take(500) {
                                    for col in &columns {
                                        let key = col["key"].as_str().unwrap_or("");
                                        let text = display(&row[key]);
                                        ui.add_sized(
                                            [120.0, 22.0],
                                            egui::Label::new(text.clone()).truncate(),
                                        )
                                        .on_hover_text(text);
                                    }
                                    ui.horizontal(|ui| {
                                        if row["load"].is_object()
                                            && ui.small_button("载入").clicked()
                                        {
                                            self.load_workbench_form(&row["load"]);
                                        }
                                        if row["apply"].is_object()
                                            && ui.small_button("使用参数").clicked()
                                        {
                                            self.load_workbench_form(&row["apply"]);
                                        }
                                        if row["open_stock"].is_object()
                                            && ui.small_button("个股").clicked()
                                        {
                                            self.navigate_stock(
                                                row["open_stock"]["symbol"].as_str().unwrap_or(""),
                                                row["open_stock"]["market"].as_str().unwrap_or(""),
                                            );
                                        }
                                        if ui.small_button("详情").clicked() {
                                            self.workbench.selected =
                                                Some((columns.clone(), row.clone()));
                                        }
                                    });
                                    ui.end_row();
                                }
                            });
                    });
            }
            if let Some((columns, row)) = self.workbench.selected.clone() {
                ui.separator();
                ui.strong("选中记录详情");
                for col in columns {
                    let k = col["key"].as_str().unwrap_or("");
                    ui.strong(col["label"].as_str().unwrap_or(""));
                    if k == "url" {
                        if let Some(url) = row[k]
                            .as_str()
                            .filter(|s| s.starts_with("https://") || s.starts_with("http://"))
                        {
                            ui.hyperlink_to("打开原文", url);
                        }
                    } else {
                        ui.label(display(&row[k]));
                    }
                }
            }
        });
    }
}
fn draw_field(ui: &mut egui::Ui, f: &Value, values: &mut Value, tool: &str) {
    let id = f["id"].as_str().unwrap_or("");
    let label = f["label"].as_str().unwrap_or("");
    let kind = f["kind"].as_str().unwrap_or("text");
    ui.label(label);
    if kind == "checkbox" {
        let mut value = values[id].as_bool().unwrap_or(false);
        ui.checkbox(&mut value, "启用");
        values[id] = json!(value);
        return;
    }
    if kind == "select" {
        let mut value = values[id].as_str().unwrap_or("").to_string();
        let options = f["options"].as_array().cloned().unwrap_or_default();
        let label = options
            .iter()
            .find(|o| o["value"] == value)
            .and_then(|o| o["label"].as_str())
            .unwrap_or("");
        egui::ComboBox::from_id_salt((tool, id))
            .selected_text(label)
            .width(ui.available_width().max(80.0))
            .show_ui(ui, |ui| {
                for o in options {
                    ui.selectable_value(
                        &mut value,
                        o["value"].as_str().unwrap_or("").into(),
                        o["label"].as_str().unwrap_or(""),
                    );
                }
            });
        values[id] = json!(value);
        return;
    }
    let mut text = if values[id].is_null() {
        String::new()
    } else if let Some(s) = values[id].as_str() {
        s.to_string()
    } else {
        values[id].to_string()
    };
    if matches!(kind, "csv" | "multiline") {
        ui.add(
            egui::TextEdit::multiline(&mut text)
                .desired_width(f32::INFINITY)
                .desired_rows(if kind == "csv" { 7 } else { 3 }),
        );
    } else {
        ui.add(
            egui::TextEdit::singleline(&mut text)
                .desired_width(f32::INFINITY)
                .margin(ui.spacing().button_padding),
        );
    }
    values[id] = json!(text);
}
fn export_csv(columns: &[Value], rows: &[Value]) -> String {
    let quote = |s: String| {
        let prefix = s
            .trim_start()
            .chars()
            .next()
            .is_some_and(|c| matches!(c, '=' | '+' | '-' | '@'));
        let s = if prefix && s.parse::<f64>().is_err() {
            format!("'{s}")
        } else {
            s
        };
        format!("\"{}\"", s.replace('"', "\"\""))
    };
    let mut lines = vec![columns
        .iter()
        .map(|c| quote(c["label"].as_str().unwrap_or("").to_string()))
        .collect::<Vec<_>>()
        .join(",")];
    for row in rows {
        lines.push(
            columns
                .iter()
                .map(|c| quote(display(&row[c["key"].as_str().unwrap_or("")])))
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    lines.join("\r\n")
}
fn draw_series(ui: &mut egui::Ui, series: &[Value], hidden: &mut HashSet<String>) {
    let palette = [
        Color32::from_rgb(118, 167, 255),
        Color32::from_rgb(226, 169, 65),
        Color32::from_rgb(62, 202, 180),
        Color32::from_rgb(198, 142, 240),
        Color32::from_rgb(230, 92, 119),
        Color32::from_rgb(179, 193, 213),
        Color32::from_rgb(224, 196, 100),
    ];
    let mut panes = Vec::<String>::new();
    for s in series {
        let pane = s["pane"].as_str().unwrap_or("曲线").to_string();
        if !panes.contains(&pane) {
            panes.push(pane);
        }
    }
    for pane in panes {
        ui.strong(&pane);
        let curves: Vec<_> = series.iter().filter(|s| s["pane"] == pane).collect();
        ui.horizontal_wrapped(|ui| {
            for (i, s) in curves.iter().enumerate() {
                let name = s["name"].as_str().unwrap_or("");
                let mut enabled = !hidden.contains(name);
                if ui
                    .checkbox(
                        &mut enabled,
                        egui::RichText::new(name).color(palette[i % palette.len()]),
                    )
                    .changed()
                {
                    if enabled {
                        hidden.remove(name);
                    } else {
                        hidden.insert(name.into());
                    }
                }
            }
        });
        let visible: Vec<_> = curves
            .iter()
            .filter(|s| !hidden.contains(s["name"].as_str().unwrap_or("")))
            .collect();
        let vals: Vec<_> = visible
            .iter()
            .flat_map(|s| {
                s["points"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| p["value"].as_f64())
            })
            .collect();
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(
                ui.available_width(),
                if pane == "价格" { 230.0 } else { 160.0 },
            ),
            Sense::hover(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, background());
        if vals.is_empty() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "没有可绘制数据",
                egui::FontId::proportional(13.0),
                muted(),
            );
            continue;
        }
        let mut low = vals.iter().copied().fold(f64::INFINITY, f64::min);
        let mut high = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if pane == "RSI" {
            low = 0.0;
            high = 100.0;
        } else if pane == "MACD" {
            low = low.min(0.0);
            high = high.max(0.0);
        } else if pane == "ATR" {
            low = 0.0;
        }
        if (high - low).abs() < 0.000001 {
            let padding = (high.abs() * 0.001).max(0.001);
            low -= padding / 2.0;
            high += padding / 2.0;
        }
        let span = high - low;
        let plot = rect.shrink2(Vec2::new(52.0, 20.0));
        for i in 0..=4 {
            let y = plot.top() + plot.height() * i as f32 / 4.0;
            painter.line_segment(
                [Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
                Stroke::new(0.5_f32, design::border()),
            );
            painter.text(
                Pos2::new(plot.left() - 5.0, y),
                egui::Align2::RIGHT_CENTER,
                format!("{:.2}", high - span * i as f64 / 4.0),
                egui::FontId::monospace(10.0),
                muted(),
            );
        }
        for s in &visible {
            let j = curves
                .iter()
                .position(|c| c["name"] == s["name"])
                .unwrap_or(0);
            let points = s["points"].as_array().cloned().unwrap_or_default();
            let mut previous = None;
            for (i, p) in points.iter().enumerate() {
                if let Some(v) = p["value"].as_f64() {
                    let point = Pos2::new(
                        plot.left()
                            + i as f32 / (points.len().saturating_sub(1).max(1)) as f32
                                * plot.width(),
                        plot.bottom() - ((v - low) / span) as f32 * plot.height(),
                    );
                    if s["style"] == "histogram" {
                        let zero = plot.bottom() - ((0.0 - low) / span) as f32 * plot.height();
                        let half =
                            (plot.width() / points.len().max(1) as f32 * 0.35).clamp(0.5, 5.0);
                        painter.rect_filled(
                            Rect::from_two_pos(
                                Pos2::new(point.x - half, zero),
                                Pos2::new(point.x + half, point.y),
                            ),
                            0.0,
                            if v >= 0.0 { up_color() } else { down_color() },
                        );
                        previous = None;
                        continue;
                    }
                    if let Some(prior) = previous {
                        painter.line_segment(
                            [prior, point],
                            Stroke::new(1.2_f32, palette[j % palette.len()]),
                        );
                    }
                    previous = Some(point);
                } else {
                    previous = None;
                }
            }
            if response.hovered() {
                if let Some(pointer) = response.hover_pos() {
                    let i = (((pointer.x - plot.left()) / plot.width()).clamp(0.0, 1.0)
                        * points.len().saturating_sub(1) as f32)
                        .round() as usize;
                    if let Some(p) = points.get(i) {
                        response.clone().on_hover_text(format!(
                            "{} · {}: {}",
                            p["date"].as_str().unwrap_or(""),
                            s["name"].as_str().unwrap_or(""),
                            display(&p["value"])
                        ));
                    }
                }
            }
        }
    }
}
