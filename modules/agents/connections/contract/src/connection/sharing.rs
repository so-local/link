//! `lnk-<connection> share`: a chat of the connection shared with the agent,
//! by a share's code a paired user sends there.

use super::Sharer;
use link_secretive::Duplicate;
use serde::{Deserialize, Serialize};

/// How long `lnk-<connection> share` waits for its code ([`Sharing`]), as
/// pairing waits for its own.
pub const SHARE_WAIT: std::time::Duration = std::time::Duration::from_secs(600);

/// What `lnk-<connection> share` reads on stdin, one JSON line: the
/// connection's settings, its token included, the one-time code
/// ([`pairing_code`](super::pairing_code)) and how long to wait for it.
/// It waits for a message in a group of the connection from a user it's
/// paired with (`allowed_users`) that mentions the bot with the code
/// ([`has_code`](super::has_code)), says there that the chat is shared,
/// and prints [`Shared`]. Only paired users' messages are read, so nobody
/// else can share a chat; the code is taken once, then it exits.
#[derive(Duplicate, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sharing {
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub settings: link_secretive::Secret<link_secretive::Json>,
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub code: link_secretive::Secret,
    /// How long it waits, in seconds ([`SHARE_WAIT`]).
    pub wait_secs: u64,
}

/// What `lnk-<connection> share` prints once a chat is shared: the chat,
/// a group the connection keeps (`chat:slack:C0TEAM`,
/// `chat:telegram:-1001234567890`, [`crate::access::Chat`]), anything else
/// refused, and its name where the connection gives one (`#team`), for
/// people, shown only made safe ([`crate::clean`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shared {
    pub chat: crate::access::Chat,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// `share` ([`About::share`](super::About::share)): one [`Sharing`] line
