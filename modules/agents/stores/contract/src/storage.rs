//! Where an agent's things are kept: its six collections, each at a
//! place, settled in layers (`docs/architecture/specs/README.md`,
//! "Where an agent's things are kept"). The agent prefers a place for
//! each (its spec's `[storage]`, [`Storage`]); the
//! machine lists the places it supports for each, its default first
//! (`agents.toml`'s `[storage]`, [`Supported`]); the preference holds
//! where the machine lists it, else the collection is kept at the
//! machine's default ([`Supported::resolve`]).
//!
//! [`FILE`] is the agent's own folder on this machine. Any other place is
//! a driver's, reached through the storage bridge: what it keeps is read
//! and written there, never on this machine's disk.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};

/// Where the agent would keep each of its collections (`[storage]`),
/// declared in the store contract's `types.toml`.
pub use crate::declared::Storage;

/// One of an agent's collections.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Collection {
    /// The person's: documents, work, and the documents memory searches.
    Home,
    /// Its instructions, which the person edits.
    Instructions,
    /// Its skills, which the person edits.
    Skills,
    /// Memory's notes and index.
    Memory,
    /// Its conversations.
    Conversations,
    /// Its scheduled jobs.
    Schedule,
}

impl Collection {
    /// Every one, in the order the specs list them.
    pub const ALL: [Collection; 6] = [
        Collection::Home,
        Collection::Instructions,
        Collection::Skills,
        Collection::Memory,
        Collection::Conversations,
        Collection::Schedule,
    ];

    /// Its name, as specs write it.
    pub const fn name(self) -> &'static str {
        match self {
            Collection::Home => "home",
            Collection::Instructions => "instructions",
            Collection::Skills => "skills",
            Collection::Memory => "memory",
            Collection::Conversations => "conversations",
            Collection::Schedule => "schedule",
        }
    }

    /// Its folder on disk. Specs name the collection, while people
    /// browse their conversations in `chats/`.
    pub const fn directory(self) -> &'static str {
        match self {
            Collection::Conversations => link_harness_base::layout::CONVERSATIONS,
            other => other.name(),
        }
    }

    /// The collection of `among` a path names by its first part
    /// (`instructions/base.md`, or `./instructions/base.md`, names
    /// [`Collection::Instructions`]): none for a path in none of them.
    /// Every classification of a path by its collection is this one, over
    /// the collections its space names ([`Collection::of_path`],
    /// [`test_seen_collection`]).
    pub fn named_by(path: &str, among: &[Collection]) -> Option<Collection> {
        let first = path.trim_start_matches("./").split('/').next()?;
        among.iter().copied().find(|c| c.directory() == first)
    }

    /// The collection a path a driver keeps is in ([`crate::store`]'s
    /// files), by its first part: any but Home, so `memory/` is memory's
    /// (its index's copy). Not what the agent sees, where `memory/` is
    /// Home's documents ([`test_seen_collection`]).
    pub fn of_path(path: &str) -> Option<Collection> {
        const KEPT: [Collection; 5] = [
            Collection::Instructions,
            Collection::Skills,
            Collection::Memory,
            Collection::Conversations,
            Collection::Schedule,
        ];
        Collection::named_by(path, &KEPT)
    }
}

/// The agent's own folder on this machine: the place every collection is
/// kept unless something says otherwise.
pub const FILE: &str = "file";

/// A SQLite file on this machine, its driver `lnk-sqlite-store`.
pub const SQLITE: &str = "sqlite";

/// Link's own places: the agent's folder, a SQLite file, and its bucket.
pub const LINK_PLACES: [&str; 3] = [FILE, SQLITE, "bucket"];

/// The places of Link's that keep `collection`: all of them, but SQLite
/// for Home.
pub fn link_places(collection: Collection) -> Vec<String> {
    LINK_PLACES
        .into_iter()
        .filter(|place| !(collection == Collection::Home && *place == SQLITE))
        .map(String::from)
        .collect()
}

/// An error unless `place` is a place's name: lowercase letters, digits
/// and dashes, starting with a letter.
pub fn check_place(place: &str) -> anyhow::Result<()> {
    crate::store::check_name(place)
        .with_context(|| format!("{:?} isn't a place's name", link_format::clean(place)))
}

/// What a machine supports (`agents.toml`'s `[storage]`): for each
/// collection, the places it may be kept at here, the default first. A
/// collection it doesn't name is kept at Link's places that serve it,
/// [`FILE`] first ([`link_places`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Supported {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversations: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<Vec<String>>,
}

