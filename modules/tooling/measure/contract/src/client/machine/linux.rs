//! Linux host capacity, affinity and the enclosing cgroup v2 limits.

use std::{collections::BTreeSet, path::Path};

use super::{Cpu, Memory};

fn read(path: impl AsRef<Path>) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn number(path: impl AsRef<Path>) -> Option<u64> {
    read(path)?.trim().parse().ok()
}

fn positive(text: &str) -> Option<f64> {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite() && *n > 0.0)
}

fn cpu_list(text: &str) -> Option<u64> {
    let mut cpus = BTreeSet::new();
    for part in text.trim().split(',') {
        let (from, to) = match part.split_once('-') {
            Some((from, to)) => (from.parse::<u32>().ok()?, to.parse::<u32>().ok()?),
            None => {
                let n = part.parse::<u32>().ok()?;
                (n, n)
            }
        };
        if from > to || to - from > u16::MAX.into() {
            return None;
        }
        cpus.extend(from..=to);
    }
    Some(cpus.len() as u64).filter(|n| *n > 0)
}

pub(super) fn capture(
    proc: &Path,
    sys: &Path,
    affinity: Option<u64>,
    parallelism: Option<u64>,
) -> (Cpu, Memory) {
    let info = read(proc.join("cpuinfo")).unwrap_or_default();
    let field = |name: &str| {
        info.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim() == name).then(|| value.trim())
        })
    };
    let logical = info
        .lines()
        .filter(|line| {
            line.split_once(':')
                .is_some_and(|(key, _)| key.trim() == "processor")
        })
        .count() as u64;
    let physical: BTreeSet<_> = info
        .split("\n\n")
        .filter_map(|block| {
            let get = |name: &str| {
                block.lines().find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    (key.trim() == name).then(|| value.trim().to_owned())
                })
            };
            Some((get("physical id")?, get("core id")?))
        })
        .collect();
    let host = read(proc.join("meminfo")).unwrap_or_default();
    let bytes = |name: &str| {
        host.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key == name)
                .then_some(value)?
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()?
                .checked_mul(1024)
        })
    };
    let mut memory = Memory {
        machine_total_bytes: bytes("MemTotal"),
        machine_available_bytes: bytes("MemAvailable"),
        ..Memory::default()
    };
    let mut available = memory.machine_available_bytes;
    let mut availability_known = true;
    let mut cpu = Cpu {
        model: field("model name")
            .or_else(|| field("Hardware"))
            .map(|s| s.chars().filter(char::is_ascii).take(256).collect()),
        logical_cores: (logical > 0).then_some(logical),
        physical_cores: (!physical.is_empty()).then_some(physical.len() as u64),
        affinity_cores: affinity,
        parallelism_cores: parallelism,
        available_cores: affinity
            .or((logical > 0).then_some(logical))
            .map(|n| n as f64),
        base_hz: number(sys.join("devices/system/cpu/cpu0/cpufreq/base_frequency"))
            .and_then(|n| n.checked_mul(1000)),
        observed_hz: field("cpu MHz")
            .and_then(positive)
            .map(|n| (n * 1e6) as u64),
        ..Cpu::default()
    };
    if let Some(relative) = read(proc.join("self/cgroup")).and_then(|text| {
        text.lines()
            .find_map(|line| line.strip_prefix("0::").map(str::to_owned))
    }) {
        let root = sys.join("fs/cgroup");
        let relative = Path::new(relative.trim().trim_start_matches('/'));
        {
            // A namespaced mount can already be rooted at this execution's
            // enclosing group, while /proc spells a path outside that mount.
            // Its root still supplies a real ancestor's allocation bounds.
            let selected = root.join(relative);
            let mut group = if !relative
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
                && selected.is_dir()
            {
                selected
            } else {
                root.clone()
            };
            loop {
                if let Some(quota) = read(group.join("cpu.max"))
                    .and_then(|text| {
                        let mut words = text.split_whitespace();
                        Some(positive(words.next()?)? / positive(words.next()?)?)
                    })
                    .filter(|n| n.is_finite() && *n > 0.0)
                {
                    cpu.quota_cores = Some(cpu.quota_cores.map_or(quota, |old| old.min(quota)));
                }
                if let Some(count) =
                    read(group.join("cpuset.cpus.effective")).and_then(|text| cpu_list(&text))
                {
                    cpu.available_cores = Some(
                        cpu.available_cores
                            .map_or(count as f64, |old| old.min(count as f64)),
                    );
                }
                if let Some(limit) = number(group.join("memory.max")) {
                    memory.limit_bytes =
                        Some(memory.limit_bytes.map_or(limit, |old| old.min(limit)));
                    if let Some(current) = number(group.join("memory.current")) {
                        let remaining = limit.saturating_sub(current);
                        available = Some(available.map_or(remaining, |old| old.min(remaining)));
                    } else {
                        availability_known = false;
                    }
                }
                if memory.used_bytes.is_none() {
                    memory.used_bytes = number(group.join("memory.current"));
                }
                if group == root || !group.pop() {
                    break;
                }
            }
        }
    }
    if let Some(quota) = cpu.quota_cores {
        cpu.available_cores = Some(cpu.available_cores.map_or(quota, |old| old.min(quota)));
    }
    if let Some(parallelism) = parallelism {
        cpu.available_cores = Some(
            cpu.available_cores
                .map_or(parallelism as f64, |old| old.min(parallelism as f64)),
        );
    }
    if memory.used_bytes.is_none() {
        memory.used_bytes = memory
            .machine_total_bytes
            .zip(memory.machine_available_bytes)
            .map(|(total, free)| total.saturating_sub(free));
    }
    memory.available_bytes = availability_known.then_some(available).flatten();
    (cpu, memory)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ancestor_limits_affinity_and_cpuset_bound_host_capacity_without_rounding_quotas() {
        let dir = link_test_kit::TempDir::new("capacity-limits");
        let proc = dir.join("proc");
        let sys = dir.join("sys");
        std::fs::create_dir_all(proc.join("self")).unwrap();
        let group = sys.join("fs/cgroup/work");
        std::fs::create_dir_all(&group).unwrap();
        std::fs::write(proc.join("cpuinfo"), "processor : 0\nphysical id : 0\ncore id : 0\nmodel name : fixture\ncpu MHz : 2400\n\nprocessor : 1\nphysical id : 0\ncore id : 0\n").unwrap();
        std::fs::write(
            proc.join("meminfo"),
            "MemTotal: 10000 kB\nMemAvailable: 8000 kB\n",
        )
        .unwrap();
        std::fs::write(proc.join("self/cgroup"), "0::/work\n").unwrap();
        for (path, text) in [
            ("cpu.max", "50000 100000"),
            ("memory.max", "4096"),
            ("memory.current", "1024"),
        ] {
            std::fs::write(group.parent().unwrap().join(path), text).unwrap();
        }
        std::fs::write(group.join("cpu.max"), "200000 100000").unwrap();
        std::fs::write(group.join("cpuset.cpus.effective"), "0-1").unwrap();
        std::fs::write(group.join("memory.max"), "8192").unwrap();
        std::fs::write(group.join("memory.current"), "2048").unwrap();
        let (cpu, memory) = capture(&proc, &sys, Some(2), Some(2));
        assert_eq!(cpu.logical_cores, Some(2));
        assert_eq!(cpu.physical_cores, Some(1));
        assert_eq!(cpu.available_cores, Some(0.5));
        assert_eq!(cpu.observed_hz, Some(2_400_000_000));
        assert_eq!(memory.limit_bytes, Some(4096));
        assert_eq!(memory.available_bytes, Some(3072));
        std::fs::remove_file(group.parent().unwrap().join("memory.current")).unwrap();
        assert_eq!(
            capture(&proc, &sys, Some(2), Some(2)).1.available_bytes,
            None
        );
    }
}
