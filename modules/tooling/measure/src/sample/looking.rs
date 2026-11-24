//! What one look reads once ([`Look`]): each proxy's traffic and each
//! GPU's processes, and the machine's counters by device ([`Counters`]).

use super::*;
use link_core::answers::Answerer;
use link_measure_contract::client::asked::{TRAFFIC, Traffic};
use link_measure_contract::client::traffic;
use std::cell::OnceCell;

/// How long a process's memory is kept before it's read again: reading
/// PSS walks every mapping it has, and holds them while it does.
pub(super) const MEMORY_EVERY: Duration = Duration::from_secs(60);

/// What one look reads once, however many units need it.
#[derive(Default)]
pub(super) struct Look {
    pub(super) nets: os::Nets,
    /// What each sandbox's proxy has carried, by its pid, as `lnk
    /// sandbox show --json` says.
    pub(super) carried: OnceCell<BTreeMap<u32, Carried>>,
    pub(super) sources: Vec<traffic::Source>,
    pub(super) answerers: Vec<Answerer>,
    /// Whether an agent's folders were walked already.
    pub(super) walked: bool,
    /// The GPU memory each process has on NVIDIA's GPUs, asked once a
    /// look, when a process's is first wanted.
    pub(super) nvidia: Option<BTreeMap<u32, u64>>,
}

impl Look {
    pub(super) fn new(published: &[traffic::Source], answerers: &[Answerer]) -> Self {
        let mut sources: Vec<traffic::Source> = Vec::new();
        for source in published {
            if source.directory.is_absolute()
                && plain(&source.provider)
                && !sources.contains(source)
            {
                sources.push(source.clone());
            }
        }
        Self {
            sources,
            answerers: answerers.to_vec(),
            ..Self::default()
        }
    }

    /// What process `pid` uses of GPUs: every engine's busy time since
    /// it started, in seconds (where its system says), and its memory on
    /// them. None with no GPU open.
    pub(super) fn gpu(&mut self, pid: u32) -> Option<(Option<f64>, u64)> {
        let nvidia = self.nvidia.get_or_insert_with(|| {
            crate::gpu::nvidia_smi()
                .map(crate::gpu::nvidia_processes)
                .unwrap_or_default()
        });
        let on_nvidia = nvidia.get(&pid).copied();
        match (os::gpu(pid), on_nvidia) {
            (None, None) => None,
            (own, nvidia) => Some((
                own.map(|(seconds, _)| seconds),
                own.map_or(0, |(_, memory)| memory)
                    .saturating_add(nvidia.unwrap_or(0)),
            )),
        }
    }

    /// What process `pid` carried, if it's a sandbox's proxy.
    pub(super) fn carried(&self, pid: u32) -> Option<Carried> {
        self.carried
            .get_or_init(|| carried(&self.sources, &self.answerers))
            .get(&pid)
            .copied()
    }
}

/// What each sandbox's proxy here has carried, by its pid, as the modules
/// that answer [`TRAFFIC`] say; none without one, or when it doesn't
/// answer.
fn carried(sources: &[traffic::Source], answerers: &[Answerer]) -> BTreeMap<u32, Carried> {
    let published = sources
        .iter()
        .flat_map(|s| traffic::read(&s.directory).traffic);
    // Providers that don't publish this contract keep their command
    // protocol. Discovery is shared with the current agent listing.
    let fallback = answerers
        .iter()
        .filter(|a| !sources.iter().any(|s| s.provider == a.module))
        .filter_map(|a| a.reply(&TRAFFIC, &(), std::process::Stdio::null()).ok())
        .flatten();
    by_pid(published.chain(fallback))
}

/// What each proxy of `traffic` carried, by its pid.
fn by_pid(traffic: impl IntoIterator<Item = Traffic>) -> BTreeMap<u32, Carried> {
    traffic
        .into_iter()
        .map(|t| {
            (
                t.pid,
                Carried {
                    up: t.up,
                    down: t.down,
                },
            )
        })
        .collect()
}

