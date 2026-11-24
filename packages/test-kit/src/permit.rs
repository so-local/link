//! A permit to start what takes much of the machine at once, such as a
//! confined program that starts a sandbox and dozens of programs in
//! turn. Tests run side by side, so without one, three such tests on a
//! 4-CPU machine start together and each outlasts its waits; with one, a
//! test waits for its permit before any deadline of its own begins, and
//! each start takes the time it takes alone.
//!
//! The permits are half the machine's CPUs, at least one, shared by every
//! test on the machine: each is a [`HeldNumber`](crate::HeldNumber) of the
//! kind `permit`, a lock the kernel lets go however its holder ends, so a
//! system test's own process and another test program share them alike.
//! A test holds its permit until it ends.

use std::time::Duration;

/// How many permits the machine has: half its CPUs, at least one.
pub fn total() -> usize {
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
    (cpus / 2).max(1)
}

/// Held while a test runs what its permits are for; dropping it frees
/// them for the next test.
#[must_use = "a permit holds only while it lives"]
pub struct Permit {
    held: Vec<crate::HeldNumber>,
}

impl Permit {
    /// How many permits it holds.
    pub fn held(&self) -> usize {
        self.held.len()
    }
}

/// Waits for `weight` permits, one for each confined program the test
/// starts at once, at most every permit there is. All are taken at once
/// or none, so two tests each holding some never wait on each other.
pub fn confined(weight: usize) -> Permit {
    confined_of("permit", weight)
}

fn confined_of(kind: &str, weight: usize) -> Permit {
    let total = total();
    let want = weight.clamp(1, total);
    let mut pause = Duration::from_millis(5);
    loop {
        let mut held = Vec::with_capacity(want);
        for slot in 0..total as u32 {
            if let Some(number) = crate::HeldNumber::hold(kind, slot) {
                held.push(number);
                if held.len() == want {
                    return Permit { held };
                }
            }
        }
        drop(held);
        // Nothing to poll but the locks themselves: another test frees
        // its permit by ending, and says so to no one.
        #[expect(
            clippy::disallowed_methods,
            reason = "waits for a permit another process frees, which signals nothing"
        )]
        std::thread::sleep(pause);
        pause = (pause * 2).min(Duration::from_millis(200));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No more permits are out at once than there are, whatever each test
    /// asks for, and every one comes back.
    #[test]
    fn permits_never_run_past_the_machines_share() {
        let kind = format!("permit-kit-{}-", std::process::id());
        let total = total();
        let out = std::sync::atomic::AtomicUsize::new(0);
        let most = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|s| {
            for weight in [1, 2, 1, total + 3, 1, 2] {
                let (out, most, kind) = (&out, &most, &kind);
                s.spawn(move || {
                    let permit = confined_of(kind, weight);
                    let now = out.fetch_add(permit.held(), std::sync::atomic::Ordering::SeqCst)
                        + permit.held();
                    most.fetch_max(now, std::sync::atomic::Ordering::SeqCst);
                    std::thread::yield_now();
                    out.fetch_sub(permit.held(), std::sync::atomic::Ordering::SeqCst);
                });
            }
        });
        assert!(most.into_inner() <= total);
        let all = confined_of(&kind, total);
        assert_eq!(all.held(), total);
        drop(all);
        for slot in 0..total as u32 {
            let _ = std::fs::remove_file(format!("/tmp/lnk-test-{kind}{slot}.lock"));
        }
    }
}
