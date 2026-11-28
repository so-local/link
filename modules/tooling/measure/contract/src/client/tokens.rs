//! The tokens of each agent's model answers: counted by the model
//! route's bridge from each answer it carries (the models module's, which
//! knows each provider's format), kept here as the measure module owns
//! them, and read by its sampler as the agent's. Numbers only, never a
//! word of a prompt or an answer.
//!
//! Each agent's count is one file in Link's settings
//! (mapped `Hidden::MeasureCredentials`, `safe/measures/credentials` by default,
//! `<agent ID>.json`, 600, a [`Tokens`]), running since
//! it was first written: a reader takes what it grew by since its last
//! look ([`Tokens::since`]).

use std::path::{Path, PathBuf};

use anyhow::Context;

use super::Tokens;

link_files::specification!(Tokens, include_str!("tokens.schema.toml"));

/// The tokens counted, which an upgrade converts.
pub fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Tokens>()
}

/// The folder of the counts in Link's `settings`.
pub fn dir(settings: &Path) -> PathBuf {
    link_files::layout::Hidden::MeasureCredentials.at(settings)
}

/// Adds `tokens`, one answer's (or a few), to `agent`'s count in Link's
/// `settings`: every count is added under one lock, so two bridges'
/// counts are both kept.
pub fn add(settings: &Path, agent: &str, tokens: &Tokens) -> anyhow::Result<()> {
    anyhow::ensure!(
        (1..=64).contains(&agent.len())
            && agent
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.@".contains(&b))
            && !agent.starts_with('.'),
        "{:?} isn't an agent's ID",
        link_format::clean(agent)
    );
    let dir = dir(settings);
    link_files::create_private_dir(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let _locked = lock(&dir.join(".lock"))?;
    let path = dir.join(format!("{agent}.json"));
    let now = read_at(&path).unwrap_or_default().plus(tokens);
    link_files::write_private(&path, &link_files::saved::print_json(&now)?)
        .with_context(|| format!("writing {}", path.display()))
}

/// What `agent`'s model answers have counted, in Link's `settings`;
/// none when nothing has counted any.
pub fn read(settings: &Path, agent: &str) -> Option<Tokens> {
    if agent.contains(['/', '\0']) || agent.starts_with('.') {
        return None;
    }
    read_at(&dir(settings).join(format!("{agent}.json")))
}

fn read_at(path: &Path) -> Option<Tokens> {
    let bytes = std::fs::read(path).ok()?;
    link_files::saved::parse_json(&bytes).ok()
}

/// The counts' lock, held until dropped.
fn lock(path: &Path) -> anyhow::Result<std::fs::File> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    // SAFETY: flock on a descriptor this owns, which blocks until held.
    let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0;
    anyhow::ensure!(locked, "locking {}", path.display());
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agents_count_runs_and_is_its_own() {
        let dir = link_test_kit::TempDir::new("measure-tokens");
        assert_eq!(read(&dir, "a1"), None);
        let one = Tokens {
            requests: 1,
            input: 10,
            output: 2,
            ..Tokens::default()
        };
        add(&dir, "a1", &one).unwrap();
        add(&dir, "a1", &one).unwrap();
        add(&dir, "b2", &one).unwrap();
        let kept = std::fs::read_to_string(super::dir(&dir).join("a1.json")).unwrap();
        assert!(kept.ends_with(r#""schema":1}"#), "{kept}");
        let a1 = read(&dir, "a1").unwrap();
        assert_eq!((a1.requests, a1.input, a1.output), (2, 20, 4));
        assert_eq!(read(&dir, "b2").unwrap().requests, 1);
        for wrong in ["", "../x", ".lock", "a/b"] {
            assert!(add(&dir, wrong, &one).is_err(), "{wrong:?}");
            assert_eq!(read(&dir, wrong), None);
        }
    }
}
