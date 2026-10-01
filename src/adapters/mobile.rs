use super::*;
use crate::runtime::command;

pub fn sdk_root() -> PathBuf {
    std::env::var_os("ANDROID_HOME")
        .or_else(|| std::env::var_os("ANDROID_SDK_ROOT"))
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join("Library/Android/sdk"))
}
fn sdk_tool(root: &Path) -> Result<PathBuf> {
    let latest = root.join("cmdline-tools/latest/bin/sdkmanager");
    if latest.is_file() {
        return Ok(latest);
    }
    let mut candidates: Vec<_> = children(&root.join("cmdline-tools"))
        .into_iter()
        .map(|p| p.join("bin/sdkmanager"))
        .filter(|p| p.is_file())
        .collect();
    candidates.sort();
    candidates.pop().context("SDK manager missing")
}
pub fn scan(c: &Config, projects: &[PathBuf], items: &mut Vec<Item>, warnings: &mut Vec<String>) {
    if runtimes(items).is_err() {
        warnings
            .push("Simulator runtime relationships unavailable; runtime cleanup disabled".into());
        for i in items.iter_mut().filter(|i| i.kind == "runtimes") {
            i.complete = false;
        }
    }
    if sdk(projects, items).is_err() {
        warnings.push(
            "Android SDK package/project relationships incomplete; SDK cleanup disabled".into(),
        );
        for i in items.iter_mut().filter(|i| i.kind == "sdk") {
            i.complete = false;
        }
    }
    for i in items
        .iter_mut()
        .filter(|i| i.kind == "runtimes" || i.kind == "sdk")
    {
        i.evidence.reconstruction = "Re-download the exact platform version. Device data is separate; retained consumers block removal.".into();
        if c.requirements.values().any(|ids| ids.contains(&i.id)) {
            i.protection = Some("explicitly required platform".into());
        }
    }
}
fn runtimes(items: &mut [Item]) -> Result<()> {
    let v = json("xcrun", &["simctl", "list", "--json"], None)?;
    let disks = json("xcrun", &["simctl", "runtime", "list", "--json"], None)?;
    let disks = disks
        .as_object()
        .context("runtime disk schema unsupported")?;
    let runtimes = v["runtimes"].as_array().context("runtime list missing")?;
    let devices = v["devices"].as_object().context("device list missing")?;
    let help = tool("xcrun", &["simctl", "help", "runtime"], None)?;
    for i in items.iter_mut().filter(|i| i.kind == "runtimes") {
        let id =
            i.id.strip_prefix("runtimes:")
                .context("runtime identifier missing")?;
        let native = runtimes
            .iter()
            .find(|r| r["identifier"] == id)
            .context("runtime missing from native list")?;
        let build = native["buildversion"]
            .as_str()
            .context("runtime build missing")?;
        for d in devices
            .get(id)
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
        {
            if let Some(device) = d["udid"].as_str() {
                i.evidence.consumer(
                    format!("simulators:{device}"),
                    "retained simulator device",
                    true,
                );
            }
        }
        i.evidence
            .metadata
            .insert("build".into(), Value::String(build.into()));
        i.evidence.metadata.insert(
            "age_preview_supported".into(),
            Value::Bool(help.contains("--notUsedSinceDays") && help.contains("--dry-run")),
        );
        for (uuid, disk) in disks {
            if disk["build"]
                .as_str()
                .or_else(|| disk["buildversion"].as_str())
                == Some(build)
                && uuid_valid(uuid)
            {
                i.cleanable = true;
                i.evidence.action = Some(Action::Runtime {
                    uuid: uuid.clone(),
                    build: build.into(),
                });
                i.evidence.native_last_used = epoch(&disk["lastUsedAt"]);
                i.evidence.source =
                    "simctl runtime disk registration and retained device references".into();
                i.evidence
                    .metadata
                    .insert("runtime_disk_uuid".into(), Value::String(uuid.clone()));
            }
        }
        if i.evidence.action.is_none() {
            i.protection =
                Some("runtime disk registration cannot be resolved; use Xcode Components".into());
        }
    }
    Ok(())
}
fn uuid_valid(s: &str) -> bool {
    s.len() == 36 && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}
