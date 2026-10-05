use crate::{
    config::{Config, home},
    evidence::Action,
    inventory::{Item, children},
    runtime::{command_at, git},
};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};
pub mod builds;
pub mod caches;
pub mod mobile;
pub mod python;

pub fn available(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|p| {
            ["", ".exe", ".cmd", ".bat"].iter().any(|ext| {
                (ext.is_empty() || cfg!(windows)) && p.join(format!("{program}{ext}")).is_file()
            })
        })
    })
}
pub fn read(path: &Path) -> Result<String> {
    ensure!(
        fs::metadata(path)?.len() <= 2 * 1024 * 1024,
        "metadata exceeds 2 MiB"
    );
    Ok(fs::read_to_string(path)?)
}
pub fn tool(program: &str, args: &[&str], cwd: Option<&Path>) -> Result<String> {
    command_at(program, args, cwd, 20)
}
pub fn json(program: &str, args: &[&str], cwd: Option<&Path>) -> Result<Value> {
    Ok(serde_json::from_str(&tool(program, args, cwd)?)?)
}
pub fn epoch(v: &Value) -> Option<u64> {
    let s = v.as_str()?;
    let t = time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .ok()?
        .unix_timestamp();
    u64::try_from(t).ok().filter(|t| *t > 0)
}
pub fn folder(kind: &str, path: PathBuf, source: &str) -> Item {
    let mut i = Item::new(
        kind,
        format!("{kind}:{}", path.display()),
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        Some(path),
        false,
    );
    i.evidence.source = source.into();
    i
}
pub fn scan(c: &Config, projects: &[PathBuf], items: &mut Vec<Item>, warnings: &mut Vec<String>) {
    if projects.len() > 250 {
        warnings.push(
            "deep project inspection limited to 250 projects; reference-dependent cleanup disabled"
                .into(),
        );
    }
    let selected = &projects[..projects.len().min(250)];
    python::scan(selected, items, warnings);
    caches::scan(items, warnings);
    builds::scan(selected, items, warnings);
    mobile::scan(c, selected, items, warnings);
    for r in &c.registered {
        if !r.path.exists() {
            continue;
        }
        if let Some(i) = items.iter_mut().find(|i| i.path.as_ref() == Some(&r.path)) {
            i.evidence.registered = true;
            i.evidence.owners.push(r.owner.clone());
            if i.kind == "python"
                && i.protection.as_deref()
                    == Some("owner unknown; no discovered project association")
                && disposable_path(&r.path, "python").is_ok()
            {
                i.protection = None;
            }
            // Registration never changes a tool-specific protection into permission.
            continue;
        }
        let mut i = folder(&r.kind, r.path.clone(), "explicit registration");
        i.cleanable = true;
        i.evidence.registered = true;
        i.evidence.owners.push(r.owner.clone());
        i.evidence.action = Some(Action::Directory);
        i.evidence
            .metadata
            .insert("purpose".into(), Value::String(r.purpose.clone()));
        if let Err(e) = disposable_path(&r.path, &r.kind) {
            i.protection = Some(e.to_string());
        }
        items.push(i);
    }
    // Partial discovery can conceal consumers. This affects all new reference-dependent adapters.
    let incomplete = projects.len() > 250
        || warnings
            .iter()
            .any(|w| w.contains("project discovery") || w.contains("skipped symlink root"));
    if incomplete {
        for i in items
            .iter_mut()
            .filter(|i| i.evidence.action.is_some() && !python::central_environment(i))
        {
            i.complete = false;
        }
    }
    // A shared environment must not be collected through a single worktree registration.
    for i in items.iter_mut().filter(|i| i.kind == "python") {
        if i.evidence
            .consumers
            .iter()
            .filter(|r| r.source == "Python project")
            .count()
            > 1
        {
            i.protection = Some("environment is shared by multiple projects".into());
        }
    }
}

