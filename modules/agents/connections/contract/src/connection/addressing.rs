//! The owner addressing a team from a chat a connection carries: a
//! message starting `@<team>` goes to the team's lead, `@<team>/<member>`
//! to one member by its unique name in the team, through the owner's own
//! team chat (`lnk agent chat send --team`), which resolves the current roster
//! and refuses an unknown, removed or unaccepted target before delivery.
//! Nothing is broadcast, and nothing falls back to another member: what
//! can't be delivered is said back on the chat.

use anyhow::Context;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;

/// The longest a team's or a member's name is.
const NAME_MAX: usize = 64;

/// A message addressed to a team, or one of its members.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    pub team: String,
    /// A member's unique name in the team; none, its lead.
    pub member: Option<String>,
    /// The message, past its address.
    pub text: String,
}

impl Address {
    /// How it's shown: `@research` or `@research/writer`.
    pub fn shown(&self) -> String {
        match &self.member {
            Some(member) => format!("@{}/{member}", self.team),
            None => format!("@{}", self.team),
        }
    }
}

/// `text`'s address, when it starts with one: else none, a message as
/// any other. An address that isn't one of names is refused, saying why.
pub fn address(text: &str) -> Option<Result<Address, String>> {
    let rest = text.trim_start().strip_prefix('@')?;
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let (target, message) = rest.split_at(end);
    let (team, member) = match target.split_once('/') {
        Some((team, member)) => (team, Some(member)),
        None => (target, None),
    };
    let named = |name: &str| {
        (1..=NAME_MAX).contains(&name.len())
            && name.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    if !named(team) || member.is_some_and(|m| !named(m)) {
        return Some(Err(format!(
            "{} isn't a team's address: @<team> for its lead, or @<team>/<member>",
            link_format::clean(target)
        )));
    }
    let message = message.trim();
    if message.is_empty() {
        return Some(Err(format!("nothing to send to @{target}")));
    }
    Some(Ok(Address {
        team: team.to_string(),
        member: member.map(str::to_string),
        text: message.to_string(),
    }))
}

/// Delivers `address`'s message through `lnk`, the owner's team chat,
/// in the team's conversation: the answer, as its target gave it.
pub async fn deliver(lnk: std::process::Command, address: &Address) -> anyhow::Result<String> {
    let mut command = tokio::process::Command::from(lnk);
    command.args(["agent", "chat", "send", "--team", &address.team]);
    if let Some(member) = &address.member {
        command.args(["--member", member]);
    }
    let mut child = command
        .args(["--message", "-", "--json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("running lnk agent chat send --team")?;
    let mut stdin = child.stdin.take().context("its input")?;
    stdin.write_all(address.text.as_bytes()).await?;
    drop(stdin);
    let out = child.wait_with_output().await?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!(
            "{}",
            link_format::clean(why.trim().lines().last().unwrap_or("it was refused"))
        );
    }
    answered(&out.stdout)
}

/// The answer in the team chat's `--json` output (`{"thread", "answer",
/// "held"}`).
fn answered(out: &[u8]) -> anyhow::Result<String> {
    let said: serde_json::Value =
        serde_json::from_slice(out).context("reading lnk agent chat send --team --json")?;
    said["answer"]
        .as_str()
        .map(str::to_string)
        .context("the team chat gave no answer")
}

/// What the chat is told for an owner's `text` to an agent on a team:
/// none when it addresses no team, so the agent takes it as any other.
pub(super) async fn said(text: &str) -> Option<String> {
    let address = match address(text)? {
        Ok(address) => address,
        Err(why) => return Some(format!("Not delivered: {why}.")),
    };
    Some(match deliver(link_core::lnk(), &address).await {
        Ok(answer) => format!("{}: {answer}", address.shown()),
        Err(err) => format!("Not delivered to {}: {err:#}", address.shown()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to(team: &str, member: Option<&str>, text: &str) -> Option<Result<Address, String>> {
        Some(Ok(Address {
            team: team.into(),
            member: member.map(str::to_string),
            text: text.into(),
        }))
    }

    #[test]
    fn an_address_names_a_team_or_one_member() {
        assert_eq!(address("hello"), None);
        assert_eq!(
            address("  @research plan it"),
            to("research", None, "plan it")
        );
        assert_eq!(
            address("@research/writer draft\nit"),
            to("research", Some("writer"), "draft\nit")
        );
        for refused in [
            "@Research hi",
            "@research/ hi",
            "@a/b/c hi",
            "@ hi",
            "@research",
        ] {
            assert!(matches!(address(refused), Some(Err(_))), "{refused}");
        }
    }

    #[test]
    fn its_answer_is_the_team_chats() {
        let out = br#"{"thread": "team-t4m9q2x7k1ab", "answer": "Drafted.", "held": []}"#;
        assert_eq!(answered(out).unwrap(), "Drafted.");
        assert!(answered(b"{}").is_err());
    }

    /// Delivered through the owner's team chat, its member selected and
    /// its message on stdin; its refusal said back, nothing else tried.
    #[tokio::test]
    async fn it_goes_through_the_owners_team_chat() {
        let dir = link_test_kit::TempDir::new("addressing");
        let fake = dir.join("lnk");
        let seen = dir.join("seen");
        link_test_kit::write_program(
            &fake,
            format!(
                "#!/bin/sh\necho \"$@\" > {seen}\ncat >> {seen}\n\
                 case \"$*\" in *--member\\ ghost*) echo 'no team member named ghost' >&2; exit 1;; esac\n\
                 echo '{{\"thread\":\"team-x\",\"answer\":\"On it.\",\"held\":[]}}'\n",
                seen = seen.display()
            ),
        )
        .unwrap();
        let Some(Ok(writer)) = address("@research/writer Draft it.") else {
            unreachable!()
        };
        let answer = deliver(link_test_kit::env::command(&fake, &dir), &writer)
            .await
            .unwrap();
        assert_eq!(answer, "On it.");
        let seen_text = std::fs::read_to_string(&seen).unwrap();
        assert!(
            seen_text
                .starts_with("agent chat send --team research --member writer --message - --json"),
            "{seen_text}"
        );
        assert!(seen_text.ends_with("Draft it."), "{seen_text}");
        let Some(Ok(ghost)) = address("@research/ghost Hi.") else {
            unreachable!()
        };
        let refused = deliver(link_test_kit::env::command(&fake, &dir), &ghost)
            .await
            .unwrap_err();
        assert!(refused.to_string().contains("no team member named ghost"));
    }
}
