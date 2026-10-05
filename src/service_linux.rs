use crate::{config::home, runtime::command};
use anyhow::{Result, ensure};
use std::{fs, path::PathBuf};
fn directory() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
        .join("systemd/user")
}
pub fn installed() -> bool {
    directory().join("aigc.timer").exists()
}
fn quote(s: &str) -> String {
    format!(
        "\"{}\"",
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}
pub fn install() -> Result<()> {
    ensure!(
        std::env::var_os("AIGC_STATE_DIR").is_none(),
        "service installation does not support AIGC_STATE_DIR"
    );
    command("systemctl", &["--user", "show-environment"])?;
    let exe = std::env::current_exe()?;
    fs::create_dir_all(directory())?;
    let path = std::env::var("PATH").unwrap_or_default();
    fs::write(
        directory().join("aigc.service"),
        format!(
            "[Unit]\nDescription=AI Garbage Collector\n[Service]\nType=oneshot\nExecStart=:{} collect\nWorkingDirectory={}\nEnvironment={}\nEnvironment={}\nNice=10\n",
            quote(&exe.to_string_lossy()),
            quote(&home().to_string_lossy()),
            quote(&format!("PATH={path}")),
            quote(&format!(
                "XDG_STATE_HOME={}",
                std::env::var("XDG_STATE_HOME")
                    .unwrap_or_else(|_| home().join(".local/state").to_string_lossy().into_owned())
            ))
        ),
    )?;
    fs::write(
        directory().join("aigc.timer"),
        "[Unit]\nDescription=Hourly AI Garbage Collector\n[Timer]\nOnStartupSec=1min\nOnUnitActiveSec=1h\nUnit=aigc.service\n[Install]\nWantedBy=timers.target\n",
    )?;
    command("systemctl", &["--user", "daemon-reload"])?;
    command("systemctl", &["--user", "enable", "--now", "aigc.timer"])?;
    command(
        "systemctl",
        &["--user", "start", "--no-block", "aigc.service"],
    )?;
    Ok(())
}
pub fn uninstall() -> Result<()> {
    ensure!(
        std::env::var_os("AIGC_STATE_DIR").is_none(),
        "service removal does not support AIGC_STATE_DIR"
    );
    command("systemctl", &["--user", "disable", "--now", "aigc.timer"])?;
    command("systemctl", &["--user", "stop", "aigc.service"])?;
    for name in ["aigc.timer", "aigc.service"] {
        let p = directory().join(name);
        if p.exists() {
            fs::remove_file(p)?;
        }
    }
    command("systemctl", &["--user", "daemon-reload"])?;
    Ok(())
}
pub fn status() -> serde_json::Value {
    serde_json::json!({"installed":installed(),"loaded":command("systemctl",&["--user","is-active","aigc.timer"]).is_ok(),"interval_seconds":3600,"scheduler":"systemd user timer"})
}
