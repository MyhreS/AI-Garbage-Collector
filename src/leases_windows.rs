//! Windows reservations intentionally survive command exit: detached children cannot
//! be proven idle using the built-in process inventory. Explicit release is required.
use crate::{config::atomic_json, runtime::lock};
use anyhow::{Result, ensure};
use serde_json::json;
use std::{fs, path::Path, process::Command};
pub fn run(dir: &Path, args: Vec<String>, resources: Vec<String>, owner: String) -> Result<()> {
    ensure!(
        !args.is_empty() && !owner.trim().is_empty(),
        "command and owner required"
    );
    let id = format!("{}-{}.json", crate::runtime::now(), std::process::id());
    let lease = dir.join("leases").join(&id);
    let mut child;
    {
        let _guard = lock(dir)?;
        for resource in &resources {
            ensure!(
                Path::new(resource).is_absolute(),
                "Windows reservations require absolute paths"
            );
            crate::runtime::canonical(Path::new(resource))?;
        }
        atomic_json(
            &lease,
            &json!({"owner":owner,"resources":resources,"created_at":crate::runtime::now(),"state":"manual-release-required"}),
        )?;
        child = match Command::new(&args[0]).args(&args[1..]).spawn() {
            Ok(c) => c,
            Err(e) => {
                fs::remove_file(&lease)?;
                return Err(e.into());
            }
        };
    }
    let status = child.wait()?;
    eprintln!(
        "Reservation retained for possible background children. When all work ends: aigc release-lease {id}"
    );
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}
