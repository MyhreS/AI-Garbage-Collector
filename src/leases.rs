//! Cooperative reservations persist if a wrapper dies; unknown ownership fails closed.
use crate::{
    config::atomic_json,
    inventory::Activity,
    runtime::{command, lock},
};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::{
    fs, os::unix::process::CommandExt, path::Path, process::Command, thread, time::Duration,
};

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
            _ => a.busy = true,
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
pub fn run(dir: &Path, args: Vec<String>, resources: Vec<String>, owner: String) -> Result<()> {
    let lease = dir.join("leases").join(format!(
        "{}-{}.json",
        crate::runtime::now(),
        std::process::id()
    ));
    let mut child;
    {
        let _guard = lock(dir)?;
        anyhow::ensure!(!owner.trim().is_empty(), "owner cannot be empty");
        let report = fs::read(dir.join("last-report.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<crate::inventory::Report>(&b).ok());
        for resource in &resources {
            if resource.starts_with('/') {
                crate::runtime::canonical(Path::new(resource))?;
            } else {
                anyhow::ensure!(
                    report
                        .as_ref()
                        .is_some_and(|r| r.items.iter().any(|i| i.id == *resource)),
                    "unknown resource ID; run status and copy an exact ID or use an absolute path"
                );
            }
        }
        // Write protection BEFORE starting the child; even immediate forks are covered.
        atomic_json(
            &lease,
            &json!({"owner":owner,"resources":resources,"created_at":crate::runtime::now(),"state":"starting"}),
        )?;
        child = match Command::new(&args[0])
            .args(&args[1..])
            .process_group(0)
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                fs::remove_file(&lease)?;
                return Err(e).context("could not start protected command");
            }
        };
        let start = command("/bin/ps", &["-p", &child.id().to_string(), "-o", "lstart="]).ok();
        atomic_json(
            &lease,
            &json!({"owner":owner,"resources":resources,"created_at":crate::runtime::now(),"pid":child.id(),"start":start.map(|s| s.trim().to_owned()),"process_group":child.id(),"state":"running"}),
        )?;
    }
    let terminal = Terminal::handoff(child.id());
    let status = child.wait()?;
    // Keep the reservation while inherited background children in the process group remain.
    loop {
        let raw = command("/bin/ps", &["-axo", "pgid="])?;
        if !raw
            .lines()
            .any(|p| p.trim().parse::<u32>().ok() == Some(child.id()))
        {
            break;
        }
        thread::sleep(Duration::from_secs(1));
    }
    {
        let _guard = lock(dir)?;
        fs::remove_file(lease)?;
        let _ = fs::remove_file(dir.join("last-report.json"));
    }
    drop(terminal);
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

// Give an interactive child its foreground terminal while it owns a new process group.
struct Terminal(Option<libc::pid_t>);
impl Terminal {
    fn handoff(pid: u32) -> Self {
        // SAFETY: libc calls use valid stack storage and the inherited stdin descriptor.
        unsafe {
            let group = libc::getpgrp();
            if libc::isatty(libc::STDIN_FILENO) == 1
                && libc::tcgetpgrp(libc::STDIN_FILENO) == group
                && set_foreground(pid as libc::pid_t)
            {
                libc::kill(-(pid as libc::pid_t), libc::SIGCONT);
                Self(Some(group))
            } else {
                Self(None)
            }
        }
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        if let Some(group) = self.0 {
            set_foreground(group);
        }
    }
}
fn set_foreground(group: libc::pid_t) -> bool {
    // Block SIGTTOU only on this thread during terminal ownership transfer.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        let mut old: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGTTOU);
        if libc::pthread_sigmask(libc::SIG_BLOCK, &set, &mut old) != 0 {
            return false;
        }
        let ok = libc::tcsetpgrp(libc::STDIN_FILENO, group) == 0;
        libc::pthread_sigmask(libc::SIG_SETMASK, &old, std::ptr::null_mut());
        ok
    }
}
