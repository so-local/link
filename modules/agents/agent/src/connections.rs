//! The connections an agent talks on (`lnk agent connect <connection>`): each
//! as its module, `lnk-<connection>`, made it (`connect`) and described it
//! (`describe`), in one map of the agent's settings. The harness module
//! knows no connection by name: what it keeps, shows and carries of one
//! comes from its module ([`link_connection_contract`]).

use link_format::WithinExt;
use std::process::Stdio;

use link_connection_contract::{Bot, Describe};
use link_secretive::Duplicate;
use serde::{Deserialize, Serialize};

/// A connected connection.
#[derive(Duplicate, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Connection {
    /// Its section of what a harness is handed
    /// ([`link_harness_contract::harness::Settings`]), as its module's `connect`
    /// made it: its token and who may talk to it. A Mac keeps its
    /// secret fields in the Keychain, not in the settings file.
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub settings: link_secretive::Secret<link_secretive::Json>,
    /// Its bot: none of it secret.
    #[serde(default)]
    pub bot: Bot,
    /// What its module said of it when it was connected.
    #[serde(default)]
    pub about: Describe,
}

impl Connection {
    /// What `connect` made, as its module describes it; nulls left out
    /// (the settings file can't hold them).
    pub fn new(connected: link_connection_contract::Connected, about: Describe) -> Self {
        Self {
            settings: without_nulls(connected.settings),
            bot: connected.bot,
            about,
        }
    }
}

/// `settings` without its nulls, at any depth, changed in place.
pub fn without_nulls(
    mut settings: link_secretive::Secret<link_secretive::Json>,
) -> link_secretive::Secret<link_secretive::Json> {
    fn drop_nulls(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                map.retain(|_, v| !v.is_null());
                map.values_mut().for_each(drop_nulls);
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(drop_nulls),
            _ => {}
        }
    }
    drop_nulls(settings.expose_mut());
    settings
}

/// An error unless `name` can name a connection: a module's name
/// (`lnk-<name>`), lowercase letters, digits and `-`, and none of the
/// names a harness's settings use for something else.
pub fn check_name(name: &str) -> anyhow::Result<()> {
    let plain = !name.is_empty()
        && name.len() <= 40
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    anyhow::ensure!(
        plain && !link_harness_contract::harness::NOT_CONNECTIONS.contains(&name),
        "{name:?} isn't a connection: a connection module's name (telegram, slack, ...)"
    );
    Ok(())
}

/// Whether `listed`, the modules `lnk module list` lists, has a
/// connection module `name`, installed or not; also when `lnk` can't say
/// (an empty listing), as [`link_core::modules::offered`] counts it.
pub fn is_module(listed: &[link_core::modules::Listed], name: &str) -> bool {
    listed.is_empty()
        || listed
            .iter()
            .any(|m| m.name == name && m.kind == Some(link_core::modules::Kind::Connection))
}

/// What connection `name`'s module says of it (`lnk-<name> describe --json`).
pub fn describe(name: &str) -> anyhow::Result<Describe> {
    let program = link_core::find_module(name)
        .within(format!("the {name} module (lnk-{name}) isn't installed"))?;
    let output = link_secretive::process::ours(&program)
        .args(["describe", "--json"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .within(format!("running {}", program.display()))?;
    anyhow::ensure!(
        output.status.success(),
        "lnk-{name} doesn't describe itself (describe --json): lnk upgrade, or it isn't a \
         connection's module"
    );
    serde_json::from_slice(&output.stdout).within(format!("reading lnk-{name} describe --json"))
}

/// The Keychain's name for secret `field` of connection `connection`, after
/// `agent/<name>/`: `slack-bot-token` for Slack's `bot_token`. Telegram's
/// token keeps the name it had before there were other connections.
pub fn key_name(connection: &str, field: &str) -> String {
    match (connection, field) {
        ("telegram", "bot_token") => "bot-token".into(),
        _ => format!("{connection}-{}", field.replace('_', "-")),
    }
}

/// The id of the bot on `connection` of an agent as another machine's `lnk
/// agent list --json` prints it, in `bots`.
pub fn listed_bot_id(agent: &serde_json::Value, connection: &str) -> Option<String> {
    match &agent["bots"][connection]["id"] {
        serde_json::Value::String(id) if !id.is_empty() => Some(id.clone()),
        serde_json::Value::Number(id) => Some(id.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listed_bot_is_found() {
        let listed = serde_json::json!({"bots": {"telegram": {"id": 123}, "slack": {"id": "U9"}}});
        assert_eq!(listed_bot_id(&listed, "telegram").as_deref(), Some("123"));
        assert_eq!(listed_bot_id(&listed, "slack").as_deref(), Some("U9"));
        assert_eq!(listed_bot_id(&listed, "discord"), None);
    }

    #[test]
    fn only_a_listed_connection_module_is_one() {
        use link_core::modules::{Kind, Listed};
        let module = |name: &str, kind| Listed {
            name: name.into(),
            kind: Some(kind),
            ..Listed::default()
        };
        let listed = [
            module("telegram", Kind::Connection),
            module("agent", Kind::Module),
        ];
        assert!(is_module(&listed, "telegram"));
        assert!(
            !is_module(&listed, "agent"),
            "a module that isn't a connection"
        );
        assert!(
            !is_module(&listed, "qa-no-such-connection"),
            "a name no module has"
        );
        assert!(is_module(&[], "anything"), "lnk couldn't say");
    }

    #[test]
    fn names_and_keys() {
        for good in ["telegram", "matrix", "my-chat2"] {
            check_name(good).unwrap();
        }
        for bad in [
            "", "Telegram", "../x", "-a", "model", "files", "tools", "a b",
        ] {
            assert!(check_name(bad).is_err(), "{bad}");
        }
        assert_eq!(key_name("telegram", "bot_token"), "bot-token");
        assert_eq!(key_name("slack", "app_token"), "slack-app-token");
        assert_eq!(key_name("discord", "bot_token"), "discord-bot-token");
        assert_eq!(
            without_nulls(serde_json::json!({"a": null, "b": [{"c": null, "d": 1}]}).into()),
            serde_json::json!({"b": [{"d": 1}]}).into()
        );
    }
}
