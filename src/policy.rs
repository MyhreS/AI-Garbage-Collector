use crate::{
    config::Config,
    inventory::{Activity, Item},
    runtime::now,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const POLICY_VERSION: u32 = 7;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Observation {
    pub signature: String,
    pub idle_since: u64,
    pub last_seen: u64,
    #[serde(default)]
    pub first_seen: u64,
    #[serde(default)]
    pub last_used: Option<u64>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    pub observations: BTreeMap<String, Observation>,
    pub last_collection: Option<u64>,
    #[serde(default)]
    pub maintenance: BTreeMap<String, u64>,
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
        let signature = format!(
            "{}:{}:{}:{:?}:{:?}:{:?}",
            i.bytes,
            i.modified,
            i.entries,
            i.evidence.identity,
            i.evidence.native_usage_count,
            i.evidence.action
        );
        let o = s.observations.entry(i.id.clone()).or_insert(Observation {
            signature: signature.clone(),
            idle_since: time,
            last_seen: time,
            first_seen: time,
            last_used: None,
        });
        if o.first_seen == 0 {
            o.first_seen = time;
        }
        let gap = time.saturating_sub(o.last_seen) > 48 * 3600;
        i.evidence.observation_gap = gap;
        let worktree = i.kind == "worktrees";
        let signature_changed = o.signature != signature;
        if i.active
            || a.touches(i.path.as_deref())
            || (worktree && (signature_changed || i.evidence.consumers.iter().any(|r| r.protects)))
        {
            o.last_used = Some(time);
        }
        i.evidence.first_seen = o.first_seen;
        i.evidence.last_observed_use = o.last_used;
        if signature_changed
            || time.saturating_sub(o.last_seen) > 48 * 3600
            || i.active
            || i.evidence.consumers.iter().any(|r| r.protects)
            || a.touches(i.path.as_deref())
        {
            o.idle_since = time;
        }
        o.signature = signature;
        o.last_seen = time;
        i.idle_seconds = if worktree {
            let head_committed_at = i.evidence.metadata["head_committed_at"]
                .as_u64()
                .unwrap_or(time);
            let last_write_or_use = i
                .modified
                .max(head_committed_at)
                .max(o.last_used.unwrap_or(0));
            time.saturating_sub(last_write_or_use)
        } else {
            time.saturating_sub(o.idle_since)
        };
        let referenced = i
            .evidence
            .consumers
            .iter()
            .filter(|r| r.protects)
            .map(|r| format!("required by {} ({})", r.id, r.source))
            .collect::<Vec<_>>();
        i.evidence.blockers.extend(referenced.clone());
        let registered = c
            .registered
            .iter()
            .find(|r| i.path.as_ref() == Some(&r.path));
        let needs_registration =
            i.evidence.action.is_some() || matches!(i.kind.as_str(), "simulators" | "emulators");
        let cooldown = s
            .maintenance
            .get(&i.id)
            .is_some_and(|last| time.saturating_sub(*last) < c.maintenance_cooldown_days * 86400);
        let (status, mut reason) = if c.paused_until > time {
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
        } else if let Some(reason) = referenced.first() {
            (Status::Protected, reason.clone())
        } else if registered.is_some_and(|r| time < r.retain_until) {
            (
                Status::Protected,
                "registered retention deadline has not passed".into(),
            )
        } else if cooldown {
            (
                Status::Protected,
                "native maintenance cooldown; avoid repeated downloads/rebuilds".into(),
            )
        } else if let Some(reason) = &i.protection {
            (Status::Protected, reason.clone())
        } else if worktree && !c.worktree_cleanup {
            (Status::Protected, "worktree cleanup disabled".into())
        } else if worktree
            && !c.worktree_force
            && i.evidence.metadata["uncommitted_or_ignored_files"]
                .as_bool()
                .unwrap_or(true)
        {
            (
                Status::Protected,
                "worktree has local files and force removal is disabled".into(),
            )
        } else if !i.cleanable {
            (
                Status::Protected,
                "report-only category in this version".into(),
            )
        } else if needs_registration && !c.managed.contains_key(&i.id) && registered.is_none() {
            (
                Status::Protected,
                "not registered as disposable; use aigc manage".into(),
            )
        } else if a.global_reserved {
            (Status::Protected, "aigc run reserves all resources".into())
        } else if a.busy && !worktree {
            (
                Status::Protected,
                "a build or agent process is running; collection deferred".into(),
            )
        } else if i.kind == "package-caches" && i.bytes <= c.cache_budget_bytes {
            (
                Status::Protected,
                "cache is within its configured budget".into(),
            )
        } else if i.idle_seconds < days * 86400 {
            (
                Status::Observing,
                if worktree {
                    format!(
                        "requires {days} days since the latest file write, HEAD commit or detected use"
                    )
                } else {
                    format!("requires {days} days of observed inactivity")
                },
            )
        } else {
            (
                Status::Eligible,
                if worktree {
                    format!(
                        "no file write, HEAD commit or detected use for at least {days} days; rechecked before deletion"
                    )
                } else {
                    format!("observed idle for at least {days} days; rechecked before deletion")
                },
            )
        };
        if worktree
            && status == Status::Eligible
            && i.evidence.metadata["pr_verification"] == "unavailable"
        {
            reason.push_str("; PR verification unavailable, allowed by configuration");
        }
        if status != Status::Eligible && !i.evidence.blockers.contains(&reason) {
            i.evidence.blockers.push(reason.clone());
        }
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
