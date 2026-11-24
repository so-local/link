//! A machine's maximum lifetime, measured by its cloud from its start:
//! core's duration, from one second through seven days, the longest a
//! signed upload works.

pub use link_types::Duration as Deadline;

/// The longest a deadline may be, seven days, in seconds.
const LONGEST: u64 = 7 * 24 * 60 * 60;

/// An error unless `deadline` is whole seconds from one through seven
/// days.
pub fn check(deadline: Deadline) -> Result<(), String> {
    let length = deadline.get();
    match length.subsec_nanos() == 0 && (1..=LONGEST).contains(&length.as_secs()) {
        true => Ok(()),
        false => Err("a deadline is from one second to seven days, as 30m, 4h or 7d".into()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn lifetimes_are_bounded_by_upload_capabilities() {
        for good in ["1s", "30m", "4h", "7d"] {
            assert!(super::check(good.parse().unwrap()).is_ok(), "{good}");
        }
        for bad in ["0s", "8d", "500ms"] {
            assert!(super::check(bad.parse().unwrap()).is_err(), "{bad}");
        }
        assert!("4h; echo bad".parse::<super::Deadline>().is_err());
    }
}
