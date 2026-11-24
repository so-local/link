//! The variables Link knows in a harness's `.env`, `variables.toml`, and
//! what Link makes of each an install holds as it brings it in: said whole before it's
//! done, as a variable may be used other than its name says.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use link_secret_contract::Sealing;
use serde::Deserialize;

/// What a known variable becomes under Link.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "becomes", rename_all = "lowercase", deny_unknown_fields)]
pub enum Variable {
    /// A hosted model's key, the agent's model key.
    Model {
        provider: String,
        #[serde(default)]
        unless: Vec<String>,
    },
    /// A bot's token, handed to its connection's `connect` as `handed_as`.
    Connection {
        connection: String,
        handed_as: String,
    },
    /// A service's key, given to the agent sealed.
    Secret {
        sealed: Sealing,
        #[serde(default)]
        unless: Vec<String>,
    },
    /// No secret: kept in the copy as it is.
    Setting,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    variable: BTreeMap<String, Variable>,
}

/// Every variable Link knows, by its name.
pub fn known() -> &'static BTreeMap<String, Variable> {
    static READ: OnceLock<BTreeMap<String, Variable>> = OnceLock::new();
    READ.get_or_init(|| {
        toml::from_str::<File>(include_str!("variables.toml"))
            .expect("variables.toml reads")
            .variable
    })
}

/// What Link makes of one variable of an install's `.env`, bringing it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Made {
    /// The agent's model key, for its provider.
    Model { provider: String },
    /// Its connection's, handed to its `connect`.
    Connection { connection: String },
    /// Given to the agent sealed, for its host.
    Sealed(Sealing),
    /// Kept in the copy: no secret.
    Setting,
    /// Taken out of the copy and left in the original, and why.
    Left(Why),
}

/// Why a secret is left in the original rather than given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// Link doesn't know what it's for or where it goes.
    Unknown,
    /// A variable beside it sends it elsewhere: that variable and its value.
    Elsewhere { by: String, to: String },
    /// A model provider's variable whose value isn't that provider's key.
    NotItsKey,
    /// The agent has its model key already, from this variable.
    SecondModel { first: String },
}

/// What Link makes of each variable of `env`, bringing it in, an install's `.env`, in
/// its order.
pub fn made(env: &[(String, link_secretive::Secret)]) -> Vec<(String, Made)> {
    let set = |name: &str| {
        env.iter()
            .find(|(n, v)| n == name && !v.expose().trim().is_empty())
            .map(|(_, v)| v.expose_owned().trim().to_string())
    };
    let elsewhere = |unless: &[String]| {
        unless
            .iter()
            .find_map(|by| set(by).map(|to| Why::Elsewhere { by: by.clone(), to }))
    };
    let mut model: Option<String> = None;
    let mut found = Vec::new();
    for (name, value) in env {
        let made = match known().get(name) {
            None => Made::Left(Why::Unknown),
            Some(Variable::Setting) => Made::Setting,
            Some(Variable::Connection { connection, .. }) => Made::Connection {
                connection: connection.clone(),
            },
            Some(Variable::Secret { sealed, unless }) => match elsewhere(unless) {
                Some(why) => Made::Left(why),
                None => Made::Sealed(sealed.clone()),
            },
            Some(Variable::Model { provider, unless }) => {
                if let Some(why) = elsewhere(unless) {
                    Made::Left(why)
                } else if crate::settings::provider_of(value.expose()) != Some(provider.as_str()) {
                    Made::Left(Why::NotItsKey)
                } else if let Some(first) = &model {
                    Made::Left(Why::SecondModel {
                        first: first.clone(),
                    })
                } else {
                    model = Some(name.clone());
                    Made::Model {
                        provider: provider.clone(),
                    }
                }
            }
        };
        found.push((name.clone(), made));
    }
    found
}

/// The variable that hands `.env` variable `name`'s token to connection
/// `connection`'s connect, if it's that connection's.
pub fn handed_as(name: &str, connection: &str) -> Option<&'static str> {
    match known().get(name)? {
        Variable::Connection {
            connection: c,
            handed_as,
        } if c == connection => Some(handed_as.as_str()),
        _ => None,
    }
}

/// The connection whose token `.env` variable `name` is, if it's one.
pub fn connection_of(name: &str) -> Option<&'static str> {
    match known().get(name)? {
        Variable::Connection { connection, .. } => Some(connection.as_str()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, link_secretive::Secret)> {
        pairs
            .iter()
            .map(|(n, v)| (n.to_string(), v.to_string().into()))
            .collect()
    }

    #[test]
    fn every_variable_reads_and_each_unless_is_a_setting() {
        assert!(known().contains_key("ANTHROPIC_API_KEY"));
        for (name, variable) in known() {
            let unless = match variable {
                Variable::Model { unless, .. } | Variable::Secret { unless, .. } => unless,
                _ => continue,
            };
            for by in unless {
                assert!(
                    matches!(known().get(by), Some(Variable::Setting)),
                    "{name}'s unless names {by}, which isn't a setting here"
                );
            }
            if let Variable::Secret { sealed, .. } = variable {
                Sealing::new(
                    &sealed.host,
                    Some(&sealed.header),
                    sealed.format.as_deref(),
                    sealed.basic.as_deref(),
                )
                .unwrap_or_else(|e| panic!("{name}'s sealing: {e}"));
            }
        }
    }

    #[test]
    fn each_variable_is_made_what_it_is_unless_it_goes_elsewhere() {
        let made = made(&env(&[
            ("ANTHROPIC_API_KEY", "sk-ant-1"),
            ("OPENROUTER_API_KEY", "sk-or-2"),
            ("OPENAI_API_KEY", "not-a-key"),
            ("GITHUB_TOKEN", "ghp_3"),
            ("TELEGRAM_BOT_TOKEN", "1:b"),
            ("GMAIL_MCP_TOKEN", "ya29.4"),
        ]));
        let of = |name: &str| made.iter().find(|(n, _)| n == name).unwrap().1.clone();
        assert_eq!(
            of("ANTHROPIC_API_KEY"),
            Made::Model {
                provider: "anthropic".into()
            }
        );
        assert_eq!(
            of("OPENROUTER_API_KEY"),
            Made::Left(Why::SecondModel {
                first: "ANTHROPIC_API_KEY".into()
            })
        );
        assert_eq!(of("OPENAI_API_KEY"), Made::Left(Why::NotItsKey));
        assert!(matches!(of("GITHUB_TOKEN"), Made::Sealed(s) if s.host == "api.github.com"));
        assert_eq!(
            of("TELEGRAM_BOT_TOKEN"),
            Made::Connection {
                connection: "telegram".into()
            }
        );
        assert_eq!(of("GMAIL_MCP_TOKEN"), Made::Left(Why::Unknown));
    }

    #[test]
    fn a_key_sent_elsewhere_is_left_and_says_where() {
        let made = made(&env(&[
            ("OPENAI_API_KEY", "sk-proj-1"),
            ("OPENAI_BASE_URL", "https://api.groq.com/openai/v1"),
        ]));
        assert_eq!(
            made[0].1,
            Made::Left(Why::Elsewhere {
                by: "OPENAI_BASE_URL".into(),
                to: "https://api.groq.com/openai/v1".into()
            })
        );
        assert_eq!(made[1].1, Made::Setting);
    }
}
