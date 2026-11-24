//! What a declaration names things by: an action's ID, a shape's name and
//! a reference to an action, each read strictly from its text.

use std::{fmt, str::FromStr};

use anyhow::ensure;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A module's ID, a namespace, a resource, a verb or a shape's own name:
/// lowercase letters, digits and dashes, starting with a letter.
pub fn is_name(text: &str) -> bool {
    link_id::is_kind_name(text)
}

fn name(text: &str, what: &str) -> anyhow::Result<()> {
    ensure!(
        is_name(text),
        "{what} {text:?} is lowercase letters, digits and dashes, starting with a letter"
    );
    Ok(())
}

/// An action's ID, `<namespace>.<resource>.<verb>`: `env.approval.add`.
/// Its command is `lnk <namespace> <resource> <verb>`, the resource left
/// out where it is the namespace's own name, and `lnk <namespace>` runs
/// the program `lnk-<namespace>` with the words after it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActionId {
    text: String,
    resource: usize,
    verb: usize,
}

impl ActionId {
    pub fn new(namespace: &str, resource: &str, verb: &str) -> anyhow::Result<Self> {
        format!("{namespace}.{resource}.{verb}").parse()
    }

    pub fn namespace(&self) -> &str {
        &self.text[..self.resource - 1]
    }

    pub fn resource(&self) -> &str {
        &self.text[self.resource..self.verb - 1]
    }

    pub fn verb(&self) -> &str {
        &self.text[self.verb..]
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The words its module's program reads, after the program's name:
    /// `["approval", "add"]` for `env.approval.add`, run as `lnk-env
    /// approval add`, or `["create"]` for `env.env.create`. A program
    /// never reads its own namespace.
    pub fn words(&self) -> Vec<&str> {
        let mut words = Vec::new();
        if self.resource() != self.namespace() {
            words.push(self.resource());
        }
        words.push(self.verb());
        words
    }

    /// The words typed after `lnk`: the namespace, then the program's
    /// words, as `["env", "approval", "add"]`. Core's program is `lnk`
    /// itself, so its commands are its program's words alone.
    pub fn typed(&self) -> Vec<&str> {
        let mut words = self.words();
        if self.namespace() != crate::core().module {
            words.insert(0, self.namespace());
        }
        words
    }

    /// The command as typed: `lnk env approval add`.
    pub fn command(&self) -> String {
        std::iter::once(crate::core().program.as_str())
            .chain(self.typed())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The program its namespace is dispatched to: `lnk-env`.
    pub fn program(&self) -> String {
        crate::program(self.namespace())
    }
}

impl FromStr for ActionId {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> anyhow::Result<Self> {
        let parts: Vec<&str> = text.split('.').collect();
        ensure!(
            parts.len() == 3 && parts.iter().all(|p| is_name(p)),
            "{text:?} isn't an action ID: namespace.resource.verb, each a lowercase name"
        );
        let resource = parts[0].len() + 1;
        Ok(Self {
            text: text.to_string(),
            resource,
            verb: resource + parts[1].len() + 1,
        })
    }
}

/// A shape's name as the catalog holds it, always qualified by the module
/// that declares it: `env.stage`, `box.id`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TypeName {
    module: String,
    name: String,
}

impl TypeName {
    pub fn new(module: &str, name: &str) -> anyhow::Result<Self> {
        self::name(module, "a module")?;
        self::name(name, "a shape's name")?;
        Ok(Self {
            module: module.to_string(),
            name: name.to_string(),
        })
    }

    /// A name as a declaration writes it, from inside `module`: a bare
    /// name is the module's own, `box.id` the `box` module's.
    pub fn written(text: &str, module: &str) -> anyhow::Result<Self> {
        match text.split_once('.') {
            Some((other, name)) => Self::new(other, name),
            None => Self::new(module, text),
        }
        .map_err(|e| anyhow::anyhow!("{text:?} isn't a shape's name: {e:#}"))
    }

    pub fn module(&self) -> &str {
        &self.module
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

impl fmt::Display for TypeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.module, self.name)
    }
}

/// A reference to an action, or to an action pinned to one resource:
/// `social.post:k7m2n9f54b1p.show`, whose resource is `post:k7m2n9f54b1p`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActionRef {
    text: String,
    action: ActionId,
    resource: Option<String>,
}

impl ActionRef {
    pub fn action(&self) -> &ActionId {
        &self.action
    }

    /// The pinned resource's ID, `<kind>:<id>`.
    pub fn resource(&self) -> Option<&str> {
        self.resource.as_deref()
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl FromStr for ActionRef {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> anyhow::Result<Self> {
        let bad = || {
            anyhow::anyhow!(
                "{text:?} isn't an action reference: namespace.resource.verb or namespace.resource:id.verb"
            )
        };
        let (namespace, rest) = text.split_once('.').ok_or_else(bad)?;
        let (middle, verb) = rest.rsplit_once('.').ok_or_else(bad)?;
        let (resource, pinned) = match middle.split_once(':') {
            Some((resource, id)) if link_id::is_id(id) => (resource, Some(middle.to_string())),
            Some(_) => return Err(bad()),
            None => (middle, None),
        };
        let action = ActionId::new(namespace, resource, verb).map_err(|_| bad())?;
        Ok(Self {
            text: text.to_string(),
            action,
            resource: pinned,
        })
    }
}

macro_rules! text {
    ($($ty:ty),*) => {$(
        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.text)
            }
        }
        impl std::borrow::Borrow<str> for $ty {
            fn borrow(&self) -> &str {
                &self.text
            }
        }
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                String::deserialize(deserializer)?
                    .parse()
                    .map_err(|e| serde::de::Error::custom(format!("{e:#}")))
            }
        }
        impl Serialize for $ty {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.text)
            }
        }
    )*};
}
text!(ActionId, ActionRef);

#[cfg(test)]
mod tests;
