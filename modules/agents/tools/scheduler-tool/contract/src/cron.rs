//! Cron expressions, five fields (minute, hour, day of the month, month,
//! day of the week), in the agent's time zone (`[tool.scheduler] zone`:
//! this machine's, unless its spec names one): `0 6 * * 1-5` is six in
//! the morning on weekdays there.

use anyhow::{Context, bail, ensure};

/// A cron expression, read.
#[derive(Clone, Debug, PartialEq)]
pub struct Cron {
    minutes: Vec<bool>,
    hours: Vec<bool>,
    days: Vec<bool>,
    months: Vec<bool>,
    weekdays: Vec<bool>,
    /// Both day fields written: a day matches either, as Vixie cron has
    /// it, where a field starting with `*` (`*/2` too) isn't written.
    either_day: bool,
    /// At a fixed time of day: neither the minute nor the hour field
    /// starts with `*`. Such a time the clocks skip fires once as they
    /// skip it, and one they repeat fires once, as Vixie cron has it
    /// ([`Cron::fires`]).
    fixed: bool,
    zone: Option<jiff::tz::TimeZone>,
}

/// The offset from UTC, in seconds, of the agent's zone (this process's,
/// the runner zone) at `secs` since 1970.
fn local_offset(secs: u64) -> i64 {
    let time = secs as libc::time_t;
    // SAFETY: localtime_r writes only into `tm`, which outlives the call.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&time, &mut tm) };
    #[allow(
        clippy::unnecessary_cast,
        reason = "a C long: the cast is a no-op where it's 64 bits, and widens where it's 32"
    )]
    let offset = tm.tm_gmtoff as i64;
    offset
}

/// A wall clock's reading, `wall` seconds since 1970 as if in UTC:
/// (minute, hour, day of the month, month 1 to 12, day of the week 0 to
/// 6 from Sunday).
fn fields(wall: i64) -> (u32, u32, u32, u32, u32) {
    let days = wall.div_euclid(86_400);
    let rest = wall.rem_euclid(86_400);
    // Howard Hinnant's civil from days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        (rest % 3600 / 60) as u32,
        (rest / 3600) as u32,
        day as u32,
        month as u32,
        // 1970-01-01 was a Thursday.
        (days + 4).rem_euclid(7) as u32,
    )
}

/// The minutes in a day.
const DAY_MINUTES: u32 = 24 * 60;

/// How far the clocks may jump at once, in seconds, at a daylight-saving
/// change: half an hour (Lord Howe Island), an hour, or two.
const JUMPS: [i64; 3] = [1800, 3600, 7200];

/// The time in the agent's zone of `secs`, as [`fields`] reads it.
#[cfg(test)]
fn local(secs: u64) -> (u32, u32, u32, u32, u32) {
    fields(secs as i64 + local_offset(secs))
}

