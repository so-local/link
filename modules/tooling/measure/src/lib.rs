//! The measure module (`lnk-measure`): what each agent and this machine
//! use, measured from outside them, around the programs as the sandbox
//! is, never part of them. `lnk measure` shows it; the watcher (`lnk
//! measure start`) keeps it, and sends it on as OTLP where you say.

pub mod capacity;
pub mod checks;
pub mod cli;
pub mod export;
pub mod gpu;
pub mod job;
#[cfg(not(target_os = "macos"))]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod mac;
pub mod printed;
pub mod sample;
pub mod settings;
pub mod store;
pub mod tail;
pub mod view;
pub mod watch;

/// The name the watcher's service went by before it named its home: this
/// home's only where its file is in this home ([`label_of`]).
pub const OLD_LABEL: &str = "link.measure";
pub const OLD_UNIT: &str = "lnk-measure.service";

/// The launchd label of `home`'s watcher. launchd and systemd keep one
/// service per name for each user, so each home of a user has its own.
pub fn label_of(home: &std::path::Path) -> String {
    format!("{OLD_LABEL}.{}", home_key(home))
}

/// The systemd unit of `home`'s watcher.
pub fn unit_of(home: &std::path::Path) -> String {
    format!("lnk-measure-{}.service", home_key(home))
}

/// `home` as a name: however it's reached, the same one.
fn home_key(home: &std::path::Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let home = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
    let mut key = link_digest::sha256(home.as_os_str().as_bytes());
    key.truncate(KEY_LENGTH);
    key
}

/// Hex digits of a home's digest in its service's name.
const KEY_LENGTH: usize = 16;

/// A problem the watcher meets and goes on after, logged as a daemon
/// does (`link_format::daemon`): `doing` is what it was doing.
pub(crate) fn problem(doing: &str, err: &dyn std::fmt::Display) {
    link_format::daemon::warn!("{doing}: {err:#}");
}

#[cfg(test)]
link_test_kit::program!(system);
