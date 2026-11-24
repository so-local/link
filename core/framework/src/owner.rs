//! Who owns what's made here: the account signed in, as the installed
//! module answering [`ACCOUNT`] says, else the local owner
//! (`link_machine::owner`, whose grammar every owner follows). With no such
//! module installed, nobody is signed in. The same module signs this
//! installation's tokens for a service, asked [`TOKEN`].

use link_machine::owner::{check_owner, is_local};

use serde::{Deserialize, Serialize};

use crate::answers::Question;
use crate::modules::AccountLogin;

/// Who is signed in here, and where: asked of the module that keeps the
/// login, which never answers with a credential.
pub const ACCOUNT: Question<(), AccountLogin> =
    Question::new("lnk.account", include_str!("owner/account.toml"));

/// The account signed in here as an owner: none when signed out, when no
/// installed module answers [`ACCOUNT`], or when what it says isn't an
/// account's.
pub fn account_owner() -> Option<String> {
    ACCOUNT
        .ask(&())
        .into_iter()
        .filter_map(|answer| answer.reply.ok()?.owner)
        .find(|owner| check_owner(owner).is_ok() && !is_local(owner))
}

/// Who owns what's made here now: the account signed in, else the local
/// owner.
pub fn current_owner() -> anyhow::Result<String> {
    match account_owner() {
        Some(account) => Ok(account),
        None => link_machine::owner::local_owner(),
    }
}

/// A token for a service, as this installation signs it: asked of the
/// module that keeps the login, which answers with the token.
pub const TOKEN: Question<TokenAsk, SignedToken> =
    Question::new("lnk.token", include_str!("owner/token.toml"));

/// What [`TOKEN`] asks: a token whose audience is the service at
/// `audience` (its host), for the workload here whose ID is `workload`,
/// granted `grants` there, when they're given; else for this installation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenAsk {
    pub audience: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workload: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grants: Option<String>,
}

/// A token [`TOKEN`]'s answer signed, with when it ends, in seconds since
/// the epoch, when it says.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedToken {
    #[serde(with = "link_secretive::bare")]
    pub token: link_secretive::Secret,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
}

/// [`TOKEN`] asked: the first module's token that answers, else the first
/// one's error, else an error saying no installed module signs tokens
/// here.
pub fn signed_token(ask: &TokenAsk) -> anyhow::Result<SignedToken> {
    let mut failed = None;
    for answer in TOKEN.ask(ask) {
        match answer.reply {
            Ok(token) => return Ok(token),
            Err(err) => {
                failed.get_or_insert(err);
            }
        }
    }
    Err(failed.unwrap_or_else(|| anyhow::anyhow!("no installed module signs tokens here")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The token question reads back as written, its absent fields left
    /// off the wire, and refuses a field it doesn't know.
    #[test]
    fn the_token_question_and_its_answer_read_back() {
        let ask = TokenAsk {
            audience: "relay.example".into(),
            workload: None,
            grants: None,
        };
        let text = serde_json::to_string(&TOKEN.asked(&ask)).unwrap();
        assert_eq!(
            text,
            r#"{"protocol":1,"question":{"audience":"relay.example"}}"#
        );
        let asked: crate::answers::Asked<TokenAsk> = serde_json::from_str(&text).unwrap();
        assert_eq!(TOKEN.question_of(asked).unwrap(), ask);
        let answered: crate::answers::Answered<SignedToken> =
            serde_json::from_str(r#"{"protocol":1,"answer":{"token":"t","expires_at":9}}"#)
                .unwrap();
        let token = TOKEN.answer_of("signer", answered).unwrap();
        assert_eq!(token.token.expose(), "t");
        assert_eq!(token.expires_at, Some(9));
        assert!(serde_json::from_str::<TokenAsk>(r#"{"audience":"a","scope":"x"}"#).is_err());
    }
}
