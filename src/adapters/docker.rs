use super::*;
use crate::inventory::local_docker;
use std::collections::BTreeSet;

fn endpoint(name: &str) -> Result<String> {
    if name.starts_with("unix://") {
        return local_socket(name);
    }
    ensure!(!name.contains("://"), "remote builder endpoint");
    let v = json("docker", &["context", "inspect", name], None)?;
    let host = v[0]["Endpoints"]["docker"]["Host"]
        .as_str()
        .context("missing local endpoint")?;
    ensure!(
        host.starts_with("unix://"),
        "remote builder is outside scope"
    );
    local_socket(host)
}
fn local_socket(endpoint: &str) -> Result<String> {
    let path = endpoint
        .strip_prefix("unix://")
        .context("not a local Unix endpoint")?;
    ensure!(
        Path::new(path).is_absolute(),
        "Unix socket path must be absolute"
    );
    Ok(format!("unix://{}", fs::canonicalize(path)?.display()))
}
fn builder_endpoint(builder: &str) -> Result<String> {
    // Never bootstrap/start a builder as part of inspection.
    let output = tool("docker", &["buildx", "ls", "--format", "{{json .}}"], None)?;
    let mut found = None;
    for line in output.lines() {
        let v: Value = serde_json::from_str(line)?;
        if v["Name"] == builder {
            found = Some(v);
            break;
        }
    }
    let v = found.context("builder missing")?;
    ensure!(
        matches!(v["Driver"].as_str(), Some("docker" | "docker-container")),
        "unsupported builder driver"
    );
    let nodes = v["Nodes"].as_array().context("builder nodes missing")?;
    ensure!(
        nodes.len() == 1,
        "multi-node builders require manual handling"
    );
    ensure!(nodes[0]["Status"] == "running", "builder is not running");
    endpoint(
        nodes[0]["Endpoint"]
            .as_str()
            .context("missing builder endpoint")?,
    )
}
pub fn scan(c: &Config, projects: &[PathBuf], items: &mut Vec<Item>, warnings: &mut Vec<String>) {
    if local_docker().is_err() {
        return;
    }
    // Replace aggregate pruning with inspectable, individually selected cache records.
    items.retain(|i| i.kind != "docker-cache");
    if let Err(e) = images(projects, items) {
        warnings.push(format!(
            "Docker project/container reference inspection incomplete: {e}"
        ));
        for i in items.iter_mut().filter(|i| i.kind == "docker-images") {
            i.complete = false;
        }
    }
    let result = (|| -> Result<()> {
        let output = tool("docker", &["buildx", "ls", "--format", "{{json .}}"], None)?;
        let mut builders = BTreeSet::new();
        let mut engines = BTreeSet::new();
        for line in output.lines() {
            let v: Value = serde_json::from_str(line)?;
            let builder = v["Name"].as_str().context("builder name missing")?;
            if !builders.insert(builder.to_owned()) {
                continue;
            }
            let local = match builder_endpoint(builder) {
                Ok(e) => e,
                Err(_) => {
                    warnings.push(format!(
                        "builder {builder}: remote, unavailable or unsupported; skipped"
                    ));
                    continue;
                }
            };
            let engine_key = if v["Driver"] == "docker" {
                local.clone()
            } else {
                format!("{local}:{builder}")
            };
            if !engines.insert(engine_key) {
                continue;
            }
            let output = match tool(
                "docker",
                &["buildx", "du", "--builder", builder, "--format=json"],
                None,
            ) {
                Ok(s) => s,
                Err(_) => {
                    warnings.push(format!("builder {builder}: structured usage unavailable"));
                    continue;
                }
            };
            for line in output.lines() {
                let v: Value = serde_json::from_str(line)?;
                let id = v["ID"].as_str().context("cache ID missing")?;
                let mut i = Item::new(
                    "docker-cache",
                    format!("docker-cache:{builder}:{id}"),
                    format!("{builder} / {id}"),
                    None,
                    true,
                );
                i.bytes = integer(&v["Size"])
                    .or_else(|| v["Size"].as_str().and_then(super::human_bytes))
                    .unwrap_or(0);
                i.complete = integer(&v["Size"]).is_some()
                    || v["Size"].as_str().and_then(super::human_bytes).is_some();
                i.active = v["Reclaimable"].as_bool() != Some(true);
                i.evidence.native_last_used = epoch(&v["LastUsedAt"]);
                i.evidence.native_usage_count = integer(&v["UsageCount"]);
                i.evidence.shared = v["Shared"].as_bool();
                i.evidence.reclaimable_bytes =
                    (v["Shared"] == false && !i.active).then_some(i.bytes);
                i.evidence.source = "BuildKit native cache record".into();
                i.evidence.action = Some(Action::Buildkit {
                    builder: builder.into(),
                    endpoint: local.clone(),
                    record: id.into(),
                });
                i.evidence.identity = Some(format!("{local}:{builder}:{id}"));
                i.evidence
                    .metadata
                    .insert("builder".into(), Value::String(builder.into()));
                for key in ["Parents", "Type", "Mutable", "Reclaimable", "CreatedAt"] {
                    i.evidence.metadata.insert(key.into(), v[key].clone());
                }
                if v["Shared"] != false || v["Mutable"] != false || v["Type"] != "regular" {
                    i.protection = Some("shared, mutable or special cache record; retain native cache mounts and internal data".into());
                }
                i.evidence
                    .metadata
                    .insert("last_used_native_text".into(), v["LastUsedAt"].clone());
                i.evidence
                    .metadata
                    .insert("size_native_text".into(), v["Size"].clone());
                i.evidence.metadata.insert(
                    "size_is_estimate".into(),
                    Value::Bool(integer(&v["Size"]).is_none()),
                );
                i.evidence.reconstruction = "BuildKit recreates cache on subsequent builds; exact host disk savings can be lower due to shared storage.".into();
                items.push(i);
            }
        }
        Ok(())
    })();
    if result.is_err() {
        warnings.push("Buildx inventory incomplete; cache cleanup disabled".into());
        for i in items.iter_mut().filter(|i| i.kind == "docker-cache") {
            i.complete = false;
        }
    }
    // Avoid even narrow pruning when a protected record may share underlying cache data.
    let protected: BTreeSet<String> = items
        .iter()
        .filter(|i| {
            i.kind == "docker-cache"
                && c.pins.iter().any(|pin| {
                    pin == &i.id
                        || (pin.starts_with("docker-cache:")
                            && pin.rsplit(':').next() == i.id.rsplit(':').next())
                })
        })
        .filter_map(|i| i.evidence.metadata["builder"].as_str().map(str::to_owned))
        .collect();
    for i in items.iter_mut().filter(|i| i.kind == "docker-cache") {
        if i.evidence
            .metadata
            .get("builder")
            .and_then(|v| v.as_str())
            .is_some_and(|b| protected.contains(b))
        {
            i.protection = Some("builder contains a pinned cache record".into());
        }
    }
}
fn images(projects: &[PathBuf], items: &mut [Item]) -> Result<()> {
    let containers = tool("docker", &["container", "ls", "-aq"], None)?;
    for id in containers.lines() {
        let v = json("docker", &["container", "inspect", id], None)?;
        let image = v[0]["Image"].as_str().context("container image missing")?;
        for i in items
            .iter_mut()
            .filter(|i| i.id == format!("docker-images:{image}"))
        {
            i.active = true;
            i.evidence.consumer(
                format!("container:{id}"),
                "Docker container (running or stopped)",
                true,
            );
        }
    }
    let mut requirements: Vec<(String, String)> = vec![];
    for p in projects {
        for name in [
            "compose.yaml",
            "compose.yml",
            "docker-compose.yaml",
            "docker-compose.yml",
        ] {
            if p.join(name).is_file() {
                // Only image names are retained; expanded environments and secrets are never stored.
                let output = tool(
                    "docker",
                    &[
                        "compose",
                        "-f",
                        &p.join(name).to_string_lossy(),
                        "config",
                        "--images",
                    ],
                    Some(p),
                )
                .map_err(|_| anyhow::anyhow!("Compose image references unresolved"))?;
                for line in output.lines().filter(|s| !s.trim().is_empty()) {
                    requirements.push((line.trim().into(), p.display().to_string()));
                }
            }
        }
        if p.join("Dockerfile").is_file() {
            let text = read(&p.join("Dockerfile"))?;
            let mut stages = BTreeSet::new();
            for line in text.lines() {
                let words: Vec<_> = line.split_whitespace().collect();
                if words
                    .first()
                    .is_some_and(|s| s.eq_ignore_ascii_case("FROM"))
                {
                    let image = words
                        .iter()
                        .skip(1)
                        .find(|w| !w.starts_with("--"))
                        .context("unresolved Dockerfile FROM")?;
                    ensure!(
                        !image.contains('$') && !image.contains('\\'),
                        "dynamic Dockerfile base image unresolved"
                    );
                    if *image != "scratch" && !stages.contains(*image) {
                        requirements.push((image.to_string(), p.display().to_string()));
                    }
                    if let Some(n) = words.iter().position(|w| w.eq_ignore_ascii_case("AS"))
                        && let Some(name) = words.get(n + 1)
                    {
                        stages.insert(name.to_string());
                    }
                }
            }
        }
    }
    let mut resolved_requirements = Vec::new();
    for (reference, project) in &requirements {
        if let Ok(resolved) = json("docker", &["image", "inspect", reference], None)
            && let Some(id) = resolved[0]["Id"].as_str()
        {
            resolved_requirements.push((id.to_owned(), project));
        }
    }
    let usage = json(
        "docker",
        &["system", "df", "--verbose", "--format", "{{json .}}"],
        None,
    )
    .ok();
    let context = tool("docker", &["context", "show"], None)?;
    let local = endpoint(context.trim())?;
    for i in items.iter_mut().filter(|i| i.kind == "docker-images") {
        let id =
            i.id.strip_prefix("docker-images:")
                .context("invalid image ID")?;
        let v = json("docker", &["image", "inspect", id], None)?;
        i.evidence.source = "Docker image and project references".into();
        for key in ["RepoTags", "RepoDigests", "Architecture", "Os", "Created"] {
            i.evidence.metadata.insert(key.into(), v[0][key].clone());
        }
        i.evidence.logical_bytes = integer(&v[0]["Size"]);
        i.evidence.reconstruction = "Pull the retained digest or rebuild from source. Locally built images may not be recoverable.".into();
        i.evidence.identity = Some(format!("{local}:{id}"));
        for (image, project) in &resolved_requirements {
            if image == id {
                i.evidence.consumer(
                    project.as_str(),
                    "Compose/Dockerfile image requirement",
                    true,
                );
            }
        }
        if let Some(row) = usage
            .as_ref()
            .and_then(|v| v["Images"].as_array())
            .and_then(|rows| rows.iter().find(|r| r["ID"] == id))
        {
            for key in ["SharedSize", "UniqueSize"] {
                i.evidence.metadata.insert(key.into(), row[key].clone());
            }
        }
        i.evidence.metadata.insert(
            "usage_history".into(),
            Value::String(
                "Only observed container references; Docker image creation time is not last use"
                    .into(),
            ),
        );
    }
    Ok(())
}
pub fn remove_cache(
    builder: &str,
    saved_endpoint: &str,
    record: &str,
    c: &Config,
) -> Result<String> {
    local_docker()?;
    ensure!(
        builder_endpoint(builder)? == saved_endpoint,
        "builder endpoint changed"
    );
    ensure!(
        record.bytes().all(|c| c.is_ascii_alphanumeric()),
        "invalid cache record ID"
    );
    let days = if crate::runtime::disk(&home())?.1 < c.min_free_bytes {
        c.pressure_retention_days
    } else {
        c.retention_days
    };
    let age = format!("until={}h", days * 24);
    let id = format!("id=^{record}$");
    let help = tool("docker", &["buildx", "prune", "--help"], None)?;
    ensure!(
        help.contains("--filter") && help.contains("--max-used-space"),
        "Buildx lacks supported bounded pruning"
    );
    let budget = c.docker_cache_budget_bytes.to_string();
    command_at(
        "docker",
        &[
            "buildx",
            "prune",
            "--builder",
            builder,
            "--force",
            "--filter",
            &id,
            "--filter",
            &age,
            "--max-used-space",
            &budget,
        ],
        None,
        300,
    )
}
