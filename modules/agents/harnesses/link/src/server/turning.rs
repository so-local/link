//! A message's turn, as every way into the harness runs it: the message
//! made an event, with its images and documents kept, and its turn
//! started, its reply read as it comes.

use std::sync::Arc;

use tokio::sync::mpsc;

use super::Server;
use crate::agent::{Agent, Thread};
use crate::event::{Event, Media};
use crate::turn::Asked;

/// The owner's, or `from`'s, message on `thread`: `text`, with `media`
/// its spec takes (`[connections] images`, `documents` and `voice`) kept
/// beside the conversation's events before its event names them, each
/// voice note's words heard before its text; what isn't taken, couldn't
/// be heard or couldn't be kept is said in its text.
#[expect(
    clippy::too_many_arguments,
    reason = "each part of a message as the server receives it"
)]
pub async fn message(
    server: &Server,
    agent: &Agent,
    thread: &Thread,
    text: String,
    media: Vec<Media>,
    from: Option<String>,
    from_name: Option<String>,
    hops: u32,
) -> Result<Event, String> {
    let config = server
        .harness
        .config(agent)
        .map_err(|err| format!("{err:#}"))?;
    let (media, untaken): (Vec<_>, Vec<_>) = media
        .into_iter()
        .partition(|media| config.takes(thread.name(), media.kind));
    let voices: Vec<Media> = media
        .iter()
        .filter(|media| media.kind == crate::event::Kind::Voice)
        .cloned()
        .collect();
    let (heard, unheard) = server.harness.heard(&config, &voices).await;
    let (parts, left) = server.harness.keep(agent, thread, media);
    let untaken = untaken.iter().map(|media| {
        let kind = media.kind.word();
        format!(
            "{} was left out: this agent takes no {kind}s",
            media.named()
        )
    });
    let left: Vec<String> = untaken.chain(unheard).chain(left).collect();
    let left = (!left.is_empty()).then(|| left.join("\n"));
    let text = match heard.is_empty() {
        true => text,
        false if text.is_empty() => heard.join("\n"),
        false => format!("{}\n\n{text}", heard.join("\n")),
    };
    let text = match left {
        Some(left) if text.is_empty() => left,
        Some(left) => format!("{text}\n\n{left}"),
        None => text,
    };
    Ok(Event {
        from,
        from_name,
        hops,
        parts,
        ..Event::user(&text)
    })
}

/// How a message's turn began.
pub enum Started {
    /// Steered into a turn already running, whose answer, on the message
    /// that started it, answers this one too.
    Steered,
    /// A turn of its own, its reply to read, and what keeps its answer
    /// should nobody take it.
    Running(Box<Reply>, Untaken),
}

/// Starts `message`'s turn. The turn runs to its end even if the caller
/// goes: the answer is kept either way. One that summarizes after its
/// answer tells it first, and the reply ends there. `busy`: what its
/// sender asked it to do while another turn runs.
pub async fn start(
    server: &Arc<Server>,
    agent: &Agent,
    thread: &Thread,
    message: Event,
    busy: Option<link_agent_spec::Busy>,
) -> Started {
    let (pieces, received) = mpsc::channel::<String>(64);
    let (begun, begins) = tokio::sync::oneshot::channel();
    let (told, answer) = tokio::sync::oneshot::channel();
    let mut turn = tokio::spawn({
        let (server, agent, thread) = (server.clone(), agent.clone(), thread.clone());
        async move {
            server
                .harness
                .ask_told(
                    &agent,
                    &thread,
                    message,
                    pieces,
                    Some(begun),
                    Some(told),
                    busy,
                )
                .await
        }
    });
    let steered = tokio::select! {
        Ok(()) = begins => None,
        ended = &mut turn => Some(ended),
    };
    let turn = match steered {
        None => turn,
        Some(Ok(Ok(Asked::Steered))) => return Started::Steered,
        Some(ended) => tokio::spawn(async move { ended.map_err(anyhow::Error::from)? }),
    };
    let reply = Reply {
        received,
        answer: Some(answer),
        told: None,
        turn,
    };
    let untaken = Untaken {
        server: server.clone(),
        agent: agent.clone(),
        thread: thread.clone(),
    };
    Started::Running(Box::new(reply), untaken)
}

/// Keeps a turn's answer whose caller went before it was told it (its
/// bridge stopped), for the bridge to send when it's back.
pub struct Untaken {
    server: Arc<Server>,
    agent: Agent,
    thread: Thread,
}

