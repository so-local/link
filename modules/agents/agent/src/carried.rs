//! What `lnk agent harness set` carries between harnesses, so nothing is lost on
//! a round trip: each harness's own state as a switch away from it left
//! it, kept in the agent's own folder (`harnesses/<name>.json`, in ASP's
//! portable forms) ([`left`]); and for the harness a switch goes to, the
//! conversations that went on elsewhere since it last held them
//! ([`elsewhere`]), imported on top of its own.
//!
//! A conversation is its thread's: the version a harness left last
//! keeps the thread, so its chat follows it. One that went another way
//! on a harness that couldn't take it in (two versions, neither the
//! other's start) comes too, as a thread of its own
//! (`<thread>~<harness>`): both are kept, neither merged.

use link_format::WithinExt;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use link_agent_spec::Spec;
use link_harness_contract::asp::acp::schema::v1::SessionUpdate;
use link_harness_contract::asp::{self, forms::Session};
use link_harness_contract::harness::layout;
use serde::{Deserialize, Serialize};

/// A harness's state as a switch away from it left it, and when, by the
/// order of switches (later ones larger).
#[derive(Serialize, Deserialize)]
struct Kept {
    order: u64,
    state: asp::State,
}

link_files::specification!(Kept, include_str!("carried.schema.toml"));

fn folder(own: &Path) -> PathBuf {
    own.join(layout::HARNESSES)
}

/// Keeps `state`, what harness `harness` exported as a switch left it,
/// in the agent's own folder `own`, over what it kept of it before: the
/// latest of every harness's. A kind `state` doesn't give (a harness
/// read while it wasn't running gives no sessions) keeps what was kept.
pub fn keep(own: &Path, harness: &str, state: &asp::State) -> anyhow::Result<()> {
    anyhow::ensure!(
        !harness.is_empty()
            && harness
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
        "{harness:?} isn't a harness's name"
    );
    let all = read(own)?;
    let order = all
        .iter()
        .map(|(_, kept)| kept.order)
        .max()
        .map_or(0, |o| o + 1);
    let before = all
        .into_iter()
        .find(|(name, _)| name == harness)
        .map(|(_, kept)| kept.state)
        .unwrap_or_default();
    let mut state = state.clone();
    if let Some(notes) = &mut state.memory {
        // A complete native memory export also reports files removed since
        // the previous export. Keep their identities as tombstones.
        for note in notes.iter_mut() {
            note.source.get_or_insert_with(|| harness.to_string());
        }
        for old in before.memory.iter().flatten() {
            let mut old = old.clone();
            old.source.get_or_insert_with(|| harness.to_string());
            if !notes
                .iter()
                .any(|note| note.source == old.source && note.path == old.path)
            {
                old.text.clear();
                old.deleted = true;
                notes.push(old);
            }
        }
    }
    let kept = Kept {
        order,
        state: asp::State {
            omissions: if state.sessions.is_some() {
                state.omissions
            } else {
                before.omissions
            },
            sessions: state.sessions.or(before.sessions),
            instructions: state.instructions.or(before.instructions),
            skills: state.skills.or(before.skills),
            tools: state.tools.or(before.tools),
            memory: state.memory.or(before.memory),
            settings: state.settings.or(before.settings),
            schedules: state.schedules.or(before.schedules),
        },
    };
    let dir = folder(own);
    std::fs::create_dir_all(&dir).within(format!("creating {}", dir.display()))?;
    link_files::write_private(
        &dir.join(format!("{harness}.json")),
        &link_files::saved::print_json(&kept)?,
    )?;
    Ok(())
}

/// Each harness's state the agent's own folder `own` keeps, by its name,
/// those a switch left earliest first. An unreadable snapshot refuses capture.
pub fn kept(own: &Path) -> anyhow::Result<Vec<(String, asp::State)>> {
    Ok(read(own)?
        .into_iter()
        .map(|(name, kept)| (name, kept.state))
        .collect())
}

fn read(own: &Path) -> anyhow::Result<Vec<(String, Kept)>> {
    let entries = match std::fs::read_dir(folder(own)) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut all = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        anyhow::ensure!(
            entry.file_type()?.is_file(),
            "a harness snapshot is not a regular file"
        );
        let name = path
            .file_stem()
            .and_then(|name| name.to_str())
            .within("a harness snapshot has an invalid name")?
            .to_string();
        let kept: Kept = link_files::saved::parse_json(&std::fs::read(&path)?).within(format!(
            "reading retained harness state at {}",
            path.display()
        ))?;
        all.push((name, kept));
    }
    all.sort_by_key(|(_, kept)| kept.order);
    Ok(all)
}

