//! An agent's own secrets, by what each is for: its model's key, its
//! fallback model's key, a connection's secret field and a webhook's
//! secret. The secrets' store keeps each by its purpose alone
//! (`link_secret_contract::Use::Own`), a word this names and the store
//! never reads.

use link_secret_contract::Use;

/// What one of an agent's own secrets is for.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OwnSecret {
    /// Its model's key (`lnk agent start`).
    Model,
    /// Its fallback model's key (`lnk agent start --model.fallback_model`).
    Fallback,
    /// A connection's secret field (`lnk agent connect`).
    Connection { connection: String, field: String },
    /// The secret its webhook `name` is signed with (`lnk agent webhook`),
    /// or while it rolls, the one before it.
    Webhook { name: String, previous: bool },
}

use crate::constants::{CONNECTION, FALLBACK, MODEL, PREVIOUS, WEBHOOK};

impl OwnSecret {
    /// Its purpose, as the store keeps it: `model`, `fallback`,
    /// `connection:<connection>:<field>`, `webhook:<name>` or
    /// `webhook:<name>:previous`.
    pub fn purpose(&self) -> String {
        match self {
            OwnSecret::Model => MODEL.into(),
            OwnSecret::Fallback => FALLBACK.into(),
            OwnSecret::Connection { connection, field } => {
                format!("{CONNECTION}:{connection}:{field}")
            }
            OwnSecret::Webhook {
                name,
                previous: false,
            } => format!("{WEBHOOK}:{name}"),
            OwnSecret::Webhook {
                name,
                previous: true,
            } => format!("{WEBHOOK}:{name}:{PREVIOUS}"),
        }
    }

    /// The store's use for it: an error for a name the store's purposes
    /// can't hold.
    pub fn to_use(&self) -> anyhow::Result<Use> {
        if let OwnSecret::Webhook { name, .. } = self {
            link_machine::names::check_name(name)?;
        }
        Use::own(&self.purpose())
    }

    /// The own secret `use` is for, when it's one an agent has.
    pub fn of(r#use: &Use) -> Option<OwnSecret> {
        let parts: Vec<&str> = r#use.purpose()?.split(':').collect();
        Some(match parts.as_slice() {
            [MODEL] => OwnSecret::Model,
            [FALLBACK] => OwnSecret::Fallback,
            [CONNECTION, connection, field] => OwnSecret::Connection {
                connection: connection.to_string(),
                field: field.to_string(),
            },
            [WEBHOOK, name] => OwnSecret::Webhook {
                name: name.to_string(),
                previous: false,
            },
            [WEBHOOK, name, PREVIOUS] => OwnSecret::Webhook {
                name: name.to_string(),
                previous: true,
            },
            _ => return None,
        })
    }
}

impl std::fmt::Display for OwnSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OwnSecret::Model => write!(f, "its model's key"),
            OwnSecret::Fallback => write!(f, "its fallback model's key"),
            OwnSecret::Connection { connection, field } => write!(f, "{connection}'s {field}"),
            OwnSecret::Webhook {
                name,
                previous: false,
            } => write!(f, "webhook {name}'s secret"),
            OwnSecret::Webhook {
                name,
                previous: true,
            } => write!(f, "webhook {name}'s previous secret"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_own_secret_is_kept_by_its_purpose_and_read_back() {
        for own in [
            OwnSecret::Model,
            OwnSecret::Fallback,
            OwnSecret::Connection {
                connection: "telegram".into(),
                field: "bot_token".into(),
            },
            OwnSecret::Webhook {
                name: "ci".into(),
                previous: false,
            },
            OwnSecret::Webhook {
                name: "ci".into(),
                previous: true,
            },
        ] {
            assert_eq!(OwnSecret::of(&own.to_use().unwrap()), Some(own));
        }
        assert_eq!(OwnSecret::of(&Use::Variable), None);
        assert_eq!(OwnSecret::of(&Use::own("other").unwrap()), None);
        let spaced = OwnSecret::Webhook {
            name: "a b".into(),
            previous: false,
        };
        assert!(spaced.to_use().is_err());
    }
}
