use crate::{
    config::{Config, atomic_json, home},
    inventory::{self, Item, Report},
    policy::{self, Status},
    runtime::{canonical, command, command_at, git, now},
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize, Deserialize)]
pub struct Event {
    pub time: u64,
    pub resource: String,
    pub outcome: String,
    pub detail: String,
    pub estimated_removed_bytes: u64,
    pub disk_free_delta_bytes: i64,
    #[serde(default)]
    pub native_reclaimed_bytes: Option<u64>,
}
pub fn history(dir: &Path) -> Result<Vec<Event>> {
    let p = dir.join("history.json");
    if !p.exists() {
        return Ok(vec![]);
    }
    Ok(serde_json::from_slice(&fs::read(p)?)?)
}
fn record(dir: &Path, event: Event) -> Result<()> {
    let mut h = history(dir)?;
    h.push(event);
    if h.len() > 500 {
        h.drain(..h.len() - 500);
    }
    atomic_json(&dir.join("history.json"), &h)
}
pub fn prepare(c: &Config, dir: &Path) -> Result<Report> {
    let (mut r, a) = inventory::report(c, dir)?;
    let mut s = policy::load_state(dir)?;
    policy::evaluate(
        &mut r.items,
        c,
        &mut s,
        &a,
        r.disk_free_bytes,
        r.generated_at,
    );
    r.last_collection = s.last_collection;
    atomic_json(&dir.join("state.json"), &s)?;
    atomic_json(&dir.join("last-report.json"), &r)?;
    Ok(r)
}
pub fn collect(c: &Config, dir: &Path, report: &Report) -> Result<Vec<Event>> {
    ensure!(
        cfg!(target_os = "macos"),
        "automatic cleanup is supported only on macOS"
    );
    let mut results = vec![];
    let mut removed = 0u64;
    for item in report
        .items
        .iter()
        .filter(|i| i.status == Status::Eligible)
        .take(10)
    {
        // Revalidate activity immediately before each action, while holding the shared lock.
        let activity = inventory::activity(dir);
        let reserved = activity.reserved.iter().any(|id| {
            id == &item.id
                || item.path.as_ref().is_some_and(|p| {
                    id.starts_with('/') && (p.starts_with(id) || Path::new(id).starts_with(p))
                })
        });
        let precheck = if !activity.reliable
            || activity.busy
            || reserved
            || activity.touches(item.path.as_deref())
        {
            Some("activity or reservation changed; deferred")
        } else if item.bytes > c.max_delete_bytes_per_run.saturating_sub(removed) {
            Some("per-pass removal budget reached")
        } else {
            None
        };
        let before = crate::runtime::disk(&home())?.1;
        let result = if let Some(reason) = precheck {
            Err(anyhow::anyhow!(reason))
        } else {
            revalidate(item, c, dir).and_then(|()| remove(item, c, dir))
        };
        let after = crate::runtime::disk(&home())?.1;
        let native_reclaimed = result.as_ref().ok().and_then(|s| native_bytes(s));
        let maintenance = matches!(
            item.evidence.action,
            Some(crate::evidence::Action::Cache { .. } | crate::evidence::Action::Buildkit { .. })
        );
        let (outcome, detail, estimate) = match result {
            Ok(detail) => {
                removed = removed.saturating_add(item.bytes);
                let outcome = if maintenance && native_reclaimed == Some(0) {
                    "no_op"
                } else if maintenance {
                    "maintained"
                } else {
                    "removed"
                };
                (
                    outcome,
                    detail,
                    if maintenance {
                        native_reclaimed.unwrap_or(0)
                    } else {
                        item.bytes
                    },
                )
            }
            Err(e) => ("skipped", e.to_string(), 0),
        };
        let event = Event {
            time: now(),
            resource: item.id.clone(),
            outcome: outcome.into(),
            detail,
            estimated_removed_bytes: estimate,
            native_reclaimed_bytes: native_reclaimed,
            disk_free_delta_bytes: (after as i128 - before as i128)
                .clamp(i64::MIN as i128, i64::MAX as i128)
                as i64,
        };
        record(
            dir,
            Event {
                time: event.time,
                resource: event.resource.clone(),
                outcome: event.outcome.clone(),
                detail: event.detail.clone(),
                estimated_removed_bytes: event.estimated_removed_bytes,
                disk_free_delta_bytes: event.disk_free_delta_bytes,
                native_reclaimed_bytes: event.native_reclaimed_bytes,
            },
        )?;
        results.push(event);
    }
    let mut state = policy::load_state(dir)?;
    state.last_collection = Some(now());
    for e in &results {
        if matches!(e.outcome.as_str(), "removed" | "maintained" | "no_op") {
            state.maintenance.insert(e.resource.clone(), e.time);
            state.observations.remove(&e.resource);
        }
    }
    state
        .maintenance
        .retain(|_, t| now().saturating_sub(*t) <= 365 * 86400);
    atomic_json(&dir.join("state.json"), &state)?;
    let _ = fs::remove_file(dir.join("last-report.json"));
    Ok(results)
}
fn verify_path(item: &Item) -> Result<PathBuf> {
    let p = canonical(item.path.as_ref().context("missing path")?)?;
    ensure!(
        p.starts_with(home()) && p != home(),
        "only resources inside the current user's home may be removed"
    );
    let current = std::env::current_dir()?;
    ensure!(
        !current.starts_with(&p),
        "resource contains the collector's working directory"
    );
    let (size, modified, entries, complete) = inventory::tree_stats(&p);
    ensure!(
        item.evidence.identity.as_ref().is_none_or(|id| {
            use std::os::unix::fs::MetadataExt;
            fs::symlink_metadata(&p).is_ok_and(|m| *id == format!("{}:{}", m.dev(), m.ino()))
        }) && complete
            && size == item.bytes
            && modified == item.modified
            && entries == item.entries,
        "resource changed after the scan; collection deferred"
    );
    Ok(p)
}
fn remove(item: &Item, c: &Config, dir: &Path) -> Result<String> {
    if item.evidence.action.is_some() {
        if matches!(
            item.evidence.action,
            Some(
                crate::evidence::Action::Directory
                    | crate::evidence::Action::Poetry { .. }
                    | crate::evidence::Action::Cache { .. }
                    | crate::evidence::Action::Sdk { .. }
            )
        ) {
            verify_path(item)?;
        }
        return crate::adapters::remove(item, c);
    }
    match item.kind.as_str() {
        "dependencies" | "builds" | "xcode-builds" => {
            let p = verify_path(item)?;
            if item.kind == "xcode-builds" {
                ensure!(
                    p.parent()
                        == Some(home().join("Library/Developer/Xcode/DerivedData").as_path()),
                    "unexpected DerivedData path"
                );
            } else {
                ensure!(
                    c.roots.iter().any(|r| p.starts_with(r)),
                    "resource is outside configured roots"
                );
                let name = p
                    .file_name()
                    .and_then(|s| s.to_str())
                    .context("invalid path")?;
                ensure!(
                    matches!(
                        name,
                        "node_modules" | ".next" | "target" | ".build" | ".venv"
                    ),
                    "unsupported generated directory"
                );
                ensure!(
                    git(p.parent().unwrap(), &["ls-files", "-z", "--", name])?.is_empty(),
                    "directory contains tracked files"
                );
            }
            fs::remove_dir_all(&p)?;
            Ok(
                "Removed regenerable directory; reinstall dependencies or rebuild when needed"
                    .into(),
            )
        }
        "worktrees" => {
            ensure!(
                c.managed.contains_key(&item.id),
                "worktree not registered as disposable"
            );
            let p = verify_path(item)?;
            inventory::worktree_safe(&p)?;
            crate::github::ensure_no_open_pr(&p)?;
            ensure!(
                !p.components().any(|p| matches!(
                    p.as_os_str().to_str(),
                    Some(".codex" | ".codex-workspaces")
                )),
                "use the owning application's archive tool"
            );
            let listed = git(&p, &["worktree", "list", "--porcelain", "-z"])?;
            let rec = listed
                .split("\0\0")
                .find(|r| r.split('\0').next() == Some(&format!("worktree {}", p.display())))
                .context("worktree no longer registered")?;
            ensure!(
                !rec.split('\0').any(|f| f.starts_with("locked")),
                "worktree is locked"
            );
            let backup = dir.join("backups");
            fs::create_dir_all(&backup)?;
            let mut hash = DefaultHasher::new();
            item.id.hash(&mut hash);
            let bundle = backup.join(format!("{}-{:x}.bundle", now(), hash.finish()));
            let bundle_str = bundle.to_str().context("invalid backup path")?;
            let existing = inventory::tree_stats(&backup).0;
            let objects = git(&p, &["count-objects", "-v"])?;
            let object_kib: u64 = objects
                .lines()
                .filter_map(|line| {
                    line.strip_prefix("size: ")
                        .or_else(|| line.strip_prefix("size-pack: "))
                        .and_then(|n| n.parse::<u64>().ok())
                })
                .sum();
            let allowance = object_kib
                .saturating_mul(2048)
                .saturating_add(16 * 1024 * 1024);
            ensure!(
                existing.saturating_add(allowance) <= c.backup_budget_bytes,
                "recovery storage budget would be exceeded; review backups or increase budget.backups"
            );
            ensure!(
                crate::runtime::disk(&backup)?.1 > allowance.saturating_add(100 * 1024 * 1024),
                "insufficient free space to safely create recovery bundle"
            );
            let result = (|| -> Result<()> {
                git(&p, &["bundle", "create", bundle_str, "HEAD"])?;
                git(&p, &["bundle", "verify", bundle_str])?;
                crate::github::ensure_no_open_pr(&p)?;
                ensure!(
                    existing.saturating_add(fs::metadata(&bundle)?.len()) <= c.backup_budget_bytes,
                    "recovery bundle exceeds storage budget"
                );
                fs::File::open(&bundle)?.sync_all()?;
                Ok(())
            })();
            if let Err(e) = result {
                let _ = fs::remove_file(&bundle);
                return Err(e);
            }
            inventory::worktree_safe(&p)?;
            git(&p, &["worktree", "remove", "--", p.to_str().unwrap()])?;
            Ok(format!(
                "Git HEAD history saved to {}; branch retained",
                bundle.display()
            ))
        }
        "simulators" => {
            ensure!(
                c.managed.contains_key(&item.id),
                "device not registered as disposable"
            );
            let id = item
                .id
                .strip_prefix("simulators:")
                .context("invalid simulator ID")?;
            ensure!(
                id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'),
                "invalid simulator UUID"
            );
            verify_path(item)?;
            let v: Value =
                serde_json::from_str(&command("xcrun", &["simctl", "list", "devices", "--json"])?)?;
            let device = v["devices"]
                .as_object()
                .context("missing devices")?
                .values()
                .filter_map(|v| v.as_array())
                .flatten()
                .find(|d| d["udid"] == id)
                .context("device no longer exists")?;
            ensure!(
                device["state"] == "Shutdown",
                "device is running or state is unknown"
            );
            command("xcrun", &["simctl", "delete", id])?;
            Ok("Deleted disposable simulator and its app data using simctl".into())
        }
        "emulators" => {
            ensure!(
                c.managed.contains_key(&item.id),
                "AVD not registered as disposable"
            );
            let p = verify_path(item)?;
            let name = item
                .id
                .strip_prefix("emulators:")
                .context("invalid emulator ID")?;
            ensure!(
                !name.starts_with('-')
                    && name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)),
                "unsupported AVD name"
            );
            let ps = command("/bin/ps", &["-axo", "comm="])?;
            ensure!(
                !ps.contains("qemu-system") && !ps.lines().any(|s| s.ends_with("/emulator")),
                "an emulator is running"
            );
            let ini = fs::read_to_string(p.with_extension("ini"))?;
            let registered = ini
                .lines()
                .find_map(|s| s.strip_prefix("path="))
                .context("AVD path absent in .ini")?;
            ensure!(
                Path::new(registered) == p,
                "AVD metadata points to another path"
            );
            let sdk = crate::adapters::mobile::sdk_root();
            let tool = sdk.join("cmdline-tools/latest/bin/avdmanager");
            ensure!(
                tool.is_file(),
                "avdmanager unavailable at {}; delete this AVD manually or install command-line tools",
                tool.display()
            );
            command(tool.to_str().unwrap(), &["delete", "avd", "-n", name])?;
            Ok("Deleted disposable Android AVD and its app data using avdmanager".into())
        }
        "docker-images" => {
            ensure!(
                !item.active,
                "image is referenced by a container; removal refused"
            );
            ensure!(
                c.managed.contains_key(&item.id),
                "image not registered as disposable"
            );
            inventory::local_docker()?;
            let id = item
                .id
                .strip_prefix("docker-images:")
                .context("invalid image ID")?;
            ensure!(
                id.starts_with("sha256:")
                    && id[7..].len() == 64
                    && id[7..].chars().all(|c| c.is_ascii_hexdigit()),
                "invalid Docker image ID"
            );
            // Docker refuses removal when a container references the image. Never force.
            command("docker", &["image", "rm", id])?;
            Ok("Removed disposable image through Docker without force".into())
        }
        "docker-cache" => {
            ensure!(c.docker_cache_cleanup, "Docker cache cleanup disabled");
            inventory::local_docker()?;
            let days = if crate::runtime::disk(&home())?.1 < c.min_free_bytes {
                c.pressure_retention_days
            } else {
                c.retention_days
            };
            let filter = format!("until={}h", days * 24);
            let budget = c.docker_cache_budget_bytes.to_string();
            let out = command_at(
                "docker",
                &[
                    "builder",
                    "prune",
                    "--force",
                    "--filter",
                    &filter,
                    "--keep-storage",
                    &budget,
                ],
                None,
                300,
            )?;
            Ok(out.trim().into())
        }
        _ => bail!("report-only resource"),
    }
}

