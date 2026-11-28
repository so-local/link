//! The history query's wire: what `lnk --link-history` answers the core
//! upgrading to it with, in the history protocol both sides state.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{name_in, renamed_in};
use link_version::Version;

/// The history protocol, as `protocol.toml` declares it: the version
/// [`Wire`] is written in, and the oldest this release accepts of the core
/// it asks, or of the core asking it.
pub static PROTOCOL: std::sync::LazyLock<link_version::Protocol> = std::sync::LazyLock::new(|| {
    link_version::Protocol::shipped_upgrade(include_str!("protocol.toml"))
});

/// The variable the core asking another for its history states its
/// [`PROTOCOL`] version in ([`asking`]), which the answerer checks and
/// answers in ([`asked`]).
pub const PROTOCOL_VARIABLE: &str = "LNK_HISTORY_PROTOCOL";

/// `command`, which asks a core for its history, stating this release's
/// [`PROTOCOL`] version.
pub fn asking(command: &mut std::process::Command) -> &mut std::process::Command {
    command.env(PROTOCOL_VARIABLE, PROTOCOL.current.to_string())
}

/// The version the core asking this one for its history stated, which
/// its answer is written in ([`Wire::stating`]): refused, naming its
/// upgrade, when it's below the oldest this release accepts. None for a
/// person asking by hand, who reads the version the answer states.
pub fn asked() -> anyhow::Result<Option<link_version::Accepted>> {
    asker(std::env::var(PROTOCOL_VARIABLE).ok().as_deref())
}

fn asker(stated: Option<&str>) -> anyhow::Result<Option<link_version::Accepted>> {
    let Some(stated) = stated else {
        return Ok(None);
    };
    let version: u32 = stated.parse().map_err(|_| {
        anyhow::anyhow!("{PROTOCOL_VARIABLE} is {stated:?}, not a protocol version")
    })?;
    Ok(Some(
        PROTOCOL.accept("the core asking for this history", version)?,
    ))
}

