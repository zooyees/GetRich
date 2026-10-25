//! Durable, bounded task queue shared by desktop, web and attached CLI clients.
use crate::{
    db,
    error::AppError,
    import::{self, ImportKind},
    service::App,
};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

#[derive(Debug, Default)]
pub struct JobState {
    pub started: AtomicBool,
}

pub fn start(app: &Arc<App>) {
    if app.jobs.started.swap(true, Ordering::SeqCst) {
        return;
    }
    let worker = Arc::clone(app);
    if let Err(error) = std::thread::Builder::new()
        .name("getrich-jobs".into())
        .spawn(move || {
            while !worker.ingest.stop.load(Ordering::SeqCst) {
                if let Err(error) = tick(&worker) {
                    tracing::warn!(%error,"job worker");
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            worker.jobs.started.store(false, Ordering::SeqCst);
        })
    {
        app.jobs.started.store(false, Ordering::SeqCst);
        tracing::error!(%error,"job worker start failed");
    }
}

impl App {
    pub fn submit_job(&self, kind: &str, params: Value) -> Result<Value, AppError> {
        if ![
            "import.file",
            "import.json",
            "tushare.sync",
            "market.sync",
            "tushare.bars",
            "stock.research",
            "tushare.valuations",
            "tushare.indices",
            "provider.test",
            "pulse.recompute",
        ]
        .contains(&kind)
        {
            return Err(AppError::InvalidArg("不支持的任务类型".into()));
        }
        let parameters =
            serde_json::to_string(&params).map_err(|e| AppError::InvalidArg(e.to_string()))?;
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        parameters.hash(&mut hash);
        let key = format!("{kind}:{:016x}", hash.finish());
        let mut conn = self.db.connect()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db::DbError::from)?;
        let active:Option<i64>=tx.query_row("SELECT id FROM jobs WHERE job_key=?1 AND status IN ('queued','running','cancelling')",[&key],|r|r.get(0)).optional().map_err(db::DbError::from)?;
        let id = if let Some(id) = active {
            id
        } else {
            let count: i64 = tx
                .query_row("SELECT COUNT(*) FROM jobs WHERE status='queued'", [], |r| {
                    r.get(0)
                })
                .map_err(db::DbError::from)?;
            if count >= 32 {
                return Err(AppError::InvalidArg("任务队列已满，请稍后重试".into()));
            }
            tx.execute("INSERT INTO jobs(kind,job_key,params_json,status,phase,created_at) VALUES (?1,?2,?3,'queued','等待执行',?4)",params![kind,key,parameters,db::now_rfc3339()]).map_err(db::DbError::from)?;
            tx.last_insert_rowid()
        };
        tx.commit().map_err(db::DbError::from)?;
        self.events.emit("job.updated", json!({"id":id}));
        self.job(id)
    }

    pub fn jobs_list(&self) -> Result<Value, AppError> {
        let conn = self.db.connect()?;
        let mut stmt=conn.prepare("SELECT id,kind,status,phase,progress,cancel_requested,result_json,error,created_at,started_at,finished_at FROM jobs ORDER BY id DESC LIMIT 100").map_err(db::DbError::from)?;
        let rows = stmt.query_map([], job_row).map_err(db::DbError::from)?;
        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row.map_err(db::DbError::from)?);
        }
        Ok(json!({"jobs":jobs,"worker_started":self.jobs.started.load(Ordering::SeqCst)}))
    }

    pub fn job(&self, id: i64) -> Result<Value, AppError> {
        let conn = self.db.connect()?;
        conn.query_row("SELECT id,kind,status,phase,progress,cancel_requested,result_json,error,created_at,started_at,finished_at FROM jobs WHERE id=?1",[id],job_row)
            .optional().map_err(db::DbError::from)?.ok_or_else(||AppError::NotFound(format!("job {id}")))
    }

    pub fn cancel_job(&self, id: i64) -> Result<Value, AppError> {
        let conn = self.db.connect()?;
        let changed=conn.execute("UPDATE jobs SET cancel_requested=1,status=CASE WHEN status='queued' THEN 'cancelled' ELSE 'cancelling' END,phase='取消请求已接收',finished_at=CASE WHEN status='queued' THEN ?2 ELSE finished_at END WHERE id=?1 AND status IN ('queued','running') AND phase!='正在原子写入'",params![id,db::now_rfc3339()]).map_err(db::DbError::from)?;
        if changed == 0 {
            return Err(AppError::InvalidArg(
                "任务已结束或正在原子写入；写入完成后可查看结果".into(),
            ));
        }
        self.events.emit("job.updated", json!({"id":id}));
        self.job(id)
    }

    pub fn retry_job(&self, id: i64) -> Result<Value, AppError> {
        let conn = self.db.connect()?;
        let (kind,parameters):(String,String)=conn.query_row("SELECT kind,params_json FROM jobs WHERE id=?1 AND status IN ('failed','partial','cancelled','interrupted')",[id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(db::DbError::from)?;
        self.submit_job(
            &kind,
            serde_json::from_str(&parameters).map_err(|e| AppError::Message(e.to_string()))?,
        )
    }
}

fn job_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let result: Option<String> = r.get(6)?;
    Ok(
        json!({"id":r.get::<_,i64>(0)?,"kind":r.get::<_,String>(1)?,"status":r.get::<_,String>(2)?,
        "phase":r.get::<_,String>(3)?,"progress":r.get::<_,u32>(4)?,"cancel_requested":r.get::<_,bool>(5)?,
        "result":result.and_then(|s|serde_json::from_str::<Value>(&s).ok()),"error":r.get::<_,Option<String>>(7)?,
        "created_at":r.get::<_,String>(8)?,"started_at":r.get::<_,Option<String>>(9)?,"finished_at":r.get::<_,Option<String>>(10)?}),
    )
}

