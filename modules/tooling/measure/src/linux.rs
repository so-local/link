//! What Linux says of processes, control groups and the machine: `/proc`
//! and `/sys/fs/cgroup`, read as this user.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::sample::Machine;
use link_measure_contract::client::process::ticks;
pub use link_measure_contract::client::process::{alive, children, process};

fn read(path: impl AsRef<Path>) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// When this machine started, in seconds since 1970.
fn booted() -> Option<u64> {
    static BOOTED: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
    *BOOTED.get_or_init(|| {
        read("/proc/stat")
            .lines()
            .find_map(|l| l.strip_prefix("btime ")?.trim().parse().ok())
    })
}

/// Each network's sockets that listen on TCP, by inode, with their
/// ports: a network's table read once a look, however many processes
/// share it.
#[derive(Default)]
pub struct Nets(BTreeMap<String, BTreeMap<String, u16>>);

impl Nets {
    /// The listening sockets of the network process `pid` is in.
    fn of(&mut self, pid: &str) -> Option<&BTreeMap<String, u16>> {
        let net = std::fs::read_link(format!("/proc/{pid}/ns/net")).ok()?;
        Some(
            self.0
                .entry(net.display().to_string())
                .or_insert_with(|| listening(&format!("/proc/{pid}/net"))),
        )
    }
}

/// The TCP ports process `pid` listens on, in its own network (a
/// sandbox's is its own).
pub fn ports(pid: u32, nets: &mut Nets) -> BTreeSet<u16> {
    let mut ports = BTreeSet::new();
    let Some(listening) = nets.of(&pid.to_string()) else {
        return ports;
    };
    // A network with nothing listening: no descriptor to look through.
    if listening.is_empty() {
        return ports;
    }
    let Ok(fds) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
        return ports;
    };
    for fd in fds.flatten() {
        let Ok(target) = std::fs::read_link(fd.path()) else {
            continue;
        };
        let target = target.display().to_string();
        if let Some(inode) = target
            .strip_prefix("socket:[")
            .and_then(|t| t.strip_suffix(']'))
            && let Some(port) = listening.get(inode)
        {
            ports.insert(*port);
        }
    }
    ports
}

/// The sockets listening on TCP in the network `/proc/.../net` shows, by
/// inode, with their ports.
fn listening(net: &str) -> BTreeMap<String, u16> {
    let mut found = BTreeMap::new();
    for table in ["tcp", "tcp6"] {
        for line in read(format!("{net}/{table}")).lines().skip(1) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() > 9 && fields[3] == "0A" {
                let port = fields[1]
                    .rsplit_once(':')
                    .and_then(|(_, p)| u16::from_str_radix(p, 16).ok());
                if let Some(port) = port {
                    found.insert(fields[9].to_string(), port);
                }
            }
        }
    }
    found
}

/// What process `pid` uses of GPUs: every engine's busy time since it
/// started, in seconds, and its memory on them, by the GPUs it has open
/// ([`crate::gpu::process`]).
pub fn gpu(pid: u32) -> Option<(f64, u64)> {
    // A machine without a GPU's device: no descriptor to look through.
    if !Path::new("/dev/dri").exists() {
        return None;
    }
    crate::gpu::process(Path::new(&format!("/proc/{pid}")))
}

/// The whole machine now.
pub fn machine(home: &Path, nets: &mut Nets) -> Machine {
    let info: BTreeMap<String, u64> = read("/proc/meminfo")
        .lines()
        .filter_map(|l| {
            let (name, rest) = l.split_once(':')?;
            let kb = rest.split_whitespace().next()?.parse::<u64>().ok()?;
            Some((name.to_string(), kb * 1024))
        })
        .collect();
    let memory = info
        .get("MemTotal")
        .zip(info.get("MemAvailable"))
        .map(|(total, available)| total.saturating_sub(*available));
    let stat = read("/proc/stat");
    let cpu = stat.lines().next().map(|line| {
        line.split_whitespace()
            .skip(1)
            .take(8)
            .enumerate()
            // Idle and waiting for I/O aren't used, nor what another
            // machine on the same host took (steal).
            .filter(|(i, _)| ![3, 4, 7].contains(i))
            .filter_map(|(_, v)| v.parse::<u64>().ok())
            .sum::<u64>() as f64
            / ticks()
    });
    let processes = std::fs::read_dir("/proc").map_or(0, |entries| {
        entries
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .bytes()
                    .all(|b| b.is_ascii_digit())
            })
            .count() as u64
    });
    let mut disks = BTreeMap::new();
    for line in read("/proc/diskstats").lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        // Disks only: a partition's are its disk's too, and a volume's
        // (LVM, RAID, a loop device) the disks' beneath it.
        if fields.len() > 9
            && Path::new("/sys/block")
                .join(fields[2])
                .join("device")
                .exists()
        {
            disks.insert(
                fields[2].to_string(),
                (
                    fields[5].parse::<u64>().unwrap_or(0) * 512,
                    fields[9].parse::<u64>().unwrap_or(0) * 512,
                ),
            );
        }
    }
    let mut networks = BTreeMap::new();
    for line in read("/proc/net/dev").lines().skip(2) {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        // Interfaces on a device only: a bridge's or a container's
        // traffic is a device's too.
        let name = name.trim();
        if !Path::new("/sys/class/net")
            .join(name)
            .join("device")
            .exists()
        {
            continue;
        }
        let fields: Vec<u64> = rest
            .split_whitespace()
            .filter_map(|f| f.parse().ok())
            .collect();
        if fields.len() > 8 {
            networks.insert(name.to_string(), (fields[0], fields[8]));
        }
    }
    Machine {
        memory,
        cpu,
        processes: Some(processes),
        ports: nets
            .of("self")
            .map(|l| l.values().collect::<BTreeSet<_>>().len() as u64),
        disk_used: crate::sample::filesystem_used(home),
        booted: booted().map(|s| s * 1_000_000_000),
        disks: Some(disks),
        networks: Some(networks),
        gpu: crate::gpu::machine(crate::gpu::nvidia_smi(), Path::new("/sys/class/drm")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listening_port_is_found() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut nets = Nets::default();
        assert!(ports(std::process::id(), &mut nets).contains(&port));
        // Its network's table, read once.
        assert_eq!(nets.0.len(), 1);
    }

    #[test]
    fn the_machine_is_read() {
        let machine = machine(Path::new("/"), &mut Nets::default());
        assert!(machine.memory.is_some_and(|m| m > 0));
        assert!(machine.processes.is_some_and(|p| p > 0));
        assert!(machine.ports.is_some());
    }
}
