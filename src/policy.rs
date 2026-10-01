use crate::{
    config::Config,
    inventory::{Activity, Item},
    runtime::now,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Observation {
    pub signature: String,
    pub idle_since: u64,
    pub last_seen: u64,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    pub observations: BTreeMap<String, Observation>,
    pub last_collection: Option<u64>,
    #[serde(default)]
    pub image_usage: BTreeMap<String, ImageUsage>,
    #[serde(default)]
    pub protected_images: BTreeSet<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImageUsage {
    pub observed_starts: u64,
    pub container_starts: BTreeMap<String, String>,
    pub last_seen: u64,
}

fn protect_popular_images(items: &mut [Item], c: &Config, s: &mut State, time: u64) {
    let mut ranking = Vec::new();
    for i in items.iter_mut().filter(|i| i.kind == "docker-images") {
        let u = s.image_usage.entry(i.id.clone()).or_default();
        if i.complete {
            for (container, start) in &i.docker_start_tokens {
                if u.container_starts.get(container) != Some(start) {
                    u.observed_starts = u.observed_starts.saturating_add(1);
                }
            }
            u.container_starts = i.docker_start_tokens.clone();
            u.last_seen = time;
        }
        i.observed_uses = u.observed_starts;
        i.docker_keep_rank = None;
        ranking.push((i.id.clone(), u.observed_starts, i.docker_created_at.clone()));
    }
    ranking.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)).then(a.0.cmp(&b.0)));
    // A hard minimum: a policy edit cannot expose the three most-used images.
    s.protected_images = ranking
        .iter()
        .take(c.docker_keep_most_used.max(3))
        .map(|r| r.0.clone())
        .collect();
    for (rank, (id, _, _)) in ranking
        .iter()
        .take(c.docker_keep_most_used.max(3))
        .enumerate()
    {
        if let Some(i) = items.iter_mut().find(|i| &i.id == id) {
            i.docker_keep_rank = Some(rank + 1);
        }
    }
    s.image_usage
        .retain(|_, u| time.saturating_sub(u.last_seen) < 90 * 86400);
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    InUse,
    Protected,
    Observing,
    Eligible,
    Unknown,
}

pub fn evaluate(items: &mut [Item], c: &Config, s: &mut State, a: &Activity, free: u64, time: u64) {
    protect_popular_images(items, c, s, time);
    let days = if free < c.min_free_bytes {
        c.pressure_retention_days
    } else {
        c.retention_days
    };
    for i in items {
        let signature = format!("{}:{}:{}", i.bytes, i.modified, i.entries);
        let o = s.observations.entry(i.id.clone()).or_insert(Observation {
            signature: signature.clone(),
            idle_since: time,
            last_seen: time,
        });
        if o.signature != signature
            || time.saturating_sub(o.last_seen) > 48 * 3600
            || i.active
            || a.touches(i.path.as_deref())
        {
            o.idle_since = time;
        }
        o.signature = signature;
        o.last_seen = time;
        i.idle_seconds = time.saturating_sub(o.idle_since);
        let (status, reason) = if c.paused_until > time {
            (Status::Protected, "collection is paused".into())
        } else if c.pins.iter().any(|p| {
            p == &i.id
                || i.path.as_ref().is_some_and(|path| {
                    path.starts_with(p) || std::path::Path::new(p).starts_with(path)
                })
        }) {
            (Status::Protected, "pinned by configuration".into())
        } else if i.active || a.touches(i.path.as_deref()) {
            (
                Status::InUse,
                "running resource or open file / working directory".into(),
            )
        } else if let Some(rank) = i.docker_keep_rank {
            (
                Status::Protected,
                format!(
                    "kept Docker image #{rank}: {} observed container starts; top {} are always protected",
                    i.observed_uses, c.docker_keep_most_used
                ),
            )
        } else if !i.complete {
            (
                Status::Unknown,
                "inventory incomplete; no automatic removal".into(),
            )
        } else if !a.reliable {
            (
                Status::Unknown,
                "process activity could not be verified".into(),
            )
        } else if let Some(reason) = &i.protection {
            (Status::Protected, reason.clone())
        } else if !i.cleanable {
            (
                Status::Protected,
                "report-only category in this version".into(),
            )
        } else if matches!(
            i.kind.as_str(),
            "worktrees" | "simulators" | "emulators" | "docker-images"
        ) && !c.managed.contains_key(&i.id)
        {
            (
                Status::Protected,
                "not registered as disposable; use aigc manage".into(),
            )
        } else if i.kind == "docker-cache" && !c.docker_cache_cleanup {
            (Status::Protected, "Docker cache cleanup disabled".into())
        } else if a.busy {
            (
                Status::Protected,
                "a build or agent process is running; collection deferred".into(),
            )
        } else if i.kind == "docker-cache" {
            (
                Status::Eligible,
                format!(
                    "Docker applies its native {days}-day cache age filter and configured storage budget"
                ),
            )
        } else if i.idle_seconds < days * 86400 {
            (
                Status::Observing,
                format!("requires {days} days of observed inactivity"),
            )
        } else {
            (
                Status::Eligible,
                format!("observed idle for at least {days} days; rechecked before deletion"),
            )
        };
        i.status = status;
        i.reason = reason;
    }
    // Keep missing observations briefly, without allowing unbounded state growth.
    s.observations
        .retain(|_, o| time.saturating_sub(o.last_seen) < 90 * 86400);
}
pub fn load_state(dir: &std::path::Path) -> anyhow::Result<State> {
    let p = dir.join("state.json");
    if !p.exists() {
        return Ok(State::default());
    }
    Ok(serde_json::from_slice(&std::fs::read(p)?)?)
}
pub fn timestamp() -> u64 {
    now()
}
