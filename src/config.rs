use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub const GIB: u64 = 1024 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub roots: Vec<PathBuf>,
    pub retention_days: u64,
    pub pressure_retention_days: u64,
    pub min_free_bytes: u64,
    pub docker_cache_budget_bytes: u64,
    pub backup_budget_bytes: u64,
    pub docker_cache_cleanup: bool,
    pub docker_keep_most_used: usize,
    pub max_delete_bytes_per_run: u64,
    pub paused_until: u64,
    pub pins: Vec<String>,
    pub managed: BTreeMap<String, String>,
}
impl Default for Config {
    fn default() -> Self {
        let h = home();
        Self {
            roots: [
                "workdir",
                "Developer",
                "Projects",
                ".codex/worktrees",
                ".codex-workspaces/worktrees",
                ".claude/worktrees",
            ]
            .map(|p| h.join(p))
            .to_vec(),
            retention_days: 30,
            pressure_retention_days: 7,
            min_free_bytes: 20 * GIB,
            docker_cache_budget_bytes: 5 * GIB,
            backup_budget_bytes: 2 * GIB,
            docker_cache_cleanup: true,
            docker_keep_most_used: 3,
            max_delete_bytes_per_run: 50 * GIB,
            paused_until: 0,
            pins: vec![],
            managed: BTreeMap::new(),
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.retention_days == 0
            || self.pressure_retention_days == 0
            || self.retention_days > 3650
            || self.pressure_retention_days > self.retention_days
        {
            bail!(
                "retention must be 1..3650 days; pressure retention cannot exceed normal retention"
            );
        }
        if !(3..=1000).contains(&self.docker_keep_most_used) {
            bail!(
                "docker.keep-most-used must be between 3 and 1000; the top three are always protected"
            );
        }
        if self.max_delete_bytes_per_run == 0 {
            bail!("max_delete_bytes_per_run must be positive");
        }
        for p in &self.roots {
            if !p.is_absolute() || p == Path::new("/") || p == &home() {
                bail!("roots must be absolute project directories, not / or your entire home");
            }
        }
        Ok(())
    }
    pub fn load(dir: &Path) -> Result<Self> {
        let p = dir.join("config.json");
        let c: Self = if p.exists() {
            serde_json::from_slice(&fs::read(p)?)
                .context("invalid config.json; collection refused")?
        } else {
            Self::default()
        };
        c.validate()?;
        Ok(c)
    }
    pub fn save(&self, dir: &Path) -> Result<()> {
        self.validate()?;
        atomic_json(&dir.join("config.json"), self)?;
        let _ = fs::remove_file(dir.join("last-report.json"));
        Ok(())
    }
}
pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .expect("HOME must be set")
}
pub fn state_dir() -> PathBuf {
    std::env::var_os("AIGC_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join("Library/Application Support/aigc"))
}
pub fn atomic_json(path: &Path, data: &impl Serialize) -> Result<()> {
    let parent = path.parent().context("missing parent")?;
    fs::create_dir_all(parent)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(data)?)?;
    fs::rename(tmp, path)?;
    Ok(())
}
pub fn bytes(s: &str) -> Result<u64> {
    let s = s.trim().to_uppercase();
    for (suffix, scale) in [
        ("GIB", GIB),
        ("GB", GIB),
        ("MIB", 1024 * 1024),
        ("MB", 1024 * 1024),
        ("KIB", 1024),
        ("KB", 1024),
        ("B", 1),
    ] {
        if let Some(n) = s.strip_suffix(suffix) {
            return n
                .trim()
                .parse::<u64>()?
                .checked_mul(scale)
                .context("size overflow");
        }
    }
    Ok(s.parse()?)
}
