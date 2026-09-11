//! Process logging (stderr + optional file).

use std::fs::OpenOptions;
use std::io;
use std::path::Path;
use std::sync::Mutex;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

pub fn init(level: &str, log_file: Option<&Path>) -> Result<(), io::Error> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(normalize_level(level))
    });

    let stderr_layer = fmt::layer()
        .with_writer(io::stderr)
        .with_target(true)
        .with_ansi(false);

    if let Some(path) = log_file {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let file_layer = fmt::layer()
            .with_writer(Mutex::new(file))
            .with_target(true)
            .with_ansi(false);
        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(stderr_layer)
            .with(file_layer)
            .try_init();
    } else {
        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(stderr_layer)
            .try_init();
    }
    Ok(())
}

fn normalize_level(level: &str) -> String {
    match level.trim().to_ascii_lowercase().as_str() {
        "trace" | "debug" | "info" | "warn" | "error" => level.trim().to_ascii_lowercase(),
        "warning" => "warn".into(),
        "fatal" => "error".into(),
        _ => "info".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_levels() {
        assert_eq!(normalize_level("INFO"), "info");
        assert_eq!(normalize_level("warning"), "warn");
        assert_eq!(normalize_level("nope"), "info");
    }
}
