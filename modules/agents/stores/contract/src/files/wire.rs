//! What a call says on the wire: the team it names ([`TeamRef`]), the
//! [`Action`] and its [`Answer`], and the paths it may name.

use super::*;

/// A team's folder, as a call names it: by the team's name as the
/// agent's sandbox file names it (`research`), or, for another account's
/// team, by that account and the name (`acct-<32 hex>/design`). Only how
/// the call reads: the bridge asks which team it is at each call
/// ([`Homes::team`]). A name never holds a `/`, so the two forms never
/// meet.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TeamRef {
    /// The team's owner, for another account's team: none, the agent's
    /// own owner's.
    pub owner: Option<String>,
    pub name: String,
}

impl TeamRef {
    /// `text` as a call writes it: `<name>` or `<account>/<name>`; an
    /// error for a name that isn't one ([`crate::machine::check_name`]),
    /// or an owner that isn't an account's.
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let (owner, name) = match text.split_once('/') {
            Some((owner, name)) => {
                crate::identity::check_owner(owner)?;
                ensure!(
                    owner.starts_with("acct-") && !crate::identity::is_local(owner),
                    "{:?} isn't an account's ID",
                    link_format::clean(owner)
                );
                (Some(owner.to_string()), name)
            }
            None => (None, text),
        };
        crate::machine::check_name(name).context("a team's name")?;
        Ok(Self {
            owner,
            name: name.to_string(),
        })
    }
}

impl std::fmt::Display for TeamRef {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match &self.owner {
            Some(owner) => write!(f, "{owner}/{}", self.name),
            None => f.write_str(&self.name),
        }
    }
}

impl Serialize for TeamRef {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for TeamRef {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// What a call asks the bridge, after its proof. Paths are Home's,
/// relative (`memory/notes.md`), or, with a `team`, that team's folder's;
/// index names are the index's (`memory/manifest.json`), Home's or,
/// with a `team`, the one beside that team's folder.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Action {
    Read {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        team: Option<TeamRef>,
    },
    Write {
        path: String,
        /// Its whole contents, base64.
        data: String,
        /// What it replaces.
        #[serde(rename = "if")]
        expect: Expect,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        team: Option<TeamRef>,
    },
    /// Every file under `folder` (`memory/`), or all of Home (or of the
    /// team's folder) for `""`.
    List {
        folder: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        team: Option<TeamRef>,
    },
    /// An object of the index: Home's, or, with a `team`, the index kept
    /// beside that team's folder, reached only by a member granted all of
    /// the folder (its memory searches all of it).
    IndexGet {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        team: Option<TeamRef>,
    },
    IndexPut {
        name: String,
        data: String,
        #[serde(rename = "if")]
        expect: Expect,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        team: Option<TeamRef>,
    },
    /// Every object of the index under `engine`.
    IndexList {
        engine: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        team: Option<TeamRef>,
    },
    IndexDelete {
        name: String,
        #[serde(rename = "if")]
        expect: Expect,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        team: Option<TeamRef>,
    },
}

impl Action {
    /// The team whose folder it reaches; none, Home.
    pub fn team(&self) -> Option<&TeamRef> {
        match self {
            Self::Read { team, .. }
            | Self::Write { team, .. }
            | Self::List { team, .. }
            | Self::IndexGet { team, .. }
            | Self::IndexPut { team, .. }
            | Self::IndexList { team, .. }
            | Self::IndexDelete { team, .. } => team.as_ref(),
        }
    }

    /// The same call, naming `team` (none: Home) whatever it named: what
    /// a folder service makes of a call, the team its route names.
    pub fn in_team(mut self, named: Option<TeamRef>) -> Self {
        match &mut self {
            Self::Read { team, .. }
            | Self::Write { team, .. }
            | Self::List { team, .. }
            | Self::IndexGet { team, .. }
            | Self::IndexPut { team, .. }
            | Self::IndexList { team, .. }
            | Self::IndexDelete { team, .. } => *team = named,
        }
        self
    }
}

/// The bridge's answer to a call: one of these, or the error envelope
/// ([`Failure`]; [`Why::Conflict`] for a write refused for its version).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Answer {
    Got { data: String, version: Version },
    Missing { missing: bool },
    Written { version: Version },
    Deleted { deleted: bool },
    Listed { files: Vec<Object> },
}

/// A Home's path, as a call gives it: relative, parts neither empty, `.`
/// nor `..`, none hidden.
pub fn check_path(path: &str) -> anyhow::Result<()> {
    ensure!(
        !path.is_empty()
            && !path.starts_with('/')
            && path.len() <= 1024
            && !path.contains('\0')
            && path
                .split('/')
                .all(|part| !part.is_empty() && !part.starts_with('.')),
        "{:?} isn't a path in Home: give it relative, as work/notes.md",
        link_format::clean(path)
    );
    Ok(())
}
