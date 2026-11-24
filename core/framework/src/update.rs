//! Cached release guidance. User commands never wait for a network check.

use crate::{
    release::advisories::Advisory,
    upgrades::{Level, NOTES, Notes, Notice, POLICY, ReleaseNotes},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

/// The existing opt-out for notices that policy allows to be quieted.
pub const OFF: &str = "LNK_UPDATE_NOTICE";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seen {
    #[serde(default)]
    pub checked: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<link_version::Version>,
    #[serde(default)]
    pub told_at: u64,
    #[serde(default)]
    pub advisories: Vec<Advisory>,
    #[serde(default)]
    pub advisories_told_at: BTreeMap<String, u64>,
    /// The release whose notes were last shown, or that `lnk upgrade`
    /// upgraded from ([`upgrading_from`]): the new release shows the notes
    /// of every release after it, once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noted: Option<link_version::Version>,
}

link_files::specification!(Seen, include_str!("update.schema.toml"));

pub fn file() -> PathBuf {
    link_files::layout::Hidden::Update.path()
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
fn load() -> Seen {
    link_files::fs::read_control(&file(), link_spec::MAX_BYTES, "an update cache's")
        .ok()
        .and_then(|read| link_files::saved::parse_json(read.text.as_bytes()).ok())
        .unwrap_or_default()
}
fn save(seen: &Seen) {
    if let Ok(bytes) = link_files::saved::print_json(seen) {
        let _ = link_files::write_private(&file(), &bytes);
    }
}

/// Pure scheduling, with a caller-owned clock for deterministic checks.
pub fn due(
    seen: &Seen,
    now: u64,
    current: &link_version::Version,
) -> (bool, Option<link_version::Version>) {
    (
        POLICY.notice.check.due(seen.checked, now),
        seen.latest
            .clone()
            .filter(|latest| latest > current)
            .filter(|_| POLICY.notice.newer.due(seen.told_at, now)),
    )
}

/// The lock of what this machine has seen of releases, held while it's
/// read and written: none where it can't be taken, which is said, since
/// the notes of a release and its notices then wait.
fn locked() -> Option<std::fs::File> {
    let path = file().with_extension("lock");
    match link_files::lock_file(&path) {
        Ok(lock) => Some(lock),
        Err(err) => {
            use link_format::{Path, Report, Sentence, aside, sentence};
            aside(
                Report::warning(sentence!(
                    "Couldn't lock {}: release notes and notices wait.",
                    Path::new(&path)
                ))
                .why(Sentence::adopt(&link_format::clean(&format!("{err:#}")))),
            );
            None
        }
    }
}

/// A failed check updates its attempt time and retains known guidance.
pub fn record(latest: Option<link_version::Version>) {
    record_release(latest.map(|latest| (latest, Vec::new())));
}

pub fn record_release(found: Option<(link_version::Version, Vec<Advisory>)>) {
    let Some(_lock) = locked() else {
        return;
    };
    let mut seen = load();
    seen.checked = now();
    if let Some((latest, advisories)) = found {
        seen.latest = Some(latest);
        seen.advisories = advisories;
        seen.advisories_told_at
            .retain(|id, _| seen.advisories.iter().any(|a| a.id == *id));
    }
    save(&seen);
}

/// Called by `lnk upgrade` before it puts a new release in place: the
/// release it's upgrading from, so the new one shows the notes of every
/// release since, once. A release whose notes are still to be shown is
/// kept, so none are lost to a second upgrade.
pub fn upgrading_from(current: &link_version::Version) {
    let Some(_lock) = locked() else {
        return;
    };
    let mut seen = load();
    if seen.noted.as_ref().is_none_or(|noted| noted > current) {
        seen.noted = Some(current.clone());
        save(&seen);
    }
}

/// What this release says of its notes, given the release last `noted`:
/// the notes to show now, oldest first, and the release to note instead,
/// where it changes. The first release noted, as on a new install, is
/// noted without its notes; a release's notes wait until they can be
/// `shown`, and are shown once.
pub fn noting<'a>(
    notes: &'a Notes,
    noted: Option<&link_version::Version>,
    current: &link_version::Version,
    shown: bool,
) -> (Vec<&'a ReleaseNotes>, Option<link_version::Version>) {
    match noted {
        Some(noted) if noted == current => (Vec::new(), None),
        Some(noted) if noted < current => match shown {
            true => (notes.between(noted, current), Some(current.clone())),
            false => (Vec::new(), None),
        },
        _ => (Vec::new(), Some(current.clone())),
    }
}

/// One release's notes as a person reads them: a line naming it, then
/// each note on a bullet.
fn show(release: &ReleaseNotes) {
    use link_format::{List, Note, Prose, Sentence, Version, aside, sentence};
    aside(Note(sentence!(
        "New in lnk {}:",
        Version::new(release.version.to_string())
    )));
    let list = release
        .notes
        .iter()
        .fold(List::new(sentence!("Nothing.")), |list, note| {
            list.item(Prose::from(Sentence::adopt(note)))
        });
    aside(list);
}

/// Suppression is policy-driven. Critical fixes remain visible with JSON,
/// redirected output and the ordinary notice opt-out.
pub fn visible(policy: &Notice, level: Level, quiet: bool, json: bool, terminal: bool) -> bool {
    !policy.quiet.contains(&level) || (!quiet && !json && terminal)
}

pub fn notice(json: bool) {
    let quiet = std::env::var_os(OFF).is_some_and(|v| v == "off");
    let terminal = link_format::streams::stderr_is_terminal();
    if !visible(&POLICY.notice, Level::Newer, quiet, json, terminal) && !file().is_file() {
        return;
    }
    let Some(lock) = locked() else {
        return;
    };
    let mut seen = load();
    let now = now();
    let current = link_version::version();
    let (due_check, say) = due(&seen, now, &current);
    let check = due_check && visible(&POLICY.notice, Level::Newer, quiet, json, terminal);
    let mut changed = false;
    let shown = visible(&POLICY.notice, Level::Newer, quiet, json, terminal);
    let (notes, noted) = noting(&NOTES, seen.noted.as_ref(), &current, shown);
    notes.into_iter().for_each(show);
    if noted.is_some() {
        seen.noted = noted;
        changed = true;
    }
    use link_format::{Note, Version, aside, cmd, sentence};
    for advisory in &seen.advisories {
        let last = seen
            .advisories_told_at
            .get(&advisory.id)
            .copied()
            .unwrap_or_default();
        if advisory.affects(&current)
            && visible(&POLICY.notice, advisory.level, quiet, json, terminal)
            && POLICY.notice.cadence(advisory.level).due(last, now)
        {
            aside(Note(sentence!(
                "{} advisory {}: {} Fixed in lnk {}. Run {}.",
                advisory.level.to_string(),
                link_format::clean(&advisory.id),
                link_format::clean(&advisory.summary),
                Version::new(advisory.fixed.to_string()),
                cmd!("lnk upgrade")
            )));
            seen.advisories_told_at.insert(advisory.id.clone(), now);
            changed = true;
        }
    }
    if let Some(latest) = say
        && visible(&POLICY.notice, Level::Newer, quiet, json, terminal)
    {
        aside(Note(sentence!(
            "A newer lnk is out, {}, and you have {}: {} installs it.",
            Version::new(latest.to_string()),
            Version::new(current.to_string()),
            cmd!("lnk upgrade")
        )));
        seen.told_at = now;
        changed = true;
    }
    // Persist before spawning: a fast check must not be overwritten by
    // this invocation after its result has already been recorded.
    if check {
        seen.checked = now;
        changed = true;
    }
    if changed {
        save(&seen);
    }
    drop(lock);
    if check {
        let _ = crate::lnk()
            .args(["upgrade", "--check"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checks_and_says_at_configured_intervals() {
        let day = POLICY.notice.check.seconds();
        let now = day * 10;
        let current = "1.0.0".parse().unwrap();
        let mut seen = Seen::default();
        assert_eq!(due(&seen, now, &current), (true, None));
        seen.checked = now;
        seen.latest = Some("1.1.0".parse().unwrap());
        assert!(due(&seen, now, &current).1.is_some());
        seen.told_at = now;
        assert_eq!(due(&seen, now + 1, &current), (false, None));
        assert!(due(&seen, now + day, &current).1.is_some());
        assert!(due(&seen, now + day, &"1.1.0".parse().unwrap()).1.is_none());
    }
    #[test]
    fn critical_guidance_survives_every_suppression_combination() {
        for quiet in [false, true] {
            for json in [false, true] {
                for terminal in [false, true] {
                    assert!(visible(
                        &POLICY.notice,
                        Level::Critical,
                        quiet,
                        json,
                        terminal
                    ));
                    assert_eq!(
                        visible(&POLICY.notice, Level::Recommended, quiet, json, terminal),
                        !quiet && !json && terminal
                    );
                }
            }
        }
    }
    fn v(text: &str) -> link_version::Version {
        text.parse().unwrap()
    }

    /// After an upgrade the new release shows the notes of every release
    /// since, once, and only where they can be shown; a new install or a
    /// downgrade shows none.
    #[test]
    fn release_notes_are_shown_once_after_an_upgrade() {
        let notes = Notes::parse(
            "[[release]]\nversion = \"0.3.0\"\nnotes = [\"Three.\"]\n\
             [[release]]\nversion = \"0.2.0\"\nnotes = [\"Two.\"]\n",
        )
        .unwrap();
        let current = v("0.3.0");
        let versions = |shown: Vec<&ReleaseNotes>| -> Vec<String> {
            shown.iter().map(|r| r.version.to_string()).collect()
        };
        // Upgraded from 0.1.0: both releases' notes, and 0.3.0 noted.
        let (shown, noted) = noting(&notes, Some(&v("0.1.0")), &current, true);
        assert_eq!(versions(shown), ["0.2.0", "0.3.0"]);
        assert_eq!(noted, Some(current.clone()));
        // Then quiet.
        let (shown, noted) = noting(&notes, Some(&current), &current, true);
        assert!(shown.is_empty() && noted.is_none());
        // Where they can't be shown, as with --json, they wait.
        let (shown, noted) = noting(&notes, Some(&v("0.2.0")), &current, false);
        assert!(shown.is_empty() && noted.is_none());
        // A new install notes its release, showing nothing.
        let (shown, noted) = noting(&notes, None, &current, true);
        assert!(shown.is_empty());
        assert_eq!(noted, Some(current.clone()));
        // So does a downgrade.
        let (shown, noted) = noting(&notes, Some(&v("0.4.0")), &current, true);
        assert!(shown.is_empty());
        assert_eq!(noted, Some(current));
    }

    #[test]
    fn existing_cache_reads_and_future_shapes_are_refused() {
        assert_eq!(serde_json::from_str::<Seen>("{}").unwrap(), Seen::default());
        assert!(serde_json::from_str::<Seen>(r#"{"latest":"garbage"}"#).is_err());
        let seen: Seen =
            serde_json::from_str(r#"{"checked":5,"latest":"1.1.0","told_at":3,"noted":"1.0.0"}"#)
                .unwrap();
        assert_eq!(
            serde_json::from_str::<Seen>(&serde_json::to_string(&seen).unwrap()).unwrap(),
            seen
        );
    }
}
