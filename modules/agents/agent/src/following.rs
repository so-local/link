//! `lnk agent chat send` follows its agent: callers say which agent, not
//! where it runs. A message for an agent paused for its move's switch, or
//! refused for now by the cloud hosting it (it's moving), waits, then is
//! sent again where the agent is: here, or on that cloud. Nothing was
//! taken until then, so each message is answered once, wherever it lands.
//! How long it waits is [`Follow`]'s (`following.toml`).

use std::time::{Duration, Instant};

use link_agent::cli;
use link_agent::{bridge, moved, service, settings::Settings};
use link_format::{Done, Name, Note, Out, Report, cmd, sentence};
use serde::Deserialize;

use crate::conversations;
use crate::moving::journal;

/// How long a message follows its agent, and how often it looks.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Follow {
    wait_secs: u64,
    look_ms: u64,
}

impl Follow {
    /// `following.toml`, read once.
    pub(crate) fn get() -> Follow {
        static READ: std::sync::OnceLock<Follow> = std::sync::OnceLock::new();
        *READ.get_or_init(|| {
            toml::from_str(include_str!("following.toml")).expect("following.toml reads")
        })
    }

    fn wait(&self) -> Duration {
        Duration::from_secs(self.wait_secs)
    }

    fn look(&self) -> Duration {
        Duration::from_millis(self.look_ms)
    }
}

/// Where a message's agent is, for now.
pub(crate) enum Found {
    /// Answered, wherever it ran.
    Done(Done),
    /// Not taken: the agent is moving, and why.
    Moving(String),
}

/// `lnk agent chat send --agent name`: one message, sent where the agent
/// is, waiting while it moves. A live chat isn't followed.
pub(crate) async fn send(out: &mut Out, name: &str, args: &cli::Chat) -> Result<Done, Report> {
    let agent = Name::new("agent", name);
    following(out, agent, args, async |out, args| {
        found(out, name, args).await
    })
    .await
}

/// `lnk agent chat send --team`: one message to a member, as [`send`], each try
/// finding the team and its member again.
pub(crate) async fn team(
    out: &mut Out,
    team: &str,
    chat: &cli::Chat,
    mut once: impl AsyncFnMut(&mut Out, &cli::Chat) -> Result<Found, Report>,
) -> Result<Done, Report> {
    let team = Name::new("team", team);
    following(out, team, chat, async |out, chat| once(out, chat).await).await
}

/// Sends `args`' message by `once` until it's taken, waiting while what
/// it's for, `what`, moves.
#[expect(
    clippy::disallowed_methods,
    reason = "looks again at the period following.toml sets: a move says nothing when it lands"
)]
async fn following(
    out: &mut Out,
    what: Name,
    args: &cli::Chat,
    mut once: impl AsyncFnMut(&mut Out, &cli::Chat) -> Result<Found, Report>,
) -> Result<Done, Report> {
    let mut args = args.clone();
    if args.message().is_none() {
        args.message = conversations::message_of(&args)?;
    }
    let follow = Follow::get();
    let until = Instant::now() + follow.wait();
    let mut told = false;
    loop {
        let why = match once(out, &args).await? {
            Found::Done(done) => return Ok(done),
            Found::Moving(why) => why,
        };
        if Instant::now() >= until {
            return Err(Report::error(sentence!(
                "{} is still moving after {} minutes: {}.",
                what.clone(),
                follow.wait_secs / 60,
                why
            ))
            .next(cmd!("lnk agent show"), "see where it is"));
        }
        if !told && !args.json && !args.stream_json {
            out.block(Note(sentence!(
                "{} is moving: your message waits for it to land.",
                what.clone()
            )));
            told = true;
        }
        tokio::time::sleep(follow.look()).await;
    }
}

/// One try: sent where agent `name` is, or why it's not taken yet.
pub(crate) async fn found(out: &mut Out, name: &str, args: &cli::Chat) -> Result<Found, Report> {
    if Settings::path_of(name).exists() {
        // A team move finishing here: the agent is leaving, its identity
        // perhaps gone already, or landing, its harness not yet answering.
        if crate::teams::finishing(name)? {
            return Ok(Found::Moving("its team's move is finishing here".into()));
        }
        let settings = Settings::resolve(Some(name))?;
        let answering =
            service::running_anywhere(name) || link_agent::idle::asleep(&settings.key());
        let id = settings.agent_id().unwrap_or_default();
        if bridge::paused(name) || (!answering && journal::switching(id)?) {
            return Ok(Found::Moving("it's switching here".into()));
        }
        return Ok(Found::Done(
            conversations::chat(out, &settings, args).await?,
        ));
    }
    let note = moved::of_name(name)?;
    match note {
        Some(note) if note.to.cloud => {
            Ok(match conversations::hosted_chat(out, &note, args).await? {
                Ok(done) => Found::Done(done),
                Err(later) => Found::Moving(later.why),
            })
        }
        Some(note) => Err(Report::error(sentence!(
            "{} moved to {} meanwhile.",
            Name::new("agent", name),
            note.place()
        ))
        .next(
            cmd!("lnk agent chat send --agent {}", name),
            "send it again there",
        )),
        // Neither here nor noted: as any chat, refused.
        None => Ok(Found::Done(
            conversations::chat(out, &Settings::resolve(Some(name))?, args).await?,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn following_toml_reads_into_a_bounded_wait() {
        let follow = Follow::get();
        assert!(follow.wait() >= Duration::from_secs(60));
        assert!(follow.look() > Duration::ZERO && follow.look() < follow.wait());
    }
}
