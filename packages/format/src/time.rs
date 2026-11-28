//! Times and durations as people read them, with no calendar dependency:
//! UTC, a day being 86,400 seconds.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// `2026-10-23T14:05:11Z`.
pub fn rfc3339(t: SystemTime) -> String {
    let s = seconds(t);
    let (y, m, d) = civil(s.div_euclid(86_400));
    let day = s.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        day / 3600,
        day % 3600 / 60,
        day % 60
    )
}

/// `2026-10-23T14:05:11.042Z`: [`rfc3339`] to the millisecond, so lines
/// written in one second keep their order.
pub fn rfc3339_millis(t: SystemTime) -> String {
    let millis = t
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.subsec_millis());
    format!("{}.{millis:03}Z", rfc3339(t).trim_end_matches('Z'))
}

/// The moment `text` names, as [`rfc3339`] or [`rfc3339_millis`] write
/// it, in UTC (`Z`); `None` for anything else.
pub fn parse_rfc3339(text: &str) -> Option<SystemTime> {
    let text = text.strip_suffix('Z')?;
    let (date, time) = text.split_once('T')?;
    let mut date = date.splitn(3, '-').map(str::parse::<i64>);
    let (y, m, d) = (date.next()?.ok()?, date.next()?.ok()?, date.next()?.ok()?);
    let (time, fraction) = match time.split_once('.') {
        Some((time, fraction)) => (time, Some(fraction)),
        None => (time, None),
    };
    let mut clock = time.splitn(3, ':').map(str::parse::<i64>);
    let (h, min, s) = (
        clock.next()?.ok()?,
        clock.next()?.ok()?,
        clock.next()?.ok()?,
    );
    let fine = (1..=12).contains(&m)
        && (1..=31).contains(&d)
        && (0..24).contains(&h)
        && (0..60).contains(&min)
        && (0..61).contains(&s);
    if !fine {
        return None;
    }
    let nanos = match fraction {
        None => 0,
        Some(f) if (1..=9).contains(&f.len()) && f.bytes().all(|b| b.is_ascii_digit()) => {
            format!("{f:0<9}").parse::<u32>().ok()?
        }
        Some(_) => return None,
    };
    let seconds = days(y, m, d) * 86_400 + h * 3600 + min * 60 + s;
    let whole = Duration::from_secs(u64::try_from(seconds).ok()?);
    Some(UNIX_EPOCH + whole + Duration::from_nanos(u64::from(nanos)))
}

/// `14:05:11` on this machine's clock, today; `2026-10-22 14:05:11` on
/// another day than `now`'s.
pub fn local_clock(t: SystemTime, now: SystemTime) -> String {
    let (day, clock) = local(t);
    match day == local(now).0 {
        true => clock,
        false => format!("{day} {clock}"),
    }
}

/// The day and time of day `t` is on this machine's clock, its zone as
/// the C library has it (`TZ`, else `/etc/localtime`).
fn local(t: SystemTime) -> (String, String) {
    // `localtime_r` says the type: musl's `time_t` is changing width.
    let at = seconds(t) as _;
    // SAFETY: `localtime_r` writes only the `tm` it's given.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&at, &mut tm) }.is_null() {
        let utc = rfc3339(t);
        let (day, clock) = utc
            .trim_end_matches('Z')
            .split_once('T')
            .unwrap_or_default();
        return (day.to_string(), clock.to_string());
    }
    (
        format!(
            "{:04}-{:02}-{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday
        ),
        format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec),
    )
}

/// `2026-10-23`.
pub fn date(t: SystemTime) -> String {
    let (y, m, d) = civil(seconds(t).div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}

/// `just now`, `5 min ago`, `3 h ago`, `3 days ago`; else its date. A
/// time ahead, as `in 5 min`.
pub fn ago(t: SystemTime, now: SystemTime) -> String {
    let (gap, ahead) = match now.duration_since(t) {
        Ok(d) => (d, false),
        Err(e) => (e.duration(), true),
    };
    let s = gap.as_secs();
    let said = match s {
        0..60 => return "just now".into(),
        60..3600 => format!("{} min", s / 60),
        3600..86_400 => format!("{} h", s / 3600),
        86_400..172_800 => "1 day".into(),
        172_800..604_800 => format!("{} days", s / 86_400),
        _ => return date(t),
    };
    match ahead {
        true => format!("in {said}"),
        false => format!("{said} ago"),
    }
}

/// `42s`, `3m 20s`, `2h 5m`, `4d`.
pub fn elapsed(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m {}s", s / 60, s % 60),
        3600..86_400 => format!("{}h {}m", s / 3600, s % 3600 / 60),
        _ => format!("{}d", s / 86_400),
    }
}

/// `0:42`, `12:05`: a spinner's clock.
pub fn clock(d: Duration) -> String {
    let s = d.as_secs();
    format!("{}:{:02}", s / 60, s % 60)
}

fn seconds(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    }
}

/// How many days after 1970-01-01 year `y`, month `m`, day `d` is
/// ([`civil`] the other way).
fn days(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The year, month and day `days` after 1970-01-01 (Howard Hinnant's).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(s)
    }

    #[test]
    fn times_read_as_people_read_them() {
        assert_eq!(rfc3339(at(1_792_764_311)), "2026-10-23T14:05:11Z");
        assert_eq!(date(at(951_782_400)), "2000-02-29");
        let now = at(1_792_764_311);
        assert_eq!(ago(at(1_792_764_311 - 300), now), "5 min ago");
        assert_eq!(ago(at(1_792_764_311 - 3 * 86_400), now), "3 days ago");
        assert_eq!(ago(at(1_792_764_311 - 30 * 86_400), now), "2026-09-23");
        assert_eq!(ago(at(1_792_764_311 + 300), now), "in 5 min");
    }

    #[test]
    fn a_written_moment_reads_back() {
        let t = at(1_792_764_311) + Duration::from_millis(42);
        assert_eq!(rfc3339_millis(t), "2026-10-23T14:05:11.042Z");
        assert_eq!(parse_rfc3339(&rfc3339_millis(t)), Some(t));
        assert_eq!(parse_rfc3339("2000-02-29T00:00:00Z"), Some(at(951_782_400)));
        for wrong in [
            "2026-10-23T14:05:11",
            "2026-13-01T00:00:00Z",
            "yesterday",
            "2026-10-23T14:05:11.Z",
        ] {
            assert_eq!(parse_rfc3339(wrong), None, "{wrong}");
        }
    }

    #[test]
    fn a_local_clock_names_its_day_only_when_not_today() {
        let t = at(1_792_764_311);
        let today = local_clock(t, t);
        assert_eq!(today.len(), "14:05:11".len(), "{today}");
        let later = local_clock(t, t + Duration::from_secs(3 * 86_400));
        assert!(
            later.ends_with(&today) && later.len() == "2026-10-23 14:05:11".len(),
            "{later}"
        );
    }

    #[test]
    fn durations_are_short() {
        assert_eq!(elapsed(Duration::from_secs(200)), "3m 20s");
        assert_eq!(elapsed(Duration::from_secs(7_500)), "2h 5m");
        assert_eq!(elapsed(Duration::from_secs(4 * 86_400)), "4d");
        assert_eq!(clock(Duration::from_secs(42)), "0:42");
    }
}
