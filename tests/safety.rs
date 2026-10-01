use aigc::{
    config::{Config, GIB},
    inventory::{Activity, Item, tree_stats, worktree_safe},
    policy::{self, State, Status},
    runtime::git,
};
use std::{fs, os::unix::fs::symlink, path::PathBuf};
fn item() -> Item {
    Item {
        id: "dependencies:/tmp/project/node_modules".into(),
        kind: "dependencies".into(),
        label: "fixture".into(),
        path: Some(PathBuf::from("/tmp/project/node_modules")),
        bytes: 100,
        modified: 1,
        entries: 2,
        complete: true,
        active: false,
        cleanable: true,
        protection: None,
        status: Status::Unknown,
        reason: String::new(),
        idle_seconds: 0,
        observed_uses: 0,
        docker_keep_rank: None,
        docker_created_at: String::new(),
        docker_start_tokens: Default::default(),
    }
}
fn activity() -> Activity {
    Activity {
        reliable: true,
        busy: false,
        open_paths: vec![],
    }
}
fn tick(i: &mut Item, c: &Config, s: &mut State, a: &Activity, day: u64) {
    policy::evaluate(std::slice::from_mut(i), c, s, a, 100 * GIB, day * 86400);
}
#[test]
fn requires_continuous_observation() {
    let mut i = item();
    let c = Config::default();
    let mut s = State::default();
    let a = activity();
    for d in 0..30 {
        tick(&mut i, &c, &mut s, &a, d);
        assert_ne!(i.status, Status::Eligible);
    }
    tick(&mut i, &c, &mut s, &a, 30);
    assert_eq!(i.status, Status::Eligible);
}
#[test]
fn long_offline_gap_resets_observation() {
    let mut i = item();
    let c = Config::default();
    let mut s = State::default();
    tick(&mut i, &c, &mut s, &activity(), 0);
    tick(&mut i, &c, &mut s, &activity(), 60);
    assert_eq!(i.status, Status::Observing);
    assert_eq!(i.idle_seconds, 0);
}
#[test]
fn modification_resets_retention() {
    let mut i = item();
    let c = Config::default();
    let mut s = State::default();
    for d in 0..30 {
        tick(&mut i, &c, &mut s, &activity(), d);
    }
    i.modified += 1;
    tick(&mut i, &c, &mut s, &activity(), 30);
    assert_eq!(i.status, Status::Observing);
}
#[test]
fn unavailable_activity_never_permits_deletion() {
    let mut i = item();
    let mut s = State::default();
    let c = Config::default();
    for d in 0..40 {
        tick(&mut i, &c, &mut s, &Activity::default(), d);
    }
    assert_eq!(i.status, Status::Unknown);
}
#[test]
fn managed_worktree_still_protects_changes() {
    let mut i = item();
    i.kind = "worktrees".into();
    i.protection = Some("contains ignored files".into());
    let mut c = Config::default();
    c.managed.insert(i.id.clone(), "test".into());
    let mut s = State::default();
    for d in 0..40 {
        tick(&mut i, &c, &mut s, &activity(), d);
    }
    assert_eq!(i.status, Status::Protected);
}
#[test]
fn unowned_devices_never_expire() {
    let mut i = item();
    i.kind = "simulators".into();
    let mut s = State::default();
    for d in 0..40 {
        tick(&mut i, &Config::default(), &mut s, &activity(), d);
    }
    assert_eq!(i.status, Status::Protected);
}
#[test]
fn pin_inside_parent_protects_parent() {
    let mut i = item();
    let mut c = Config::default();
    c.pins.push("/tmp/project/node_modules/keep".into());
    let mut s = State::default();
    for d in 0..40 {
        tick(&mut i, &c, &mut s, &activity(), d);
    }
    assert_eq!(i.status, Status::Protected);
}
#[test]
fn open_file_protects_parent() {
    let mut i = item();
    let mut a = activity();
    a.open_paths
        .push("/tmp/project/node_modules/pkg/index.js".into());
    tick(&mut i, &Config::default(), &mut State::default(), &a, 40);
    assert_eq!(i.status, Status::InUse);
}
#[test]
fn pressure_shortens_only_idle_retention() {
    let mut i = item();
    let c = Config::default();
    let mut s = State::default();
    for d in 0..=7 {
        policy::evaluate(
            std::slice::from_mut(&mut i),
            &c,
            &mut s,
            &activity(),
            GIB,
            d * 86400,
        );
    }
    assert_eq!(i.status, Status::Eligible);
    i.complete = false;
    policy::evaluate(
        std::slice::from_mut(&mut i),
        &c,
        &mut s,
        &activity(),
        GIB,
        8 * 86400,
    );
    assert_eq!(i.status, Status::Unknown);
}
#[test]
fn sizes_do_not_follow_links_or_double_count_hardlinks() {
    let tmp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("large"), vec![1u8; 1_000_000]).unwrap();
    fs::write(tmp.path().join("a"), vec![1u8; 8192]).unwrap();
    let original = tree_stats(tmp.path()).0;
    fs::hard_link(tmp.path().join("a"), tmp.path().join("b")).unwrap();
    symlink(outside.path(), tmp.path().join("outside")).unwrap();
    let (size, _, _, complete) = tree_stats(tmp.path());
    assert!(complete);
    assert!(size <= original + 8192);
}
#[test]
fn malformed_configuration_fails_closed() {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("config.json"), r#"{"retention_days":0}"#).unwrap();
    assert!(Config::load(d.path()).is_err());
    fs::write(d.path().join("config.json"), r#"{"typo":true}"#).unwrap();
    assert!(Config::load(d.path()).is_err());
}
#[test]
fn worktree_with_ignored_or_untracked_data_is_protected() {
    let d = tempfile::tempdir().unwrap();
    let main = d.path().join("main");
    fs::create_dir(&main).unwrap();
    git(&main, &["init"]).unwrap();
    git(&main, &["config", "user.email", "test@example.invalid"]).unwrap();
    git(&main, &["config", "user.name", "Test"]).unwrap();
    fs::write(main.join(".gitignore"), "secret\n").unwrap();
    git(&main, &["add", "."]).unwrap();
    git(&main, &["commit", "-m", "fixture"]).unwrap();
    let wt = d.path().join("worktree");
    git(
        &main,
        &["worktree", "add", "-b", "test", wt.to_str().unwrap()],
    )
    .unwrap();
    assert!(worktree_safe(&wt).is_ok());
    fs::write(wt.join("secret"), "must survive").unwrap();
    assert!(worktree_safe(&wt).is_err());
    fs::remove_file(wt.join("secret")).unwrap();
    fs::write(wt.join("untracked"), "must survive").unwrap();
    assert!(worktree_safe(&wt).is_err());
    assert!(worktree_safe(&main).is_err());
}

#[test]
fn working_in_parent_directory_does_not_mark_every_child_in_use() {
    let mut a = activity();
    a.open_paths.push("/tmp".into());
    assert!(!a.touches(Some(std::path::Path::new("/tmp/project/node_modules"))));
}
#[test]
fn docker_uses_native_age_filter_without_manufactured_observations() {
    let mut i = item();
    i.kind = "docker-cache".into();
    i.path = None;
    tick(
        &mut i,
        &Config::default(),
        &mut State::default(),
        &activity(),
        0,
    );
    assert_eq!(i.status, Status::Eligible);
    let c = Config {
        docker_cache_cleanup: false,
        ..Config::default()
    };
    tick(&mut i, &c, &mut State::default(), &activity(), 0);
    assert_eq!(i.status, Status::Protected);
}

fn image(n: usize, uses: usize) -> Item {
    let mut i = item();
    i.id = format!("docker-images:image-{n}");
    i.kind = "docker-images".into();
    i.path = None;
    i.docker_created_at = format!("2026-01-0{n}T00:00:00Z");
    i.docker_start_tokens = (0..uses)
        .map(|v| (format!("container-{n}-{v}"), "2026-01-01T00:00:00Z".into()))
        .collect();
    i
}
#[test]
fn three_most_used_images_survive_explicit_disposable_registration_and_pressure() {
    let mut images: Vec<_> = (1..=5).map(|n| image(n, n)).collect();
    let mut c = Config::default();
    for i in &images {
        c.managed.insert(i.id.clone(), "test".into());
    }
    let mut s = State::default();
    for d in 0..=40 {
        policy::evaluate(&mut images, &c, &mut s, &activity(), 0, d * 86400);
    }
    assert_eq!(
        images
            .iter()
            .filter(|i| i.docker_keep_rank.is_some())
            .count(),
        3
    );
    for i in &images[2..] {
        assert_eq!(i.status, Status::Protected);
    }
    for i in &images[..2] {
        assert_eq!(i.status, Status::Eligible);
    }
    assert_eq!(
        images[4].observed_uses, 5,
        "repeated scans must not inflate frequency"
    );
}
#[test]
fn popularity_persists_when_containers_are_removed_and_counts_restarts() {
    let mut images = vec![image(1, 1), image(2, 2), image(3, 3), image(4, 4)];
    let mut s = State::default();
    let c = Config::default();
    policy::evaluate(&mut images, &c, &mut s, &activity(), 0, 0);
    images[3]
        .docker_start_tokens
        .insert("container-4-0".into(), "2026-01-02T00:00:00Z".into());
    policy::evaluate(&mut images, &c, &mut s, &activity(), 0, 86400);
    assert_eq!(images[3].observed_uses, 5);
    for i in &mut images {
        i.docker_start_tokens.clear();
    }
    let saved = serde_json::to_string(&s).unwrap();
    let mut restored: State = serde_json::from_str(&saved).unwrap();
    policy::evaluate(&mut images, &c, &mut restored, &activity(), 0, 2 * 86400);
    assert_eq!(images[3].observed_uses, 5);
    assert_eq!(images[3].docker_keep_rank, Some(1));
}
#[test]
fn fewer_than_three_images_are_all_kept() {
    let mut images = vec![image(1, 0), image(2, 0)];
    policy::evaluate(
        &mut images,
        &Config::default(),
        &mut State::default(),
        &activity(),
        0,
        0,
    );
    assert!(images.iter().all(|i| i.status == Status::Protected));
    let c = Config {
        docker_keep_most_used: 2,
        ..Config::default()
    };
    assert!(c.validate().is_err());
}
