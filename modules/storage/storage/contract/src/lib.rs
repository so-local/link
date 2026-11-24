//! What `lnk storage` asks of the modules built on it
//! ([`link_core::answers`]): the folders here that are an owner's, not
//! yours ([`FOLDERS`]), an owned folder kept on the cloud account that
//! hosts it, pushed or pulled ([`HOME`]), and the ID of an owner named
//! by its kind and name ([`OWNER`]).
//!
//! An owner is named by a stable ID, `<kind>:<id>` ([`Owner`]), on
//! `link-id`'s one generic. Its kind is open: the module that keeps a
//! kind names it, and `lnk storage` treats it as data.

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use link_core::answers::Question;
use link_id::{Id, Kind, NotOne};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A kind of owner, as written before its ID's first `:`: open, each
/// named by the module that keeps it, so this contract names none.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OwnerKind(String);

impl OwnerKind {
    /// The kind `name` names, if it can name one
    /// ([`link_id::is_kind_name`]).
    pub fn new(name: &str) -> Result<Self, NotOne> {
        link_id::kind(name)
    }

    /// As it's written.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Kind for OwnerKind {
    const NOUN: &'static str = "owner";
    fn parse(text: &str) -> Option<Self> {
        link_id::is_kind_name(text).then(|| Self(text.to_string()))
    }
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OwnerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for OwnerKind {
    type Err = NotOne;
    fn from_str(text: &str) -> Result<Self, NotOne> {
        link_id::kind(text)
    }
}

impl Serialize for OwnerKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for OwnerKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// Whose a folder is, `<kind>:<id>`, as the module that keeps the kind
/// names it.
pub type Owner = Id<OwnerKind>;

/// The folders here that are an owner's, so `lnk storage` never takes
/// one for your files, and backs each up as that owner's.
pub const FOLDERS: Question<(), Folders> =
    Question::new("storage.folders", include_str!("folders.toml"));

/// [`FOLDERS`]'s answer.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Folders {
    /// Each owner's folder here.
    pub owned: Vec<Owned>,
}

/// An owner's folder here.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Owned {
    pub owner: Owner,
    /// The owner's name, to show.
    pub name: String,
    /// Its folder here, absolute.
    pub root: PathBuf,
    /// Whether the owner runs now, and so could change its files; none
    /// when that can't be said.
    #[serde(default)]
    pub running: Option<bool>,
    /// The cloud account that keeps it, when it's kept there, not in
    /// your buckets: this folder is where it's pushed from and pulled to,
    /// through [`HOME`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

/// `lnk storage push` and `pull` for an owned folder kept on the cloud
/// account that hosts it ([`Owned::account`]): pushed from, or pulled to,
/// its folder here, through that account. What it says on stderr is
/// shown as it comes.
pub const HOME: Question<Home, Moved> = Question::new("storage.home", include_str!("home.toml"));

/// [`HOME`]'s question.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Home {
    pub way: Way,
    /// The folder, its account named.
    pub owned: Owned,
    /// What to push (relative to its folder here), or what to pull
    /// (patterns); none, all of it.
    #[serde(default)]
    pub paths: Vec<String>,
}

/// Which way [`HOME`] goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Way {
    Push,
    Pull,
}

/// What [`HOME`] moved.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Moved {
    /// The files that went.
    pub sent: Vec<String>,
    /// The files the other end held the same already.
    pub same: usize,
}

/// `--owner <kind>:<name>`'s question to the modules that keep owners:
/// the ID of the owner of that kind and name, so `lnk storage` keeps
/// only its ID. A module takes its own ID in place of a name.
pub const OWNER: Question<OwnerName, Found> =
    Question::new("storage.owner", include_str!("owner.toml"));

/// [`OWNER`]'s input: an owner as a person names it, `<kind>:<name>`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerName {
    pub kind: OwnerKind,
    /// Its name, or its ID's id.
    pub name: String,
}

/// [`OWNER`]'s answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Found {
    /// Here, by its ID, and its name, to show.
    Here { owner: Owner, name: String },
    /// A kind this module keeps, with that name, but no ID yet.
    Unstarted,
    /// A kind this module keeps, but none of that name.
    None,
    /// Not a kind this module keeps.
    Unkept,
}

#[cfg(test)]
mod tests {

    /// Each question this owns declares its protocol beside it.
    #[test]
    fn its_questions_declare_their_protocols() {
        for protocol in [FOLDERS.protocol(), HOME.protocol(), OWNER.protocol()] {
            assert!(protocol.oldest <= protocol.current, "{protocol:?}");
        }
    }
    use super::*;

    /// An owner is any open kind and an opaque ID, written as one string.
    #[test]
    fn an_owner_is_an_open_kind_and_an_id() {
        let owner: Owner = "group:t4m9q2x7k1ab".parse().unwrap();
        assert_eq!(owner.kind().as_str(), "group");
        assert_eq!(owner.id(), "t4m9q2x7k1ab");
        assert_eq!(
            serde_json::to_string(&owner).unwrap(),
            "\"group:t4m9q2x7k1ab\""
        );
        for not in ["t4m9q2x7k1ab", "Group:x", "group:", "group:a b", ":x"] {
            assert!(not.parse::<Owner>().is_err(), "{not}");
        }
    }

    /// An answer carries the ID, never a name in its place, and a key it
    /// doesn't know is refused.
    #[test]
    fn answers_are_read_as_written() {
        let here: Found =
            serde_json::from_str(r#"{"here": {"owner": "job:k3v9q2x7m1ab", "name": "main"}}"#)
                .unwrap();
        assert_eq!(
            here,
            Found::Here {
                owner: "job:k3v9q2x7m1ab".parse().unwrap(),
                name: "main".into()
            }
        );
        assert!(
            serde_json::from_str::<Found>(r#"{"here": {"owner": "main", "name": "main"}}"#)
                .is_err()
        );
        assert_eq!(
            serde_json::from_str::<Found>(r#""unkept""#).unwrap(),
            Found::Unkept
        );
        let folders: Folders = serde_json::from_str(
            r#"{"owned": [{"owner": "group:t4m9q2x7k1ab", "name": "research", "root": "/t", "account": "link"}]}"#,
        )
        .unwrap();
        assert_eq!(folders.owned[0].account.as_deref(), Some("link"));
        assert_eq!(folders.owned[0].running, None);
        assert!(serde_json::from_str::<Folders>(r#"{"owned": [], "hosted": []}"#).is_err());
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
