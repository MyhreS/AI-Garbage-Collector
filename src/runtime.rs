use anyhow::{Context, Result, bail};
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn lock(dir: &Path) -> Result<File> {
    fs::create_dir_all(dir)?;
    let f = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("lock"))?;
    let started = Instant::now();
    loop {
        match f.try_lock_exclusive() {
            Ok(()) => break,
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    && started.elapsed() < Duration::from_secs(3) =>
            {
                thread::sleep(Duration::from_millis(50))
            }
            Err(e) => {
                return Err(e).context(
                    "another aigc command is running or locking failed; retry when it finishes",
                );
            }
        }
    }
    Ok(f)
}
pub fn command(program: &str, args: &[&str]) -> Result<String> {
    command_at(program, args, None, 60)
}
pub fn command_at(
    program: &str,
    args: &[&str],
    cwd: Option<&Path>,
    timeout: u64,
) -> Result<String> {
    let mut c = Command::new(program);
    c.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    c.env("GIT_TERMINAL_PROMPT", "0").env("LC_ALL", "C");
    if let Some(p) = cwd {
        c.current_dir(p);
    }
    let mut child = c.spawn().with_context(|| format!("cannot run {program}"))?;
    let out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    // Drain both pipes concurrently. Oversized or incomplete output fails closed.
    let a = thread::spawn(move || read_limited(out));
    let b = thread::spawn(move || read_limited(err));
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if start.elapsed() > Duration::from_secs(timeout) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("{program} timed out after {timeout}s");
        }
        thread::sleep(Duration::from_millis(30));
    };
    let out = a
        .join()
        .map_err(|_| anyhow::anyhow!("stdout reader failed"))??;
    let err = b
        .join()
        .map_err(|_| anyhow::anyhow!("stderr reader failed"))??;
    if !status.success() {
        bail!("{program} failed: {}", String::from_utf8_lossy(&err).trim());
    }
    String::from_utf8(out).context("command output was not UTF-8")
}
pub fn git(path: &Path, args: &[&str]) -> Result<String> {
    command_at("git", args, Some(path), 30)
}
pub fn canonical(p: &Path) -> Result<PathBuf> {
    let c = fs::canonicalize(p)?;
    if p != c {
        bail!(
            "path must be canonical and must not traverse symlinks: {}",
            p.display()
        );
    }
    Ok(c)
}
pub fn size_label(n: u64) -> String {
    format!("{:.1} GiB", n as f64 / 1073741824.0)
}
pub fn disk(path: &Path) -> Result<(u64, u64)> {
    Ok((fs2::total_space(path)?, fs2::available_space(path)?))
}

fn read_limited(mut reader: impl Read) -> std::io::Result<Vec<u8>> {
    let mut data = Vec::new();
    let mut buffer = [0; 8192];
    let mut exceeded = false;
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        if data.len() + n > 16 * 1024 * 1024 {
            exceeded = true;
        } else if !exceeded {
            data.extend_from_slice(&buffer[..n]);
        }
    }
    if exceeded {
        return Err(std::io::Error::other(
            "tool output exceeded 16 MiB; inventory is incomplete",
        ));
    }
    Ok(data)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oversized_output_fails_closed() {
        assert!(read_limited(std::io::repeat(0).take(16 * 1024 * 1024 + 1)).is_err());
        assert_eq!(read_limited(&b"small"[..]).unwrap(), b"small");
    }
}
