use aigc::{
    collect,
    config::{self, Config},
    inventory::Report,
    policy::Status,
    runtime, service,
};
use anyhow::{Context, Result, bail, ensure};
use clap::{Parser, Subcommand};
use serde::Serialize;
use std::{
    collections::BTreeMap, fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command,
};

#[derive(Parser)]
#[command(
    version,
    about = "Keep your Mac's development storage under control",
    long_about = "Local storage inventory and conservative background garbage collection. No cloud, account or AI model required. Run 'aigc status' to inspect resources and 'aigc clean --dry-run' to explain eligibility."
)]
struct Cli {
    #[arg(
        long,
        global = true,
        help = "Emit structured JSON (schema version 1 for reports)"
    )]
    json: bool,
    #[command(subcommand)]
    command: Commands,
}
#[derive(Subcommand)]
enum Commands {
    /// Inspect resources, sizes, activity and cleanup eligibility. Updates observation history.
    Status {
        #[arg(
            help = "Filter: worktrees, docker, simulators, emulators, dependencies, builds, sdk, runtimes"
        )]
        category: Option<String>,
        #[arg(long, help = "Ignore the cached snapshot and inspect resources again")]
        refresh: bool,
    },
    /// Explain the current cleanup plan without deleting resources.
    Plan,
    /// Remove eligible resources after rechecking activity and safety conditions.
    Clean {
        #[arg(long)]
        dry_run: bool,
    },
    /// Run one scheduled cleanup pass (used by launchd).
    Collect,
    /// Read or change configuration. Changes also affect scheduled collection.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Mark a resource ID from status --json as disposable. Device data will be lost on deletion.
    Manage {
        id: String,
        #[arg(long, default_value = "user")]
        owner: String,
    },
    /// Stop treating a resource as disposable.
    Unmanage { id: String },
    /// Protect a resource ID or an absolute directory and its descendants.
    Pin { target: String },
    /// Remove an explicit pin.
    Unpin { target: String },
    /// Pause collection for a duration, such as 2h or 7d.
    Pause { duration: String },
    /// Resume collection.
    Resume,
    /// Inspect the last 500 cleanup outcomes.
    History,
    /// Protect all resources while running a command. Use: aigc run -- npm test
    Run {
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Install, remove or inspect the hourly per-user macOS service.
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Show state paths and basic diagnostics.
    Doctor,
}
#[derive(Subcommand)]
enum ConfigAction {
    Show,
    Path,
    Set { key: String, value: String },
}
#[derive(Subcommand)]
enum ServiceAction {
    Install,
    Uninstall,
    Status,
}
fn output<T: Serialize>(v: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}
fn duration(s: &str) -> Result<u64> {
    for (suffix, scale) in [("d", 86400), ("h", 3600), ("m", 60)] {
        if let Some(n) = s.strip_suffix(suffix) {
            let n: u64 = n.parse()?;
            ensure!(n > 0 && n <= 3650, "duration out of range");
            return n.checked_mul(scale).context("duration overflow");
        }
    }
    bail!("use a duration such as 2h or 7d")
}
fn show(r: &Report, category: Option<&str>, json: bool) -> Result<()> {
    let items: Vec<_> = r
        .items
        .iter()
        .filter(|i| {
            category.is_none_or(|k| i.kind == k || (k == "docker" && i.kind.starts_with("docker")))
        })
        .collect();
    if json {
        let mut v = serde_json::to_value(r)?;
        v["items"] = serde_json::to_value(items)?;
        return output(&v);
    }
    println!("AI Garbage Collector · {}", env!("CARGO_PKG_VERSION"));
    println!(
        "Snapshot: {} minutes ago{}",
        runtime::now().saturating_sub(r.generated_at) / 60,
        if r.cached {
            " (cached; use --refresh to rescan)"
        } else {
            ""
        }
    );
    println!(
        "Disk: {} total · {} available · {} free-space target",
        runtime::size_label(r.disk_total_bytes),
        runtime::size_label(r.disk_free_bytes),
        runtime::size_label(r.min_free_bytes)
    );
    println!(
        "Service: {} · last collection: {}",
        if r.service_installed {
            "installed (use service status to check it is loaded)"
        } else {
            "not installed"
        },
        r.last_collection
            .map(|t| format!("{} minutes ago", runtime::now().saturating_sub(t) / 60))
            .unwrap_or_else(|| "never".into())
    );
    println!(
        "\n{:<19} {:>6} {:>7} {:>9} {:>9} {:>9} {:>9} {:>12}",
        "Resource", "Total", "In use", "Protected", "Observing", "Eligible", "Unknown", "Size*"
    );
    let mut groups: BTreeMap<&str, Vec<_>> = BTreeMap::new();
    for i in &items {
        groups.entry(&i.kind).or_default().push(i);
    }
    for (kind, group) in groups {
        let count = |s: Status| group.iter().filter(|i| i.status == s).count();
        println!(
            "{:<19} {:>6} {:>7} {:>9} {:>9} {:>9} {:>9} {:>12}",
            kind,
            group.len(),
            count(Status::InUse),
            count(Status::Protected),
            count(Status::Observing),
            count(Status::Eligible),
            count(Status::Unknown),
            if kind == "docker-cache" {
                "see details".into()
            } else {
                runtime::size_label(group.iter().map(|i| i.bytes).sum())
            }
        );
    }
    println!(
        "\n* Category sizes overlap (worktrees include dependencies); Docker image layers can be shared."
    );
    println!(
        "  These are inventory sizes, not a promise of space reclaimable. No combined total is shown."
    );
    if category.is_some() {
        for i in items {
            println!(
                "\n{}\n  {:?} · {} · idle observed {:.1} days\n  {}",
                i.id,
                i.status,
                runtime::size_label(i.bytes),
                i.idle_seconds as f64 / 86400.0,
                i.reason
            );
            if i.kind == "docker-images" {
                println!(
                    "  Observed container starts: {} · kept rank: {}",
                    i.observed_uses,
                    i.docker_keep_rank
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "outside protected set".into())
                );
            }
            if i.kind == "docker-cache" {
                println!("  {}", i.label);
            }
        }
    }
    for w in &r.warnings {
        println!("\nNote: {w}");
    }
    Ok(())
}
fn set_config(c: &mut Config, key: &str, value: &str) -> Result<()> {
    match key {
        "retention-days" => c.retention_days = value.trim_end_matches('d').parse()?,
        "pressure-retention-days" => {
            c.pressure_retention_days = value.trim_end_matches('d').parse()?
        }
        "min-free-space" => c.min_free_bytes = config::bytes(value)?,
        "budget.docker-cache" => c.docker_cache_budget_bytes = config::bytes(value)?,
        "budget.backups" => c.backup_budget_bytes = config::bytes(value)?,
        "docker.keep-most-used" => c.docker_keep_most_used = value.parse()?,
        "max-delete-per-run" => c.max_delete_bytes_per_run = config::bytes(value)?,
        "docker-cache-cleanup" => c.docker_cache_cleanup = value.parse()?,
        "roots" => {
            c.roots = serde_json::from_str(value)
                .context("roots must be a JSON array of absolute paths")?
        }
        _ => bail!(
            "unknown setting; use retention-days, pressure-retention-days, min-free-space, budget.docker-cache, budget.backups, docker.keep-most-used, max-delete-per-run, docker-cache-cleanup, or roots"
        ),
    }
    c.validate()
}
fn main() {
    if let Err(e) = run() {
        eprintln!("aigc: {e:#}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let cli = Cli::parse();
    ensure!(
        unsafe { libc::getuid() } != 0,
        "run aigc as your normal user, never with sudo"
    );
    let dir = config::state_dir();
    fs::create_dir_all(&dir)?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    if let Commands::Run { command } = cli.command {
        return protected_run(&dir, command);
    }
    // All config/state updates and destructive operations share one advisory lock.
    let _lock = runtime::lock(&dir)?;
    let mut c = Config::load(&dir)?;
    match cli.command {
        Commands::Status { category, refresh } => {
            let cached = if refresh {
                None
            } else {
                fs::read(dir.join("last-report.json"))
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Report>(&b).ok())
                    .filter(|r| runtime::now().saturating_sub(r.generated_at) < 3600)
            };
            let r = match cached {
                Some(mut r) => {
                    r.cached = true;
                    r
                }
                None => collect::prepare(&c, &dir)?,
            };
            show(&r, category.as_deref(), cli.json)
        }
        Commands::Plan | Commands::Clean { dry_run: true } => {
            let r = collect::prepare(&c, &dir)?;
            if cli.json {
                output(&r)
            } else {
                show(&r, None, false)?;
                for i in &r.items {
                    println!("{:?} {} — {}", i.status, i.id, i.reason);
                }
                Ok(())
            }
        }
        Commands::Clean { dry_run: false } | Commands::Collect => {
            let r = collect::prepare(&c, &dir)?;
            let events = collect::collect(&c, &dir, &r)?;
            if cli.json {
                output(&events)
            } else {
                if events.is_empty() {
                    println!(
                        "No eligible resources removed. Run aigc plan for protection and observation reasons."
                    );
                }
                for e in events {
                    println!("{} {} — {}", e.outcome, e.resource, e.detail);
                }
                Ok(())
            }
        }
        Commands::Config { action } => match action {
            ConfigAction::Show => output(&c),
            ConfigAction::Path => {
                println!("{}", dir.join("config.json").display());
                Ok(())
            }
            ConfigAction::Set { key, value } => {
                set_config(&mut c, &key, &value)?;
                c.save(&dir)?;
                output(&c)
            }
        },
        Commands::Manage { id, owner } => {
            let r = collect::prepare(&c, &dir)?;
            let item = r
                .items
                .iter()
                .find(|i| i.id == id)
                .context("resource ID not found; copy an exact ID from aigc status --json")?;
            ensure!(
                matches!(
                    item.kind.as_str(),
                    "worktrees" | "simulators" | "emulators" | "docker-images"
                ),
                "only worktrees, simulators, emulators and Docker images need registration"
            );
            ensure!(
                item.protection.is_none(),
                "resource is protected: {}",
                item.protection.as_deref().unwrap_or_default()
            );
            c.managed.insert(id.clone(), owner);
            c.save(&dir)?;
            output(
                &serde_json::json!({"managed":id,"effect":"eligible after retention and safety checks; disposable device/image contents are not backed up"}),
            )
        }
        Commands::Unmanage { id } => {
            c.managed.remove(&id);
            c.save(&dir)?;
            output(&serde_json::json!({"unmanaged":id}))
        }
        Commands::Pin { target } => {
            let target = if target.starts_with('/') {
                fs::canonicalize(&target)?.to_string_lossy().into_owned()
            } else {
                target
            };
            if !c.pins.contains(&target) {
                c.pins.push(target.clone());
            }
            c.save(&dir)?;
            output(&serde_json::json!({"pinned":target}))
        }
        Commands::Unpin { target } => {
            c.pins.retain(|p| p != &target);
            c.save(&dir)?;
            output(&serde_json::json!({"unpinned":target}))
        }
        Commands::Pause { duration: d } => {
            c.paused_until = runtime::now() + duration(&d)?;
            c.save(&dir)?;
            output(&serde_json::json!({"paused_until":c.paused_until}))
        }
        Commands::Resume => {
            c.paused_until = 0;
            c.save(&dir)?;
            output(&serde_json::json!({"paused":false}))
        }
        Commands::History => output(&collect::history(&dir)?),
        Commands::Service { action } => match action {
            ServiceAction::Install => {
                c.save(&dir)?;
                service::install()?;
                output(&service::status())
            }
            ServiceAction::Uninstall => {
                service::uninstall()?;
                output(&service::status())
            }
            ServiceAction::Status => output(&service::status()),
        },
        Commands::Doctor => output(
            &serde_json::json!({"version":env!("CARGO_PKG_VERSION"),"platform":std::env::consts::OS,"state_directory":dir,"configuration_valid":true,"service":service::status(),"backups":dir.join("backups"),"scope":"local macOS, current user"}),
        ),
        Commands::Run { .. } => unreachable!(),
    }
}
fn protected_run(dir: &std::path::Path, args: Vec<String>) -> Result<()> {
    let lease: PathBuf = dir
        .join("leases")
        .join(format!("{}.json", std::process::id()));
    let mut child;
    {
        let _lock = runtime::lock(dir)?;
        child = Command::new(&args[0])
            .args(&args[1..])
            .spawn()
            .context("could not start protected command")?;
        let pid = child.id();
        match runtime::command("/bin/ps", &["-p", &pid.to_string(), "-o", "lstart="]) {
            Ok(start) => {
                if let Err(e) = config::atomic_json(
                    &lease,
                    &serde_json::json!({"pid":pid,"start":start.trim()}),
                ) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(e);
                }
            }
            Err(_) => {
                if child.try_wait()?.is_none() {
                    let _ = child.kill();
                    let _ = child.wait();
                    bail!("could not verify child identity; command stopped");
                }
            }
        }
    }
    let status = child.wait()?;
    {
        let _lock = runtime::lock(dir)?;
        let _ = fs::remove_file(&lease);
    }
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}