/// A sum of counters kept per device or interface, which come and go:
/// it grows by what each grew, and never falls when one goes.
#[derive(Default)]
pub(super) struct Counters {
    last: BTreeMap<String, (u64, u64)>,
    sum: Option<(u64, u64)>,
}

impl Counters {
    pub(super) fn add(&mut self, now: &BTreeMap<String, (u64, u64)>) -> (u64, u64) {
        let (mut a, mut b) = match self.sum {
            Some(sum) => sum,
            // From what all had at the first look.
            None => {
                let sum = now.values().fold((0u64, 0u64), |s, v| {
                    (s.0.saturating_add(v.0), s.1.saturating_add(v.1))
                });
                self.last = now.clone();
                self.sum = Some(sum);
                return sum;
            }
        };
        for (name, (x, y)) in now {
            // Since the last look, or since it came, or started over.
            let (lx, ly) = self.last.get(name).copied().unwrap_or((0, 0));
            a = a.saturating_add(if *x >= lx { x - lx } else { *x });
            b = b.saturating_add(if *y >= ly { y - ly } else { *y });
        }
        self.last = now.clone();
        self.sum = Some((a, b));
        (a, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn traffic_is_lazy_shared_within_a_look_and_fresh_in_the_next() {
        let dir = link_test_kit::TempDir::new("lazy-traffic");
        let pid = std::process::id();
        let sources = [traffic::Source {
            provider: "publisher".into(),
            directory: dir.to_path_buf(),
        }];
        let look = Look::new(&sources, &[]);
        let path = dir.join(format!("{pid}.json"));
        let write = |up: u64| {
            let t = Traffic {
                pid,
                sandbox: "main-link".into(),
                up,
                down: 20,
                started: link_measure_contract::client::started(pid),
            };
            link_files::write_private(&path, &serde_json::to_vec(&t).unwrap()).unwrap();
        };
        // Published after the look was made, before its first consumer.
        write(10);
        assert_eq!(look.carried(pid), Some(Carried { up: 10, down: 20 }));
        write(30);
        assert_eq!(look.carried(pid).unwrap().up, 10);
        assert_eq!(look.carried(999_999_999), None);
        assert_eq!(Look::new(&sources, &[]).carried(pid).unwrap().up, 30);
    }

    /// The machine's disk and network grow by what each device grew:
    /// one going away never makes the sum fall.
    #[test]
    fn a_device_going_away_never_takes_from_the_sum() {
        let mut counters = Counters::default();
        let of = |pairs: &[(&str, u64)]| -> BTreeMap<String, (u64, u64)> {
            pairs
                .iter()
                .map(|(n, v)| (n.to_string(), (*v, *v)))
                .collect()
        };
        assert_eq!(
            counters.add(&of(&[("eth0", 100), ("veth1", 50)])),
            (150, 150)
        );
        assert_eq!(counters.add(&of(&[("eth0", 110)])), (160, 160));
        // A new one counts from nothing; one started over, from 0.
        assert_eq!(counters.add(&of(&[("eth0", 5), ("veth2", 7)])), (172, 172));
    }

    /// What the sandbox says of each proxy is its process's.
    #[test]
    fn a_proxys_traffic_is_what_the_sandbox_says() {
        let said: Vec<Traffic> = serde_json::from_value(json!([
            {"pid": 7, "sandbox": "main-x", "up": 3, "down": 4, "started": 1},
            {"pid": 8, "sandbox": "main-y", "up": 5, "down": 6},
        ]))
        .unwrap();
        let carried = by_pid(said);
        assert_eq!(carried.get(&7), Some(&Carried { up: 3, down: 4 }));
        assert_eq!(carried[&8].down, 6);
        let look = Look {
            carried: OnceCell::from(carried),
            ..Look::default()
        };
        assert_eq!(look.carried(7).unwrap().up, 3);
        assert_eq!(look.carried(10), None);
    }
}
