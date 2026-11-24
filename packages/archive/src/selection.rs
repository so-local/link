//! `kind.name`: how an archive selects a thing, in `--select` and as
//! `archive.toml`'s keys. A kind and a name are joined by `.`, which
//! neither holds, so it splits one way. Only the archive reads it: every
//! other command takes the bare name.

use std::collections::BTreeSet;

use anyhow::{Context, bail, ensure};

/// A thing, by its kind and its name (`agent.programmer`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Selection {
    kind: String,
    name: String,
}

impl Selection {
    /// Thing `name` of kind `kind`, each checked.
    pub fn new(kind: &str, name: &str) -> anyhow::Result<Self> {
        link_machine::machine::check_shape(kind).with_context(|| format!("in {kind}.{name}"))?;
        link_machine::machine::check_name(name).with_context(|| format!("in {kind}.{name}"))?;
        Ok(Self {
            kind: kind.into(),
            name: name.into(),
        })
    }

    /// The selection `text` writes, `kind.name`.
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let (kind, name) = text.split_once('.').with_context(|| {
            format!(
                "{:?} isn't a thing: <kind>.<name>, as agent.programmer",
                link_format::clean(text)
            )
        })?;
        ensure!(
            !name.contains('.'),
            "{:?} isn't a thing: <kind>.<name>, with one dot",
            link_format::clean(text)
        );
        Self::new(kind, name)
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The same thing named `name`.
    pub fn named(&self, name: &str) -> anyhow::Result<Self> {
        Self::new(&self.kind, name)
    }
}

impl std::fmt::Display for Selection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.kind, self.name)
    }
}

/// One thing a command picks, and the name it's imported under, as
/// `agent.programmer=agent.my-programmer` writes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pick {
    pub thing: Selection,
    pub to: Option<String>,
}

impl Pick {
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let (thing, to) = match text.split_once('=') {
            Some((thing, to)) => (thing, Some(to)),
            None => (text, None),
        };
        let thing = Selection::parse(thing)?;
        let to = match to {
            None => None,
            Some(to) => {
                let to = Selection::parse(to)?;
                ensure!(
                    to.kind() == thing.kind(),
                    "{thing} can only be imported as another {}",
                    thing.kind()
                );
                Some(to.name().to_string())
            }
        };
        Ok(Self { thing, to })
    }
}

/// The things a comma-separated list picks, each once.
pub fn parse_picks(text: &str) -> anyhow::Result<Vec<Pick>> {
    let mut picks = Vec::new();
    let mut seen = BTreeSet::new();
    let mut names = BTreeSet::new();
    for part in text.split(',') {
        let pick = Pick::parse(part.trim())?;
        if !seen.insert(pick.thing.clone()) {
            bail!("{} is named twice", pick.thing);
        }
        let to = pick.to.clone().unwrap_or(pick.thing.name().into());
        if !names.insert((pick.thing.kind().to_string(), to.clone())) {
            bail!("two things would be named {}.{to}", pick.thing.kind());
        }
        picks.push(pick);
    }
    Ok(picks)
}
