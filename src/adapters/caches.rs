use super::*;

pub fn scan(items: &mut Vec<Item>, warnings: &mut Vec<String>) {
    for (manager, program, args) in [
        ("uv", "uv", vec!["cache", "dir"]),
        ("pip", "pip3", vec!["cache", "dir"]),
        ("pnpm", "pnpm", vec!["store", "path"]),
        ("npm", "npm", vec!["config", "get", "cache"]),
        (
            "poetry",
            "poetry",
            vec!["--no-plugins", "config", "cache-dir"],
        ),
        ("homebrew", "brew", vec!["--cache"]),
    ] {
        if !available(program) {
            continue;
        }
        match tool(program, &args, None) {
            Ok(path) => {
                let root = PathBuf::from(path.trim());
                if !root.is_absolute() || !root.is_dir() {
                    continue;
                }
                items.retain(|i| {
                    !(i.kind == "package-caches"
                        && i.path
                            .as_ref()
                            .is_some_and(|p| p == &root || p.starts_with(&root)))
                });
                let mut i = folder(
                    "package-caches",
                    root.clone(),
                    "manager-configured cache path",
                );
                i.label = format!("{manager} cache");
                i.cleanable = matches!(manager, "pip" | "pnpm" | "npm");
                i.evidence.action = i.cleanable.then(|| Action::Cache {
                    manager: manager.into(),
                    root: root.clone(),
                });
                i.evidence
                    .metadata
                    .insert("manager".into(), Value::String(manager.into()));
                i.evidence.reconstruction = "Native cache maintenance can cause later downloads or rebuilds; age, size budget and cooldown checks apply.".into();
                if root.is_symlink() || !root.starts_with(home()) {
                    i.protection = Some("shared or external cache location".into());
                }
                if manager == "uv" {
                    i.protection = Some("native uv prune also removes centralized environments; collect recognized project environments instead".into());
                }
                if manager == "poetry" {
                    i.protection = Some(
                        "Poetry cache root can contain environments; cache listing only".into(),
                    );
                }
                if manager == "homebrew" {
                    i.protection = Some("Homebrew cleanup can remove Cellar versions outside the cache; use the native preview".into());
                    i.evidence.metadata.insert(
                        "preview_command".into(),
                        serde_json::json!(["brew", "cleanup", "--dry-run"]),
                    );
                }
                if manager == "pip"
                    && let Ok(info) = tool(program, &["cache", "info"], None)
                {
                    i.evidence
                        .metadata
                        .insert("native_summary".into(), Value::String(info));
                }
                if manager == "poetry"
                    && let Ok(list) = tool(program, &["--no-plugins", "cache", "list"], None)
                {
                    i.evidence.metadata.insert(
                        "cache_names".into(),
                        serde_json::json!(list.lines().collect::<Vec<_>>()),
                    );
                }
                items.push(i);
                if manager == "npm" {
                    for p in children(&root.join("_npx")) {
                        let mut i = folder("tool-downloads", p, "npm npx cache directory");
                        i.protection = Some(
                            "cached executable may be referenced outside discovered projects"
                                .into(),
                        );
                        items.push(i);
                    }
                }
            }
            Err(_) => warnings.push(format!("{manager} configured cache discovery unavailable")),
        }
    }
    for (manager, roots) in [
        (
            "gradle",
            vec![
                std::env::var_os("GRADLE_USER_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home().join(".gradle")),
            ],
        ),
        (
            "cargo",
            vec![
                std::env::var_os("CARGO_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home().join(".cargo"))
                    .join("registry"),
            ],
        ),
        (
            "yarn",
            vec![
                crate::platform::cache_dir("Yarn"),
                home().join(".yarn/berry/cache"),
            ],
        ),
        (
            "bun",
            vec![
                std::env::var_os("BUN_INSTALL_CACHE_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home().join(".bun/install/cache")),
            ],
        ),
        (
            "playwright",
            vec![
                std::env::var_os("PLAYWRIGHT_BROWSERS_PATH")
                    .filter(|v| v != "0")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| crate::platform::cache_dir("ms-playwright")),
            ],
        ),
    ] {
        for root in roots.into_iter().filter(|p| p.is_dir()) {
            let mut i = folder(
                if manager == "playwright" {
                    "browsers"
                } else {
                    "package-caches"
                },
                root.clone(),
                "native cache storage",
            );
            i.label = format!("{manager} managed storage");
            i.protection = Some(
                "native ownership/retention controls apply; no direct directory deletion".into(),
            );
            i.evidence
                .metadata
                .insert("manager".into(), Value::String(manager.into()));
            if manager == "playwright" {
                let links = children(&root.join(".links"));
                for link in links {
                    if let Ok(package) = read(&link) {
                        let p = PathBuf::from(package.trim());
                        if p.is_absolute() {
                            i.evidence.consumer(
                                p.display().to_string(),
                                "Playwright package registration",
                                true,
                            );
                        }
                    }
                }
                for browser in children(&root).into_iter().filter(|p| {
                    p.is_dir()
                        && p.file_name()
                            .is_some_and(|n| !n.to_string_lossy().starts_with('.'))
                }) {
                    let mut child = folder(
                        "browsers",
                        browser,
                        "Playwright downloaded browser revision",
                    );
                    child.protection =
                        Some("use Playwright's package-aware browser garbage collection".into());
                    child.evidence.consumers = i.evidence.consumers.clone();
                    items.push(child);
                }
            }
            if manager == "gradle" {
                i.evidence.metadata.insert("retention".into(), Value::String("Gradle performs native version-aware cleanup; init.d settings may customize it. No Gradle scripts are executed by inventory.".into()));
            }
            if manager == "cargo" {
                i.evidence.metadata.insert("retention".into(), Value::String("Cargo global cache uses its own automatic GC; this is separate from target directories.".into()));
            }
            items.push(i);
        }
    }
}
pub fn remove(manager: &str, root: &Path, _c: &Config) -> Result<String> {
    let (program, query, action): (&str, Vec<&str>, Vec<&str>) = match manager {
        "pip" => ("pip3", vec!["cache", "dir"], vec!["cache", "purge"]),
        "pnpm" => ("pnpm", vec!["store", "path"], vec!["store", "prune"]),
        "npm" => (
            "npm",
            vec!["config", "get", "cache"],
            vec!["cache", "verify"],
        ),
        _ => anyhow::bail!("manager is report-only"),
    };
    ensure!(
        Path::new(tool(program, &query, None)?.trim()) == root,
        "cache configuration changed"
    );
    crate::runtime::canonical(root)?;
    ensure!(
        root.starts_with(home()) && root != home(),
        "unsafe cache location"
    );
    command_at(program, &action, None, 300)?;
    Ok("Native cache maintenance completed; inspect observed free-space change (not a guaranteed reclaim amount)".into())
}
