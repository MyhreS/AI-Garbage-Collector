use serde_json::{Value, json};
use std::{fs, process::Command};
fn cli(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_aigc"))
        .env("AIGC_STATE_DIR", dir)
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn cached_json_status_is_explicit_and_fast() {
    let d = tempfile::tempdir().unwrap();
    let report = json!({"schema_version":1,"generated_at":aigc::runtime::now(),"disk_total_bytes":100,"disk_free_bytes":50,"min_free_bytes":20,"last_collection":null,"service_installed":false,"warnings":[],"items":[]});
    fs::write(d.path().join("last-report.json"), report.to_string()).unwrap();
    let out = cli(d.path(), &["status", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["cached"], true);
    assert_eq!(v["disk_free_bytes"], 50);
}
#[test]
fn setting_updates_invalidate_snapshot() {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("last-report.json"), "{}").unwrap();
    let out = cli(d.path(), &["config", "set", "min-free-space", "25GB"]);
    assert!(out.status.success());
    assert!(!d.path().join("last-report.json").exists());
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["min_free_bytes"], 25 * 1024u64 * 1024 * 1024);
}
#[test]
fn invalid_policy_is_not_written() {
    let d = tempfile::tempdir().unwrap();
    let out = cli(d.path(), &["config", "set", "retention-days", "0"]);
    assert!(!out.status.success());
    assert!(!d.path().join("config.json").exists());
}
#[test]
fn command_exit_code_and_lease_cleanup_are_preserved() {
    let d = tempfile::tempdir().unwrap();
    let out = cli(d.path(), &["run", "--", "/bin/sh", "-c", "exit 7"]);
    assert_eq!(out.status.code(), Some(7));
    if let Ok(es) = fs::read_dir(d.path().join("leases")) {
        assert_eq!(es.count(), 0);
    }
}
