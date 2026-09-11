//! Path helpers — writable root vs bundled defaults (WiParse `paths` parity).

use std::env;
use std::path::{Path, PathBuf};

/// Writable data root: directory containing the running executable, or cwd in tests/dev.
pub fn app_root() -> PathBuf {
    if let Ok(root) = env::var("GETRICH_DATA_ROOT") {
        let p = PathBuf::from(root);
        if !p.as_os_str().is_empty() {
            return p;
        }
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            let s = parent.to_string_lossy();
            if s.contains("target") && (s.contains("debug") || s.contains("release")) {
                if let Ok(manifest) = env::var("CARGO_MANIFEST_DIR") {
                    let p = PathBuf::from(manifest);
                    if let Some(ws) = p.parent().and_then(|p| p.parent()) {
                        return ws.to_path_buf();
                    }
                }
            }
            return parent.to_path_buf();
        }
    }
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub fn project_path(relative: impl AsRef<Path>) -> PathBuf {
    let p = relative.as_ref();
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        app_root().join(p)
    }
}

pub fn config_file() -> PathBuf {
    if let Ok(env_path) = env::var("GETRICH_CONFIG") {
        return PathBuf::from(env_path);
    }
    project_path("config.json")
}

pub fn default_config_file() -> PathBuf {
    project_path("config.default.json")
}

pub fn app_icon_path() -> Option<PathBuf> {
    let rels = [
        Path::new("icon").join("GetRich.ico"),
        Path::new("Icon").join("GetRich.ico"),
        PathBuf::from("GetRich.ico"),
        Path::new("packaging").join("GetRich.ico"),
    ];
    for rel in rels {
        let candidate = project_path(&rel);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in ["GetRich.ico", "icon/GetRich.ico", "Icon/GetRich.ico"] {
                let p = dir.join(name);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    None
}

pub fn db_path(db_name: &str) -> PathBuf {
    if let Ok(env_path) = env::var("GETRICH_DB") {
        if !env_path.is_empty() {
            return PathBuf::from(env_path);
        }
    }
    project_path(db_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_path_keeps_absolute() {
        let abs = PathBuf::from("/tmp/getrich.db");
        assert_eq!(project_path(&abs), abs);
    }

    #[test]
    fn db_path_joins_name_when_env_unset() {
        let p = db_path("getrich.db");
        assert_eq!(p.file_name().unwrap(), "getrich.db");
    }
}
