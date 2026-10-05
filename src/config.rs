use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    pub path: PathBuf,
    pub kind: String,
    pub owner: String,
    pub purpose: String,
    pub retain_until: u64,
}

pub const GIB: u64 = 1024 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub roots: Vec<PathBuf>,
    pub retention_days: u64,
    pub pressure_retention_days: u64,
    pub min_free_bytes: u64,
    // Read legacy configurations without creating backups or enforcing their budget.
    #[serde(rename = "backup_budget_bytes", skip_serializing)]
    legacy_backup_budget_bytes: Option<u64>,
    pub worktree_cleanup: bool,
    pub worktree_force: bool,
    pub worktree_require_pr_verification: bool,
    // Accept old configuration files without retaining or enforcing the removed cap.
    #[serde(rename = "max_delete_bytes_per_run", skip_serializing)]
    legacy_max_delete_bytes_per_run: Option<u64>,
    pub paused_until: u64,
    pub pins: Vec<String>,
    pub managed: BTreeMap<String, String>,
    pub owners: BTreeMap<String, String>,
    pub requirements: BTreeMap<String, Vec<String>>,
    pub registered: Vec<Registration>,
    // Accept old settings without enforcing or preserving the removed size threshold.
    #[serde(rename = "cache_budget_bytes", skip_serializing)]
    legacy_cache_budget_bytes: Option<u64>,
    pub maintenance_cooldown_days: u64,
    pub deep_inventory: bool,
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
            retention_days: 7,
            pressure_retention_days: 7,
            min_free_bytes: 20 * GIB,
            legacy_backup_budget_bytes: None,
            worktree_cleanup: true,
            worktree_force: true,
            worktree_require_pr_verification: false,
            legacy_max_delete_bytes_per_run: None,
            paused_until: 0,
            pins: vec![],
            managed: BTreeMap::new(),
            owners: BTreeMap::new(),
            requirements: BTreeMap::new(),
            registered: vec![],
            legacy_cache_budget_bytes: None,
            maintenance_cooldown_days: 7,
            deep_inventory: true,
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
        if self.maintenance_cooldown_days == 0 || self.maintenance_cooldown_days > 3650 {
            bail!("maintenance cooldown must be 1..3650 days");
        }
        for r in &self.registered {
            if !matches!(r.kind.as_str(), "scratch" | "builds" | "python")
                || r.owner.trim().is_empty()
                || r.purpose.trim().is_empty()
                || !r.path.is_absolute()
                || !r.path.starts_with(home())
                || r.path == home()
            {
                bail!("invalid registered resource");
            }
        }
        for p in &self.roots {
            if !p.is_absolute() || p.parent().is_none() || p == &home() {
                bail!("roots must be absolute project directories, not / or your entire home");
            }
        }
        Ok(())
    }
    pub fn load(dir: &Path) -> Result<Self> {
        let p = dir.join("config.json");
        let c: Self = if p.exists() {
            let mut value: serde_json::Value = serde_json::from_slice(&fs::read(p)?)
                .context("invalid config.json; collection refused")?;
            // Discard retired settings when upgrading older installations.
            if let Some(object) = value.as_object_mut() {
                for key in [
                    "docker_keep_most_used",
                    "docker_cache_budget_bytes",
                    "docker_cache_cleanup",
                ] {
                    object.remove(key);
                }
            }
            serde_json::from_value(value).context("invalid config.json; collection refused")?
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
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .expect("HOME must be set")
}
pub fn state_dir() -> PathBuf {
    std::env::var_os("AIGC_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(crate::platform::state_dir)
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
