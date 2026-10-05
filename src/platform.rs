//! OS boundaries. Missing activity or file identity is never permission to delete.
use crate::{config::home, runtime::command};
use anyhow::Result;
use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

pub fn state_dir() -> PathBuf {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join("AppData/Local"))
            .join("aigc")
    } else if cfg!(target_os = "macos") {
        home().join("Library/Application Support/aigc")
    } else {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local/state"))
            .join("aigc")
    }
}
pub fn android_sdk() -> PathBuf {
    if cfg!(windows) {
        home().join("AppData/Local/Android/Sdk")
    } else if cfg!(target_os = "macos") {
        home().join("Library/Android/sdk")
    } else {
        home().join("Android/Sdk")
    }
}
pub fn private_dir(p: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(p, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(windows)]
    {
        let _ = p;
    }
    Ok(())
}
pub fn normalize(p: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let s = p.to_string_lossy().replace('/', "\\");
        PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s))
    }
    #[cfg(unix)]
    {
        p
    }
}
pub fn is_link(m: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        m.file_attributes() & 0x400 != 0
    }
    #[cfg(unix)]
    {
        m.file_type().is_symlink()
    }
}
pub fn file_id(p: &Path, m: &fs::Metadata) -> Result<file_id::FileId> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = p;
        Ok(file_id::FileId::new_inode(m.dev(), m.ino()))
    }
    #[cfg(windows)]
    {
        let _ = m;
        Ok(file_id::get_file_id(p)?)
    }
}
pub fn identity(p: &Path) -> Result<String> {
    Ok(format!("{:?}", file_id(p, &fs::symlink_metadata(p)?)?))
}
pub fn bytes(m: &fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        m.blocks().saturating_mul(512)
    }
    // Windows reports logical bytes; compression and sparse allocation can differ.
    #[cfg(windows)]
    {
        m.len()
    }
}
pub fn modified(m: &fs::Metadata) -> u64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}
pub fn process_names() -> Result<String> {
    #[cfg(unix)]
    {
        command("/bin/ps", &["-axo", "comm="])
    }
    #[cfg(windows)]
    {
        powershell("Get-Process -ErrorAction Stop | ForEach-Object { $_.ProcessName }")
    }
}
#[cfg(windows)]
pub fn powershell(script: &str) -> Result<String> {
    command(
        "powershell.exe",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!(
                "$ErrorActionPreference='Stop'; [Console]::OutputEncoding=[System.Text.UTF8Encoding]::new(); {script}"
            ),
        ],
    )
}
#[cfg(windows)]
pub fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

pub fn executable(program: &str) -> PathBuf {
    #[cfg(windows)]
    {
        if Path::new(program).extension().is_none()
            && let Some(paths) = std::env::var_os("PATH")
        {
            for dir in std::env::split_paths(&paths) {
                for ext in ["exe", "cmd", "bat"] {
                    let candidate = dir.join(program).with_extension(ext);
                    if candidate.is_file() {
                        return candidate;
                    }
                }
            }
        }
    }
    PathBuf::from(program)
}

pub fn cache_dir(name: &str) -> PathBuf {
    if cfg!(windows) {
        home().join("AppData/Local").join(name)
    } else if cfg!(target_os = "macos") {
        home().join("Library/Caches").join(name)
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".cache"))
            .join(name)
    }
}

pub fn volume(id: file_id::FileId) -> u64 {
    match id {
        file_id::FileId::Inode { device_id, .. } => device_id,
        file_id::FileId::LowRes {
            volume_serial_number,
            ..
        } => u64::from(volume_serial_number),
        file_id::FileId::HighRes {
            volume_serial_number,
            ..
        } => volume_serial_number,
    }
}
