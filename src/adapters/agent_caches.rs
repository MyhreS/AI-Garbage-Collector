//! Only explicitly identified, regenerable cache entries are disposable.
use super::*;
use std::time::UNIX_EPOCH;

pub const RETENTION_SECONDS: u64 = 7 * 86400;

fn roots(c: &Config) -> Vec<(String, PathBuf)> {
    let mut roots = vec![
        ("codex".into(), home().join(".codex/cache")),
        ("claude".into(), home().join(".claude/cache")),
    ];
    for (owner, paths) in &c.agent_cache_roots {
        roots.extend(paths.iter().map(|p| (owner.clone(), p.clone())));
    }
    roots.sort();
    roots.dedup();
    roots
}

// Custom locations are opt-in and must have an unambiguous cache basename.
// Never permit arbitrary application, session, source, or home roots here.
pub fn validate_root(owner: &str, path: &Path) -> Result<()> {
    ensure!(matches!(owner, "codex" | "claude"), "unknown cache owner");
    ensure!(
        path.is_absolute()
            && path
                .file_name()
                .is_some_and(|name| name == format!("{owner}-cache").as_str())
            && !path
                .components()
                .any(|p| matches!(p, std::path::Component::ParentDir)),
        "custom agent cache roots must be absolute directories named codex-cache or claude-cache"
    );
    ensure!(
        !home().starts_with(path) && path != home(),
        "cache root cannot contain the user's home"
    );
    Ok(())
}

fn inspect(root: &Path, path: &Path) -> Result<u64> {
    crate::runtime::canonical(root)?;
    crate::runtime::canonical(path)?;
    ensure!(path.parent() == Some(root), "not a direct cache entry");
    for ancestor in path.ancestors() {
        ensure!(
            !crate::platform::is_link(&fs::symlink_metadata(ancestor)?),
            "linked cache path"
        );
    }
    // An explicit cache name does not authorize deletion of tracked source.
    for ancestor in root.ancestors() {
        if ancestor.join(".git").try_exists()? {
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .context("invalid cache entry")?;
            ensure!(
                git(root, &["ls-files", "-z", "--", name])?.is_empty(),
                "cache entry contains tracked source"
            );
            break;
        }
    }
    let mut last_used = 0;
    for (count, entry) in walkdir::WalkDir::new(path)
        .follow_links(false)
        .same_file_system(true)
        .into_iter()
        .enumerate()
    {
        ensure!(count < 500_000, "cache inspection incomplete");
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        ensure!(!crate::platform::is_link(&metadata), "linked cache content");
        ensure!(
            metadata.is_file() || metadata.is_dir(),
            "special cache file"
        );
        ensure!(
            entry.file_name() != ".git" && entry.file_name() != ".gitmodules",
            "repository inside cache; preserve it"
        );
        let modified = metadata.modified()?.duration_since(UNIX_EPOCH)?.as_secs();
        last_used = last_used.max(modified);
        // Directory reads by inventory itself must not keep resetting the timer.
        if metadata.is_file() {
            last_used = last_used.max(metadata.accessed()?.duration_since(UNIX_EPOCH)?.as_secs());
        }
    }
    Ok(last_used)
}

pub fn scan(c: &Config, items: &mut Vec<Item>, warnings: &mut Vec<String>) {
    for (owner, root) in roots(c) {
        if !root.exists() {
            continue;
        }
        let entries = match fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(e) => {
                warnings.push(format!(
                    "agent cache discovery failed at {}: {e}",
                    root.display()
                ));
                continue;
            }
        };
        for entry in entries {
            let path = match entry {
                Ok(entry) => entry.path(),
                Err(e) => {
                    warnings.push(format!("agent cache entry discovery failed: {e}"));
                    continue;
                }
            };
            let mut item = folder(
                "agent-caches",
                path.clone(),
                "recognized or explicitly configured agent cache",
            );
            item.label = format!("{owner} cache: {}", item.label);
            item.cleanable = true;
            item.evidence.owners.push(owner.clone());
            item.evidence.action = Some(Action::AgentCache {
                owner: owner.clone(),
                root: root.clone(),
            });
            item.evidence.reconstruction = "Regenerable cache only; sessions, credentials, plugins, application data and virtual disks are not discovered. Seven-day observation and file/use age checks apply.".into();
            match inspect(&root, &path) {
                Ok(last_used) => item.evidence.native_last_used = Some(last_used),
                Err(e) => item.protection = Some(e.to_string()),
            }
            items.push(item);
        }
    }
}

pub fn remove(item: &Item, owner: &str, root: &Path, c: &Config) -> Result<String> {
    ensure!(
        roots(c).iter().any(|(o, p)| o == owner && p == root),
        "cache root configuration changed"
    );
    let path = item.path.as_ref().context("missing cache path")?;
    let last_used = inspect(root, path)?;
    ensure!(
        crate::runtime::now().saturating_sub(last_used) >= RETENTION_SECONDS,
        "cache was used or changed within seven days"
    );
    ensure!(
        !std::env::current_dir()?.starts_with(path),
        "cache contains collector working directory"
    );
    let (bytes, modified, entries, complete) = crate::inventory::tree_stats(path);
    ensure!(
        complete
            && bytes == item.bytes
            && modified == item.modified
            && entries == item.entries
            && item.evidence.identity.as_ref().is_some_and(
                |id| crate::platform::identity(path).is_ok_and(|current| current == *id)
            ),
        "cache identity or contents changed after inventory"
    );
    if fs::symlink_metadata(path)?.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok("Removed regenerable agent cache entry after seven-day inactivity and safety checks".into())
}
