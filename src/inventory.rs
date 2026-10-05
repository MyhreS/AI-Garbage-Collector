use crate::{
    config::{Config, home},
    policy::Status,
    runtime::{command, git, now},
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeSet, HashSet},
    fs,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub path: Option<PathBuf>,
    pub bytes: u64,
    pub modified: u64,
    pub entries: u64,
    pub complete: bool,
    pub active: bool,
    pub cleanable: bool,
    pub protection: Option<String>,
    pub status: Status,
    pub reason: String,
    pub idle_seconds: u64,
    #[serde(default)]
    pub evidence: crate::evidence::Evidence,
}
impl Item {
    pub fn new(
        kind: &str,
        id: String,
        label: String,
        path: Option<PathBuf>,
        cleanable: bool,
    ) -> Self {
        let (bytes, modified, entries, complete) = path
            .as_ref()
            .map(|p| tree_stats(p))
            .unwrap_or((0, 0, 0, true));
        Self {
            id,
            kind: kind.into(),
            label,
            path,
            bytes,
            modified,
            entries,
            complete,
            active: false,
            cleanable,
            protection: None,
            status: Status::Unknown,
            reason: String::new(),
            idle_seconds: 0,
            evidence: Default::default(),
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    #[serde(default)]
    pub policy_version: u32,
    #[serde(default)]
    pub cached: bool,
    pub generated_at: u64,
    pub disk_total_bytes: u64,
    pub disk_free_bytes: u64,
    pub min_free_bytes: u64,
    pub last_collection: Option<u64>,
    pub service_installed: bool,
    pub warnings: Vec<String>,
    pub items: Vec<Item>,
    #[serde(default)]
    pub storage: crate::evidence::StorageSummary,
}
#[derive(Debug, Default)]
pub struct Activity {
    pub reliable: bool,
    pub busy: bool,
    pub global_reserved: bool,
    pub open_paths: Vec<PathBuf>,
    pub processes: Vec<crate::evidence::Process>,
    pub reserved: Vec<String>,
}
impl Activity {
    pub fn touches(&self, path: Option<&Path>) -> bool {
        path.is_some_and(|p| self.open_paths.iter().any(|open| open.starts_with(p)))
    }
}
#[cfg(unix)]
pub fn activity(state_dir: &Path) -> Activity {
    let uid = unsafe { libc::getuid() }.to_string();
    let files = command("lsof", &["-nP", "-a", "-u", &uid, "-Fpcfn"]);
    let procs = crate::platform::process_names();
    let mut a = Activity {
        reliable: files.is_ok() && procs.is_ok(),
        ..Default::default()
    };
    if let Ok(files) = files {
        a.processes = crate::evidence::parse_lsof(&files);
        a.open_paths = a.processes.iter().flat_map(|p| p.paths.clone()).collect();
        a.open_paths.retain(|p| p != Path::new("/") && p != &home());
    }

    if let Ok(procs) = procs {
        a.busy = procs.lines().any(|p| {
            let name = Path::new(p.trim())
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            matches!(
                name,
                "xcodebuild"
                    | "swift-build"
                    | "swift-frontend"
                    | "rustc"
                    | "cargo"
                    | "clang"
                    | "clang++"
                    | "make"
                    | "ninja"
                    | "gradle"
                    | "GradleDaemon"
                    | "npm"
                    | "pnpm"
                    | "yarn"
                    | "pip"
                    | "uv"
                    | "python"
                    | "python3"
                    | "node"
                    | "java"
                    | "poetry"
                    | "pip3"
                    | "bun"
                    | "codex"
                    | "claude"
                    | "aider"
                    | "opencode"
            )
        });
    }
    crate::leases::apply(state_dir, &mut a);

    a
}
// Allocated bytes; never follow symbolic links or cross into another mounted filesystem.
pub fn tree_stats(path: &Path) -> (u64, u64, u64, bool) {
    let Ok(root) = fs::symlink_metadata(path) else {
        return (0, 0, 0, false);
    };
    if crate::platform::is_link(&root) {
        return (0, 0, 0, false);
    }
    let Ok(root_id) = crate::platform::file_id(path, &root) else {
        return (0, 0, 0, false);
    };
    let mut bytes = 0u64;
    let mut modified = 0;
    let mut count = 0;
    let mut complete = true;
    let mut inodes = HashSet::new();
    for e in WalkDir::new(path)
        .follow_links(false)
        .same_file_system(true)
    {
        count += 1;
        if count > 500_000 {
            complete = false;
            break;
        }
        match e {
            Ok(e) => {
                let Ok(m) = e.metadata() else {
                    complete = false;
                    continue;
                };
                if cfg!(windows) && crate::platform::is_link(&m) {
                    complete = false;
                    continue;
                }
                let Ok(id) = crate::platform::file_id(e.path(), &m) else {
                    complete = false;
                    continue;
                };
                if crate::platform::volume(id) != crate::platform::volume(root_id) {
                    complete = false;
                    continue;
                }
                if inodes.insert(id) {
                    bytes = bytes.saturating_add(crate::platform::bytes(&m));
                }
                modified = modified.max(crate::platform::modified(&m));
            }
            Err(_) => complete = false,
        }
    }
    (bytes, modified, count, complete)
}
pub(crate) fn children(path: &Path) -> Vec<PathBuf> {
    fs::read_dir(path)
        .map(|es| es.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}
pub(crate) fn add_folder(items: &mut Vec<Item>, kind: &str, p: PathBuf, cleanable: bool) {
    if p.is_dir() && !p.is_symlink() {
        items.push(Item::new(
            kind,
            format!("{kind}:{}", p.display()),
            p.file_name().unwrap_or_default().to_string_lossy().into(),
            Some(p),
            cleanable,
        ));
    }
}
pub fn scan(c: &Config) -> (Vec<Item>, Vec<String>) {
    let mut items = vec![];
    let mut warnings = vec![];
    let h = home();
    for p in children(&h.join("Library/Developer/Xcode/DerivedData")) {
        add_folder(&mut items, "xcode-builds", p, true);
    }
    for (kind, paths) in [
        (
            "package-caches",
            vec![
                "Library/Caches/Homebrew",
                "Library/Caches/pip",
                "Library/Caches/uv",
                ".npm/_cacache",
            ],
        ),
        (
            "sdk",
            vec![
                "Library/Android/sdk/system-images",
                "Library/Android/sdk/ndk",
                "Library/Android/sdk/platforms",
            ],
        ),
        ("archives", vec!["Library/Developer/Xcode/Archives"]),
    ] {
        for rel in paths {
            add_folder(&mut items, kind, h.join(rel), false);
        }
    }
    let projects = discover_projects(c, &mut items, &mut warnings);
    if cfg!(target_os = "macos") {
        scan_simulators(&mut items, &mut warnings);
    }
    scan_android(&mut items, &mut warnings);
    if c.deep_inventory {
        crate::adapters::scan(c, &projects, &mut items, &mut warnings);
    }
    let mut seen = HashSet::new();
    items.retain(|i| seen.insert(i.id.clone()));
    items.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.id.cmp(&b.id)));
    (items, warnings)
}
fn discover_projects(
    c: &Config,
    items: &mut Vec<Item>,
    warnings: &mut Vec<String>,
) -> Vec<PathBuf> {
    let mut projects = BTreeSet::new();
    let mut repos = BTreeSet::new();
    let mut visited = HashSet::new();
    for root in &c.roots {
        if !root.exists() {
            continue;
        }
        if root.is_symlink() {
            warnings.push(format!("skipped symlink root {}", root.display()));
            continue;
        }
        let mut n = 0;
        let walker = WalkDir::new(root)
            .max_depth(8)
            .follow_links(false)
            .same_file_system(true)
            .into_iter()
            .filter_entry(|e| {
                if e.depth() == 0 {
                    return true;
                }
                let name = e.file_name().to_string_lossy();
                !matches!(
                    name.as_ref(),
                    ".git"
                        | "node_modules"
                        | "target"
                        | ".build"
                        | ".venv"
                        | "venv"
                        | ".next"
                        | "Pods"
                        | ".gradle"
                        | "build"
                        | "dist"
                        | "Library"
                )
            });
        for e in walker {
            n += 1;
            if n > 50_000 {
                warnings.push(format!(
                    "project discovery limit reached under {}",
                    root.display()
                ));
                break;
            }
            let e = match e {
                Ok(e) => e,
                Err(e) => {
                    warnings.push(format!("project discovery: {e}"));
                    continue;
                }
            };
            if !e.file_type().is_dir() {
                continue;
            }
            let p = e.path();
            if !visited.insert(p.to_path_buf()) {
                continue;
            }
            if p.join(".git").exists() {
                repos.insert(p.to_path_buf());
            }
            if [
                ".git",
                "package.json",
                "Cargo.toml",
                "Package.swift",
                "pyproject.toml",
                "build.gradle",
                "build.gradle.kts",
            ]
            .iter()
            .any(|name| p.join(name).exists())
            {
                projects.insert(p.to_path_buf());
            }
            let mut generated = vec![];
            if p.join("package.json").is_file() {
                generated.extend(["node_modules", ".next"]);
            }
            if p.join("Cargo.toml").is_file() {
                generated.push("target");
            }
            if p.join("Package.swift").is_file() {
                generated.push(".build");
            }
            if p.join("pyproject.toml").is_file() && p.join(".venv/pyvenv.cfg").is_file() {
                generated.push(".venv");
            }
            for name in generated {
                let kind = if matches!(name, "node_modules" | ".venv") {
                    "dependencies"
                } else {
                    "builds"
                };
                let target = p.join(name);
                if !target.is_dir() {
                    continue;
                }
                let mut i = Item::new(
                    kind,
                    format!("{kind}:{}", target.display()),
                    format!(
                        "{} / {name}",
                        p.file_name().unwrap_or_default().to_string_lossy()
                    ),
                    Some(target),
                    true,
                );
                match git(p, &["ls-files", "-z", "--", name]) {
                    Ok(s) if s.is_empty() => {}
                    Ok(_) => i.protection = Some("contains Git-tracked files".into()),
                    Err(_) => {
                        i.protection = Some("cannot verify generated directory against Git".into())
                    }
                }
                items.push(i);
            }
        }
    }
    let mut measured_worktrees = HashSet::new();
    for repo in repos {
        match git(&repo, &["worktree", "list", "--porcelain", "-z"]) {
            Ok(output) => {
                for (index, record) in output.split("\0\0").filter(|s| !s.is_empty()).enumerate() {
                    let mut fields = record.split('\0');
                    let Some(path) = fields.next().and_then(|s| s.strip_prefix("worktree ")) else {
                        continue;
                    };
                    let path = PathBuf::from(path);
                    if !measured_worktrees.insert(path.clone()) {
                        continue;
                    }
                    if !path.exists() {
                        continue;
                    }
                    let flags: Vec<_> = fields.collect();
                    let mut i = Item::new(
                        "worktrees",
                        format!("worktrees:{}", path.display()),
                        path.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into(),
                        Some(path.clone()),
                        true,
                    );
                    match git(&path, &["log", "-1", "--format=%ct", "HEAD"])
                        .and_then(|s| Ok(s.trim().parse::<u64>()?))
                    {
                        Ok(committed_at) => {
                            i.evidence
                                .metadata
                                .insert("head_committed_at".into(), committed_at.into());
                        }
                        Err(_) => {
                            i.protection = Some("cannot verify worktree HEAD commit time".into());
                        }
                    }
                    match git(&path, &["rev-parse", "HEAD"]) {
                        Ok(head) => {
                            i.evidence
                                .metadata
                                .insert("head_oid".into(), head.trim().into());
                        }
                        Err(_) => i.protection = Some("cannot verify worktree HEAD".into()),
                    }
                    match git(&path, &["rev-parse", "--symbolic-full-name", "HEAD"]) {
                        Ok(branch) => {
                            i.evidence
                                .metadata
                                .insert("branch".into(), branch.trim().into());
                        }
                        Err(_) => i.protection = Some("cannot verify worktree branch".into()),
                    }
                    if index == 0 {
                        i.protection = Some("primary checkout".into());
                    } else if flags
                        .iter()
                        .any(|s| s.starts_with("locked") || s.starts_with("prunable"))
                    {
                        i.protection = Some("Git marks this worktree locked or prunable".into());
                    } else {
                        match worktree_removable(&path) {
                            Ok(dirty) => {
                                i.evidence
                                    .metadata
                                    .insert("uncommitted_or_ignored_files".into(), dirty.into());
                            }
                            Err(e) => i.protection = Some(e.to_string()),
                        }
                    }
                    let latest_write_or_commit = i.modified.max(
                        i.evidence.metadata["head_committed_at"]
                            .as_u64()
                            .unwrap_or(u64::MAX),
                    );
                    if i.protection.is_none()
                        && c.worktree_cleanup
                        && (c.worktree_force
                            || !i.evidence.metadata["uncommitted_or_ignored_files"]
                                .as_bool()
                                .unwrap_or(true))
                        && now().saturating_sub(latest_write_or_commit)
                            >= c.pressure_retention_days.min(c.retention_days) * 86400
                    {
                        i.protection = match crate::github::open_pr(&path) {
                            Ok(Some(url)) => Some(format!("open GitHub pull request: {url}")),
                            Ok(None) => None,
                            Err(_) => {
                                i.evidence
                                    .metadata
                                    .insert("pr_verification".into(), "unavailable".into());
                                if c.worktree_require_pr_verification {
                                    Some(
                                        "could not verify GitHub pull requests; worktree protected"
                                            .into(),
                                    )
                                } else {
                                    None
                                }
                            }
                        };
                    }
                    items.push(i);
                }
            }
            Err(e) => warnings.push(format!("{}: {e}", repo.display())),
        }
    }
    projects.into_iter().collect()
}
pub fn worktree_removable(path: &Path) -> Result<bool> {
    let status = git(
        path,
        &[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--ignored",
        ],
    )?;
    anyhow::ensure!(
        !path.join(".gitmodules").exists(),
        "submodules require manual handling"
    );
    let common = git(path, &["rev-parse", "--git-common-dir"])?;
    let gd = git(path, &["rev-parse", "--git-dir"])?;
    anyhow::ensure!(common.trim() != gd.trim(), "primary checkout");
    let current = std::env::current_dir()?;
    anyhow::ensure!(!current.starts_with(path), "current working directory");
    Ok(!status.is_empty())
}
fn scan_simulators(items: &mut Vec<Item>, warnings: &mut Vec<String>) {
    match command("xcrun", &["simctl", "list", "--json"])
        .and_then(|s| Ok(serde_json::from_str::<Value>(&s)?))
    {
        Ok(v) => {
            if let Some(groups) = v["devices"].as_object() {
                for ds in groups.values().filter_map(|v| v.as_array()) {
                    for d in ds {
                        let Some(id) = d["udid"].as_str() else {
                            continue;
                        };
                        let path = home()
                            .join("Library/Developer/CoreSimulator/Devices")
                            .join(id);
                        let mut i = Item::new(
                            "simulators",
                            format!("simulators:{id}"),
                            d["name"].as_str().unwrap_or(id).into(),
                            Some(path),
                            true,
                        );
                        i.active = d["state"].as_str() != Some("Shutdown");
                        items.push(i);
                    }
                }
            }
            if let Some(rs) = v["runtimes"].as_array() {
                for r in rs {
                    let id = r["identifier"].as_str().unwrap_or("unknown");
                    let path = r["bundlePath"].as_str().map(PathBuf::from);
                    items.push(Item::new(
                        "runtimes",
                        format!("runtimes:{id}"),
                        r["name"].as_str().unwrap_or(id).into(),
                        path,
                        false,
                    ));
                }
            }
        }
        Err(e) => warnings.push(format!("iOS inventory unavailable: {e}")),
    }
}
fn scan_android(items: &mut Vec<Item>, warnings: &mut Vec<String>) {
    let root = std::env::var_os("ANDROID_AVD_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".android/avd"));
    let ps = crate::platform::process_names();
    let running = ps
        .as_ref()
        .map(|s| crate::platform::emulator_running(s))
        .unwrap_or(true);
    for p in children(&root) {
        if p.extension().is_none_or(|s| s != "avd") {
            continue;
        }
        let name = p
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let mut i = Item::new(
            "emulators",
            format!("emulators:{name}"),
            name,
            Some(p),
            true,
        );
        // Defer all AVD removal while any emulator runs; mapping individual instances is not assumed.
        if running {
            i.protection = Some("an Android emulator may be running".into());
        }
        if ps.is_err() {
            i.complete = false;
        }
        items.push(i);
    }
    if !root.exists() {
        warnings.push("Android AVD directory not present".into());
    }
}
pub fn report(c: &Config, dir: &Path) -> Result<(Report, Activity)> {
    let (mut items, mut warnings) = scan(c);
    add_folder(&mut items, "recovery-backups", dir.join("backups"), false);
    let a = activity(dir);
    let storage = crate::evidence::enrich(&mut items, c, &a);
    if !a.reliable {
        warnings.push("Activity inspection incomplete: cleanup is disabled for this scan".into());
    }
    let (total, free) = crate::runtime::disk(&home())?;
    Ok((
        Report {
            schema_version: 2,
            policy_version: crate::policy::POLICY_VERSION,
            cached: false,
            generated_at: now(),
            disk_total_bytes: total,
            disk_free_bytes: free,
            min_free_bytes: c.min_free_bytes,
            last_collection: None,
            service_installed: crate::service::installed(),
            warnings,
            items,
            storage,
        },
        a,
    ))
}

#[cfg(windows)]
pub fn activity(state_dir: &Path) -> Activity {
    let names = crate::platform::process_names();
    let mut a = Activity {
        reliable: names.is_ok(),
        ..Default::default()
    };
    if let Ok(names) = names {
        // Windows has no lsof equivalent built in. Without per-directory attribution,
        // any recognized developer process reserves the entire cleanup pass.
        a.busy = names.lines().any(|n| {
            let n = n.trim().to_ascii_lowercase();
            [
                "codex", "claude", "aider", "opencode", "rustc", "cargo", "clang", "cl", "msbuild",
                "devenv", "cmake", "ninja", "gradle", "java", "node", "python", "pip", "uv",
                "poetry", "bun", "emulator", "qemu", "studio", "code",
            ]
            .iter()
            .any(|p| n == *p || n.starts_with(&format!("{p}-")))
        });
        a.global_reserved = a.busy;
    }
    crate::leases::apply(state_dir, &mut a);
    a
}
