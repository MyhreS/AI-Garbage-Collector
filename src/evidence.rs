//! Evidence describes observations; it never grants disposal permission.
use crate::{
    config::{Config, home},
    inventory::{Activity, Item},
    runtime::{command, now},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Process {
    pub pid: u32,
    pub executable: String,
    pub started: Option<String>,
    pub cwd: Option<PathBuf>,
    #[serde(skip)]
    pub paths: Vec<PathBuf>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Consumer {
    pub id: String,
    pub source: String,
    pub protects: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    Directory,
    Poetry {
        project: PathBuf,
        environment: PathBuf,
    },
    Cache {
        manager: String,
        root: PathBuf,
    },
    Buildkit {
        builder: String,
        endpoint: String,
        record: String,
    },
    Runtime {
        uuid: String,
        build: String,
    },
    Sdk {
        root: PathBuf,
        package: String,
    },
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Evidence {
    pub owners: Vec<String>,
    pub consumers: Vec<Consumer>,
    pub processes: Vec<Process>,
    pub source: String,
    pub observed_at: u64,
    pub first_seen: u64,
    pub last_observed_use: Option<u64>,
    pub native_last_used: Option<u64>,
    pub native_usage_count: Option<u64>,
    pub observation_gap: bool,
    pub reclaimable_bytes: Option<u64>,
    pub shared: Option<bool>,
    pub identity: Option<String>,
    pub logical_bytes: Option<u64>,
    pub fingerprint: Option<String>,
    pub reconstruction: String,
    pub metadata: BTreeMap<String, Value>,
    pub action: Option<Action>,
    pub blockers: Vec<String>,
    pub registered: bool,
}
impl Evidence {
    pub fn consumer(&mut self, id: impl Into<String>, source: &str, protects: bool) {
        let id = id.into();
        if !self
            .consumers
            .iter()
            .any(|c| c.id == id && c.source == source)
        {
            self.consumers.push(Consumer {
                id,
                source: source.into(),
                protects,
            });
        }
    }
}
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct StorageSummary {
    pub filesystem_allocated_union_bytes: u64,
    pub complete: bool,
    pub measured_entries: u64,
    pub note: String,
}

pub fn parse_lsof(output: &str) -> Vec<Process> {
    let starts = command("/bin/ps", &["-axo", "pid=,lstart="]).unwrap_or_default();
    let starts: BTreeMap<u32, String> = starts
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            let (pid, start) = l.split_once(char::is_whitespace)?;
            Some((pid.parse().ok()?, start.trim().into()))
        })
        .collect();
    let mut result = Vec::new();
    let mut current: Option<Process> = None;
    let mut fd = "";
    for line in output.lines() {
        if let Some(pid) = line.strip_prefix('p').and_then(|p| p.parse::<u32>().ok()) {
            if let Some(p) = current.take() {
                result.push(p);
            }
            current = Some(Process {
                pid,
                started: starts.get(&pid).cloned(),
                ..Default::default()
            });
        } else if let Some(p) = current.as_mut() {
            if let Some(name) = line.strip_prefix('c') {
                p.executable = name.into();
            }
            if let Some(f) = line.strip_prefix('f') {
                fd = f;
            }
            if let Some(path) = line.strip_prefix('n').filter(|s| s.starts_with('/')) {
                let path = PathBuf::from(path);
                if fd == "cwd" {
                    p.cwd = Some(path.clone());
                }
                if path != Path::new("/") && path != home() {
                    p.paths.push(path);
                }
            }
        }
    }
    if let Some(p) = current {
        result.push(p);
    }
    result
}

pub fn enrich(items: &mut [Item], c: &Config, a: &Activity) -> StorageSummary {
    let paths: Vec<_> = items
        .iter()
        .filter_map(|i| i.path.as_ref().map(|p| (i.id.clone(), p.clone())))
        .collect();
    for i in items.iter_mut() {
        i.evidence.observed_at = now();
        if i.evidence.source.is_empty() {
            i.evidence.source = "filesystem and native inventory".into();
        }
        if let Some(owner) = c.owners.get(&i.id).or_else(|| c.managed.get(&i.id)) {
            i.evidence.owners.push(owner.clone());
        }
        if let Some(p) = &i.path {
            if let Ok(m) = fs::symlink_metadata(p) {
                i.evidence.identity = Some(format!("{}:{}", m.dev(), m.ino()));
            }
            for (id, parent) in &paths {
                if p != parent && p.starts_with(parent) {
                    i.evidence.consumer(id, "containing resource", false);
                }
            }
            for process in &a.processes {
                let project_use = i.evidence.consumers.iter().any(|r| {
                    matches!(
                        r.source.as_str(),
                        "Python project"
                            | "build project"
                            | "JavaScript project"
                            | "Xcode DerivedData metadata"
                    ) && r.id.starts_with('/')
                        && process
                            .cwd
                            .as_ref()
                            .is_some_and(|cwd| cwd.starts_with(&r.id))
                });
                if project_use || process.paths.iter().any(|open| open.starts_with(p)) {
                    i.evidence.processes.push(process.clone());
                }
            }
        }
        for (project, requirements) in &c.requirements {
            if requirements.contains(&i.id) {
                i.evidence.consumer(project, "explicit requirement", true);
            }
        }
        if a.reserved.iter().any(|id| {
            id == &i.id
                || i.path.as_ref().is_some_and(|p| {
                    id.starts_with('/') && (p.starts_with(id) || Path::new(id).starts_with(p))
                })
        }) {
            i.active = true;
            i.evidence.consumer("active reservation", "aigc run", true);
        }
        if !i.evidence.processes.is_empty() {
            i.active = true;
        }
        for (id, path) in &paths {
            if i.path
                .as_ref()
                .is_some_and(|p| p != path && (p.starts_with(path) || path.starts_with(p)))
                && (c.pins.contains(id)
                    || a.reserved.contains(id)
                    || c.requirements.values().any(|ids| ids.contains(id)))
            {
                i.evidence
                    .consumer(id, "protected related filesystem resource", true);
            }
        }
        i.evidence.owners.sort();
        i.evidence.owners.dedup();
    }
    // Walk only outermost filesystem resources; deduplicate hardlinks across all roots.
    let mut roots: Vec<PathBuf> = paths
        .into_iter()
        .map(|(_, p)| p)
        .filter(|p| !p.is_symlink())
        .collect();
    roots.sort();
    roots.dedup();
    let roots: Vec<_> = roots
        .iter()
        .filter(|p| !roots.iter().any(|q| q != *p && p.starts_with(q)))
        .collect();
    let mut summary = StorageSummary { complete: items.iter().filter(|i| i.path.is_some()).all(|i| i.complete), note: "Filesystem union counts hardlinks once and excludes nested duplicate totals. APFS clone/snapshot sharing and Docker VM internals are not exact reclaimable bytes.".into(), ..Default::default() };
    let mut seen = HashSet::new();
    'roots: for p in roots {
        for entry in WalkDir::new(p).follow_links(false).same_file_system(true) {
            summary.measured_entries += 1;
            if summary.measured_entries > 1_000_000 {
                summary.complete = false;
                break 'roots;
            }
            match entry.and_then(|e| e.metadata()) {
                Ok(m) if seen.insert((m.dev(), m.ino())) => {
                    summary.filesystem_allocated_union_bytes = summary
                        .filesystem_allocated_union_bytes
                        .saturating_add(m.blocks().saturating_mul(512))
                }
                Ok(_) => {}
                Err(_) => summary.complete = false,
            }
        }
    }
    summary
}

pub fn duplicates(items: &[Item]) -> Vec<Value> {
    let mut groups: BTreeMap<&str, Vec<&Item>> = BTreeMap::new();
    for item in items {
        if let Some(key) = item.evidence.fingerprint.as_deref() {
            groups.entry(key).or_default().push(item);
        }
    }
    groups.into_iter().filter(|(_, group)| group.len() > 1).map(|(fingerprint, group)| serde_json::json!({
        "fingerprint":fingerprint, "resources":group.iter().map(|i| &i.id).collect::<Vec<_>>(),
        "confidence":"matching recorded inputs; not proof of interchangeable installations",
        "action":"report only; mutable environments are never merged"
    })).collect()
}
