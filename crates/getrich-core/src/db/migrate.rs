//! Ordered, transactional schema migrations.

use super::DbError;
use rusqlite::{params, Connection};
use serde::Serialize;

const MIGRATIONS: &[(i64, &str, &str)] = &[
    (
        1,
        "001_init",
        include_str!("../../migrations/001_init.sql"),
    ),
    (
        2,
        "002_quote_turnover",
        include_str!("../../migrations/002_quote_turnover.sql"),
    ),
];

#[derive(Debug, Clone, Serialize)]
pub struct MigrationStatus {
    pub current_version: i64,
    pub pending: Vec<String>,
    pub applied: Vec<AppliedMigration>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppliedMigration {
    pub version: i64,
    pub name: String,
    pub applied_at: String,
}

pub fn ensure_migrations_table(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            applied_at TEXT NOT NULL
        );
        "#,
    )?;
    Ok(())
}

pub fn migrate(conn: &Connection) -> Result<Vec<String>, DbError> {
    ensure_migrations_table(conn)?;
    let current: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |r| r.get(0),
    )?;
    let mut applied = Vec::new();
    for (version, name, sql) in MIGRATIONS {
        if *version <= current {
            continue;
        }
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)
            .map_err(|e| DbError::Schema(format!("{name}: {e}")))?;
        tx.execute(
            "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
            params![version, name, super::now_rfc3339()],
        )?;
        tx.commit()?;
        tracing::info!(version, name, "applied migration");
        applied.push((*name).to_string());
    }
    Ok(applied)
}

pub fn applied_migrations(conn: &Connection) -> Result<Vec<AppliedMigration>, DbError> {
    ensure_migrations_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT version, name, applied_at FROM schema_migrations ORDER BY version",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(AppliedMigration {
            version: r.get(0)?,
            name: r.get(1)?,
            applied_at: r.get(2)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn migration_status(conn: &Connection) -> Result<MigrationStatus, DbError> {
    let applied = applied_migrations(conn)?;
    let current_version = applied.last().map(|m| m.version).unwrap_or(0);
    let pending = MIGRATIONS
        .iter()
        .filter(|(v, _, _)| *v > current_version)
        .map(|(_, name, _)| (*name).to_string())
        .collect();
    Ok(MigrationStatus {
        current_version,
        pending,
        applied,
    })
}
