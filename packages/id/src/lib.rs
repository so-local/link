//! A stable identity, `<kind>:<id>`, by what never changes: the one
//! generic ([`Id`]) every kind of ID rides, each with its own set of
//! kinds ([`Kind`]), closed or open. A principal (`link-principal`) is who acts; a
//! [`Workload`] is what runs, kept across its runs; an [`Execution`] is
//! one run of a workload, `<workload>@<run>`. One agent is all three:
//! it acts as the principal `service:<id>`, which holds roles, the
//! workload `agent:<id>` is measured and placed, and each start is an
//! execution.
//! An ID's owner draws it ([`candidate`]) and reserves it under its own
//! lock ([`Ledger`]), so it never names two things; an execution's parent
//! is a field beside it ([`Lineage`]).

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

mod candidate;
mod limits;
mod lineage;
mod output;
mod reserving;

pub use candidate::{NoRandomness, candidate, is_candidate};
pub use limits::{Limits, limits};
pub use lineage::Lineage;
pub use output::{axes, candidate_of, is_axis};
pub use reserving::{Ledger, Refused, Table, admit, reserve, reserve_from};

/// The longest an execution's run is, in bytes.
pub const RUN_MAX: usize = 64;

/// The bytes besides ASCII letters and digits an execution's run may
/// hold: none that's an ID's own, so `<workload>@<run>` splits one way.
pub const RUN_PUNCTUATION: &str = "._-";

/// What separates an execution's workload from its run: no ID holds it.
pub const RUN_SEPARATOR: char = '@';

/// A set of kinds an ID is one of, as each is written before its first
/// `:`: a closed list (a principal's, which the account service's API
/// fixes) or open, each owner naming its own (a workload's).
pub trait Kind: Clone + Eq + Ord + Hash + fmt::Debug + Send + Sync + 'static {
    /// What an ID of these kinds is called, as an error says it.
    const NOUN: &'static str;
    /// The kind `text` writes, if it's one.
    fn parse(text: &str) -> Option<Self>;
    /// As it's written.
    fn as_str(&self) -> &str;
}

/// The kind `text` writes, of `K`'s.
pub fn kind<K: Kind>(text: &str) -> Result<K, NotOne> {
    K::parse(text).ok_or_else(|| NotOne(format!("not a {}'s kind", K::NOUN)))
}

/// The longest an open kind's name is, in bytes.
pub const KIND_MAX: usize = 30;

/// Whether `text` can name an open kind: lowercase ASCII letters, digits
/// and `-`, from a letter, at most [`KIND_MAX`].
pub fn is_kind_name(text: &str) -> bool {
    text.len() <= KIND_MAX
        && text.starts_with(|c: char| c.is_ascii_lowercase())
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Whether `id` is an ID's id: one Link minted, `limits.toml`'s
/// `candidate_length` of its lowercase letters and digits
/// ([`is_candidate`]). A thing whose own ID comes from outside Link gets a
/// minted ID, its outside ID a field beside it.
pub fn is_id(id: &str) -> bool {
    is_candidate(id)
}

fn opaque(text: &str, max: usize, punctuation: &str) -> bool {
    (1..=max).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || punctuation.as_bytes().contains(&b))
}

/// What isn't an ID, or one's kind, saying why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotOne(pub(crate) String);

impl fmt::Display for NotOne {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NotOne {}

/// An ID, `<kind>:<id>`: a kind of `K`'s, then an id Link minted
/// ([`is_id`]). Made only by parsing or from a kind and an id checked
/// alike, so every one is well formed.
pub struct Id<K> {
    text: String,
    kind: PhantomData<K>,
}

impl<K: Kind> Id<K> {
    /// The ID of `kind` whose id is `id`.
    pub fn new(kind: &K, id: &str) -> Result<Self, NotOne> {
        match is_id(id) {
            true => Ok(Self {
                text: format!("{}:{id}", kind.as_str()),
                kind: PhantomData,
            }),
            false => Err(NotOne(format!("not a {}'s id", K::NOUN))),
        }
    }

    /// The ID whose kind `kind` writes and whose id is `id`.
    pub fn of(kind: &str, id: &str) -> Result<Self, NotOne> {
        Self::new(&self::kind(kind)?, id)
    }

    /// Its kind, before its first `:`.
    pub fn kind(&self) -> K {
        let (kind, _) = self.text.split_once(':').expect("parsed with a colon");
        self::kind(kind).expect("parsed with a kind")
    }