impl Cron {
    /// Reads `expr`: each field `*`, a number, a range `a-b`, a list of
    /// them with `,`, any with a step `/n`.
    pub fn parse(expr: &str) -> anyhow::Result<Self> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        ensure!(
            fields.len() == 5,
            "{expr:?} isn't a cron expression: five fields, minute hour day month weekday"
        );
        let field = |text: &str, low: u32, high: u32| -> anyhow::Result<Vec<bool>> {
            let mut set = vec![false; high as usize + 1];
            for part in text.split(',') {
                let (range, step) = match part.split_once('/') {
                    Some((range, step)) => (
                        range,
                        step.parse::<u32>()
                            .ok()
                            .filter(|s| *s > 0)
                            .with_context(|| format!("{part:?}: a step is a number"))?,
                    ),
                    None => (part, 1),
                };
                let (from, to) = match range {
                    "*" => (low, high),
                    _ => match range.split_once('-') {
                        Some((a, b)) => (number(a)?, number(b)?),
                        None => {
                            let n = number(range)?;
                            (n, if step > 1 { high } else { n })
                        }
                    },
                };
                ensure!(
                    low <= from && from <= to && to <= high,
                    "{part:?} is out of {low} to {high}"
                );
                let mut n = from;
                while n <= to {
                    set[n as usize] = true;
                    let Some(next) = n.checked_add(step) else {
                        break;
                    };
                    n = next;
                }
            }
            Ok(set)
        };
        let mut weekdays = field(fields[4], 0, 7)?;
        // 7 is Sunday too.
        if weekdays[7] {
            weekdays[0] = true;
        }
        weekdays.truncate(7);
        Ok(Self {
            minutes: field(fields[0], 0, 59)?,
            hours: field(fields[1], 0, 23)?,
            days: field(fields[2], 1, 31)?,
            months: field(fields[3], 1, 12)?,
            weekdays,
            either_day: !fields[2].starts_with('*') && !fields[4].starts_with('*'),
            fixed: !fields[0].starts_with('*') && !fields[1].starts_with('*'),
            zone: None,
        })
    }

    /// The fewest minutes between two of its firings, read from its
    /// minute and hour fields as if it fired every day (the clocks'
    /// changes aside): at most a day.
    pub fn shortest_gap(&self) -> u32 {
        let times: Vec<u32> = (0..24u32)
            .filter(|h| self.hours[*h as usize])
            .flat_map(|h| {
                (0..60u32)
                    .filter(|m| self.minutes[*m as usize])
                    .map(move |m| h * 60 + m)
            })
            .collect();
        let (Some(first), Some(last)) = (times.first(), times.last()) else {
            return DAY_MINUTES;
        };
        times
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .chain(std::iter::once(DAY_MINUTES - last + first))
            .min()
            .unwrap_or(DAY_MINUTES)
    }

    /// An explicit IANA zone, independent of the runner's default zone.
    pub fn in_zone(expr: &str, zone: &str) -> anyhow::Result<Self> {
        let mut cron = Self::parse(expr)?;
        cron.zone = Some(jiff::tz::TimeZone::get(zone)?);
        Ok(cron)
    }

    /// The offset from UTC of its zone at `secs`, in seconds.
    fn offset(&self, secs: u64) -> i64 {
        let Some(zone) = &self.zone else {
            return local_offset(secs);
        };
        let time = jiff::Timestamp::from_second(secs as i64).expect("checked timestamp");
        i64::from(zone.to_offset(time).seconds())
    }

    /// The time in its zone of `secs` since 1970.
    fn local(&self, secs: u64) -> (u32, u32, u32, u32, u32) {
        fields(secs as i64 + self.offset(secs))
    }

    /// Whether the wall clock reading `wall` matches it.
    fn matches(&self, wall: i64) -> bool {
        let (minute, hour, day, month, weekday) = fields(wall);
        self.minutes[minute as usize] && self.hours[hour as usize] && self.on(day, month, weekday)
    }

    /// Whether it fires in the minute holding `secs`. At a fixed time of
    /// day, as Vixie cron: where the clocks jump forward into `secs`, a
    /// time they skipped fires then, once; where they went back, a time
    /// read a second time doesn't fire again. One with `*` in its minute
    /// or hour fires by the clock as it reads.
    fn fires(&self, secs: u64) -> bool {
        let offset = self.offset(secs);
        let wall = secs as i64 + offset;
        if !self.fixed {
            return self.matches(wall);
        }
        if self.matches(wall) {
            // The same reading came before, the clocks since gone back.
            return !JUMPS.iter().any(|jump| {
                secs.checked_sub(*jump as u64)
                    .is_some_and(|before| self.offset(before) - offset == *jump)
            });
        }
        // The clocks jumped forward into this minute: each reading they
        // skipped fires now.
        let before = self.offset(secs.saturating_sub(60));
        (1..=(offset - before) / 60).any(|skipped| self.matches(wall - 60 * skipped))
    }

    /// Whether it fires on that day at all.
    fn on(&self, day: u32, month: u32, weekday: u32) -> bool {
        let days = match self.either_day {
            true => self.days[day as usize] || self.weekdays[weekday as usize],
            false => self.days[day as usize] && self.weekdays[weekday as usize],
        };
        self.months[month as usize] && days
    }

    /// The first minute it fires after `secs`, within about four years;
    /// none if never (the 31st of February).
    pub fn next_after(&self, secs: u64) -> Option<u64> {
        let mut at = (secs / 60).checked_add(1)?.checked_mul(60)?;
        jiff::Timestamp::from_second(i64::try_from(at).ok()?).ok()?;
        let until = at
            .checked_add(4 * 366 * 86_400)?
            .min(jiff::Timestamp::MAX.as_second() as u64);
        while at < until {
            if self.fires(at) {
                return Some(at);
            }
            let (minute, hour, day, month, weekday) = self.local(at);
            // A day that can't match, skipped to its last hour (a day is
            // 23 hours where the clocks go forward, so never further);
            // an hour that can't, skipped whole.
            let left = (23 - u64::from(hour)) * 60 + 60 - u64::from(minute);
            // Explicit zones can change by half an hour or skip a date.
            // Walk UTC minutes so neither gaps nor repeated wall time are guessed.
            if self.zone.is_some() {
                at += 60;
                continue;
            }
            at += match (self.on(day, month, weekday), self.hours[hour as usize]) {
                (false, _) if left > 60 => (left - 60) * 60,
                (_, false) | (false, _) => (60 - u64::from(minute)) * 60,
                (true, true) => 60,
            };
        }
        None
    }
}

