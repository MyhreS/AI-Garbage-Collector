use crate::{
    config::Config,
    inventory::{Activity, Item},
    runtime::now,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
