//! The transfers an upgrade has under way, journaled in the Safe
//! (`~/.lnk/safe/upgrades/transfers.toml`, the place its `schema.toml`
//! lists): each is recorded before it writes anything, its destination
//! committed first and its source second, so a rerun after an
//! interruption finishes it and takes nothing twice. Every writer of the
//! three files is held off by their locks while one runs.
//!
//! An entry names its transfer by the two kinds and what each `*` of
//! their places stood for, so its files are wherever the map has them on
//! the rerun. Until its destination is written the source is untouched:
//! an entry whose files changed meanwhile is dropped and its records
//! taken again, under the same locks.

use std::path::{Path, PathBuf};

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use toml_edit::DocumentMut;

use super::{Change, Saved, at, replace, take, text};
use crate::layout::map::Roots;
use crate::schema::{Specification, Transfer};

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Journal {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    transfers: Vec<Entry>,
}

crate::specification!(Journal, include_str!("journal.schema.toml"));

/// One transfer of one source file into one destination file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Entry {
    /// The destination's kind and the source's.
    kind: String,
    from: String,
    /// What each `*` of the two places stood for.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    names: Vec<String>,
    /// The source paths taken.
    paths: Vec<String>,
    /// The source's hash as it was when they were taken.
    taken: String,
    stage: Stage,
    /// The destination's hash before, none when it wasn't there, and
    /// after.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    before: Option<String>,
    after: String,
    /// The destination's whole text, kept while it's being written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text: Option<String>,
}

/// How far a transfer has come.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Stage {
    /// Recorded, its destination being written.
    Writing,
    /// Its destination written, its source's records being removed.
    Written,
}

fn hash(text: &str) -> String {
    link_digest::sha256(text.as_bytes())
}

/// The journal's path in `roots`, at the place its format lists.
fn path(roots: &Roots) -> anyhow::Result<PathBuf> {
    let place = Journal::format()
        .files
        .first()
        .context("the upgrade journal lists its file")?;
    at(place, roots, &[])
}

/// Whether an earlier upgrade left a transfer under way in `roots`, which
/// a rerun finishes even when it plans nothing else.
pub(super) fn under_way(roots: &Roots) -> anyhow::Result<bool> {
    let journal: Journal = crate::saved::read(&path(roots)?)?;
    Ok(!journal.transfers.is_empty())
}

/// Write `journal` to `path` through `lock`, its lock, or remove it when
/// it holds no transfer.
fn write(lock: &crate::saved::Held, path: &Path, journal: &Journal) -> anyhow::Result<()> {
    if journal.transfers.is_empty() {
        return match std::fs::remove_file(path) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.into()),
            _ => Ok(()),
        };
    }
    lock.write(journal)
}

/// Run `transfer` of `owned` between its files for `names`, or finish it
/// where an earlier run stopped: the source paths it took, none when
/// there was nothing to take. A transfer it starts must be one of
/// `agreed`, the plan the person agreed to.
pub(super) fn run(
    owned: &Saved,
    transfer: &Transfer,
    names: &[String],
    roots: &Roots,
    agreed: &[Change],
) -> anyhow::Result<Vec<String>> {
    run_until(owned, transfer, names, roots, agreed, Stop::Never).with_context(|| {
        format!(
            "moving records from {} to {}",
            transfer.from, owned.format.kind
        )
    })
}

/// Where a run stops: as an interruption leaves it, for the tests.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Stop {
    /// Once the transfer is recorded, before anything is written.
    Recorded,
    /// Once its destination is written.
    Destination,
    /// Once its source is written, before the entry leaves the journal.
    Source,
    Never,
}

/// The two files of a transfer, by this machine's map.
struct Files {
    source: PathBuf,
    into: PathBuf,
}

