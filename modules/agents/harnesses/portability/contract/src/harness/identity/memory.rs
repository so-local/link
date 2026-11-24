//! Native memory revisions are compared before applying a portable correction.
//! The original source survives every handoff, including a deletion.

use std::{collections::BTreeMap, path::Path};

use anyhow::{Context, ensure};
use link_agent_state_protocol::forms::Note;

use super::{outside_block, read, write_outside};

/// An adapter's native memory as it last exported it, by each note's
/// path, so a later export keeps a note's deletion and a correction is
/// compared with what the harness had.
#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Revisions {
    notes: BTreeMap<String, Note>,
}

link_files::specification!(Revisions, include_str!("memory.schema.toml"));

fn path(home: &Path, adapter: &str) -> std::path::PathBuf {
    link_files::state::path(home, link_files::state::Area::Context)
        .join(format!("memory-{adapter}.json"))
}

pub fn belongs(note: &Note, adapter: &str) -> bool {
    note.source.as_deref().is_some_and(|source| {
        source == adapter
            || source
                .strip_prefix(adapter)
                .is_some_and(|rest| rest.starts_with(':'))
    })
}

fn previous(home: &Path, adapter: &str) -> anyhow::Result<BTreeMap<String, Note>> {
    let Some(text) = read(&path(home, adapter))? else {
        return Ok(BTreeMap::new());
    };
    link_files::saved::parse_json::<Revisions>(text.as_bytes())
        .map(|revisions| revisions.notes)
        .context("reading native memory revisions; originals are retained")
}

fn keep(home: &Path, adapter: &str, notes: &BTreeMap<String, Note>) -> anyhow::Result<()> {
    Ok(link_files::write_private(
        &path(home, adapter),
        &link_files::saved::print_json(&Revisions {
            notes: notes.clone(),
        })?,
    )?)
}

/// Publish a complete native export, retaining a sourced deletion for a
/// previously exported file that disappeared. No native file is changed.
pub fn exported(home: &Path, adapter: &str, notes: Vec<Note>) -> anyhow::Result<Vec<Note>> {
    let old = previous(home, adapter)?;
    let mut notes: BTreeMap<_, _> = notes
        .into_iter()
        .map(|note| (note.path.clone(), note))
        .collect();
    for (name, mut note) in old {
        notes.entry(name).or_insert_with(|| {
            note.text.clear();
            note.deleted = true;
            note
        });
    }
    keep(home, adapter, &notes)?;
    Ok(notes.into_values().collect())
}

/// Apply only this adapter's sourced revisions to its actual native files.
/// A native edit after export conflicts rather than being silently erased.
/// `own` excludes generated foreign-memory blocks from the comparison.
pub fn reconcile(
    home: &Path,
    root: &Path,
    adapter: &str,
    notes: &[Note],
    own: impl Fn(&str) -> String,
) -> anyhow::Result<()> {
    let mut revisions = previous(home, adapter)?;
    let mut changes = Vec::new();
    for note in notes.iter().filter(|note| belongs(note, adapter)) {
        let relative = Path::new(&note.path);
        ensure!(
            !note.path.is_empty()
                && relative
                    .components()
                    .all(|c| matches!(c, std::path::Component::Normal(_))),
            "memory path leaves its native folder"
        );
        let file = root.join(relative);
        let current = own(&read(&file)?.unwrap_or_default());
        let wanted = if note.deleted { "" } else { note.text.trim() };
        let unchanged = revisions
            .get(&note.path)
            .is_some_and(|before| before.text.trim() == current.trim());
        ensure!(
            current.trim() == wanted || unchanged || current.trim().is_empty(),
            "native memory {} changed since export; reconcile its portable correction before continuing",
            note.path
        );
        if current.trim() != wanted {
            changes.push((file, wanted.to_string()));
        }
        revisions.insert(note.path.clone(), note.clone());
    }
    // Validate every revision before changing any native file. Repeating a
    // partly completed write is safe because an already applied value matches.
    for (file, wanted) in changes {
        write_outside(&file, &wanted)?;
    }
    keep(home, adapter, &revisions)
}

pub fn outside(text: &str) -> String {
    outside_block(text).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrections_and_deletions_change_native_memory_and_stale_edits_conflict() {
        let home = link_test_kit::TempDir::new("memory-revisions");
        let root = home.join("native");
        let file = root.join("MEMORY.md");
        write_outside(&file, "Use the old address").unwrap();
        let original = Note {
            path: "MEMORY.md".into(),
            text: "Use the old address".into(),
            source: Some("example:MEMORY.md".into()),
            deleted: false,
        };
        exported(&home, "example", vec![original.clone()]).unwrap();
        let corrected = Note {
            text: "Use the corrected address".into(),
            ..original.clone()
        };
        reconcile(
            &home,
            &root,
            "example",
            std::slice::from_ref(&corrected),
            outside,
        )
        .unwrap();
        assert_eq!(outside(&read(&file).unwrap().unwrap()), corrected.text);
        let deleted = Note {
            text: String::new(),
            deleted: true,
            ..corrected
        };
        reconcile(
            &home,
            &root,
            "example",
            std::slice::from_ref(&deleted),
            outside,
        )
        .unwrap();
        assert!(outside(&read(&file).unwrap().unwrap()).is_empty());
        assert!(exported(&home, "example", Vec::new()).unwrap()[0].deleted);
        write_outside(&file, "A divergent native edit").unwrap();
        assert!(reconcile(&home, &root, "example", &[deleted], outside).is_err());
        assert_eq!(
            outside(&read(&file).unwrap().unwrap()),
            "A divergent native edit"
        );
    }
}
