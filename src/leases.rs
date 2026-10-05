//! Cooperative reservations persist if a wrapper dies; unknown ownership fails closed.
use crate::inventory::Activity;
use anyhow::Result;
use serde_json::{Value, json};
use std::{fs, path::Path};

pub fn apply(dir: &Path, a: &mut Activity) {
    let entries = match fs::read_dir(dir.join("leases")) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(_) => {
            a.reliable = false;
            return;
        }
    };
    for entry in entries {
        let Ok(entry) = entry else {
            a.reliable = false;
            continue;
        };
        let Some(v) = fs::read(entry.path())
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        else {
            a.reliable = false;
            continue;
        };
        // A crashed wrapper may have left detached descendants. Never expire a lease on PID absence.
        match v["resources"].as_array() {
            Some(ids) if !ids.is_empty() => {
                for id in ids {
                    if let Some(id) = id.as_str() {
                        a.reserved.push(id.into());
                    } else {
                        a.reliable = false;
                    }
                }
            }
            _ => {
                a.busy = true;
                a.global_reserved = true;
            }
        }
    }
}
pub fn list(dir: &Path) -> Result<Vec<Value>> {
    let mut result = vec![];
    if !dir.join("leases").exists() {
        return Ok(result);
    }
    for entry in fs::read_dir(dir.join("leases"))? {
        let entry = entry?;
        let mut v: Value = serde_json::from_slice(&fs::read(entry.path())?)?;
        v["id"] = json!(entry.file_name().to_string_lossy());
        result.push(v);
    }
    Ok(result)
}
pub fn release(dir: &Path, id: &str) -> Result<()> {
    anyhow::ensure!(
        !id.is_empty()
            && id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'-')
            && id.ends_with(".json"),
        "copy a lease ID from aigc leases"
    );
    fs::remove_file(dir.join("leases").join(id))?;
    Ok(())
}

#[cfg(unix)]
#[path = "leases_unix.rs"]
mod native;
#[cfg(windows)]
#[path = "leases_windows.rs"]
mod native;
pub use native::run;
