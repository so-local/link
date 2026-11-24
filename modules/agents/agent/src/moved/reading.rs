//! The notes in a folder, each read or said unreadable by its file: a
//! list shows the rest, and everything else refuses the first.

use std::path::Path;

use link_format::WithinExt;

use super::Note;

/// The notes in a folder: those read, oldest first, and why each other
/// one couldn't be, naming its file.
pub struct Read {
    pub notes: Vec<Note>,
    pub unreadable: Vec<anyhow::Error>,
}

/// The notes in folder `dir`, refused at the first it can't read, as
/// [`super::notes`] reads them.
#[cfg(test)]
pub(super) fn notes_in(dir: &Path) -> anyhow::Result<Vec<Note>> {
    let read = read_in(dir)?;
    match read.unreadable.into_iter().next() {
        Some(err) => Err(err),
        None => Ok(read.notes),
    }
}

/// Each note in folder `dir`, read or said unreadable.
pub(super) fn read_in(dir: &Path) -> anyhow::Result<Read> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Read {
                notes: Vec::new(),
                unreadable: Vec::new(),
            });
        }
        Err(e) => return Err(e).within(format!("reading {}", dir.display())),
    };
    let mut notes = Vec::new();
    let mut unreadable = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|x| x != "toml") {
            continue;
        }
        let note = std::fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|text| link_files::saved::parse::<Note>(&text))
            .within(format!("reading {}", path.display()));
        let note = match note {
            Ok(note) => note,
            Err(err) => {
                unreadable.push(err);
                continue;
            }
        };
        if link_machine::machine::check_name(&note.name).is_ok()
            && note
                .id
                .as_deref()
                .is_none_or(|id| link_machine::machine::check_id(id).is_ok())
        {
            notes.push(note);
        }
    }
    notes.sort_by_key(|note| note.at);
    Ok(Read { notes, unreadable })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::moved::Place;

    /// A list names a note it can't read and reads the rest.
    #[test]
    fn a_list_names_an_unreadable_note_and_reads_the_rest() {
        let fixture = link_test_kit::TempDir::new("moved-listed");
        let dir = fixture.canonicalize().unwrap();
        let note = Note::new(None, "main", Place::of_account("acme", None));
        let text = link_files::saved::print(&note).unwrap();
        std::fs::write(dir.join("main.toml"), &text).unwrap();
        std::fs::write(
            dir.join("work.toml"),
            text.replace("schema = 1", "schema = 2"),
        )
        .unwrap();
        let read = read_in(&dir).unwrap();
        assert_eq!(read.notes.len(), 1);
        assert_eq!(read.notes[0].name, "main");
        let said = format!("{:#}", read.unreadable[0]);
        assert!(said.contains("work.toml"), "{said}");
    }
}