/// [`run`], stopping at `stop`.
pub(super) fn run_until(
    owned: &Saved,
    transfer: &Transfer,
    names: &[String],
    roots: &Roots,
    agreed: &[Change],
    stop: Stop,
) -> anyhow::Result<Vec<String>> {
    let format = owned.format;
    let place = format
        .files
        .first()
        .context("a format that takes records lists its file")?;
    let files = Files {
        source: at(&transfer.source, roots, names)?,
        into: at(place, roots, names)?,
    };
    let (source, into) = (&files.source, &files.into);
    let journal_path = path(roots)?;
    let journal_lock = crate::settings::lock(&journal_path)?;
    let _source = crate::settings::lock(source)?;
    let _into = crate::settings::lock(into)?;
    let mut journal: Journal = crate::saved::read(&journal_path)?;
    let found = journal
        .transfers
        .iter()
        .position(|e| e.kind == format.kind && e.from == transfer.from && e.names == names);
    let resumed = match found {
        Some(found) => resumable(journal.transfers.remove(found), &files)?,
        None => None,
    };
    let mut entry = match resumed {
        Some(entry) => entry,
        // Taken afresh: a stale entry, dropped, leaves the journal too.
        None => match fresh(owned, transfer, names, &files, agreed) {
            Ok(Some(entry)) => entry,
            Ok(None) => {
                write(&journal_lock, &journal_path, &journal)?;
                return Ok(Vec::new());
            }
            Err(err) => {
                write(&journal_lock, &journal_path, &journal)?;
                return Err(err);
            }
        },
    };
    // Recorded before anything is written.
    journal.transfers.push(entry.clone());
    write(&journal_lock, &journal_path, &journal)?;
    if stop == Stop::Recorded {
        return Ok(entry.paths);
    }
    if entry.stage == Stage::Writing {
        if text(into)?.as_deref().map(hash).as_deref() != Some(entry.after.as_str()) {
            let written = entry
                .text
                .as_deref()
                .context("the journal keeps what it writes")?;
            replace(into, written)?;
        }
        entry.stage = Stage::Written;
        entry.text = None;
        journal.transfers.pop();
        journal.transfers.push(entry.clone());
        write(&journal_lock, &journal_path, &journal)?;
    }
    if stop == Stop::Destination {
        return Ok(entry.paths);
    }
    let original = text(source)?.unwrap_or_default();
    let mut doc: DocumentMut = original.parse()?;
    let held: Vec<&String> = entry
        .paths
        .iter()
        .filter(|path| crate::schema::lookup(doc.as_table(), path).is_some())
        .collect();
    if !held.is_empty() {
        if hash(&original) != entry.taken {
            // Changed since: each record left must be what was moved.
            let moved: DocumentMut = text(into)?.unwrap_or_default().parse()?;
            for path in &held {
                let same = transfer.migration.is_none()
                    && transfer
                        .take
                        .get(*path)
                        .and_then(|new| crate::schema::lookup(moved.as_table(), new))
                        .map(crate::schema::plain)
                        == crate::schema::lookup(doc.as_table(), path).map(crate::schema::plain);
                ensure!(
                    same,
                    "{} changed after its {path} moved to {}, and holds another one: keep one, \
                     take it out of {} and run lnk upgrade again",
                    source.display(),
                    into.display(),
                    source.display()
                );
            }
        }
        for path in held {
            crate::schema::take(doc.as_table_mut(), path);
        }
        replace(source, &doc.to_string())?;
    }
    if stop == Stop::Source {
        return Ok(entry.paths);
    }
    journal.transfers.pop();
    write(&journal_lock, &journal_path, &journal)?;
    Ok(entry.paths)
}

/// `entry`, if it can go on where it stopped: once its destination is
/// written, or while both files are as it recorded them. Otherwise none:
/// its source is untouched, so its records are taken again.
fn resumable(entry: Entry, files: &Files) -> anyhow::Result<Option<Entry>> {
    if entry.stage == Stage::Written {
        return Ok(Some(entry));
    }
    let into = text(&files.into)?.as_deref().map(hash);
    if into.as_deref() == Some(entry.after.as_str()) {
        return Ok(Some(entry));
    }
    let source = text(&files.source)?.as_deref().map(hash);
    Ok((into == entry.before && source.as_deref() == Some(entry.taken.as_str())).then_some(entry))
}

/// A new entry for `transfer` between `files`, none when there's nothing
/// to take, refused unless the person agreed to it.
fn fresh(
    owned: &Saved,
    transfer: &Transfer,
    names: &[String],
    files: &Files,
    agreed: &[Change],
) -> anyhow::Result<Option<Entry>> {
    let Some(original) = text(&files.source)? else {
        return Ok(None);
    };
    let held = text(&files.into)?;
    let Some(taken) = take(owned, transfer, &original, held.as_deref())? else {
        return Ok(None);
    };
    let change = Change::Transfer {
        source: files.source.clone(),
        from: transfer.from.clone(),
        into: files.into.clone(),
        kind: owned.format.kind.clone(),
        paths: taken.paths.clone(),
    };
    ensure!(
        agreed.contains(&change),
        "{} or {} isn't as it was when you agreed to the upgrade: run lnk upgrade again",
        files.source.display(),
        files.into.display()
    );
    Ok(Some(Entry {
        kind: owned.format.kind.clone(),
        from: transfer.from.clone(),
        names: names.to_vec(),
        paths: taken.paths,
        taken: hash(&original),
        stage: Stage::Writing,
        before: held.as_deref().map(hash),
        after: hash(&taken.destination),
        text: Some(taken.destination),
    }))
}
