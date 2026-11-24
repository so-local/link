//! Background-service process management.

pub mod cli;
mod service;
pub use service::*;
pub mod peer;
pub mod protocol;
pub mod system;

#[cfg(target_os = "linux")]
mod guest;
#[cfg(target_os = "linux")]
pub use guest::guest_service_entry;
#[cfg(not(target_os = "linux"))]
pub fn guest_service_entry(_given: cli::GuestEntry) -> anyhow::Result<()> {
    anyhow::bail!("guest services require Linux")
}

/// When process `pid` started, in clock ticks since the machine did, as
/// `/proc/<pid>/stat` says (Linux): with its number, it names one
/// process, though numbers come back. None where it can't be read.
pub fn started(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // After its name, which may hold anything but ends at the last `)`:
    // its state, then the 19th field on, its start.
    let (_, rest) = stat.rsplit_once(')')?;
    rest.split_whitespace().nth(19)?.parse().ok()
}

#[cfg(test)]
link_test_kit::program!(system);
