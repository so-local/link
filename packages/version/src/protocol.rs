//! A protocol between two programs, as its owner declares it in a TOML
//! file beside its code: its name, the version this release speaks, the
//! oldest it still accepts, and what a peer too old to speak it runs to
//! catch up: once `oldest` is raised, through the bridge release, the
//! newest that still accepts the version below it. Both sides state their
//! version when they connect; each
//! refuses a peer below the oldest it accepts, by name
//! ([`Protocol::accept`]). A newer peer is the newer side's to judge, as
//! it knows both versions, but for a one-way message, a stream or a
//! command whose sender hears no version back: its reader refuses an older
//! one and a newer one alike ([`Protocol::read`]).

use std::fmt;

use serde::Deserialize;

/// A protocol, as its owner declares it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Protocol {
    /// What people call it (`bridge`, `answers`).
    pub name: String,
    /// The version this release speaks.
    pub current: u32,
    /// The oldest version this release still accepts of a peer.
    pub oldest: u32,
    /// What a peer too old runs to catch up (`lnk upgrade`).
    pub upgrade: String,
    /// The newest release that still accepts the version below `oldest`,
    /// which a peer too old upgrades to first: never named before `oldest`
    /// is raised past 1, and always once it is for a protocol `lnk upgrade`
    /// itself speaks, whose refusal `lnk upgrade` alone can't get past.
    #[serde(default)]
    pub bridge: Option<crate::Version>,
}

impl Protocol {
    /// The declaration in `text`: an error unless it's one, with
    /// `1 <= oldest <= current`, and a bridge named only once `oldest` is
    /// raised past 1.
    pub fn parse(text: &str) -> Result<Protocol, String> {
        let protocol: Protocol = toml::from_str(text).map_err(|e| e.to_string())?;
        if protocol.name.trim().is_empty() || protocol.upgrade.trim().is_empty() {
            return Err("a protocol names itself and its upgrade".into());
        }
        if protocol.oldest == 0 || protocol.oldest > protocol.current {
            return Err(format!(
                "protocol {}: the oldest it accepts ({}) is between 1 and its current ({})",
                protocol.name, protocol.oldest, protocol.current
            ));
        }
        if protocol.oldest == 1 && protocol.bridge.is_some() {
            return Err(format!(
                "protocol {}: a bridge release is named only once the oldest it accepts is past 1",
                protocol.name
            ));
        }
        Ok(protocol)
    }

    /// The declaration in `text` of a protocol `lnk upgrade` itself
    /// speaks, which comes with its owner: as [`Protocol::shipped`], and a
    /// panic unless a raised `oldest` names its bridge release, the one
    /// an older core upgrades through.
    pub fn shipped_upgrade(text: &str) -> Protocol {
        let protocol = Protocol::shipped(text);
        assert!(
            protocol.oldest == 1 || protocol.bridge.is_some(),
            "protocol {}: lnk upgrade speaks it, so its raised oldest names a bridge release",
            protocol.name
        );
        protocol
    }

    /// The version an answer to a peer that stated `asked` is written in:
    /// the asker's, so it reads what it understands, or this release's
    /// own where it stated none or a newer one. Only a version this
    /// release accepted ([`Protocol::accept`]) is answered, so an answer
    /// is never written in one below its oldest. A version that changes
    /// the shape keeps a writer for each it still accepts.
    pub fn answering(&self, asked: Option<Accepted>) -> u32 {
        asked.map_or(self.current, |asked| asked.0.min(self.current))
    }

    /// The declaration in `text`, which comes with its owner: a panic
    /// unless it's one, as a built-in declaration always is (its owner's
    /// tests read it).
    pub fn shipped(text: &str) -> Protocol {
        Protocol::parse(text).unwrap_or_else(|e| panic!("a built-in protocol declaration: {e}"))
    }

    /// Whether `peer` (as a person names it: "the bridge"), speaking
    /// `version`, is one this release talks to: an error naming it and
    /// its upgrade when it's below [`Protocol::oldest`].
    pub fn accept(&self, peer: &str, version: u32) -> Result<Accepted, Older> {
        match version >= self.oldest {
            true => Ok(Accepted(version)),
            false => Err(Older {
                protocol: self.name.clone(),
                peer: peer.to_string(),
                version,
                oldest: self.oldest,
                upgrade: self.upgrade.clone(),
                bridge: self.bridge.as_ref().map(ToString::to_string),
            }),
        }
    }
}

impl Protocol {
    /// Whether a one-way message from `peer`, stating `version`, is one
    /// this release reads: an older one is refused as [`Protocol::accept`]
    /// refuses it, and a newer one naming this release's upgrade, since
    /// its sender hears nothing back to judge by.
    pub fn read(&self, peer: &str, version: u32) -> Result<(), Unread> {
        self.accept(peer, version).map_err(Unread::Older)?;
        match version <= self.current {
            true => Ok(()),
            false => Err(Unread::Newer(Newer {
                protocol: self.name.clone(),
                peer: peer.to_string(),
                version,
                current: self.current,
                upgrade: self.upgrade.clone(),
            })),
        }
    }
}

/// A version a peer stated that [`Protocol::accept`] accepted, the only
/// kind [`Protocol::answering`] answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Accepted(u32);

impl Accepted {
    /// The version the peer stated.
    pub fn version(self) -> u32 {
        self.0
    }
}

/// A one-way message refused: from an older peer, or a newer one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unread {
    Older(Older),
    Newer(Newer),
}

impl fmt::Display for Unread {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Unread::Older(older) => older.fmt(f),
            Unread::Newer(newer) => newer.fmt(f),
        }
    }
}

impl std::error::Error for Unread {}

