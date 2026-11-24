//! What the archive module asks of the modules whose kinds it carries,
//! each answered from files alone: no network call, nothing started or
//! stopped. A module owning a kind in `layout.toml`'s `[owners]` answers
//! all five for its things: [`CHECK`], [`RUNNING`], [`CARRY`], [`MISSING`]
//! and [`ADOPT`].

use std::collections::BTreeMap;
use std::path::PathBuf;

use link_core::answers::Question;
use serde::{Deserialize, Serialize};

/// A thing and its folders, each by the key of the location it's a
/// folder of (`"link.agent-settings"`): where it is here, or where an
/// import staged it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Thing {
    /// Its kind, as an archive selects it (`agent`).
    pub kind: String,
    /// Its name.
    pub name: String,
    /// Its folders, absolute.
    pub folders: BTreeMap<String, PathBuf>,
}

/// Reads a thing's files with their owning types, refusing what they
/// refuse, or what its files alone don't carry whole: failing, it says
/// why on stderr.
pub const CHECK: Question<Thing, Checked> =
    Question::new("archive.check", include_str!("check.toml"));

/// [`CHECK`]'s answer: its files are its owner's, whole.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checked {}

/// Whether a thing is running here, so export refuses it.
pub const RUNNING: Question<Thing, Running> =
    Question::new("archive.running", include_str!("running.toml"));

/// [`RUNNING`]'s answer.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Running {
    /// The command that stops it, while it runs; none when it doesn't.
    #[serde(default)]
    pub stop: Option<String>,
}

/// What of a thing's files an export carries in place of its own, so
/// the archive holds nothing of this machine's: the owner writes each
/// file it changes into the staging folder it's given.
pub const CARRY: Question<Carry, Carried> =
    Question::new("archive.carry", include_str!("carry.toml"));

/// [`CARRY`]'s question.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Carry {
    /// The thing, in its folders here.
    pub thing: Thing,
    /// An empty folder of the asker's, where the owner writes the files
    /// it carries in place of its own.
    pub staging: PathBuf,
}

/// [`CARRY`]'s answer: each file of the thing carried in place of its
/// own, or left out; none when its files go as they are.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Carried {
    #[serde(default)]
    pub replaced: Vec<Replaced>,
}

/// One file carried in place of another, or left out.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Replaced {
    /// The thing's file, absolute, inside one of its folders.
    pub file: PathBuf,
    /// What goes in its place, absolute, inside the staging folder; none
    /// to leave it out.
    #[serde(default)]
    pub with: Option<PathBuf>,
}

/// What an imported thing still needs to run here, read from its files.
pub const MISSING: Question<Thing, Missing> =
    Question::new("archive.missing", include_str!("missing.toml"));

/// [`MISSING`]'s answer.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Missing {
    pub needs: Vec<Need>,
}

/// One thing it needs, and the one command that sets it up, which
/// import names and never runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Need {
    pub what: String,
    pub setup: String,
}

/// Writes the owner's records in `~/.lnk` for a thing arrived in its
/// folders, under the name chosen.
pub const ADOPT: Question<Adopt, Adopted> =
    Question::new("archive.adopt", include_str!("adopt.toml"));

/// [`ADOPT`]'s question.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Adopt {
    /// The thing, under the name chosen, in its folders here.
    pub thing: Thing,
    /// Its name in the archive.
    pub was: String,
}

/// [`ADOPT`]'s answer.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Adopted {
    /// What to tell you of it.
    #[serde(default)]
    pub notes: Vec<String>,
    /// The command that starts it, once what it needs is set up.
    pub next: String,
}

/// The questions, by name, each owner answers all of.
pub const QUESTIONS: [&str; 5] = [
    CHECK.name,
    RUNNING.name,
    CARRY.name,
    MISSING.name,
    ADOPT.name,
];

#[cfg(test)]
link_test_kit::program!(unit);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_question_reads_its_protocol() {
        for protocol in [
            CHECK.protocol(),
            RUNNING.protocol(),
            CARRY.protocol(),
            MISSING.protocol(),
            ADOPT.protocol(),
        ] {
            assert_eq!(protocol.current, 1);
        }
        assert_eq!(QUESTIONS[0], "archive.check");
    }
}