fn tick(app: &Arc<App>) -> Result<(), AppError> {
    let mut conn = app.db.connect()?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db::DbError::from)?;
    tx.execute("UPDATE jobs SET status='interrupted',phase='执行进程失联，可重试',finished_at=?1 WHERE status IN ('running','cancelling') AND julianday(heartbeat_at)<julianday('now','-2 minutes')",[db::now_rfc3339()]).map_err(db::DbError::from)?;
    let selected:Option<(i64,String,String)>=tx.query_row("SELECT id,kind,params_json FROM jobs WHERE status='queued' AND NOT EXISTS(SELECT 1 FROM jobs WHERE status IN ('running','cancelling')) ORDER BY id LIMIT 1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db::DbError::from)?;
    let Some((id, kind, parameters)) = selected else {
        return Ok(());
    };
    tx.execute("UPDATE jobs SET status='running',phase='准备执行',progress=5,started_at=?2,heartbeat_at=?2,owner=?3 WHERE id=?1",params![id,db::now_rfc3339(),std::process::id().to_string()]).map_err(db::DbError::from)?;
    tx.commit().map_err(db::DbError::from)?;
    let alive = Arc::new(AtomicBool::new(true));
    let heart = Arc::clone(&alive);
    let heartbeat_app = Arc::clone(app);
    let heartbeat = std::thread::spawn(move || {
        while heart.load(Ordering::SeqCst) {
            if let Ok(conn) = heartbeat_app.db.connect() {
                let _ = conn.execute(
                    "UPDATE jobs SET heartbeat_at=?2 WHERE id=?1",
                    params![id, db::now_rfc3339()],
                );
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    });
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        execute(app, id, &kind, &parameters)
    }))
    .unwrap_or_else(|_| {
        Err(AppError::Message(
            "任务执行失败，输入已隔离；可检查数据后重试".into(),
        ))
    });
    alive.store(false, Ordering::SeqCst);
    let _ = heartbeat.join();
    let (status, result, error) = match outcome {
        Ok(value) => (
            if value.get("status").and_then(Value::as_str) == Some("partial") {
                "partial"
            } else {
                "succeeded"
            },
            Some(value.to_string()),
            None,
        ),
        Err(error) if error.to_string() == "任务已取消" => (
            "cancelled",
            None,
            Some("已停止后续步骤；已提交批次保留".into()),
        ),
        Err(error) => ("failed", None, Some(error.to_string())),
    };
    conn.execute("UPDATE jobs SET status=?2,phase=?2,progress=100,result_json=?3,error=?4,finished_at=?5,heartbeat_at=?5 WHERE id=?1",params![id,status,result,error,db::now_rfc3339()]).map_err(db::DbError::from)?;
    app.events
        .emit("job.updated", json!({"id":id,"status":status}));
    Ok(())
}