/// A one-way message refused for a protocol newer than this release's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Newer {
    pub protocol: String,
    pub peer: String,
    pub version: u32,
    pub current: u32,
    pub upgrade: String,
}

impl fmt::Display for Newer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} speaks the {} protocol {}, newer than {}, the newest this release reads: \
             upgrade this one with {}",
            self.peer, self.protocol, self.version, self.current, self.upgrade
        )
    }
}

impl std::error::Error for Newer {}

/// A peer refused for speaking a protocol below the oldest accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Older {
    pub protocol: String,
    pub peer: String,
    pub version: u32,
    pub oldest: u32,
    pub upgrade: String,
    /// The release it upgrades to first, which still accepts it.
    pub bridge: Option<String>,
}

impl fmt::Display for Older {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} speaks the {} protocol {}, below {}, the oldest this release accepts: ",
            self.peer, self.protocol, self.version, self.oldest
        )?;
        match &self.bridge {
            Some(bridge) => write!(
                f,
                "upgrade it with {} {bridge} first, the newest release accepting it, then with {}",
                self.upgrade, self.upgrade
            ),
            None => write!(f, "upgrade it with {}", self.upgrade),
        }
    }
}

impl std::error::Error for Older {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_older_peer_is_refused_by_name() {
        let protocol = Protocol::parse(
            "name = \"bridge\"\ncurrent = 3\noldest = 2\nupgrade = \"lnk upgrade\"\n",
        )
        .unwrap();
        assert_eq!(
            protocol.accept("the bridge", 2).map(Accepted::version),
            Ok(2)
        );
        assert_eq!(
            protocol.accept("the bridge", 4).map(Accepted::version),
            Ok(4)
        );
        let older = protocol.accept("the bridge", 1).unwrap_err();
        assert_eq!(
            older.to_string(),
            "the bridge speaks the bridge protocol 1, below 2, the oldest this release \
             accepts: upgrade it with lnk upgrade"
        );
    }

    #[test]
    fn a_one_way_message_is_read_from_its_own_versions_alone() {
        let protocol = Protocol::parse(
            "name = \"move\"\ncurrent = 4\noldest = 3\nupgrade = \"lnk upgrade\"\n",
        )
        .unwrap();
        assert_eq!(protocol.read("the stream", 3), Ok(()));
        assert_eq!(protocol.read("the stream", 4), Ok(()));
        assert!(matches!(
            protocol.read("the stream", 2),
            Err(Unread::Older(_))
        ));
        let newer = protocol.read("the stream", 5).unwrap_err();
        assert_eq!(
            newer.to_string(),
            "the stream speaks the move protocol 5, newer than 4, the newest this release \
             reads: upgrade this one with lnk upgrade"
        );
    }

    /// A raised `oldest` names its bridge release, and a peer below it is
    /// told to upgrade there first: `lnk upgrade` straight to this release
    /// would refuse it again.
    #[test]
    fn a_raised_oldest_names_the_bridge_release_its_refusal_names() {
        let protocol = Protocol::parse(
            "name = \"history\"\ncurrent = 3\noldest = 2\nupgrade = \"lnk upgrade\"\n\
             bridge = \"0.20.0\"\n",
        )
        .unwrap();
        let older = protocol.accept("the core asking", 1).unwrap_err();
        assert_eq!(
            older.to_string(),
            "the core asking speaks the history protocol 1, below 2, the oldest this release \
             accepts: upgrade it with lnk upgrade 0.20.0 first, the newest release accepting \
             it, then with lnk upgrade"
        );
        let early = "name = \"history\"\ncurrent = 3\noldest = 1\nupgrade = \"lnk upgrade\"\n\
             bridge = \"0.20.0\"\n";
        assert!(Protocol::parse(early).is_err());
    }

    /// A protocol `lnk upgrade` speaks, its oldest raised with no bridge
    /// release, is never released.
    #[test]
    #[should_panic(expected = "names a bridge release")]
    fn an_upgrades_raised_oldest_without_a_bridge_never_ships() {
        Protocol::shipped_upgrade(
            "name = \"history\"\ncurrent = 3\noldest = 2\nupgrade = \"lnk upgrade\"\n",
        );
    }

    /// An answer is written in the version its asker stated, or this
    /// release's own where it stated none or a newer one.
    #[test]
    fn an_answer_is_in_the_askers_version() {
        let protocol = Protocol::parse(
            "name = \"history\"\ncurrent = 3\noldest = 2\nupgrade = \"lnk upgrade\"\n\
             bridge = \"0.20.0\"\n",
        )
        .unwrap();
        let accepted = |version| protocol.accept("the asker", version).ok();
        assert_eq!(protocol.answering(accepted(2)), 2);
        assert_eq!(protocol.answering(accepted(4)), 3);
        assert_eq!(protocol.answering(None), 3);
        // Below the oldest, there's no accepted version to answer in.
        assert_eq!(accepted(1), None);
    }

    #[test]
    fn a_declaration_is_whole_and_ordered() {
        let declared = |oldest: u32, current: u32| {
            Protocol::parse(&format!(
                "name = \"answers\"\ncurrent = {current}\noldest = {oldest}\nupgrade = \"lnk upgrade\"\n"
            ))
        };
        assert!(declared(1, 1).is_ok());
        assert!(declared(0, 1).is_err());
        assert!(declared(1, 2).is_ok());
        assert!(declared(2, 1).is_err());
        assert!(Protocol::parse("name = \"x\"\ncurrent = 1\noldest = 1\n").is_err());
        assert!(
            Protocol::parse("name = \"x\"\ncurrent = 1\noldest = 1\nupgrade = \"u\"\nmore = 1\n")
                .is_err()
        );
    }
}
