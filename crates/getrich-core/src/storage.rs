//! Local consistent backup, created on the durable task worker.
use crate::{db, error::AppError, service::App};
use serde_json::{json, Value};
impl App {
    pub fn backup_store(&self) -> Result<Value, AppError> {
        let conn = self.db.connect()?;
        let parent = self
            .db
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(std::path::Path::new("."))
            .canonicalize()?;
        let dir = parent.join("backups");
        std::fs::create_dir_all(&dir)?;
        if !dir.canonicalize()?.starts_with(&parent) {
            return Err(AppError::InvalidArg(
                "备份目录必须位于当前数据目录内".into(),
            ));
        }
        let name = format!(
            "GetRich-{}-{}",
            chrono::Utc::now().format("%Y%m%dT%H%M%S"),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        let partial = dir.join(format!("{name}.partial"));
        let destination = dir.join(format!("{name}.db"));
        if partial.exists() || destination.exists() {
            return Err(AppError::InvalidArg("备份文件已存在，请重试".into()));
        }
        conn.execute("VACUUM INTO ?1", [partial.to_string_lossy().as_ref()])
            .map_err(db::DbError::from)?;
        let verify = rusqlite::Connection::open_with_flags(
            &partial,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(db::DbError::from)?;
        let check: String = verify
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(db::DbError::from)?;
        if check != "ok" {
            return Err(AppError::Message(
                "备份完整性检查未通过，原数据库未更改".into(),
            ));
        }
        drop(verify);
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&partial)?
            .sync_all()?;
        std::fs::rename(&partial, &destination)?;
        Ok(
            json!({"path":destination,"bytes":std::fs::metadata(&destination)?.len(),"schema_version":db::schema_version(),"integrity":"ok","config_included":false,"message":format!("数据库备份完成：{}；未包含配置文件，请单独保管配置",destination.display())}),
        )
    }
}