/// A history as `lnk --link-history` answers it, to the core upgrading
/// to this one, in the [`PROTOCOL`] version it states: a retired
/// spelling's `command` and no `until`, which only its own release's
/// checks need. A change to its shape advances the protocol.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wire {
    /// The [`PROTOCOL`] version it's written in.
    pub protocol: u32,
    #[serde(default)]
    pub gone: Vec<WireGone>,
    #[serde(default)]
    pub renamed: Vec<WireRenamed>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<WireRemoved>,
    /// Each module's own retired spellings, by its name, that the core
    /// was released with: what a name a spec or a conversation holds follows
    /// ([`crate::follow`]). Written only where a module has any.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub modules: BTreeMap<String, Vec<WireGone>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireRemoved {
    pub module: String,
    pub why: String,
    pub since: Version,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireGone {
    pub command: String,
    pub since: Version,
    pub now: Option<String>,
    pub why: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireRenamed {
    pub module: String,
    pub now: String,
    pub since: Version,
}

impl Wire {
    /// The history `peer` answered, its protocol checked before its shape
    /// is read: one this release doesn't read, older or newer, or one
    /// stating none, is refused by name. A peer answers in the version
    /// this release stated, so a newer one is a peer that didn't.
    pub fn parse(peer: &str, text: &[u8]) -> anyhow::Result<Wire> {
        #[derive(Deserialize)]
        struct Stated {
            protocol: Option<u32>,
        }
        let stated: Stated = serde_json::from_slice(text)?;
        let Some(protocol) = stated.protocol else {
            anyhow::bail!("{peer}'s history states no history protocol");
        };
        PROTOCOL.read(peer, protocol)?;
        Ok(serde_json::from_slice(text)?)
    }

    /// This history, written in the version `asked`, the asker's
    /// ([`asked`]): its own where it stated none or a newer one. A version
    /// that changes the shape keeps a writer for each it still accepts.
    pub fn stating(self, asked: Option<link_version::Accepted>) -> Wire {
        Wire {
            protocol: PROTOCOL.answering(asked),
            ..self
        }
    }

    fn renames(&self) -> Vec<(&str, &str)> {
        self.renamed
            .iter()
            .map(|r| (r.module.as_str(), r.now.as_str()))
            .collect()
    }

    /// [`History::name`], of the incoming core's renames.
    pub fn name<'a>(&'a self, module: &'a str) -> anyhow::Result<&'a str> {
        name_in(self.renames(), module)
    }

    /// The history that says nothing, in this release's [`PROTOCOL`].
    pub fn empty() -> Wire {
        Wire {
            protocol: PROTOCOL.current,
            gone: Vec::new(),
            renamed: Vec::new(),
            removed: Vec::new(),
            modules: BTreeMap::new(),
        }
    }

    /// The programs in `dir` of removed modules, by their names or one
    /// renamed to them, which aren't among `modules`, the release's: what
    /// an upgrade uninstalls, each program and its removal.
    pub fn removed_in(
        &self,
        dir: &std::path::Path,
        modules: &[&str],
    ) -> Vec<(String, &WireRemoved)> {
        let names = self
            .removed
            .iter()
            .map(|r| r.module.as_str())
            .chain(self.renamed.iter().map(|r| r.module.as_str()));
        let mut found: Vec<(String, &WireRemoved)> = Vec::new();
        for module in names {
            let Some(removed) = self
                .name(module)
                .ok()
                .and_then(|now| self.removed.iter().find(|r| r.module == now))
            else {
                continue;
            };
            let program = format!("lnk-{module}");
            if dir.join(&program).is_file()
                && !modules.contains(&module)
                && !found.iter().any(|(p, _)| *p == program)
            {
                found.push((program, removed));
            }
        }
        found
    }

    /// [`History::renamed_in`], of the incoming core's renames.
    pub fn renamed_in<'a>(
        &'a self,
        dir: &std::path::Path,
        modules: &[&str],
    ) -> anyhow::Result<Vec<(String, &'a str)>> {
        renamed_in(self.renames(), dir, modules)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::History;

    /// The history wire states its protocol, and reads back as written.
    #[test]
    fn the_history_wire_states_its_protocol() {
        let history = History::parse(
            "[[gone]]\nwas = \"task show --raw\"\nnow = \"lnk task spec show\"\nsince = \"0.0.4\"\nuntil = \"0.0.6\"\n[[renamed]]\nmodule = \"memory\"\nnow = \"notes\"\nsince = \"0.0.4\"\nuntil = \"0.0.6\"\n",
        )
        .unwrap();
        let text = serde_json::to_string(&history.wire()).unwrap();
        let wire = Wire::parse("the new lnk", text.as_bytes()).unwrap();
        assert_eq!(wire.protocol, PROTOCOL.current);
        assert_eq!(wire.gone[0].command, "task show --raw");
        assert_eq!(wire.name("memory").unwrap(), "notes");
    }

    /// A history of a protocol this release doesn't read, below the oldest
    /// it accepts or newer than its own, is refused by its version before
    /// its shape is read; so is one stating none.
    #[test]
    fn a_history_of_an_older_protocol_is_refused_naming_the_upgrade() {
        let older = PROTOCOL.oldest - 1;
        let text = format!(r#"{{"protocol": {older}, "unknown": []}}"#);
        let refused = Wire::parse("the new lnk", text.as_bytes())
            .unwrap_err()
            .to_string();
        assert!(
            refused.starts_with(&format!(
                "the new lnk speaks the history protocol {older}, below {}",
                PROTOCOL.oldest
            )),
            "{refused}"
        );
        assert!(
            refused.ends_with("upgrade it with lnk upgrade"),
            "{refused}"
        );
        let newer = PROTOCOL.current + 1;
        let text = format!(r#"{{"protocol": {newer}, "unknown": []}}"#);
        let refused = Wire::parse("the new lnk", text.as_bytes())
            .unwrap_err()
            .to_string();
        assert!(
            refused.starts_with(&format!(
                "the new lnk speaks the history protocol {newer}, newer than {}",
                PROTOCOL.current
            )),
            "{refused}"
        );
        let refused = Wire::parse("the new lnk", br#"{"gone": []}"#)
            .unwrap_err()
            .to_string();
        assert_eq!(refused, "the new lnk's history states no history protocol");
    }

    /// A core answers in the version its asker stated, which an asker of
    /// that version reads, and in its own to a person asking by hand.
    #[test]
    fn a_history_is_answered_in_the_askers_version() {
        let wire = Wire::empty();
        assert_eq!(wire.clone().stating(None).protocol, PROTOCOL.current);
        assert_eq!(
            wire.clone()
                .stating(PROTOCOL.accept("a newer asker", PROTOCOL.current + 1).ok())
                .protocol,
            PROTOCOL.current
        );
        let answered = wire.stating(PROTOCOL.accept("the asker", PROTOCOL.oldest).ok());
        assert_eq!(answered.protocol, PROTOCOL.oldest);
        let text = serde_json::to_string(&answered).unwrap();
        assert!(Wire::parse("the new lnk", text.as_bytes()).is_ok());
    }

    /// The asking core states its version in the variable: the core
    /// answering refuses one below the oldest it accepts, naming the upgrade,
    /// and answers one that states none.
    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "a command never run, only its stated variable read"
    )]
    fn a_core_refuses_an_older_asker_naming_the_upgrade() {
        let current = PROTOCOL.current.to_string();
        assert_eq!(asker(None).unwrap(), None);
        assert_eq!(
            asker(Some(&current))
                .unwrap()
                .map(link_version::Accepted::version),
            Some(PROTOCOL.current)
        );
        let older = (PROTOCOL.oldest - 1).to_string();
        let refused = asker(Some(&older)).unwrap_err().to_string();
        assert!(
            refused.starts_with("the core asking for this history speaks the history protocol"),
            "{refused}"
        );
        assert!(
            refused.ends_with("upgrade it with lnk upgrade"),
            "{refused}"
        );
        assert!(asker(Some("one")).is_err());
        let mut command = std::process::Command::new("lnk");
        let stated = asking(&mut command)
            .get_envs()
            .find(|(name, _)| *name == PROTOCOL_VARIABLE)
            .and_then(|(_, value)| value);
        assert_eq!(stated, Some(std::ffi::OsStr::new(&current)));
    }
}
