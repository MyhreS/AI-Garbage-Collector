use super::*;
use sha2::{Digest, Sha256};

pub fn scan(projects: &[PathBuf], items: &mut Vec<Item>, warnings: &mut Vec<String>) {
    for project in projects {
        let mut outputs = Vec::new();
        if project.join("Cargo.toml").is_file() {
            let mut target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from);
            if target.is_none() {
                for parent in project.ancestors() {
                    let cfg = parent.join(".cargo/config.toml");
                    if cfg.exists() {
                        match read(&cfg).ok().and_then(|s| s.parse::<toml::Value>().ok()) {
                            Some(v) => {
                                if let Some(path) = v
                                    .get("build")
                                    .and_then(|v| v.get("target-dir"))
                                    .and_then(|v| v.as_str())
                                {
                                    target = Some(parent.join(path));
                                    break;
                                }
                            }
                            None => warnings.push(format!(
                                "Cargo configuration could not be read: {}",
                                cfg.display()
                            )),
                        }
                    }
                }
            }
            outputs.push(
                target
                    .map(|p| if p.is_absolute() { p } else { project.join(p) })
                    .unwrap_or_else(|| project.join("target")),
            );
        }
        if project.join("build.gradle").is_file() || project.join("build.gradle.kts").is_file() {
            outputs.push(project.join("build"));
        }
        if project.join("Package.swift").is_file() {
            outputs.push(project.join(".build"));
        }
        for path in outputs.into_iter().filter(|p| p.is_dir()) {
            let index = items
                .iter()
                .position(|i| i.path.as_ref() == Some(&path))
                .unwrap_or_else(|| {
                    let mut i = folder(
                        "builds",
                        path.clone(),
                        "project build-output convention or static Cargo configuration",
                    );
                    i.cleanable = true;
                    i.evidence.action = Some(Action::Directory);
                    items.push(i);
                    items.len() - 1
                });
            let i = &mut items[index];
            i.evidence
                .consumer(project.display().to_string(), "build project", false);
            i.evidence.reconstruction = "Rebuild with the original toolchain, target and flags. Preserve exported release deliverables separately.".into();
            if path.file_name().is_some_and(|p| p == "build") {
                i.evidence.metadata.insert("scope".into(), Value::String("Gradle conventional output; dynamic custom destinations require explicit registration".into()));
            }
            if let Some(lock) = ["Cargo.lock", "Package.resolved"]
                .iter()
                .find_map(|n| read(&project.join(n)).ok())
            {
                let mut h = Sha256::new();
                h.update(lock);
                h.update(std::env::consts::ARCH);
                i.evidence.fingerprint = Some(format!("build-inputs:{:x}", h.finalize()));
            }
        }
        if project.join("package.json").is_file() {
            for i in items
                .iter_mut()
                .filter(|i| i.path.as_ref().is_some_and(|p| p.parent() == Some(project)))
            {
                if let Some(lock) = [
                    "pnpm-lock.yaml",
                    "package-lock.json",
                    "yarn.lock",
                    "bun.lock",
                ]
                .iter()
                .find_map(|n| read(&project.join(n)).ok())
                {
                    let mut h = Sha256::new();
                    h.update(lock);
                    h.update(&i.kind);
                    i.evidence.fingerprint = Some(format!("node-inputs:{:x}", h.finalize()));
                }
                i.evidence
                    .consumer(project.display().to_string(), "JavaScript project", false);
            }
        }
    }
    for i in items.iter_mut().filter(|i| i.kind == "builds") {
        if i.path
            .as_ref()
            .is_some_and(|p| p.join("checkouts").is_dir())
        {
            i.protection = Some("build output contains source checkouts; register disposable intermediates separately".into());
        }
        if i.evidence
            .consumers
            .iter()
            .filter(|c| c.source == "build project")
            .count()
            > 1
        {
            i.protection = Some("output is shared by multiple projects".into());
        }
    }
    for i in items.iter_mut().filter(|i| i.kind == "xcode-builds") {
        if let Some(path) = &i.path {
            if let Ok(v) = json(
                "/usr/bin/plutil",
                &[
                    "-convert",
                    "json",
                    "-o",
                    "-",
                    &path.join("info.plist").to_string_lossy(),
                ],
                None,
            ) && let Some(project) = v["WorkspacePath"].as_str()
            {
                i.evidence
                    .consumer(project, "Xcode DerivedData metadata", false);
            }
            let sources = path.join("SourcePackages/checkouts");
            if sources.is_dir() {
                // Package checkouts can contain manual edits, ignored files or detached work.
                i.protection = Some("DerivedData contains source package checkouts; register disposable build subdirectories separately".into());
            }
            i.evidence.reconstruction = "Xcode rebuilds indexes and intermediates. Archives and source checkouts remain protected.".into();
        }
    }
}