fn sdk(projects: &[PathBuf], items: &mut Vec<Item>) -> Result<()> {
    let root = sdk_root();
    if !root.is_dir() {
        return Ok(());
    }
    let manager = sdk_tool(&root)?;
    let out = tool(
        &manager.to_string_lossy(),
        &[
            "--list_installed",
            &format!("--sdk_root={}", root.display()),
        ],
        None,
    )?;
    let mut packages = vec![];
    for line in out.lines() {
        let Some((package, _)) = line.split_once('|') else {
            continue;
        };
        let package = package.trim();
        if ![
            "platforms;",
            "system-images;",
            "ndk;",
            "build-tools;",
            "cmake;",
        ]
        .iter()
        .any(|p| package.starts_with(p))
        {
            continue;
        }
        ensure!(
            package
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b";._-".contains(&c)),
            "unsupported SDK package ID"
        );
        let path = root.join(package.replace(';', "/"));
        if !path.is_dir() {
            continue;
        }
        let mut i = folder("sdk", path, "sdkmanager installed package ID");
        i.id = format!("sdk:{}:{package}", root.display());
        i.label = package.into();
        i.cleanable = true;
        i.evidence.action = Some(Action::Sdk {
            root: root.clone(),
            package: package.into(),
        });
        packages.push(i);
    }
    let avd_root = std::env::var_os("ANDROID_AVD_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".android/avd"));
    for avd in children(&avd_root)
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "avd"))
    {
        let cfg = read(&avd.join("config.ini"))?;
        let image = cfg
            .lines()
            .find_map(|l| l.strip_prefix("image.sysdir.1="))
            .context("AVD system image unknown")?;
        let image = root.join(image.trim()).canonicalize()?;
        for i in &mut packages {
            if i.path
                .as_ref()
                .is_some_and(|p| fs::canonicalize(p).is_ok_and(|p| p == image))
            {
                i.evidence
                    .consumer(avd.display().to_string(), "AVD system image", true);
            }
        }
    }
    let mut unresolved = false;
    for project in projects {
        for file in ["build.gradle", "build.gradle.kts"] {
            let path = project.join(file);
            if !path.exists() {
                continue;
            }
            let text = read(&path)?;
            let mut found = false;
            for line in text.lines().filter(|l| !l.trim_start().starts_with("//")) {
                for (setting, prefix) in [
                    ("compileSdk", "platforms;android-"),
                    ("ndkVersion", "ndk;"),
                    ("buildToolsVersion", "build-tools;"),
                ] {
                    if let Some((_, value)) = line.split_once(setting) {
                        let value = value
                            .trim_start_matches("Version")
                            .trim()
                            .trim_start_matches(['=', '('])
                            .trim()
                            .trim_matches(['\'', '"', ')', ';']);
                        if !value.is_empty()
                            && value.chars().all(|c| c.is_ascii_digit() || c == '.')
                        {
                            found = true;
                            let required = format!("{prefix}{value}");
                            for i in &mut packages {
                                if i.label == required {
                                    i.evidence.consumer(
                                        project.display().to_string(),
                                        "Gradle declared platform",
                                        true,
                                    );
                                }
                            }
                        } else {
                            unresolved = true;
                        }
                    }
                }
            }
            // Convention plugins/catalogs may hide requirements; absence is not permission.
            if !found {
                unresolved = true;
            }
            for i in &mut packages {
                if i.label.starts_with("cmake;")
                    || i.label.starts_with("build-tools;")
                    || (i.label.starts_with("ndk;") && !text.contains("ndkVersion"))
                {
                    i.evidence.consumer(
                        project.display().to_string(),
                        "implicit Android build tool requirement",
                        true,
                    );
                }
            }
        }
    }
    for i in &mut packages {
        if unresolved {
            i.protection = Some("dynamic or incomplete Gradle SDK requirements".into());
        }
    }
    items.retain(|i| i.kind != "sdk");
    items.extend(packages);
    Ok(())
}
pub fn remove_runtime(uuid: &str, build: &str, _c: &Config) -> Result<String> {
    ensure!(uuid_valid(uuid), "invalid runtime UUID");
    let disks = json("xcrun", &["simctl", "runtime", "list", "--json"], None)?;
    ensure!(
        disks[uuid]["build"]
            .as_str()
            .or_else(|| disks[uuid]["buildversion"].as_str())
            == Some(build),
        "runtime identity changed"
    );
    let v = json("xcrun", &["simctl", "list", "--json"], None)?;
    let runtimes = v["runtimes"].as_array().context("runtime list missing")?;
    let devices = v["devices"].as_object().context("devices missing")?;
    for r in runtimes.iter().filter(|r| r["buildversion"] == build) {
        let id = r["identifier"]
            .as_str()
            .context("runtime identifier missing")?;
        ensure!(
            devices
                .get(id)
                .and_then(|d| d.as_array())
                .is_none_or(|ds| ds.is_empty()),
            "runtime has retained devices"
        );
    }
    command("xcrun", &["simctl", "runtime", "delete", uuid])?;
    Ok("Removed registered unused runtime through simctl".into())
}
pub fn remove_sdk(root: &Path, package: &str) -> Result<String> {
    ensure!(sdk_root() == root, "SDK location changed");
    crate::runtime::canonical(root)?;
    ensure!(
        root.starts_with(home()) && root != home(),
        "SDK must be inside home"
    );
    let tool = sdk_tool(root)?;
    command_at(
        &tool.to_string_lossy(),
        &[
            &format!("--sdk_root={}", root.display()),
            "--uninstall",
            package,
        ],
        None,
        300,
    )?;
    Ok("Uninstalled registered unreferenced SDK package through sdkmanager".into())
}

pub fn preview(i: &Item, c: &Config) -> Result<String> {
    if i.kind == "runtimes" {
        let help = tool("xcrun", &["simctl", "help", "runtime"], None)?;
        ensure!(
            help.contains("--notUsedSinceDays") && help.contains("--dry-run"),
            "Xcode lacks age preview support"
        );
        return tool(
            "xcrun",
            &[
                "simctl",
                "runtime",
                "delete",
                "--notUsedSinceDays",
                &c.retention_days.to_string(),
                "--dry-run",
            ],
            None,
        );
    }
    if i.evidence.metadata.get("manager").and_then(|v| v.as_str()) == Some("homebrew") {
        return tool("brew", &["cleanup", "--dry-run"], None);
    }
    Ok(format!(
        "{}\n{}\n{}",
        i.id,
        i.reason,
        serde_json::to_string_pretty(&i.evidence.action)?
    ))
}