pub fn disposable_path(path: &Path, kind: &str) -> Result<()> {
    crate::runtime::canonical(path)?;
    ensure!(
        path.starts_with(home()) && path != home(),
        "resource must be inside your home"
    );
    ensure!(
        path.components().count() >= home().components().count() + 2,
        "refusing a top-level home directory"
    );
    for protected in [
        ".ssh",
        ".gnupg",
        ".aws",
        "Library/Keychains",
        "Library/Application Support",
        "Library/Developer/CoreSimulator",
        "Library/Developer/Xcode/Archives",
        ".codex",
        ".codex-workspaces",
        ".local/share/uv/tools",
        ".local/pipx",
        ".cargo/bin",
        "Documents",
        "Desktop",
        "Downloads",
    ] {
        let p = home().join(protected);
        ensure!(
            !path.starts_with(&p) && !p.starts_with(path),
            "protected storage location"
        );
    }
    ensure!(
        !path.join(".git").exists() && !path.join(".gitmodules").exists(),
        "Git checkout needs worktree handling"
    );
    if kind == "python" {
        ensure!(
            path.join("pyvenv.cfg").is_file(),
            "not a recognized Python environment"
        );
    }
    // Explicit registration cannot authorize removal of source tracked by a surrounding repository.
    if let Some(parent) = path.parent()
        && git(parent, &["rev-parse", "--is-inside-work-tree"]).is_ok()
    {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .context("invalid path")?;
        ensure!(
            git(parent, &["ls-files", "-z", "--", name])?.is_empty(),
            "contains tracked source"
        );
    }
    let mut n = 0;
    for e in walkdir::WalkDir::new(path)
        .follow_links(false)
        .same_file_system(true)
    {
        n += 1;
        ensure!(n <= 500_000, "registered resource inspection incomplete");
        let e = e?;
        ensure!(
            e.file_name() != ".git" && e.file_name() != ".gitmodules",
            "nested repository requires manual handling"
        );
    }
    Ok(())
}

pub fn remove(i: &Item, c: &Config) -> Result<String> {
    match i.evidence.action.as_ref().context("no native action")? {
        Action::Directory => {
            let p = i.path.as_ref().context("missing path")?;
            python::verify_central_environment(i)?;
            disposable_path(p, &i.kind)?;
            fs::remove_dir_all(p)?;
            Ok("Removed idle generated resource".into())
        }
        Action::Poetry {
            project,
            environment,
        } => {
            disposable_path(environment, "python")?;
            let listed = tool(
                "poetry",
                &[
                    "--no-plugins",
                    "--no-interaction",
                    "env",
                    "list",
                    "--full-path",
                ],
                Some(project),
            )?;
            ensure!(
                listed
                    .lines()
                    .any(|l| l.trim_end_matches(" (Activated)").trim()
                        == environment.to_string_lossy()),
                "Poetry association changed"
            );
            let executable = environment.join(if cfg!(windows) {
                "Scripts/python.exe"
            } else {
                "bin/python"
            });
            let name = executable.to_str().context("invalid environment path")?;
            tool(
                "poetry",
                &["--no-plugins", "--no-interaction", "env", "remove", name],
                Some(project),
            )?;
            Ok("Removed disposable environment through Poetry".into())
        }
        Action::Cache { manager, root } => caches::remove(manager, root, c),
        Action::Runtime { uuid, build } => mobile::remove_runtime(uuid, build, c),
        Action::Sdk { root, package } => mobile::remove_sdk(root, package),
    }
}

// Upper estimate for human-formatted native sizes. Keep the original text alongside it.
pub fn human_bytes(s: &str) -> Option<u64> {
    let s = s.trim().replace(' ', "").to_uppercase();
    let split = s
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(s.len());
    let n: f64 = s[..split].parse().ok()?;
    if !n.is_finite() || n < 0.0 {
        return None;
    }
    let scale = match &s[split..] {
        "B" | "" => 1.0,
        "KB" => 1000.0,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        "KIB" => 1024.0,
        "MIB" => 1048576.0,
        "GIB" => 1073741824.0,
        _ => return None,
    };
    let resolution = s[..split]
        .split_once('.')
        .map(|(_, decimals)| 10f64.powi(-(decimals.len() as i32)))
        .unwrap_or(1.0);
    Some(if n == 0.0 {
        0
    } else {
        ((n + resolution) * scale).ceil() as u64
    })
}
