//! GetRich shared core: config, storage, import, and quote viewing.
//!
//! Layering mirrors WiParse (`*-core` / `*-cli` / long-running API):
//! config and paths at the bottom, SQLite access in `db`, file import in
//! `import`, read models in `view`, orchestration in `service`, JSON invoke
//! in `invoke`.

pub mod config;
pub mod db;
pub mod error;
pub mod import;
pub mod invoke;
pub mod logging;
pub mod models;
pub mod paths;
pub mod service;
pub mod symbol;
pub mod view;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_NAME: &str = "getrich";