fn number(text: &str) -> anyhow::Result<u32> {
    match text.parse() {
        Ok(n) => Ok(n),
        Err(_) => bail!("{text:?} isn't a number"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crons_fire_when_they_say() {
        let every_minute = Cron::parse("* * * * *").unwrap();
        assert_eq!(every_minute.next_after(600), Some(660));
        assert_eq!(every_minute.next_after(659), Some(660));
        let hourly = Cron::parse("15 * * * *").unwrap();
        let next = hourly.next_after(0).unwrap();
        assert_eq!(local(next).0, 15);
        let six = Cron::parse("0 6 * * 1-5").unwrap();
        let next = six.next_after(1_790_690_587).unwrap();
        let (minute, hour, _, _, weekday) = local(next);
        assert_eq!((minute, hour), (0, 6));
        assert!((1..=5).contains(&weekday));
        let every_ten = Cron::parse("*/10 * * * *").unwrap();
        assert_eq!(local(every_ten.next_after(0).unwrap()).0 % 10, 0);
        assert_eq!(Cron::parse("0 0 31 2 *").unwrap().next_after(0), None);
    }

    /// Skipping the days and hours that can't match finds what walking
    /// every minute finds, and a rare day quickly.
    #[test]
    fn the_next_time_is_every_minutes_first() {
        let walked = |cron: &Cron, secs: u64| {
            let mut at = (secs / 60 + 1) * 60;
            while !cron.fires(at) {
                at += 60;
            }
            at
        };
        for text in [
            "30 0 1 * *",
            "0 12 * * 0",
            "15 3 13 * 5",
            "*/20 23 * 3,10 *",
            "59 23 31 * *",
        ] {
            let cron = Cron::parse(text).unwrap();
            for start in [1_790_690_587, 1_774_828_800, 1_793_000_000] {
                assert_eq!(
                    cron.next_after(start),
                    Some(walked(&cron, start)),
                    "{text} {start}"
                );
            }
        }
        let leap = Cron::parse("0 9 29 2 *").unwrap();
        let next = leap.next_after(1_790_690_587).unwrap();
        let (minute, hour, day, month, _) = local(next);
        assert_eq!((minute, hour, day, month), (0, 9, 29, 2));
    }

    /// A step in a day field isn't a written day: `*/2` in the day of the
    /// month with a weekday is both, as Vixie cron has it, not either.
    #[test]
    fn a_starred_day_field_narrows_the_other() {
        let cron = Cron::parse("0 9 */2 * 1").unwrap();
        assert!(!cron.either_day);
        // Mondays on odd days of the month only.
        assert!(cron.on(1, 6, 1));
        assert!(!cron.on(2, 6, 1));
        assert!(!cron.on(3, 6, 2));
        let both = Cron::parse("0 9 1-7 * 1").unwrap();
        assert!(both.either_day);
        assert!(both.on(3, 6, 2));
        assert!(both.on(20, 6, 1));
    }

    /// Daylight saving, as Vixie cron: a fixed time the clocks skip fires
    /// once as they skip it, and a repeated one fires once; New York's
    /// clocks went forward at 2026-03-08 07:00 UTC and back at 2026-11-01
    /// 06:00 UTC.
    #[test]
    fn daylight_saving_skips_nothing_and_repeats_nothing() {
        let at = |text: &str| text.parse::<jiff::Timestamp>().unwrap().as_second() as u64;
        let skipped = Cron::in_zone("30 2 * * *", "America/New_York").unwrap();
        assert_eq!(
            skipped.next_after(at("2026-03-08T00:00:00Z")),
            Some(at("2026-03-08T07:00:00Z"))
        );
        assert_eq!(
            skipped.next_after(at("2026-03-08T07:00:00Z")),
            Some(at("2026-03-09T06:30:00Z"))
        );
        let repeated = Cron::in_zone("30 1 * * *", "America/New_York").unwrap();
        assert_eq!(
            repeated.next_after(at("2026-11-01T05:30:00Z")),
            Some(at("2026-11-02T06:30:00Z"))
        );
        // A time the jump doesn't touch is as ever.
        let six = Cron::in_zone("0 6 * * *", "America/New_York").unwrap();
        assert_eq!(
            six.next_after(at("2026-03-08T00:00:00Z")),
            Some(at("2026-03-08T10:00:00Z"))
        );
    }

    /// The shortest gap is read from the minute and hour fields.
    #[test]
    fn the_shortest_gap_between_firings() {
        for (text, gap) in [
            ("* * * * *", 1),
            ("*/5 * * * *", 5),
            ("*/7 * * * *", 4),
            ("0,1 9 * * 1", 1),
            ("0 * * * *", 60),
            ("58 9,10 * * *", 60),
            ("58,2 9,10 * * *", 4),
            ("0 6 * * 1-5", 1440),
            ("0 6,18 * * *", 720),
        ] {
            assert_eq!(Cron::parse(text).unwrap().shortest_gap(), gap, "{text}");
        }
    }

    #[test]
    fn nonsense_is_refused() {
        for bad in [
            "",
            "* * * *",
            "60 * * * *",
            "* 24 * * *",
            "a * * * *",
            "5-1 * * * *",
            "*/0 * * * *",
        ] {
            assert!(Cron::parse(bad).is_err(), "{bad:?}");
        }
    }
}