// Reports are plans, never authority to mutate stale resources. Rescan consumers and activity.
fn revalidate(item: &Item, c: &Config, dir: &Path) -> Result<()> {
    let (mut report, activity) = inventory::report(c, dir)?;
    let mut state = policy::load_state(dir)?;
    policy::evaluate(
        &mut report.items,
        c,
        &mut state,
        &activity,
        report.disk_free_bytes,
        report.generated_at,
    );
    let current = report
        .items
        .iter()
        .find(|i| i.id == item.id)
        .context("resource disappeared")?;
    ensure!(
        current.status == Status::Eligible,
        "eligibility changed: {}",
        current.reason
    );
    ensure!(
        current.evidence.action == item.evidence.action
            && current.evidence.identity == item.evidence.identity
            && current.bytes == item.bytes
            && current.modified == item.modified
            && current.entries == item.entries,
        "resource identity or content changed"
    );
    Ok(())
}
fn native_bytes(output: &str) -> Option<u64> {
    for line in output.lines() {
        let text = line.trim();
        if let Some(n) = text
            .strip_prefix("Total reclaimed space:")
            .or_else(|| text.strip_prefix("Total:"))
        {
            let value = n.trim().replace(' ', "").to_uppercase();
            let split = value
                .find(|c: char| !c.is_ascii_digit() && c != '.')
                .unwrap_or(value.len());
            let number: f64 = value[..split].parse().ok()?;
            let scale = match &value[split..] {
                "B" | "" => 1.0,
                "KB" => 1000.0,
                "MB" => 1e6,
                "GB" => 1e9,
                "KIB" => 1024.0,
                "MIB" => 1048576.0,
                "GIB" => 1073741824.0,
                _ => return None,
            };
            return Some((number * scale) as u64);
        }
    }
    None
}
