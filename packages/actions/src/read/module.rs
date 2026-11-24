//! `module.toml`: the module's ID, its kind and what it depends on. Its
//! other tables (documentation, removal) are `link-component`'s.

use std::collections::BTreeMap;

use anyhow::{Context, ensure};
use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub id: String,
    pub kind: String,
    pub owner: String,
    /// Whether its commands read control, as `link-component` reads it:
    /// true unless the module says otherwise.
    #[serde(rename = "reads-control", default = "reads_control")]
    pub reads_control: bool,
}

/// What a module saying nothing about control is taken to do: read it.
fn reads_control() -> bool {
    true
}

/// A component this one needs, told apart by its `kind`, each kind with
/// its own keys, as `link-component`'s installer reads it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "kebab-case")]
pub enum Dependency {
    /// Another module, at least at `protocol` where it says.
    Module {
        id: String,
        #[serde(default)]
        protocol: Option<u32>,
    },
    /// A program from outside Link, at `version` where it says.
    External {
        id: String,
        #[serde(default)]
        version: Option<String>,
    },
    /// What the machine must be able to do, at least at `min_abi`.
    Capability {
        id: String,
        #[serde(default)]
        min_abi: Option<u32>,
    },
    /// A package whose shapes this module names, such as a contract.
    Package { id: String },
}

impl Dependency {
    pub fn id(&self) -> &str {
        match self {
            Self::Module { id, .. }
            | Self::External { id, .. }
            | Self::Capability { id, .. }
            | Self::Package { id } => id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Recommend {
    pub dependency: String,
}

pub(crate) struct File {
    pub identity: Identity,
    pub dependencies: BTreeMap<String, Dependency>,
    pub recommends: Vec<String>,
    pub answers: Vec<String>,
    pub guest_service: bool,
    /// Whether it declares `[removal]`, the parts it keeps.
    pub removal: bool,
    /// The actions that lead into it from `lnk up`, by their IDs.
    pub next: Vec<crate::ActionId>,
}

pub(crate) fn read(text: &str) -> anyhow::Result<File> {
    let mut table = super::versioned(text)?;
    let identity: Identity = table
        .remove("module")
        .context("a [module] table")?
        .try_into()
        .context("[module]")?;
    ensure!(
        crate::names::is_name(&identity.id),
        "the module's ID {:?} is a lowercase name",
        identity.id
    );
    let dependencies: BTreeMap<String, Dependency> = table
        .remove("dependencies")
        .map(toml::Value::try_into)
        .transpose()
        .context("[dependencies]")?
        .unwrap_or_default();
    let recommends: Vec<Recommend> = table
        .remove("recommends")
        .map(toml::Value::try_into)
        .transpose()
        .context("recommends")?
        .unwrap_or_default();
    let answers: Vec<String> = table
        .remove("answers")
        .map(toml::Value::try_into)
        .transpose()
        .context("answers lists questions by name")?
        .unwrap_or_default();
    let removal = table.contains_key("removal");
    let next: Vec<crate::ActionId> = table
        .remove("next")
        .map(toml::Value::try_into)
        .transpose()
        .context("next lists the actions that lead into the module, by their IDs")?
        .unwrap_or_default();
    let guest_service = match table.remove("guest_service") {
        Some(toml::Value::Boolean(on)) => on,
        Some(_) => anyhow::bail!("guest_service is true or false"),
        None => false,
    };
    Ok(File {
        identity,
        dependencies,
        recommends: recommends.into_iter().map(|r| r.dependency).collect(),
        answers,
        guest_service,
        removal,
        next,
    })
}
