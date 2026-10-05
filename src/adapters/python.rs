use super::*;
use sha2::{Digest, Sha256};

pub fn scan(projects: &[PathBuf], items: &mut Vec<Item>, warnings: &mut Vec<String>) {
    let poetry = available("poetry");
    let mut mapping_failed = false;
    let mut known = std::collections::BTreeSet::new();
    for project in projects
        .iter()
        .filter(|p| p.join("pyproject.toml").is_file())
    {
        let local = project.join(".venv");
        if local.join("pyvenv.cfg").is_file() {
            let p = crate::platform::normalize(fs::canonicalize(&local).unwrap_or(local.clone()));
            add(&p, Some(project), local.is_symlink(), None, items);
            known.insert(p);
        }
        if poetry && project.join("poetry.lock").is_file() {
            match tool(
                "poetry",
                &[
                    "--no-plugins",
                    "--no-interaction",
                    "env",
                    "list",
                    "--full-path",
                ],
                Some(project),
            ) {
                Ok(output) => {
                    for line in output.lines() {
                        let p = PathBuf::from(line.trim_end_matches(" (Activated)").trim());
                        if p.is_absolute() && p.join("pyvenv.cfg").is_file() {
                            add(&p, Some(project), false, Some("poetry"), items);
                            known.insert(p);
                        }
                    }
                }
                Err(_) => {
                    mapping_failed = true;
                    warnings.push(format!(
                        "Poetry environment mapping unavailable for {}",
                        project.display()
                    ));
                }
            }
        }
    }
    if poetry
        && let Ok(root) = tool(
            "poetry",
            &["--no-plugins", "config", "virtualenvs.path"],
            None,
        )
        && let Ok(root) = crate::runtime::canonical(Path::new(root.trim()))
    {
        for p in children(&root) {
            if p.join("pyvenv.cfg").is_file() {
                if !known.contains(&p) {
                    add(&p, None, false, Some("poetry"), items);
                }
                if let Some(i) = items.iter_mut().find(|i| i.path.as_ref() == Some(&p)) {
                    i.evidence.metadata.insert(
                        "poetry_environment_root".into(),
                        Value::String(root.display().to_string()),
                    );
                }
            }
        }
    }
    if mapping_failed {
        for i in items
            .iter_mut()
            .filter(|i| i.kind == "python" && !central_environment(i))
        {
            i.complete = false;
        }
    }
    // Tool environments are deliberately visible and protected.
    for root in [
        home().join(".local/share/uv/tools"),
        home().join(".local/share/pipx/venvs"),
        home().join(".local/pipx/venvs"),
    ] {
        for p in children(&root) {
            if p.join("pyvenv.cfg").is_file() {
                let mut i = folder("python-tools", p, "installed tool environment");
                i.protection =
                    Some("installed command-line tool; uninstall through its manager".into());
                items.push(i);
            }
        }
    }
}
fn add(
    path: &Path,
    project: Option<&Path>,
    linked: bool,
    manager: Option<&str>,
    items: &mut Vec<Item>,
) {
    let existing = items.iter().position(|i| i.path.as_deref() == Some(path));
    let index = existing.unwrap_or_else(|| {
        items.push(folder(
            "python",
            path.to_path_buf(),
            "Python environment metadata",
        ));
        items.len() - 1
    });
    let i = &mut items[index];
    i.kind = "python".into();
    // Keep the ID of existing project-local resources for backward-compatible pins.
    i.cleanable = true;
    i.evidence.action = Some(Action::Directory);
    i.evidence.reconstruction = "Recreate from the project's lockfile and original dependency groups/extras; downloads may be required. Never merged with another mutable environment.".into();
    if let Some(project) = project {
        i.evidence
            .consumer(project.display().to_string(), "Python project", false);
        if manager == Some("poetry") {
            i.evidence.action = Some(Action::Poetry {
                project: project.into(),
                environment: path.into(),
            });
        }
        if let Some(f) = fingerprint(project, path) {
            i.evidence.fingerprint = Some(f);
        }
    } else if manager != Some("poetry") {
        i.protection = Some("owner unknown; no discovered project association".into());
    }
    if linked {
        i.protection =
            Some("linked or centralized environment; target may have other consumers".into());
    }
    match read(&path.join("pyvenv.cfg")) {
        Ok(cfg) => {
            for line in cfg.lines() {
                if let Some((key, value)) = line.split_once('=')
                    && matches!(
                        key.trim(),
                        "home" | "version" | "version_info" | "implementation" | "executable"
                    )
                {
                    i.evidence
                        .metadata
                        .insert(key.trim().into(), Value::String(value.trim().into()));
                }
            }
        }
        Err(_) => i.complete = false,
    }
    if let Err(e) = disposable_path(path, "python") {
        i.protection = Some(e.to_string());
    }
}
fn fingerprint(project: &Path, environment: &Path) -> Option<String> {
    let lock = ["poetry.lock", "uv.lock"]
        .iter()
        .find_map(|name| read(&project.join(name)).ok())?;
    let cfg = read(&environment.join("pyvenv.cfg")).ok()?;
    let mut records = Vec::new();
    let libs = if cfg!(windows) {
        vec![environment.join("Lib")]
    } else {
        children(&environment.join("lib"))
    };
    for lib in libs {
        for info in children(&lib.join("site-packages")) {
            if info.extension().is_some_and(|s| s == "dist-info") {
                records.push(info.file_name()?.to_string_lossy().into_owned());
                if info.join("direct_url.json").exists() {
                    records.push(read(&info.join("direct_url.json")).ok()?);
                }
            }
        }
    }
    if records.is_empty() {
        return None;
    }
    records.sort();
    let mut hash = Sha256::new();
    hash.update(std::env::consts::ARCH);
    hash.update(lock);
    let mut interpreter: Vec<_> = cfg
        .lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(key, _)| {
            matches!(
                key.trim(),
                "home"
                    | "version"
                    | "version_info"
                    | "implementation"
                    | "executable"
                    | "include-system-site-packages"
            )
        })
        .map(|(k, v)| format!("{}={}", k.trim(), v.trim()))
        .collect();
    interpreter.sort();
    hash.update(interpreter.join("\0"));
    hash.update(records.join("\0"));
    Some(format!("python:{:x}", hash.finalize()))
}

// A native manager root establishes that an orphan is a generated environment,
// even after its original worktree has disappeared. Recheck the root before removal.
pub fn central_environment(i: &Item) -> bool {
    i.kind == "python"
        && i.evidence
            .metadata
            .get("poetry_environment_root")
            .and_then(Value::as_str)
            .is_some()
}
pub fn verify_central_environment(i: &Item) -> Result<()> {
    if let Some(root) = i
        .evidence
        .metadata
        .get("poetry_environment_root")
        .and_then(Value::as_str)
    {
        let current = tool(
            "poetry",
            &["--no-plugins", "config", "virtualenvs.path"],
            None,
        )?;
        let current = crate::runtime::canonical(Path::new(current.trim()))?;
        let path = i.path.as_ref().context("missing environment path")?;
        ensure!(
            current == Path::new(root) && path.parent() == Some(current.as_path()),
            "Poetry environment root changed"
        );
        disposable_path(path, "python")?;
    }
    Ok(())
}