/// from `input`, its wait capped at [`SHARE_WAIT`], handed to `share`,
/// which is stopped when the wait is over; the chat it says, refused
/// unless it's one of connection `name`'s. Neither the code nor the settings
/// are ever in what it prints or fails with: a line it can't read is said
/// by where, never by what's in it.
pub(super) async fn take_share(
    name: &str,
    share: Sharer,
    input: &mut impl std::io::BufRead,
) -> anyhow::Result<Shared> {
    let mut line = String::new();
    std::io::BufRead::read_line(&mut std::io::Read::take(input, 64 * 1024), &mut line)?;
    let mut sharing: Sharing = serde_json::from_str(&line).map_err(|err| {
        anyhow::anyhow!(
            "reading the share on stdin: not a share's line ({:?} at line {}, column {})",
            err.classify(),
            err.line(),
            err.column()
        )
    })?;
    anyhow::ensure!(!sharing.code.is_empty(), "the share's code is empty");
    let wait = std::time::Duration::from_secs(sharing.wait_secs).min(SHARE_WAIT);
    sharing.wait_secs = wait.as_secs();
    let shared = tokio::time::timeout(wait, share(sharing))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "no code came in {} minutes: run lnk agent share add {name} again",
                wait.as_secs().div_ceil(60)
            )
        })??;
    match shared.chat.connection() == name {
        true => Ok(shared),
        false => anyhow::bail!(
            "lnk-{name} shared {:?}, not a chat of {name}",
            link_format::clean(shared.chat.as_str())
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::has_code;

    /// The code `share`'s tests hand it, which nothing they're told back
    /// may hold.
    const CODE: &str = "4K7P2QXM";

    /// What a test's sharer was handed to wait, in seconds.
    static WAITED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(u64::MAX);

    fn sharing_line(wait_secs: u64) -> String {
        format!(
            r#"{{"settings": {{"bot_token": "x"}}, "code": "{CODE}", "wait_secs": {wait_secs}}}"#
        )
    }

    /// `share` hands its sharer the line's wait, never more than
    /// [`SHARE_WAIT`], and prints the chat it says when it's the
    /// connection's.
    #[tokio::test]
    async fn share_hands_on_a_capped_wait_and_takes_the_connections_chat() {
        let share: Sharer = |sharing| {
            WAITED.store(sharing.wait_secs, std::sync::atomic::Ordering::SeqCst);
            assert!(has_code(&format!("@company {CODE}"), &sharing.code));
            Box::pin(async {
                Ok(Shared {
                    chat: crate::access::chat("mini", "C0TEAM")?,
                    name: Some("#team".into()),
                })
            })
        };
        let line = sharing_line(86_400);
        let shared = take_share("mini", share, &mut line.as_bytes())
            .await
            .unwrap();
        assert_eq!(shared.chat, crate::access::chat("mini", "C0TEAM").unwrap());
        assert_eq!(
            WAITED.load(std::sync::atomic::Ordering::SeqCst),
            SHARE_WAIT.as_secs()
        );
        // Another connection's chat, said by this one: refused.
        let elsewhere: Sharer = |_| {
            Box::pin(async {
                Ok(Shared {
                    chat: crate::access::chat("slack", "C0TEAM")?,
                    name: None,
                })
            })
        };
        let err = take_share("mini", elsewhere, &mut line.as_bytes()).await;
        assert!(format!("{:#}", err.unwrap_err()).contains("not a chat of mini"));
    }

    /// A sharer still waiting once the wait is over is stopped, saying to
    /// run `share add` again.
    #[tokio::test]
    async fn share_stops_when_the_wait_is_over() {
        let forever: Sharer = |_| Box::pin(std::future::pending());
        let line = sharing_line(0);
        let err = take_share("mini", forever, &mut line.as_bytes()).await;
        let err = format!("{:#}", err.unwrap_err());
        assert!(err.contains("lnk agent share add mini"), "{err}");
    }

    /// Neither the share's code nor its settings are ever in what `share`
    /// fails with: a line it can't read is said by where, an empty code
    /// refused before anything waits.
    #[tokio::test]
    async fn share_never_tells_the_code() {
        let never: Sharer = |_| panic!("a line it can't read reached the sharer");
        for line in [
            // The code where a number goes, as a field's name, after the end.
            format!(r#"{{"settings": {{}}, "code": "{CODE}", "wait_secs": "{CODE}"}}"#),
            format!(r#"{{"settings": {{}}, "code": "x", "wait_secs": 1, "{CODE}": 1}}"#),
            format!(r#"{{"settings": {{"bot_token": "{CODE}"}}, "code": 7, "wait_secs": 1}}"#),
            format!("{} {CODE}", sharing_line(1)),
            r#"{"settings": {}, "code": "", "wait_secs": 1}"#.to_string(),
        ] {
            let err = take_share("mini", never, &mut line.as_bytes()).await;
            let err = err.unwrap_err();
            let err = format!("{err:#} {err:?}");
            assert!(!err.contains(CODE), "{line}: {err}");
            assert!(err.contains("share"), "{line}: {err}");
        }
    }

    /// What `share` prints names a chat, as a chat principal, or is
    /// refused.
    #[test]
    fn a_shared_chat_is_a_chat_principal() {
        let shared: Shared =
            serde_json::from_str(r##"{"chat": "chat:slack:C0TEAM", "name": "#team"}"##).unwrap();
        assert_eq!(shared.chat, crate::access::chat("slack", "C0TEAM").unwrap());
        for bad in [
            r#"{"chat": "C0TEAM"}"#,
            r#"{"chat": "member:slack:T0ACME/U0ALICE"}"#,
            r#"{"chat": "agent:acct-b7/k3m4n5p6q7r8"}"#,
            r#"{"chat": "chat:Not A Connection:C0TEAM"}"#,
            r#"{"chat": "chat:slack:"}"#,
            r#"{"chat": "chat:slack:C0TEAM", "admin": true}"#,
        ] {
            assert!(serde_json::from_str::<Shared>(bad).is_err(), "{bad}");
        }
    }
}