/// A switch away from harness `harness`, which exported `state`: kept in
/// the agent's own folder `own`, who the agent became there brought into
/// it as `spec` names its files wherever it has none
/// ([`crate::identity::bring_out`]), and what it remembered there
/// written into its notes ([`crate::identity::remember_elsewhere`]).
/// What came into the folder, as said (`its persona`, `skill summarize`).
pub fn left(
    spec: &Spec,
    own: &Path,
    harness: &str,
    state: &asp::State,
) -> anyhow::Result<Vec<String>> {
    keep(own, harness, state)?;
    let taken = crate::identity::bring_out(spec, own, state)?;
    crate::identity::remember_elsewhere(own)?;
    Ok(taken)
}

/// The conversations harness `to` doesn't hold as they stand: each
/// thread's latest version that isn't the start of one `to` kept when
/// it last left, or of a later version elsewhere; the latest keeping its
/// thread, any that went another way named `<thread>~<harness>`.
pub fn elsewhere(own: &Path, to: &str) -> anyhow::Result<asp::State> {
    let all = read(own)?;
    let mut theirs: BTreeMap<String, Vec<Vec<SessionUpdate>>> = BTreeMap::new();
    for (_, kept) in all.iter().filter(|(name, _)| name == to) {
        for session in kept.state.sessions.iter().flatten() {
            theirs
                .entry(session.session_id.0.to_string())
                .or_default()
                .push(session.updates.clone());
        }
    }
    let mut threads: BTreeMap<String, Vec<Version>> = BTreeMap::new();
    for (name, kept) in all.iter().filter(|(name, _)| name != to) {
        for session in kept.state.sessions.iter().flatten() {
            threads
                .entry(session.session_id.0.to_string())
                .or_default()
                .push((name, session, session.updates.clone()));
        }
    }
    let mut sessions = Vec::new();
    for (thread, versions) in threads {
        // A version later ones went on from, or `to` holds, is theirs.
        let standing: Vec<_> = versions
            .iter()
            .enumerate()
            .filter(|(i, (_, _, messages))| {
                let went_on = versions.iter().enumerate().any(|(j, (_, _, later))| {
                    j != *i && starts(messages, later) && (messages != later || j > *i)
                });
                !went_on
                    && !theirs
                        .get(&thread)
                        .is_some_and(|held| held.iter().any(|updates| starts(messages, updates)))
            })
            .map(|(_, version)| version)
            .collect();
        let last = standing.len().saturating_sub(1);
        for (i, (name, session, _)) in standing.into_iter().enumerate() {
            let id = match i == last {
                true => thread.clone(),
                false => format!("{thread}~{name}"),
            };
            sessions.push(Session {
                evidence: None,
                session_id: id.into(),
                harness: Some(name.to_string()),
                ..(*session).clone()
            });
        }
    }
    let omissions = all
        .iter()
        .filter(|(name, _)| name != to)
        .flat_map(|(_, kept)| kept.state.omissions.clone())
        .collect();
    Ok(asp::State {
        sessions: Some(sessions),
        omissions,
        ..asp::State::default()
    })
}

/// Each session of `coming` for a thread `held` has a version of, that
/// doesn't go on from its complete portable prefix, named `<thread>~<harness>` for the harness it
/// came from, so the destination's own keeps the thread: both kept,
/// neither merged. Each so named, as (thread, its new name, harness).
pub fn beside(held: &asp::State, coming: &mut asp::State) -> Vec<(String, String, String)> {
    let held: BTreeMap<String, Vec<SessionUpdate>> = held
        .sessions
        .iter()
        .flatten()
        .map(|session| (session.session_id.0.to_string(), session.updates.clone()))
        .collect();
    let mut named = Vec::new();
    for session in coming.sessions.iter_mut().flatten() {
        let thread = session.session_id.0.to_string();
        let (Some(own), Some(from)) = (held.get(&thread), session.harness.clone()) else {
            continue;
        };
        let goes_on = session.updates.starts_with(own);
        if !goes_on {
            let mut copy = format!("{thread}~{from}");
            if copy.len() > 128
                || held.get(&copy).is_some_and(|old| {
                    !session.updates.starts_with(old) && !old.starts_with(&session.updates)
                })
            {
                let bytes = serde_json::to_vec(&(&thread, &from, &session.updates))
                    .expect("portable session updates");
                copy = format!("branch:{}", link_digest::sha256(&bytes));
            }
            session.session_id = copy.clone().into();
            named.push((thread, copy, from));
        }
    }
    named
}

/// One harness's version of a thread's conversation: whose, the
/// session, and its messages.
type Version<'a> = (&'a str, &'a Session, Vec<SessionUpdate>);

/// Whether `messages` is where `other` starts, or all of it.
fn starts(messages: &[SessionUpdate], other: &[SessionUpdate]) -> bool {
    messages.len() <= other.len() && other[..messages.len()] == *messages
}

#[cfg(test)]
mod tests;
