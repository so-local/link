//! Timing validation and recurrence shared by import and execution.
use anyhow::ensure;
use link_agent_state_protocol::schedules::{Schedule, When};
use link_agent_state_protocol::schedules::{WhenAt, WhenCron, WhenEvery};

pub fn check(schedule: &Schedule) -> anyhow::Result<()> {
    schedule.check()?;
    match &schedule.when {
        When::Cron(WhenCron { cron, zone }) => {
            crate::cron::Cron::in_zone(cron, zone)?;
        }
        When::Every(WhenEvery { seconds, anchor }) => {
            ensure!(*seconds > 0, "interval must be positive");
            jiff::Timestamp::from_second(i64::try_from(*anchor)?)?;
        }
        When::At(WhenAt { at }) => {
            jiff::Timestamp::from_second(i64::try_from(*at)?)?;
        }
    }
    Ok(())
}

pub fn next(when: &When, at: u64) -> anyhow::Result<Option<u64>> {
    Ok(match when {
        When::At(WhenAt { at: instant }) => (*instant > at).then_some(*instant),
        When::Cron(WhenCron { cron, zone }) => {
            crate::cron::Cron::in_zone(cron, zone)?.next_after(at)
        }
        When::Every(WhenEvery { seconds, anchor }) => {
            ensure!(*seconds > 0, "interval must be positive");
            if at < *anchor {
                Some(*anchor)
            } else {
                at.checked_sub(*anchor)
                    .and_then(|elapsed| elapsed.checked_div(*seconds))
                    .and_then(|n| n.checked_add(1))
                    .and_then(|n| n.checked_mul(*seconds))
                    .and_then(|delta| anchor.checked_add(delta))
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn at(text: &str) -> u64 {
        text.parse::<jiff::Timestamp>().unwrap().as_second() as u64
    }
    #[test]
    fn explicit_zones_are_independent_and_keep_dst_transitions() {
        let start = at("2026-03-28T09:00:00Z");
        let paris = When::Cron(WhenCron {
            cron: "0 9 * * *".into(),
            zone: "Europe/Paris".into(),
        });
        let new_york = When::Cron(WhenCron {
            cron: "0 9 * * *".into(),
            zone: "America/New_York".into(),
        });
        assert_eq!(
            next(&paris, start).unwrap(),
            Some(at("2026-03-29T07:00:00Z"))
        );
        assert_eq!(
            next(&new_york, start).unwrap(),
            Some(at("2026-03-28T13:00:00Z"))
        );
        let skipped = When::Cron(WhenCron {
            cron: "30 2 * * *".into(),
            zone: "Europe/Paris".into(),
        });
        // Vixie cron's: a time the clocks skip fires as they skip it, once.
        assert_eq!(
            next(&skipped, at("2026-03-28T02:00:00Z")).unwrap(),
            Some(at("2026-03-29T01:00:00Z"))
        );
        assert_eq!(
            next(&skipped, at("2026-03-29T01:00:00Z")).unwrap(),
            Some(at("2026-03-30T00:30:00Z"))
        );
        let repeated = When::Cron(WhenCron {
            cron: "30 1 * * *".into(),
            zone: "America/New_York".into(),
        });
        // A time the clocks repeat fires the first time only.
        assert_eq!(
            next(&repeated, at("2026-11-01T05:00:00Z")).unwrap(),
            Some(at("2026-11-01T05:30:00Z"))
        );
        assert_eq!(
            next(&repeated, at("2026-11-01T05:30:00Z")).unwrap(),
            Some(at("2026-11-02T06:30:00Z"))
        );
        // One by the clock, every half hour, fires at each reading.
        let hourly = When::Cron(WhenCron {
            cron: "*/30 * * * *".into(),
            zone: "America/New_York".into(),
        });
        assert_eq!(
            next(&hourly, at("2026-11-01T05:30:00Z")).unwrap(),
            Some(at("2026-11-01T06:00:00Z"))
        );
        assert_eq!(
            next(&hourly, at("2026-11-01T06:00:00Z")).unwrap(),
            Some(at("2026-11-01T06:30:00Z"))
        );
    }
    #[test]
    fn interval_phase_survives_late_wakes_and_invalid_zones_do_not_become_local() {
        let every = When::Every(WhenEvery {
            seconds: 600,
            anchor: 617,
        });
        assert_eq!(next(&every, 600).unwrap(), Some(617));
        assert_eq!(next(&every, 617).unwrap(), Some(1217));
        assert_eq!(next(&every, 1818).unwrap(), Some(2417));
        assert!(
            next(
                &When::Cron(WhenCron {
                    cron: "* * * * *".into(),
                    zone: "Nowhere/Unknown".into()
                }),
                600
            )
            .is_err()
        );
    }
}