impl Supported {
    /// The places listed for `collection`, as written: none where it
    /// isn't named.
    fn listed(&self, collection: Collection) -> Option<&Vec<String>> {
        match collection {
            Collection::Home => self.home.as_ref(),
            Collection::Instructions => self.instructions.as_ref(),
            Collection::Skills => self.skills.as_ref(),
            Collection::Memory => self.memory.as_ref(),
            Collection::Conversations => self.conversations.as_ref(),
            Collection::Schedule => self.schedule.as_ref(),
        }
    }

    /// The places this machine supports for `collection`, its default
    /// first.
    pub fn places(&self, collection: Collection) -> Vec<String> {
        self.listed(collection)
            .cloned()
            .unwrap_or_else(|| link_places(collection))
    }

    /// An error unless each list is at least one place, each a place's
    /// name and none twice, and none of Link's places for a collection
    /// it doesn't keep (SQLite for Home).
    pub fn check(&self) -> anyhow::Result<()> {
        for collection in Collection::ALL {
            let Some(places) = self.listed(collection) else {
                continue;
            };
            let name = collection.name();
            ensure!(
                !places.is_empty(),
                "[storage] {name} lists no place: name at least one, its default first"
            );
            for (n, place) in places.iter().enumerate() {
                check_place(place).with_context(|| format!("[storage] {name}"))?;
                ensure!(
                    !places[..n].contains(place),
                    "[storage] {name} lists {place} twice"
                );
                ensure!(
                    !(collection == Collection::Home && place == SQLITE),
                    "[storage] home can't be kept at sqlite: SQLite keeps all but Home"
                );
            }
        }
        Ok(())
    }

    /// Where each of an agent's collections is kept on this machine: the
    /// place `preferred` names where this machine supports it, else its
    /// default, marked as overriding the agent's.
    pub fn resolve(&self, preferred: &Storage) -> Places {
        let mut places = BTreeMap::new();
        for collection in Collection::ALL {
            let supported = self.places(collection);
            let wanted = preferred.place(collection);
            let place = match supported.contains(&wanted) {
                true => Place {
                    place: wanted,
                    overridden: false,
                },
                false => Place {
                    place: supported.first().cloned().unwrap_or_else(|| FILE.into()),
                    overridden: true,
                },
            };
            places.insert(collection, place);
        }
        Places(places)
    }
}

/// Where one collection is kept, and whether the machine overrode the
/// agent's preference for it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    pub place: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub overridden: bool,
}

/// Where each of an agent's collections is kept, resolved for one
/// machine ([`Supported::resolve`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Places(pub BTreeMap<Collection, Place>);

impl Places {
    /// Where `collection` is kept: [`FILE`] where nothing says.
    pub fn of(&self, collection: Collection) -> &str {
        self.0.get(&collection).map_or(FILE, |p| p.place.as_str())
    }

    /// Whether `collection` is kept at a place other than the agent's
    /// folder on this machine.
    pub fn elsewhere(&self, collection: Collection) -> bool {
        self.of(collection) != FILE
    }
}

/// Where an agent's collections are kept, as a harness is handed them:
/// each one's place, and the storage bridge's socket, each call proved
/// with the agent's key. A harness handed one reaches each collection
/// kept elsewhere through the bridge.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageAt {
    pub places: Places,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socket: Option<PathBuf>,
}

/// `[storage] conversations`: a line of JSON per event, in a file per
/// conversation, beside a copy for people.
pub const LOG: &str = "log";

/// `[storage] conversations`: the same events compressed, in a file per
/// conversation, read with `lnk agent chat show`.
pub const COMPACT: &str = "compact";

impl Storage {
    /// Where a new conversation is kept, as [`Self::conversations`]
    /// says: [`LOG`] when it doesn't, or names the agent's own folder
    /// ([`crate::storage::FILE`]), as a printed spec does.
    pub fn conversations(&self) -> &str {
        match self.conversations.as_deref() {
            None | Some(crate::storage::FILE) => LOG,
            Some(way) => way,
        }
    }

