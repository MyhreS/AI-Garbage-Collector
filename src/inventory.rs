use crate::{
    config::{Config, home},
    policy::Status,
    runtime::{command, git, now},
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs,
    os::unix::fs::MetadataExt,
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
    pub observed_uses: u64,
    #[serde(default)]
    pub docker_keep_rank: Option<usize>,
    #[serde(default)]
    pub docker_created_at: String,
    #[serde(skip_serializing, default)]
    pub docker_start_tokens: BTreeMap<String, String>,
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
            observed_uses: 0,
            docker_keep_rank: None,
            docker_created_at: String::new(),
            docker_start_tokens: BTreeMap::new(),
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
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
}
#[derive(Debug, Default)]
pub struct Activity {
    pub reliable: bool,
    pub busy: bool,
    pub open_paths: Vec<PathBuf>,
}
impl Activity {
    pub fn touches(&self, path: Option<&Path>) -> bool {
        path.is_some_and(|p| self.open_paths.iter().any(|open| open.starts_with(p)))
    }
}
pub fn activity(state_dir: &Path) -> Activity {
    let uid = unsafe { libc::getuid() }.to_string();
    let files = command("/usr/sbin/lsof", &["-nP", "-a", "-u", &uid, "-Fn"]);
    let procs = command("/bin/ps", &["-axo", "comm="]);
    let mut a = Activity {
        reliable: files.is_ok() && procs.is_ok(),
        ..Default::default()
    };
    if let Ok(files) = files {
        a.open_paths = files
            .lines()
            .filter_map(|s| s.strip_prefix('n'))
            .filter(|s| s.starts_with('/'))
            .map(PathBuf::from)
            .collect();
        // Open executable/library parent directories are not activity evidence for all siblings.
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
                    | "codex"
                    | "claude"
                    | "aider"
                    | "opencode"
            )
        });
    }
    if let Ok(leases) = fs::read_dir(state_dir.join("leases")) {
        for lease in leases.flatten() {
            let data = fs::read(lease.path())
                .ok()
                .and_then(|v| serde_json::from_slice::<Value>(&v).ok());
            if let Some(v) = data {
                if let (Some(pid), Some(start)) = (v["pid"].as_u64(), v["start"].as_str()) {
                    if command("/bin/ps", &["-p", &pid.to_string(), "-o", "lstart="])
                        .is_ok_and(|s| s.trim() == start)
                    {
                        a.busy = true;
                    }
                } else {
                    a.reliable = false;
                }
            } else {
                a.reliable = false;
            }
        }
    }
    a
}
// Allocated bytes; never follow symbolic links or cross into another mounted filesystem.
pub fn tree_stats(path: &Path) -> (u64, u64, u64, bool) {
    let Ok(root) = fs::symlink_metadata(path) else {
        return (0, 0, 0, false);
    };
    if root.file_type().is_symlink() {
        return (0, 0, 0, false);
    }
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
        match e.and_then(|e| e.metadata()) {
            Ok(m) => {
                if m.dev() != root.dev() {
                    complete = false;
                    continue;
                }
                if inodes.insert((m.dev(), m.ino())) {
                    bytes = bytes.saturating_add(m.blocks() * 512);
                }
                modified = modified.max(m.mtime().max(0) as u64);
            }
            Err(_) => complete = false,
        }
    }
    (bytes, modified, count, complete)
}
fn children(path: &Path) -> Vec<PathBuf> {
    fs::read_dir(path)
        .map(|es| es.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}
fn add_folder(items: &mut Vec<Item>, kind: &str, p: PathBuf, cleanable: bool) {
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
    discover_projects(c, &mut items, &mut warnings);
    scan_simulators(&mut items, &mut warnings);
    scan_android(&mut items, &mut warnings);
    scan_docker(&mut items, &mut warnings);
    let mut seen = HashSet::new();
    items.retain(|i| seen.insert(i.id.clone()));
    items.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.id.cmp(&b.id)));
    (items, warnings)
}
fn discover_projects(c: &Config, items: &mut Vec<Item>, warnings: &mut Vec<String>) {
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
                    if index == 0 {
                        i.protection = Some("primary checkout".into());
                    } else if flags
                        .iter()
                        .any(|s| s.starts_with("locked") || s.starts_with("prunable"))
                    {
                        i.protection = Some("Git marks this worktree locked or prunable".into());
                    } else if let Err(e) = worktree_safe(&path) {
                        i.protection = Some(e.to_string());
                    }
                    // App-managed trees must be archived by their owning application.
                    if path.components().any(|p| {
                        matches!(p.as_os_str().to_str(), Some(".codex" | ".codex-workspaces"))
                    }) {
                        i.protection = Some(
                            "application-managed worktree; archive through its owning application"
                                .into(),
                        );
                    }
                    items.push(i);
                }
            }
            Err(e) => warnings.push(format!("{}: {e}", repo.display())),
        }
    }
}
pub fn worktree_safe(path: &Path) -> Result<()> {
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
        status.is_empty(),
        "contains modified, untracked or ignored files"
    );
    anyhow::ensure!(
        !path.join(".gitmodules").exists(),
        "submodules require manual handling"
    );
    let common = git(path, &["rev-parse", "--git-common-dir"])?;
    let gd = git(path, &["rev-parse", "--git-dir"])?;
    anyhow::ensure!(common.trim() != gd.trim(), "primary checkout");
    let current = std::env::current_dir()?;
    anyhow::ensure!(!current.starts_with(path), "current working directory");
    Ok(())
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
    let ps = command("/bin/ps", &["-axo", "comm="]);
    let running = ps
        .as_ref()
        .map(|s| {
            s.lines()
                .any(|l| l.contains("qemu-system") || l.ends_with("/emulator"))
        })
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
pub fn local_docker() -> Result<()> {
    // Never operate on remote Docker contexts, including environment overrides.
    anyhow::ensure!(
        std::env::var_os("DOCKER_HOST").is_none(),
        "DOCKER_HOST override is protected; use a local Docker context"
    );
    let context = command("docker", &["context", "show"])?;
    let raw = command("docker", &["context", "inspect", context.trim()])?;
    let v: Value = serde_json::from_str(&raw)?;
    let endpoint = v[0]["Endpoints"]["docker"]["Host"]
        .as_str()
        .context("Docker endpoint missing")?;
    anyhow::ensure!(
        endpoint.starts_with("unix://"),
        "remote Docker contexts are outside this tool's scope"
    );
    Ok(())
}
fn scan_docker(items: &mut Vec<Item>, warnings: &mut Vec<String>) {
    let result = (|| -> Result<()> {
        local_docker()?;
        let ids = command("docker", &["image", "ls", "-aq", "--no-trunc"])?;
        let container_ids = command("docker", &["container", "ls", "-aq"])?;
        let mut used = HashSet::new();
        let mut starts: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        for id in container_ids.lines() {
            let data = command("docker", &["container", "inspect", id])?;
            let v: Value = serde_json::from_str(&data)?;
            if let Some(image_id) = v[0]["Image"].as_str() {
                used.insert(image_id.to_string());
                if let Some(start) = v[0]["State"]["StartedAt"]
                    .as_str()
                    .filter(|s| !s.starts_with("0001-"))
                {
                    starts
                        .entry(image_id.to_string())
                        .or_default()
                        .insert(id.to_string(), start.to_string());
                }
            }
        }
        for id in ids.lines().collect::<BTreeSet<_>>() {
            let raw = command("docker", &["image", "inspect", id])?;
            let v: Value = serde_json::from_str(&raw)?;
            let v = &v[0];
            let tag = v["RepoTags"][0].as_str().unwrap_or(id);
            let mut i = Item::new(
                "docker-images",
                format!("docker-images:{id}"),
                tag.into(),
                None,
                true,
            );
            i.bytes = v["Size"].as_u64().unwrap_or(0);
            i.active = used.contains(id);
            i.docker_start_tokens = starts.remove(id).unwrap_or_default();
            i.docker_created_at = v["Created"].as_str().unwrap_or_default().to_string();
            items.push(i);
        }
        let df = command("docker", &["system", "df", "--format", "{{json .}}"])?;
        for line in df.lines() {
            let v: Value = serde_json::from_str(line)?;
            if v["Type"] == "Build Cache" {
                let mut i = Item::new(
                    "docker-cache",
                    "docker-cache:default".into(),
                    format!(
                        "Default builder: {} total; {} reclaimable (Docker)",
                        v["Size"].as_str().unwrap_or("unknown"),
                        v["Reclaimable"].as_str().unwrap_or("unknown")
                    ),
                    None,
                    true,
                );
                i.entries = v["TotalCount"]
                    .as_u64()
                    .or_else(|| v["TotalCount"].as_str().and_then(|s| s.parse().ok()))
                    .unwrap_or(0);
                // Human-formatted Docker sizes are kept as labels; do not pretend these are exact bytes.
                i.active = v["Active"]
                    .as_u64()
                    .or_else(|| v["Active"].as_str().and_then(|s| s.parse().ok()))
                    .unwrap_or(1)
                    > 0;
                items.push(i);
            }
        }
        Ok(())
    })();
    if let Err(e) = result {
        warnings.push(format!("Docker inventory unavailable or incomplete: {e}"));
        for i in items.iter_mut().filter(|i| i.kind.starts_with("docker")) {
            i.complete = false;
        }
    }
}
pub fn report(c: &Config, dir: &Path) -> Result<(Report, Activity)> {
    let (mut items, mut warnings) = scan(c);
    add_folder(&mut items, "recovery-backups", dir.join("backups"), false);
    let a = activity(dir);
    if !a.reliable {
        warnings.push("Activity inspection incomplete: cleanup is disabled for this scan".into());
    }
    let (total, free) = crate::runtime::disk(&home())?;
    Ok((
        Report {
            schema_version: 1,
            cached: false,
            generated_at: now(),
            disk_total_bytes: total,
            disk_free_bytes: free,
            min_free_bytes: c.min_free_bytes,
            last_collection: None,
            service_installed: crate::service::plist_path().exists(),
            warnings,
            items,
        },
        a,
    ))
}
