//! The receiving bridge's per-teammate UTC-day bound, wherever its
//! counter is kept. Storage must serialize changes to a receiver/sender pair.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Received {
    pub day: u64,
    pub taken: u32,
}

impl Received {
    /// Takes a receipt, or returns one for a message the harness never
    /// took. Rebinding never resets the counter; only a new UTC day does.
    pub fn receive(&mut self, day: u64, cap: Option<u32>, take: bool) -> anyhow::Result<()> {
        if self.day != day {
            *self = Self { day, taken: 0 };
        }
        if take {
            let cap = cap.ok_or_else(|| {
                anyhow::anyhow!(
                    "its spec sets no [tool.scheduler] per_day: a teammate's message isn't taken"
                )
            })?;
            anyhow::ensure!(
                self.taken < cap,
                "a teammate has sent {cap} messages today, as many as it takes: not taken"
            );
            self.taken += 1;
        } else {
            self.taken = self.taken.saturating_sub(1);
        }
        Ok(())
    }
}
