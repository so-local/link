//! A time, as the agent's clock reads it, for a model.

use super::*;

/// `secs` as the agent's clock reads it, with its offset:
/// `2026-10-06T20:00:00+02:00`.
pub(super) fn link_time(secs: u64) -> String {
    let offset = local_offset(secs);
    let wall = (secs as i64 + offset).max(0) as u64;
    let days = (wall / 86_400) as i64;
    let rest = wall % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}{}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60,
        offset_text(offset)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A time as the agent's clock reads it, with its offset, which the
    /// zone's own reading agrees with.
    #[test]
    fn times_are_written_on_the_agents_clock() {
        let written = link_time(1_790_690_587);
        let read = jiff::Timestamp::strptime("%Y-%m-%dT%H:%M:%S%:z", &written)
            .unwrap_or_else(|err| panic!("{written}: {err}"));
        assert_eq!(read.as_second(), 1_790_690_587);
    }
}