impl Untaken {
    /// `ended`'s answer kept as untaken: a command's reply is no answer
    /// to keep.
    pub fn keep(&self, ended: &Ended) {
        if let Ok(Ok(Asked::Answered(answer))) = ended
            && answer.is_answer()
        {
            let (server, agent, thread, seq) = (
                self.server.clone(),
                self.agent.clone(),
                self.thread.clone(),
                answer.seq,
            );
            tokio::task::spawn_blocking(move || server.harness.untaken(&agent, &thread, seq));
        }
    }
}

/// How a turn ended.
pub type Ended = Result<anyhow::Result<Asked>, tokio::task::JoinError>;

/// A turn's reply: its pieces as they come, then how it ended. A turn
/// that summarizes after its answer tells it first, and the reply ends
/// then, without waiting for the summary.
pub struct Reply {
    received: mpsc::Receiver<String>,
    answer: Option<tokio::sync::oneshot::Receiver<Event>>,
    told: Option<Event>,
    turn: tokio::task::JoinHandle<anyhow::Result<Asked>>,
}

impl Reply {
    /// The next piece; none once the turn ended, or told its answer
    /// and every piece before it was taken.
    pub async fn next(&mut self) -> Option<String> {
        loop {
            // Every piece was sent before it was told.
            if self.told.is_some() {
                return self.received.try_recv().ok();
            }
            let Some(answer) = &mut self.answer else {
                return self.received.recv().await;
            };
            tokio::select! {
                biased;
                piece = self.received.recv() => return piece,
                told = answer => {
                    // Not told: the turn's end says.
                    self.answer = None;
                    self.told = told.ok();
                }
            }
        }
    }

    /// How it ended: told, or the turn's own end.
    pub async fn end(self) -> Ended {
        match self.told {
            Some(told) => Ok(Ok(Asked::Answered(Box::new(told)))),
            None => self.turn.await,
        }
    }
}

/// Pursues `thread`'s goal after a turn of it ended ([`crate::goal`]):
/// while its model judges it doesn't hold and its spec's `[goals]`
/// limits allow, a turn of the agent's own toward it, each answer kept
/// for its chat as an answer nobody waited for is, which its bridge
/// sends. One at a time on a chat; it ends the goal, saying why, when
/// it holds, can't be reached, or its limits are reached.
pub async fn pursue(server: Arc<Server>, agent: Agent, thread: Thread) {
    let harness = &server.harness;
    let Some(_pursuing) = harness.pursuing(&agent, &thread) else {
        return;
    };
    let end = |why: String| {
        if let Err(err) = harness.end_goal(&agent, &thread, &why) {
            crate::warn!(crate::said::in_chat(&agent, &thread) => "ending its goal: {err:#}");
        }
    };
    loop {
        let Ok(config) = harness.config(&agent) else {
            return;
        };
        let opinions = config.opinions();
        let on = opinions.commands.as_ref().and_then(|c| c.goal) == Some(true);
        let Some(limits) = opinions.goals.filter(|g| on && g.turns.is_some()) else {
            return;
        };
        // Found from what the turn added, never by reading the chat whole
        // after every answer.
        let Ok(Some((goal, answer))) = harness.pursued(&agent, &thread) else {
            return;
        };
        let answer = answer.as_deref().unwrap_or("(none yet)");
        let (verdict, why) = match harness.judged(&agent, &thread, &goal, answer).await {
            Ok(judged) => judged,
            Err(err) => return end(format!("it couldn't be judged: {err:#}")),
        };
        match verdict {
            crate::goal::Verdict::Holds => return end(format!("it holds: {why}")),
            crate::goal::Verdict::Impossible => {
                return end(format!("it can't be reached: {why}"));
            }
            crate::goal::Verdict::Going => {}
        }
        if let Some(spent) = crate::goal::spent(&goal, &limits) {
            return end(spent);
        }
        // Nobody reads it as it's written: its answer is kept for its chat.
        let (pieces, mut unread) = mpsc::channel::<String>(64);
        tokio::spawn(async move { while unread.recv().await.is_some() {} });
        let message = Event::user(&crate::goal::going_on(&goal, &why));
        match harness
            .ask_told(&agent, &thread, message, pieces, None, None, None)
            .await
        {
            Ok(Asked::Answered(answer)) if answer.is_answer() => {
                harness.untaken(&agent, &thread, answer.seq);
            }
            Ok(_) => {}
            Err(err) => return end(format!("its turn failed: {err:#}")),
        }
    }
}