    /// Its id, after its first `:`.
    pub fn id(&self) -> &str {
        let (_, id) = self.text.split_once(':').expect("parsed with a colon");
        id
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl<K: Kind> FromStr for Id<K> {
    type Err = NotOne;
    fn from_str(text: &str) -> Result<Self, NotOne> {
        let (kind, id) = text
            .split_once(':')
            .ok_or_else(|| NotOne(format!("not a {}: <kind>:<id>", K::NOUN)))?;
        Self::new(&self::kind(kind)?, id)
    }
}

impl<K: Kind> TryFrom<String> for Id<K> {
    type Error = NotOne;
    fn try_from(text: String) -> Result<Self, NotOne> {
        text.parse()
    }
}

impl<K: Kind> TryFrom<&str> for Id<K> {
    type Error = NotOne;
    fn try_from(text: &str) -> Result<Self, NotOne> {
        text.parse()
    }
}

impl<K> From<Id<K>> for String {
    fn from(id: Id<K>) -> Self {
        id.text
    }
}

impl<K> std::ops::Deref for Id<K> {
    type Target = str;
    fn deref(&self) -> &str {
        &self.text
    }
}

impl<K> fmt::Display for Id<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl<K> fmt::Debug for Id<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.text, f)
    }
}

impl<K> Clone for Id<K> {
    fn clone(&self) -> Self {
        Self {
            text: self.text.clone(),
            kind: PhantomData,
        }
    }
}

impl<K> PartialEq for Id<K> {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
    }
}

impl<K> Eq for Id<K> {}

impl<K> PartialOrd for Id<K> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<K> Ord for Id<K> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.text.cmp(&other.text)
    }
}

impl<K> Hash for Id<K> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text.hash(state);
    }
}

impl<K> Serialize for Id<K> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.text)
    }
}

impl<'de, K: Kind> Deserialize<'de> for Id<K> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// A kind of what runs, kept across its runs: open, each named by the
/// module that keeps it (`agent`, `job`), so this package names none.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkloadKind(String);

impl WorkloadKind {
    /// The kind `name` names, if it can name one ([`is_kind_name`]).
    pub fn new(name: &str) -> Result<Self, NotOne> {
        kind(name)
    }
}

impl Kind for WorkloadKind {
    const NOUN: &'static str = "workload";
    fn parse(text: &str) -> Option<Self> {
        is_kind_name(text).then(|| Self(text.to_string()))
    }
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorkloadKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for WorkloadKind {
    type Err = NotOne;
    fn from_str(text: &str) -> Result<Self, NotOne> {
        kind(text)
    }
}

impl Serialize for WorkloadKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for WorkloadKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// What runs, kept across its runs, `<kind>:<id>`, as its owner names it:
/// `agent:<its ID>`, `job:<its ID>`.
pub type Workload = Id<WorkloadKind>;

/// A kind of what's acted on: open, each named by the module that keeps
/// it (`box`, `bucket`, `team`, `machine`), so this package names none.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceKind(String);

impl Kind for ResourceKind {
    const NOUN: &'static str = "resource";
    fn parse(text: &str) -> Option<Self> {
        is_kind_name(text).then(|| Self(text.to_string()))
    }
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl ResourceKind {
    /// The kind `name` names, if it can name one ([`is_kind_name`]).
    pub fn new(name: &str) -> Result<Self, NotOne> {
        kind(name)
    }
}

impl fmt::Display for ResourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ResourceKind {
    type Err = NotOne;
    fn from_str(text: &str) -> Result<Self, NotOne> {
        kind(text)
    }
}

impl Serialize for ResourceKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ResourceKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// What's acted on, `<kind>:<id>`, as its owner names it: `box:<its
/// installation ID>`, `group:<its ID>`.
pub type Resource = Id<ResourceKind>;

/// One run of a workload, `<workload>@<run>`: the workload's ID, then
/// the run's, 1 to [`RUN_MAX`] ASCII letters, digits and
/// [`RUN_PUNCTUATION`], allocated before the run starts and kept with
/// its outcome, never a process's pid, which is reused.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Execution {
    workload: Workload,
    run: String,
}

impl Execution {
    /// The run `run` of `workload`.
    pub fn new(workload: Workload, run: &str) -> Result<Self, NotOne> {
        match opaque(run, RUN_MAX, RUN_PUNCTUATION) {
            true => Ok(Self {
                workload,
                run: run.to_string(),
            }),
            false => Err(NotOne("not an execution's run".to_string())),
        }
    }

    /// The workload it's a run of.
    pub fn workload(&self) -> &Workload {
        &self.workload
    }

    /// Its run, after the workload.
    pub fn run(&self) -> &str {
        &self.run
    }
}

impl FromStr for Execution {
    type Err = NotOne;
    fn from_str(text: &str) -> Result<Self, NotOne> {
        let (workload, run) = text
            .split_once(RUN_SEPARATOR)
            .ok_or_else(|| NotOne("not an execution: <workload>@<run>".to_string()))?;
        Self::new(workload.parse()?, run)
    }
}

impl TryFrom<String> for Execution {
    type Error = NotOne;
    fn try_from(text: String) -> Result<Self, NotOne> {
        text.parse()
    }
}

impl From<Execution> for String {
    fn from(execution: Execution) -> Self {
        execution.to_string()
    }
}

impl fmt::Display for Execution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{RUN_SEPARATOR}{}", self.workload, self.run)
    }
}

impl Serialize for Execution {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Execution {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
link_test_kit::program!(unit);
