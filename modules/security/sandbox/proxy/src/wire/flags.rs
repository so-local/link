//! The flags in Link's settings that stop every wire, or pause one.

use super::*;

pub(super) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `sandbox/` in Link's settings, created (only the user's).
pub fn dir() -> anyhow::Result<PathBuf> {
    let dir = link_sandbox_client::wire_dir()?;
    link_files::create_private_dir(&dir)?;
    Ok(dir)
}

pub(super) fn stop_file(dir: &Path) -> PathBuf {
    link_sandbox_client::stop_file(dir)
}

pub(super) fn pause_file(dir: &Path, sandbox: &str) -> PathBuf {
    link_sandbox_client::pause_file(dir, sandbox)
}

/// Pauses sandbox `sandbox`'s wire in `dir`, for `reason`: it refuses
/// every new call, and ends those in progress within a second, until
/// [`resume`].
pub fn pause(dir: &Path, sandbox: &str, reason: &str) -> anyhow::Result<()> {
    link_sandbox_client::check_name(sandbox)?;
    link_files::create_private_dir(&dir.join("paused"))?;
    let reason = link_format::clean(reason);
    link_files::write_private(&pause_file(dir, sandbox), format!("{reason}\n").as_bytes())?;
    Ok(())
}

/// Opens sandbox `sandbox`'s paused wire in `dir` again; whether it was.
pub fn resume(dir: &Path, sandbox: &str) -> anyhow::Result<bool> {
    match std::fs::remove_file(pause_file(dir, sandbox)) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}

/// The sandboxes whose wires are paused in `dir`, and why.
pub fn paused(dir: &Path) -> Vec<(String, String)> {
    link_sandbox_client::Wires::read(dir)
        .map(|w| w.paused.into_iter().map(|p| (p.sandbox, p.why)).collect())
        .unwrap_or_default()
}

pub(super) fn log_file(dir: &Path) -> PathBuf {
    dir.join("wire.log")
}

/// Stops every wire in `dir` (`on`: false starts them again). Each
/// refuses every new call, and ends those in progress within a second.
pub fn stop(dir: &Path, on: bool) -> anyhow::Result<()> {
    let flag = stop_file(dir);
    if on {
        link_files::write_private(&flag, b"stopped by lnk sandbox stop\n")?;
    } else if flag.exists() {
        std::fs::remove_file(&flag)?;
    }
    Ok(())
}

/// Whether the wires in `dir` are stopped.
pub fn is_stopped(dir: &Path) -> bool {
    stop_file(dir).exists()
}