    /// The place it prefers for `collection`: [`crate::storage::FILE`]
    /// where it doesn't say, and for [`LOG`] or [`COMPACT`].
    pub fn place(&self, collection: crate::storage::Collection) -> String {
        use crate::storage::{Collection, FILE};
        let named = match collection {
            Collection::Home => &self.home,
            Collection::Instructions => &self.instructions,
            Collection::Skills => &self.skills,
            Collection::Memory => &self.memory,
            Collection::Conversations => &self.conversations,
            Collection::Schedule => &self.schedule,
        };
        match named.as_deref() {
            None | Some(LOG) | Some(COMPACT) => FILE.into(),
            Some(place) => place.into(),
        }
    }
}

#[cfg(test)]
fn test_seen_collection(path: &str) -> Option<Collection> {
    Collection::named_by(
        path,
        &[
            Collection::Instructions,
            Collection::Skills,
            Collection::Conversations,
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supported(toml: &str) -> Supported {
        let supported: Supported = toml::from_str(toml).unwrap();
        supported.check().unwrap();
        supported
    }

    /// The agent's own folder, named as a printed spec names it, keeps a
    /// conversation as the log does; a driver's name is its own.
    #[test]
    fn the_own_folder_keeps_conversations_as_the_log() {
        let way = |named: Option<&str>| {
            Storage {
                conversations: named.map(String::from),
                ..Storage::default()
            }
            .conversations()
            .to_string()
        };
        assert_eq!(way(None), LOG);
        assert_eq!(way(Some(FILE)), LOG);
        assert_eq!(way(Some("compact")), "compact");
        assert_eq!(way(Some("sqlite")), "sqlite");
    }

    /// An agent's preference holds where its machine lists the place;
    /// elsewhere it's the machine's default, marked as overridden.
    #[test]
    fn an_agent_prefers_and_its_machine_decides() {
        let machine = supported(
            "instructions = [\"postgres\"]\nskills = [\"postgres\"]\nconversations = [\"postgres\", \"file\"]\n",
        );
        let places = machine.resolve(&Storage::default());
        assert_eq!(places.of(Collection::Instructions), "postgres");
        assert!(places.0[&Collection::Instructions].overridden);
        assert_eq!(places.of(Collection::Skills), "postgres");
        // Supported here: the agent's file holds.
        assert_eq!(places.of(Collection::Conversations), FILE);
        assert!(!places.0[&Collection::Conversations].overridden);
        // Not named: Link's places, file first.
        assert_eq!(places.of(Collection::Home), FILE);
        assert!(!places.elsewhere(Collection::Home));
        assert!(places.elsewhere(Collection::Instructions));
        let sqlite = Storage {
            skills: Some("sqlite".into()),
            ..Storage::default()
        };
        // Link's sqlite isn't among this machine's places for skills.
        assert_eq!(machine.resolve(&sqlite).of(Collection::Skills), "postgres");
        // On a machine that names nothing, the agent's own holds.
        assert_eq!(
            Supported::default().resolve(&sqlite).of(Collection::Skills),
            "sqlite"
        );
    }

    /// A machine's list is a place's names, at least one, none twice, and
    /// never SQLite for Home.
    #[test]
    fn a_machines_places_are_checked() {
        for bad in [
            "home = []",
            "home = [\"file\", \"file\"]",
            "home = [\"sqlite\"]",
            "skills = [\"Not A Place\"]",
        ] {
            let parsed: Supported = toml::from_str(bad).unwrap();
            assert!(parsed.check().is_err(), "{bad}");
        }
        assert!(toml::from_str::<Supported>("pictures = [\"file\"]").is_err());
    }

    /// A path of the agent's files is in the collection its first part
    /// names.
    #[test]
    fn a_path_is_in_its_first_parts_collection() {
        assert_eq!(
            Collection::of_path("instructions/base.md"),
            Some(Collection::Instructions)
        );
        assert_eq!(
            Collection::of_path("skills/greet/SKILL.md"),
            Some(Collection::Skills)
        );
        assert_eq!(Collection::of_path("work/notes.md"), None);
        assert_eq!(Collection::of_path("home/x"), None);
        assert_eq!(
            Collection::of_path("chats/link-work/events.jsonl"),
            Some(Collection::Conversations)
        );
        // A driver's `memory/` is memory's; what the agent sees as
        // `memory/` is Home's documents.
        assert_eq!(
            Collection::of_path("./memory/index"),
            Some(Collection::Memory)
        );
        assert_eq!(test_seen_collection("memory/a.pdf"), None);
        assert_eq!(test_seen_collection("./skills/a"), Some(Collection::Skills));
    }
}
