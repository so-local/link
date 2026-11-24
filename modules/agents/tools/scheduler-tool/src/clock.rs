//! The clock: this process's time zone, a time's day and local hour in
//! it, and now.

/// Where a time zone's rules are on this machine, the zone database's
/// folder, on Linux and on a Mac.
const ZONES: &[&str] = &["/usr/share/zoneinfo", "/var/db/timezone/zoneinfo"];

/// The zone's name [`set_zone`] made this process's: none, this
/// machine's.
static ZONE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Makes this process's times `zone`'s: `local`, this machine's, else a
/// zone's name (`Europe/Paris`) this machine has the rules of. Called
/// first thing, before any thread starts.
pub fn set_zone(zone: &str) -> anyhow::Result<()> {
    if zone == "local" {
        return Ok(());
    }
    anyhow::ensure!(
        link_agent_spec::is_zone(zone)
            && ZONES
                .iter()
                .any(|dir| std::path::Path::new(dir).join(zone).is_file()),
        "{:?} isn't a time zone this machine knows (its zone database has no such zone)",
        link_format::clean(zone)
    );
    unsafe extern "C" {
        fn tzset();
    }
    // SAFETY: called before any thread starts, so nothing reads the
    // environment meanwhile; tzset, POSIX's, reads TZ into libc's own
    // state, which localtime_r reads after.
    unsafe {
        std::env::set_var("TZ", format!(":{zone}"));
        tzset();
    }
    let _ = ZONE.set(zone.to_string());
    Ok(())
}

/// The agent's time zone as a model reads it: its name, or this
/// machine's, with its offset from UTC at `secs` (`Europe/Paris, UTC+02:00
/// now`).
pub(crate) fn zone_said(secs: u64) -> String {
    let name = ZONE
        .get()
        .map_or("this machine's time zone", String::as_str);
    format!("{name}, UTC{} now", offset_text(local_offset(secs)))
}

/// The offset from UTC of the agent's time zone at `secs`, in seconds.
pub(crate) fn local_offset(secs: u64) -> i64 {
    #[allow(
        clippy::unnecessary_cast,
        reason = "a C long: the cast is a no-op where it's 64 bits, and widens where it's 32"
    )]
    let offset = local_tm(secs).tm_gmtoff as i64;
    offset
}

/// An offset from UTC in seconds as a time writes it: `+02:00`, `-05:30`.
pub(crate) fn offset_text(offset: i64) -> String {
    let sign = if offset < 0 { '-' } else { '+' };
    let minutes = offset.abs() / 60;
    format!("{sign}{:02}:{:02}", minutes / 60, minutes % 60)
}

/// `secs` in the agent's time zone ([`set_zone`]).
fn local_tm(secs: u64) -> libc::tm {
    let time = secs as libc::time_t;
    // SAFETY: localtime_r writes only into `tm`, which outlives the call.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&time, &mut tm) };
    tm
}

/// `hour:minute` in the agent's time zone, `days` after the day holding
/// `secs`: tomorrow is the next date, never 24 hours on (a day can be 23
/// or 25).
pub(crate) fn local_at(secs: u64, days: i32, hour: i32, minute: i32) -> u64 {
    let mut tm = local_tm(secs);
    tm.tm_mday += days;
    tm.tm_hour = hour;
    tm.tm_min = minute;
    tm.tm_sec = 0;
    tm.tm_isdst = -1;
    // SAFETY: mktime reads and writes only `tm`.
    (unsafe { libc::mktime(&mut tm) }).max(0) as u64
}

/// The day holding `secs` in the agent's time zone, as days since 1970.
pub(super) fn local_day(secs: u64) -> u64 {
    (secs as i64 + local_offset(secs)).max(0) as u64 / 86_400
}

