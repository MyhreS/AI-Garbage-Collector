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
use std::{collections::BTreeMap, fs, os::unix::fs::PermissionsExt, path::PathBuf};

#[derive(Parser)]
#[command(
    version,
    about = "Keep your Mac's development storage under control",
    long_about = "Local storage inventory and background garbage collection. Regular linked worktrees older than seven days can be force-removed with local files discarded. No cloud, account or AI model required. Run 'aigc status' to inspect resources and 'aigc clean --dry-run' to explain eligibility."
)]
struct Cli {
    #[arg(
        long,
        global = true,
        help = "Emit structured JSON (schema version 2 for reports)"
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
        #[arg(long, help = "Show owner and process evidence")]
        owners: bool,
        #[arg(long, help = "Show per-builder cache evidence")]
        builders: bool,
    },
    /// Inspect one exact resource with ownership, native metadata and all recorded blockers.
    Inspect { id: String },
    /// Report matching dependency/build inputs. Does not merge or delete environments.
    Duplicates,
    /// Show a native non-destructive preview where supported, otherwise show the planned action.
    Preview { id: String },
    /// Record ownership without granting disposal permission.
    Own {
        id: String,
        #[arg(long)]
        owner: String,
    },
    /// Protect a resource required by a project, including future builds.
    Require {
        id: String,
        #[arg(long)]
        project: String,
    },
    /// Remove a project's explicit resource requirement.
    Unrequire {
        id: String,
        #[arg(long)]
        project: String,
    },
    /// Register a disposable generated directory with an owner, purpose and minimum retention.
    Register {
        path: PathBuf,
        #[arg(long, value_parser = ["scratch", "builds", "python"])]
        kind: String,
        #[arg(long)]
        owner: String,
        #[arg(long)]
        purpose: String,
        #[arg(long, default_value_t = 30)]
        retain_days: u64,
    },
    /// Stop managing a registered generated directory.
    Unregister { path: PathBuf },
    /// List cooperative reservations, including ones retained after a crashed wrapper.
    Leases,
    /// Release a retained reservation after verifying its processes and detached work have ended.
    ReleaseLease { id: String },
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
    /// Mark an opt-in resource ID from status --json as disposable. Device data will be lost on deletion.
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
    /// Protect all resources while running a command. Use: aigc run -- npm run build
    Run {
        #[arg(
            long = "resource",
            help = "Exact resource ID or absolute path; repeatable. Omit to protect everything."
        )]
        resources: Vec<String>,
        #[arg(long, default_value = "user")]
        owner: String,
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
            category.is_none_or(|k| {
                i.kind == k
                    || (k == "docker" && i.kind.starts_with("docker"))
                    || (k == "python" && i.kind.starts_with("python"))
                    || (k == "builds" && i.kind == "xcode-builds")
            })
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
            runtime::size_label(group.iter().map(|i| i.bytes).sum())
        );
    }
    println!(
        "\n* Category sizes overlap (worktrees include dependencies); Docker image layers can be shared."
    );
    println!(
        "  These are inventory sizes, not a promise of space reclaimable. No combined total is shown."
    );
    println!(
        "Filesystem union: {}{} (hardlinks counted once; not guaranteed reclaimable)",
        runtime::size_label(r.storage.filesystem_allocated_union_bytes),
        if r.storage.complete {
            ""
        } else {
            " — incomplete"
        }
    );
    if category.is_some() {
        for i in items {
            println!(
                "\n{}\n  {:?} · {} · idle {:.1} days\n  {}",
                i.id,
                i.status,
                runtime::size_label(i.bytes),
                i.idle_seconds as f64 / 86400.0,
                i.reason
            );
            println!(
                "  Owners: {} · processes: {} · consumers: {}",
                i.evidence.owners.join(", "),
                i.evidence.processes.len(),
                i.evidence.consumers.len()
            );
            for p in &i.evidence.processes {
                println!(
                    "  PID {} {} · started {}",
                    p.pid,
                    p.executable,
                    p.started.as_deref().unwrap_or("unknown")
                );
            }
            for r in &i.evidence.consumers {
                println!(
                    "  {}: {}{}",
                    r.source,
                    r.id,
                    if r.protects { " (protects)" } else { "" }
                );
            }
            if let Some(t) = i.evidence.native_last_used {
                println!(
                    "  Native last use: {t} · native use count: {:?}",
                    i.evidence.native_usage_count
                );
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
        "max-delete-per-run" => c.max_delete_bytes_per_run = config::bytes(value)?,
        "docker-cache-cleanup" => c.docker_cache_cleanup = value.parse()?,
        "worktree-cleanup" => c.worktree_cleanup = value.parse()?,
        "worktree-force" => c.worktree_force = value.parse()?,
        "budget.package-cache" => c.cache_budget_bytes = config::bytes(value)?,
        "maintenance-cooldown-days" => c.maintenance_cooldown_days = value.parse()?,
        "deep-inventory" => c.deep_inventory = value.parse()?,
        "roots" => {
            c.roots = serde_json::from_str(value)
                .context("roots must be a JSON array of absolute paths")?
        }
        _ => bail!(
            "unknown setting; use retention-days, pressure-retention-days, min-free-space, budget.docker-cache, budget.backups, max-delete-per-run, docker-cache-cleanup, worktree-cleanup, worktree-force, budget.package-cache, maintenance-cooldown-days, deep-inventory, or roots"
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
    if let Commands::Run {
        command,
        resources,
        owner,
    } = cli.command
    {
        return aigc::leases::run(&dir, command, resources, owner);
    }
    // All config/state updates and destructive operations share one advisory lock.
    let _lock = runtime::lock(&dir)?;
    let mut c = Config::load(&dir)?;
    match cli.command {
        Commands::Status {
            category,
            refresh,
            owners,
            builders,
        } => {
            let cached = if refresh {
                None
            } else {
                fs::read(dir.join("last-report.json"))
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Report>(&b).ok())
                    .filter(|r| {
                        r.policy_version == aigc::policy::POLICY_VERSION
                            && runtime::now().saturating_sub(r.generated_at) < 3600
                    })
            };
            let r = match cached {
                Some(mut r) => {
                    r.cached = true;
                    r
                }
                None => collect::prepare(&c, &dir)?,
            };
            show(&r, category.as_deref(), cli.json)?;
            if !cli.json && (owners || builders) {
                for i in r.items.iter().filter(|i| {
                    category.as_ref().is_none_or(|k| {
                        i.kind == *k || (k == "docker" && i.kind.starts_with("docker"))
                    })
                }) {
                    println!("{}\n{}", i.id, serde_json::to_string_pretty(&i.evidence)?);
                }
            }
            Ok(())
        }
        Commands::Inspect { id } => {
            let r = collect::prepare(&c, &dir)?;
            output(
                r.items
                    .iter()
                    .find(|i| i.id == id)
                    .context("resource not found")?,
            )
        }
        Commands::Preview { id } => {
            let r = collect::prepare(&c, &dir)?;
            let item = r
                .items
                .iter()
                .find(|i| i.id == id)
                .context("resource not found")?;
            let text = aigc::adapters::mobile::preview(item, &c)?;
            output(
                &serde_json::json!({"resource":id,"dry_run":true,"native_scope":"native preview may include protected candidates; it does not override aigc policy", "preview":text}),
            )
        }
        Commands::Duplicates => {
            let r = collect::prepare(&c, &dir)?;
            output(&aigc::evidence::duplicates(&r.items))
        }
        Commands::Own { id, owner } => {
            ensure!(!owner.trim().is_empty(), "owner cannot be empty");
            c.owners.insert(id, owner);
            c.save(&dir)?;
            output(&c.owners)
        }
        Commands::Require { id, project } => {
            ensure!(!project.trim().is_empty(), "project cannot be empty");
            let ids = c.requirements.entry(project).or_default();
            if !ids.contains(&id) {
                ids.push(id);
            }
            c.save(&dir)?;
            output(&c.requirements)
        }
        Commands::Unrequire { id, project } => {
            if let Some(ids) = c.requirements.get_mut(&project) {
                ids.retain(|r| r != &id);
            }
            c.requirements.retain(|_, ids| !ids.is_empty());
            c.save(&dir)?;
            output(&c.requirements)
        }
        Commands::Register {
            path,
            kind,
            owner,
            purpose,
            retain_days,
        } => {
            ensure!(
                (1..=3650).contains(&retain_days),
                "retention must be 1..3650 days"
            );
            let path = fs::canonicalize(path)?;
            aigc::adapters::disposable_path(&path, &kind)?;
            c.registered.retain(|r| r.path != path);
            c.registered.push(config::Registration {
                path,
                kind,
                owner,
                purpose,
                retain_until: runtime::now() + retain_days * 86400,
            });
            c.save(&dir)?;
            output(&c.registered)
        }
        Commands::Unregister { path } => {
            let path = fs::canonicalize(&path).unwrap_or(path);
            c.registered.retain(|r| r.path != path);
            c.save(&dir)?;
            output(&c.registered)
        }
        Commands::Leases => output(&aigc::leases::list(&dir)?),
        Commands::ReleaseLease { id } => {
            aigc::leases::release(&dir, &id)?;
            let _ = fs::remove_file(dir.join("last-report.json"));
            output(&serde_json::json!({"released":id}))
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
                item.cleanable,
                "this resource has no supported cleanup action"
            );
            ensure!(
                item.protection.is_none(),
                "resource is protected: {}",
                item.protection.as_deref().unwrap_or_default()
            );
            c.managed.insert(id.clone(), owner);
            c.save(&dir)?;
            output(
                &serde_json::json!({"managed":id,"effect":"eligible after retention and safety checks; native cache maintenance may affect its entire reported scope; no universal undo"}),
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
