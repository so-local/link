//! An agent's own webhooks, as both sides of their `sift` know them: the
//! harness module, which keeps them (`lnk agent webhook`) and whose bridge
//! takes their posts, and the webhook module (`lnk-webhook-connection`), which checks
//! each post. Their connection's name, who signs a webhook's posts, and what
//! `sift` is handed of one ([`super::pushed::Sifting::settings`]).

use serde::{Deserialize, Serialize};

use link_secretive::inbound::Scheme;

/// The connection a webhook's posts are its module's: `lnk-webhook-connection`.
pub const CONNECTION: &str = "webhook-connection";

/// What a webhook's posts are named by, before its name: its thread, its
/// sender and the receiver its posts are signed for are `webhook:<name>`,
/// whatever its module is called.
pub const PREFIX: &str = "webhook";

/// Who signs a webhook's posts, and so how.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sender {
    /// Link's own scheme: `link-time`, `link-id` and `link-signature`.
    #[default]
    Link,
    /// GitHub's: `x-hub-signature-256`, and `x-github-delivery`.
    Github,
}

impl Sender {
    /// Its name, as `--from` and the settings say it.
    pub fn name(self) -> &'static str {
        match self {
            Sender::Link => "link",
            Sender::Github => "github",
        }
    }

    /// How it signs ([`link_secretive::inbound`]).
    pub fn scheme(self) -> Scheme {
        match self {
            Sender::Link => Scheme::LINK,
            Sender::Github => Scheme::GITHUB,
        }
    }

    /// Whether it's Link's own scheme, the one a webhook has unless it
    /// says another.
    pub fn is_link(&self) -> bool {
        *self == Sender::Link
    }
}

/// What `sift` is handed of a webhook: its name and who signs it, both
/// always said, so a webhook described without its sender takes nothing.
/// Nothing in it is secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub name: String,
    pub from: Sender,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_settings_say_its_name_and_sender_and_nothing_else() {
        let settings = Settings {
            name: "repo".into(),
            from: Sender::Github,
        };
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(json, serde_json::json!({"name": "repo", "from": "github"}));
        assert_eq!(serde_json::from_value::<Settings>(json).unwrap(), settings);
        for wrong in [
            serde_json::json!({"name": "repo"}),
            serde_json::json!({"name": "repo", "from": "gitlab"}),
            serde_json::json!({"name": "repo", "from": "link", "secret": "x"}),
        ] {
            assert!(serde_json::from_value::<Settings>(wrong).is_err());
        }
    }
}