/// Now, in seconds since 1970.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only a zone this machine has the rules of: `local` changes
    /// nothing, and a name it doesn't know is refused, never taken as
    /// UTC.
    #[test]
    fn a_zone_is_one_the_machine_knows() {
        set_zone("local").unwrap();
        for bad in ["Nowhere/At_All", "../etc/passwd", "/etc/localtime", ""] {
            assert!(set_zone(bad).is_err(), "{bad}");
        }
    }

    /// The zone [`daylight_saving_in_this_process_zone`] runs in.
    const NEW_YORK: &str = "America/New_York";

    /// Daylight saving through `Cron::parse`, in this process's own zone
    /// as [`set_zone`] leaves it, the path every job takes, as Vixie cron
    /// has it: a fixed time New York's clocks skip (2:30 on 2026-03-08,
    /// forward at 07:00 UTC) fires once as they skip it, and one they
    /// repeat (1:30 on 2026-11-01, back at 06:00 UTC) fires once, each
    /// day around them once too. The zone is the process's, so the test
    /// runs again as a child of its own with `TZ` set, as `set_zone` sets
    /// it before any thread starts.
    #[test]
    fn daylight_saving_in_this_process_zone() {
        let zone = format!(":{NEW_YORK}");
        if std::env::var_os("TZ").is_none_or(|tz| tz != zone.as_str()) {
            if !ZONES
                .iter()
                .any(|dir| std::path::Path::new(dir).join(NEW_YORK).is_file())
            {
                link_test_kit::gate::lacks(
                    "New York's zone rules",
                    "no zone database here has them",
                    None,
                );
                return;
            }
            let home = link_test_kit::TempDir::new("scheduler-zone");
            let log = home.join("fixture.log");
            let thread = std::thread::current();
            let test = thread.name().expect("libtest names its test's thread");
            let mut command = link_test_kit::process::rerun(test, &home, &log).unwrap();
            command.env("TZ", &zone);
            let child = link_test_kit::ChildGroup::spawn(&mut command).unwrap();
            let status = link_test_kit::eventually(
                "the test in New York's zone",
                std::time::Duration::from_secs(30),
                || child.try_status().unwrap(),
            );
            let said = link_test_kit::process::rerun_said(&log);
            assert!(
                status.success() && said.contains("1 passed;"),
                "{status}\n{said}"
            );
            return;
        }
        let at = |text: &str| text.parse::<jiff::Timestamp>().unwrap().as_second() as u64;
        // The zone is New York's: five hours behind in winter, four in summer.
        assert_eq!(local_offset(at("2026-01-15T12:00:00Z")), -5 * 3600);
        assert_eq!(local_offset(at("2026-07-15T12:00:00Z")), -4 * 3600);
        let firings = |expr: &str, from: &str, until: &str| {
            let cron = crate::cron::Cron::parse(expr).unwrap();
            let (mut next, until) = (at(from), at(until));
            let mut fired = Vec::new();
            while let Some(due) = cron.next_after(next).filter(|due| *due < until) {
                fired.push(due);
                next = due;
            }
            fired
        };
        assert_eq!(
            firings("30 2 * * *", "2026-03-07T00:00:00Z", "2026-03-10T00:00:00Z"),
            [
                at("2026-03-07T07:30:00Z"),
                at("2026-03-08T07:00:00Z"),
                at("2026-03-09T06:30:00Z"),
            ]
        );
        assert_eq!(
            firings("30 1 * * *", "2026-10-31T00:00:00Z", "2026-11-03T00:00:00Z"),
            [
                at("2026-10-31T05:30:00Z"),
                at("2026-11-01T05:30:00Z"),
                at("2026-11-02T06:30:00Z"),
            ]
        );
        // A time the clocks' changes don't touch fires by the clock.
        assert_eq!(
            firings("0 6 * * *", "2026-03-07T12:00:00Z", "2026-03-09T12:00:00Z"),
            [at("2026-03-08T10:00:00Z"), at("2026-03-09T10:00:00Z")]
        );
    }
}
