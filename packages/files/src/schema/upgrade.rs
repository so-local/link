//! What an upgrade needs of a format: whether its files hold state or
//! history, where they are, and the records it takes from another owner.
//! `lnk upgrade` converts every file of state to its current schema and
//! runs the transfers, after the person's yes; a file of history is never
//! rewritten, only read line by line as each line's schema says
//! ([`crate::upgrade`]).

use std::collections::{BTreeMap, BTreeSet};

use anyhow::ensure;
use serde::{Deserialize, Serialize};

/// What a format's files hold.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Holds {
    /// What is so now (settings, grants, journals of what's under way),
    /// which an upgrade converts to the current schema.
    #[default]
    State,
    /// What happened (a conversation, a log, a ledger), never rewritten:
    /// its steps read each line as written, and so are never deleted.
    History,
}

/// How a file is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Encoding {
    /// One TOML document, rewritten keeping its comments.
    Toml,
    /// One JSON object.
    Json,
    /// JSON lines, each naming its own schema.
    Lines,
}

/// Where files of a format are: a location of the map, by its key
/// (`lnk.secrets`), and, inside it, a path where a `*` stands for any
/// part of one name (`*/grants.toml`, `*.toml`). With no path, the
/// location is the file.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Place {
    pub location: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub encoding: Encoding,
}

impl Place {
    /// The segments of its path with a `*`, which a transfer's two places
    /// match one to one.
    pub fn wildcards(&self) -> usize {
        self.segments().filter(|s| s.contains('*')).count()
    }

    pub(crate) fn segments(&self) -> impl Iterator<Item = &str> {
        self.path.iter().flat_map(|path| path.split('/'))
    }

    fn check(&self, kind: &str) -> anyhow::Result<()> {
        ensure!(
            !self.location.trim().is_empty(),
            "{kind}: a place names its location"
        );
        for segment in self.segments() {
            ensure!(
                !segment.is_empty()
                    && segment != "."
                    && segment != ".."
                    && segment.matches('*').count() <= 1,
                "{kind}: {:?} isn't a path inside {}: name each folder, with at most one * in a name",
                self.path.as_deref().unwrap_or_default(),
                self.location
            );
        }
        Ok(())
    }
}

/// Records another owner's file gives up to this format's file: the
/// source's kind, the schema it's in when they're taken, where it is,
/// each dotted path taken with where it goes, and optionally this
/// owner's code that turns what's taken into this format's shape.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Transfer {
    /// The source's kind.
    pub from: String,
    /// The source's schema when its records are taken: its next one
    /// refuses each path it gave up, naming this owner.
    pub schema: u32,
    pub source: Place,
    /// Each dotted path taken from the source, and the dotted path it
    /// becomes in this format's file.
    pub take: BTreeMap<String, String>,
    /// This owner's code that turns what's taken into its shape, named
    /// by [`name`] and registered as a step's is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migration: Option<String>,
    /// The release that brought it.
    pub since: link_version::Version,
    /// The release that deletes it.
    pub until: link_version::Version,
}

/// The name a transfer from `from`'s schema `schema` into `kind` gives
/// its code.
pub fn name(kind: &str, from: &str, schema: u32) -> String {
    format!("{kind}_from_{from}_{schema}")
}

/// A dotted path a step or transfer names: never the envelope, never an
/// empty segment.
fn path_ok(path: &str) -> bool {
    path != "schema" && path != "kind" && super::path::check(path)
}

/// Whether the location named `location` is control, which only a Safe
/// holds. The map is, at its fixed path.
fn control(location: &str) -> anyhow::Result<bool> {
    use crate::layout::{Hidden, Kind, map::Entry};
    if location == Entry::Hidden(Hidden::Map).name() {
        return Ok(true);
    }
    Ok(Entry::parse(location)?.kind() == Kind::Control)
}

pub(super) fn check(format: &super::Format) -> anyhow::Result<()> {
    let kind = &format.kind;
    match format.holds {
        Holds::History => ensure!(
            format.files.is_empty() && format.transfers.is_empty(),
            "{kind} holds history, which an upgrade never rewrites: it lists no files and \
             takes no records"
        ),
        Holds::State => {
            for step in &format.steps {
                ensure!(
                    step.until.is_some(),
                    "{kind} schema step {} names no until: only a format of history keeps its \
                     steps forever (holds = \"history\")",
                    step.from
                );
            }
        }
    }
    for place in &format.files {
        place.check(kind)?;
    }
    if format.transfers.is_empty() {
        return Ok(());
    }
    ensure!(
        format.files.len() == 1,
        "{kind} takes records into its file, so it lists exactly one place for it"
    );
    let into = &format.files[0];
    let mut sources = BTreeSet::new();
    for transfer in &format.transfers {
        let from = &transfer.from;
        ensure!(
            !from.trim().is_empty() && from != kind && transfer.schema > 0,
            "{kind} takes records from another kind, at a positive schema"
        );
        ensure!(
            sources.insert((from, transfer.schema)),
            "{kind} takes from {from} schema {} twice: say it in one transfer",
            transfer.schema
        );
        transfer.source.check(kind)?;
        ensure!(
            transfer.source != *into,
            "{kind} takes from {from} out of its own file: a transfer moves records between two \
             owners' files"
        );
        ensure!(
            control(&transfer.source.location)?,
            "{kind} takes from {from} in {}, outside control: a transfer reads only what a Safe \
             holds",
            transfer.source.location
        );
        ensure!(
            transfer.source.encoding == Encoding::Toml && into.encoding == Encoding::Toml,
            "{kind} takes records between TOML files, never JSON or lines"
        );
        ensure!(
            transfer.source.wildcards() == into.wildcards(),
            "{kind} takes from {from}: each * of its source stands for one of its own file's"
        );
        ensure!(
            !transfer.take.is_empty(),
            "{kind} takes from {from}: name the paths it takes"
        );
        let mut targets = BTreeSet::new();
        for (old, new) in &transfer.take {
            ensure!(
                path_ok(old) && path_ok(new),
                "{kind} takes from {from}: invalid path {old:?} or {new:?}"
            );
            ensure!(
                targets.insert(new),
                "{kind} takes two paths of {from} into {new}"
            );
        }
        if let Some(named) = &transfer.migration {
            let wanted = name(kind, from, transfer.schema);
            ensure!(
                *named == wanted,
                "{kind}'s transfer from {from} names {named:?}: name it {wanted:?}"
            );
        }
        ensure!(
            transfer.since < transfer.until,
            "{kind}'s transfer from {from} is deleted at {}, before or as it came at {}",
            transfer.until,
            transfer.since
        );
    }
    Ok(())
}
