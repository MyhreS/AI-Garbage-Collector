use crate::{
    config::{Config, atomic_json, home},
    inventory::{self, Item, Report},
    policy::{self, Status},
    runtime::{canonical, git, now},
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
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
        cfg!(any(target_os = "macos", target_os = "linux", windows)),
        "automatic cleanup requires macOS, Linux or Windows"
    );
    let mut results = vec![];
    for item in report.items.iter().filter(|i| i.status == Status::Eligible) {
        // Revalidate activity immediately before each action, while holding the shared lock.
        let activity = inventory::activity(dir);
        let reserved = activity.reserved.iter().any(|id| {
            id == &item.id
                || item.path.as_ref().is_some_and(|p| {
                    Path::new(id).is_absolute()
                        && (p.starts_with(id) || Path::new(id).starts_with(p))
                })
        });
        let precheck = if !activity.reliable
            || activity.global_reserved
            || (activity.busy && !matches!(item.kind.as_str(), "worktrees" | "python"))
            || (item.kind == "python" && activity.python_activity_unknown)
            || reserved
            || activity.touches(item.path.as_deref())
        {
            Some("activity or reservation changed; deferred")
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
            Some(crate::evidence::Action::Cache { .. })
        );
        let (outcome, detail, estimate) = match result {
            Ok(detail) => {
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
        item.evidence
            .identity
            .as_ref()
            .is_none_or(|id| { crate::platform::identity(&p).is_ok_and(|current| *id == current) })
            && complete
            && size == item.bytes
            && modified == item.modified
            && entries == item.entries,
        "resource changed after the scan; collection deferred"
    );
    Ok(p)
}
fn verify_worktree_ref(item: &Item, path: &Path) -> Result<()> {
    let head = git(path, &["rev-parse", "HEAD"])?;
    let branch = git(path, &["rev-parse", "--symbolic-full-name", "HEAD"])?;
    ensure!(
        item.evidence
            .metadata
            .get("head_oid")
            .and_then(serde_json::Value::as_str)
            == Some(head.trim())
            && item
                .evidence
                .metadata
                .get("branch")
                .and_then(serde_json::Value::as_str)
                == Some(branch.trim()),
        "worktree HEAD or branch changed after the scan"
    );
    Ok(())
}
fn remove(item: &Item, c: &Config, dir: &Path) -> Result<String> {
    if item.evidence.action.is_some() {
        if matches!(
            item.evidence.action,
            Some(
                crate::evidence::Action::Directory
                    | crate::evidence::Action::Poetry { .. }
                    | crate::evidence::Action::Cache { .. }
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
            ensure!(c.worktree_cleanup, "worktree cleanup disabled");
            let p = verify_path(item)?;
            ensure!(
                c.roots.iter().any(|r| p.starts_with(r)),
                "worktree is outside configured roots"
            );
            let dirty = inventory::worktree_removable(&p)?;
            verify_worktree_ref(item, &p)?;
            ensure!(
                !dirty || c.worktree_force,
                "worktree has local files and force removal is disabled"
            );
            crate::github::ensure_no_open_pr(&p, c.worktree_require_pr_verification)?;
            let listed = git(&p, &["worktree", "list", "--porcelain", "-z"])?;
            let rec = listed
                .split("\0\0")
                .find(|r| {
                    r.split('\0')
                        .next()
                        .and_then(|f| f.strip_prefix("worktree "))
                        .is_some_and(|registered| {
                            canonical(Path::new(registered)).is_ok_and(|registered| registered == p)
                        })
                })
                .context("worktree no longer registered")?;
            ensure!(
                !rec.split('\0').any(|f| f.starts_with("locked")),
                "worktree is locked"
            );
            let activity = inventory::activity(dir);
            ensure!(
                activity.reliable
                    && !activity.global_reserved
                    && !activity.touches(Some(&p))
                    && !activity.reserved.iter().any(|id| {
                        id == &item.id
                            || (Path::new(id).is_absolute()
                                && (p.starts_with(id) || Path::new(id).starts_with(&p)))
                    }),
                "worktree became active or reserved"
            );
            verify_path(item)?;
            ensure!(
                inventory::worktree_removable(&p)? == dirty,
                "worktree status changed before removal"
            );
            crate::github::ensure_no_open_pr(&p, c.worktree_require_pr_verification)?;
            verify_worktree_ref(item, &p)?;
            let path = p.to_str().context("invalid worktree path")?;
            // Windows keeps a process's working directory open. Run removal
            // outside the target tree and select its repository explicitly.
            let common = git(
                &p,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )?;
            let mut args = vec!["--git-dir", common.trim(), "worktree", "remove"];
            if dirty {
                args.push("--force");
            }
            args.extend(["--", path]);
            // Removing large dependency trees can legitimately exceed the
            // short timeout used for read-only Git metadata queries.
            crate::runtime::command_at("git", &args, Some(&home()), 3600)?;
            Ok(if dirty {
                "Force-removed idle worktree and discarded tracked edits, untracked and ignored files; named branch retained if present; no recovery archive"
            } else {
                "Removed idle clean worktree; named branch retained if present; no recovery archive"
            }.into())
        }
        _ => bail!("report-only resource"),
    }
}

// Reports are plans, never authority to mutate stale resources. Rescan consumers and activity.
fn revalidate(item: &Item, c: &Config, dir: &Path) -> Result<()> {
    // Worktree removal already rechecks Git registration, refs, dirty state, PRs,
    // activity and the complete tree. Do not rescan every unrelated cache/tree.
    if item.kind == "worktrees" {
        verify_path(item)?;
        return Ok(());
    }
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
            && current.entries == item.entries
            && (item.kind != "worktrees"
                || (current.evidence.metadata.get("head_oid")
                    == item.evidence.metadata.get("head_oid")
                    && current.evidence.metadata.get("branch")
                        == item.evidence.metadata.get("branch"))),
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