pub(crate) fn stage(app: &App, id: i64, phase: &str, progress: u32) -> Result<(), AppError> {
    let mut conn = app.db.connect()?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db::DbError::from)?;
    let cancelled: bool = tx
        .query_row("SELECT cancel_requested FROM jobs WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .map_err(db::DbError::from)?;
    if cancelled {
        return Err(AppError::Message("任务已取消".into()));
    }
    tx.execute(
        "UPDATE jobs SET phase=?2,progress=?3 WHERE id=?1",
        params![id, phase, progress],
    )
    .map_err(db::DbError::from)?;
    tx.commit().map_err(db::DbError::from)?;
    app.events.emit(
        "job.updated",
        json!({"id":id,"phase":phase,"progress":progress}),
    );
    Ok(())
}

fn execute(app: &App, id: i64, kind: &str, parameters: &str) -> Result<Value, AppError> {
    let p: Value =
        serde_json::from_str(parameters).map_err(|e| AppError::InvalidArg(e.to_string()))?;
    stage(app, id, "校验输入", 10)?;
    match kind {
        "provider.test" => {
            stage(app, id, "测试数据源连接", 20)?;
            let result = app.provider_test()?;
            stage(app, id, "连接测试完成", 95)?;
            Ok(result)
        }
        "pulse.recompute" => {
            stage(app, id, "重算市场截面", 20)?;
            crate::realtime::recompute(app).map_err(AppError::Message)?;
            Ok(json!({"message":"市场截面已重新计算"}))
        }
        "import.file" | "import.json" => {
            let bundle = app.import_bundle_from_params(kind, &p)?;
            let preview = import::preview(&app.db, &bundle, import_source(kind, &p))?;
            if let Some(expected) = p.get("fingerprint").and_then(Value::as_str) {
                if preview["fingerprint"].as_str() != Some(expected) {
                    return Err(AppError::InvalidArg(
                        "输入文件已变化，请重新校验预览".into(),
                    ));
                }
            }
            stage(app, id, "正在原子写入", 45)?;
            let source = import_source(kind, &p);
            serde_json::to_value(app.import_bundle(&bundle, source)?)
                .map_err(|e| AppError::Message(e.to_string()))
        }
        "stock.research" => crate::research::execute(app, id, &p),
        "tushare.bars" => {
            let mut p = p;
            p["section"] = json!("history");
            crate::research::execute(app, id, &p)
        }
        "tushare.valuations" | "tushare.indices" => {
            stage(app, id, "获取同日市值 / 指数日线", 20)?;
            let provider = app.data_provider(
                p.get("provider")
                    .and_then(Value::as_str)
                    .unwrap_or("tushare"),
            )?;
            let bundle = if kind == "tushare.valuations" {
                provider.valuations()?
            } else {
                provider.indices()?
            };
            stage(app, id, "正在原子写入", 75)?;
            serde_json::to_value(app.import_bundle(&bundle, provider.id())?)
                .map_err(|e| AppError::Message(e.to_string()))
        }
        "market.sync" => {
            let provider = app.data_provider(
                p.get("provider")
                    .and_then(Value::as_str)
                    .unwrap_or("tushare"),
            )?;
            crate::market_sync::execute(app, id, provider.as_ref())
        }
        "tushare.sync" => {
            stage(app, id, "获取股票列表", 15)?;
            let client = app.data_provider(
                p.get("provider")
                    .and_then(Value::as_str)
                    .unwrap_or("tushare"),
            )?;
            let listings = client.listings()?;
            stage(app, id, "获取行情快照", 40)?;
            let mut daily = client.snapshot()?;
            daily.listings = listings.listings;
            stage(app, id, "正在原子写入", 75)?;
            serde_json::to_value(app.import_bundle(&daily, client.id())?)
                .map_err(|e| AppError::Message(e.to_string()))
        }
        _ => Err(AppError::InvalidArg("不支持的任务".into())),
    }
}

pub(crate) fn import_source<'a>(kind: &str, p: &'a Value) -> &'a str {
    if kind == "import.file" {
        p.get("path")
            .and_then(Value::as_str)
            .and_then(|s| std::path::Path::new(s).file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("import.file")
    } else {
        "upload"
    }
}

impl App {
    pub fn import_bundle_from_params(
        &self,
        kind: &str,
        p: &Value,
    ) -> Result<import::ImportBundle, AppError> {
        if kind == "import.file" {
            let path = p
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| AppError::InvalidArg("缺少文件路径".into()))?;
            let resolved = self.resolve_import_path(std::path::Path::new(path), true)?;
            import::load_file(&resolved, ImportKind::Bundle)
        } else {
            let data = p
                .get("data")
                .ok_or_else(|| AppError::InvalidArg("缺少导入数据".into()))?;
            import::parse_json(&data.to_string(), ImportKind::Bundle)
        }
    }
}
