use crate::{
    config::{home, state_dir},
    runtime::command,
};
use anyhow::{Result, ensure};
use std::{fs, path::PathBuf};

const LABEL: &str = "io.aigc.collector";
pub fn plist_path() -> PathBuf {
    home().join("Library/LaunchAgents/io.aigc.collector.plist")
}
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
pub fn install() -> Result<()> {
    ensure!(
        cfg!(target_os = "macos"),
        "service installation requires macOS"
    );
    ensure!(
        std::env::var_os("AIGC_STATE_DIR").is_none(),
        "service installation does not support AIGC_STATE_DIR"
    );
    let exe = std::env::current_exe()?;
    let state = state_dir();
    fs::create_dir_all(&state)?;
    let parent = plist_path().parent().unwrap().to_path_buf();
    fs::create_dir_all(parent)?;
    let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin:/usr/sbin:/sbin".into());
    let body = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{LABEL}</string>
<key>ProgramArguments</key><array><string>{}</string><string>collect</string></array>
<key>RunAtLoad</key><true/><key>StartInterval</key><integer>3600</integer>
<key>WorkingDirectory</key><string>{}</string>
<key>ProcessType</key><string>Background</string><key>LowPriorityIO</key><true/>
<key>EnvironmentVariables</key><dict><key>PATH</key><string>{}</string><key>HOME</key><string>{}</string></dict>
</dict></plist>
"#,
        xml(&exe.to_string_lossy()),
        xml(&home().to_string_lossy()),
        xml(&path),
        xml(&home().to_string_lossy())
    );
    fs::write(plist_path(), body)?;
    let target = format!("gui/{}", unsafe { libc::getuid() });
    let service = format!("{target}/{LABEL}");
    let _ = command("launchctl", &["bootout", &service]);
    command(
        "launchctl",
        &["bootstrap", &target, plist_path().to_str().unwrap()],
    )?;
    Ok(())
}
pub fn uninstall() -> Result<()> {
    ensure!(
        std::env::var_os("AIGC_STATE_DIR").is_none(),
        "service removal does not support AIGC_STATE_DIR"
    );
    let service = format!("gui/{}/{LABEL}", unsafe { libc::getuid() });
    let _ = command("launchctl", &["bootout", &service]);
    ensure!(
        command("launchctl", &["print", &service]).is_err(),
        "service is still loaded; uninstall refused to report success"
    );
    if plist_path().exists() {
        fs::remove_file(plist_path())?;
    }
    Ok(())
}
pub fn status() -> serde_json::Value {
    let service = format!("gui/{}/{LABEL}", unsafe { libc::getuid() });
    serde_json::json!({"installed":plist_path().exists(),"loaded":command("launchctl",&["print",&service]).is_ok(),"interval_seconds":3600,"plist":plist_path()})
}

pub fn installed() -> bool {
    plist_path().exists()
}
