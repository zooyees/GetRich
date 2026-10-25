//! GetRich shared core: config, storage, import, and quote viewing.
//!
//! Layering mirrors WiParse (`*-core` / `*-cli` / long-running API):
//! config and paths at the bottom, SQLite access in `db`, file import in
//! `import`, read models in `view`, orchestration in `service`, JSON invoke
//! in `invoke`.

pub mod config;
pub mod db;
pub mod error;
pub mod events;
pub mod import;
pub mod ingest;
pub mod invoke;
pub mod jobs;
pub mod logging;
pub mod market_map;
mod market_sync;
pub mod models;
pub mod paths;
pub mod providers;
pub mod pulse;
pub mod read_model;
pub mod realtime;
pub mod resample;
pub mod research;
pub mod service;
pub mod session;
pub mod settings;
pub mod symbol;
pub mod tushare;
pub mod validation;
pub mod valuations;
pub mod view;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_NAME: &str = "getrich";
