//! `lnk agent chat send`: talking to an agent from this machine, on `link`, a
//! connection of Link's own with no account and no bot. Its threads are
//! `link:<name>` (`link:main` unless named).
//!
//! A message goes as a connection's bridge sends one: to the harness's
//! endpoint, with its thread, its key and its proof
//! ([`link_connection_contract::ask_for`]), as the owner's, so a harness
//! that answers an endpoint speaks it with nothing new. Only the agent's
//! user reaches it: what reaches the endpoint (its plan,
//! [`crate::bridge::Plan`]) is readable by that user alone.
//!
//! An answer in a `link` thread nobody waited for is handed over at the
//! next chat on that thread: one to a message from others (a schedule, a
//! background command ending) is held by the agent's bridge ([`Held`]),
//! and one its harness finished after its caller went (a crash, Ctrl-C)
//! is kept by its harness until a chat on that thread asks for it
//! ([`crate::bridge::WayIn::recover_for`]).
//!
//! An agent a cloud account hosts is reached through that account's
//! adapter (`agent-chat`, [`link_agent_place::chat`]) instead, on the
//! same threads: what it answers nobody waited for stays where it runs.

use link_format::WithinExt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::bridge::WayIn;

/// The connection's name, its threads, and the thread a chat is on unless it
/// names one: the `link` connection's, as every side of it names them.
pub use link_connection_contract::{
    LINK as CONNECTION, LINK_MAIN as MAIN, is_link_thread as is_link, link_thread as thread,
};

/// What the owner and their agent's model call this connection: their
/// terminal, where `lnk agent chat` runs.
pub const TERMINAL: &str = "cli";

/// How many answers are held, the latest: a schedule firing in a thread
/// nobody chats on fills nothing.
const HELD_KEPT: usize = 1000;

/// The most of an answer held, in bytes: past it, the answer is cut, and
/// says where it is whole (its conversation).
const HELD_MAX: usize = 64 << 10;

/// How often a chat waiting for its next message asks for answers nobody
/// waited for, and a message that joined a running turn for that turn's
/// answer.
pub const LOOK_EVERY: Duration = Duration::from_millis(500);

/// How long a message that joined a running turn waits for its answer.
const JOINED_WAIT: Duration = Duration::from_secs(30 * 60);

/// An answer held for the next chat on its thread: a line of
/// `held.jsonl`, naming its schema.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HeldAnswer {
    thread: String,
    text: String,
}

link_files::specification!(HeldAnswer, include_str!("chat/held.schema.toml"));

/// The answers in `link` threads to messages from others, which nobody
/// waited for, held for the next chat on each thread: one JSON line each,
/// in the agent's own folder for the connection, which only its user reads
/// and its harness never reaches.
#[derive(Clone, Debug)]
pub struct Held {
    path: PathBuf,
}

impl Held {
    /// Agent `agent`'s: `~/.lnk/safe/agents/<agent>/connections/link/held.jsonl`.
    pub fn of(agent: &str) -> Self {
        Self::at(&crate::bridge::state(agent, CONNECTION).join("held.jsonl"))
    }

    /// Those kept at `path`.
    pub fn at(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }

    /// Holds `text`, an answer in `thread`, for the next chat on it.
    /// Past [`HELD_MAX`], it's cut, saying where it's whole.
    pub fn keep(&self, thread: &str, text: &str) -> anyhow::Result<()> {
        let (_lock, mut held) = self.locked()?;
        held.push(HeldAnswer {
            thread: thread.to_string(),
            text: cut(text, thread),
        });
        let extra = held.len().saturating_sub(HELD_KEPT);
        held.drain(..extra);
        self.write(&held)
    }

    /// The answers held for `thread`, in order, no longer held.
    pub fn take(&self, thread: &str) -> anyhow::Result<Vec<String>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let (_lock, held) = self.locked()?;
        let (taken, kept): (Vec<HeldAnswer>, Vec<HeldAnswer>) =
            held.into_iter().partition(|answer| answer.thread == thread);
        if !taken.is_empty() {
            self.write(&kept)?;
        }
        Ok(taken.into_iter().map(|answer| answer.text).collect())
    }

    /// Thread `thread`'s lock, held while a chat on it takes what its
    /// harness kept: two chats on one thread never both show one answer.
    fn taking(&self, thread: &str) -> anyhow::Result<std::fs::File> {
        let dir = self.path.parent().within("no folder for held answers")?;
        link_files::create_private_dir(dir).within(format!("making {}", dir.display()))?;
        let hex: String = thread.bytes().map(|b| format!("{b:02x}")).collect();
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(format!("{hex}.lock")))?;
        lock.lock()?;
        Ok(lock)
    }

    /// Its answers, locked for one reader and writer.
    fn locked(&self) -> anyhow::Result<(std::fs::File, Vec<HeldAnswer>)> {
        let dir = self.path.parent().within("no folder for held answers")?;
        link_files::create_private_dir(dir).within(format!("making {}", dir.display()))?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.path.with_extension("lock"))?;
        lock.lock()?;
        let held = match std::fs::read_to_string(&self.path) {
            Ok(text) => text
                .lines()
                .filter_map(|line| link_files::saved::parse_line(line.as_bytes()).ok())
                .collect(),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(err) => return Err(err).within("reading the held answers"),
        };
        Ok((lock, held))
    }

    /// `held` written, or the file gone when there are none.
    fn write(&self, held: &[HeldAnswer]) -> anyhow::Result<()> {
        if held.is_empty() {
            return match std::fs::remove_file(&self.path) {
                Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.into()),
                _ => Ok(()),
            };
        }
        let mut text = Vec::new();
        for answer in held {
            text.extend(link_files::saved::line(answer)?);
        }
        link_files::write_private(&self.path, &text)
            .within(format!("writing {}", self.path.display()))
    }
}

/// `text`, cut at [`HELD_MAX`] (on a character's edge) with a line saying
/// `lnk agent chat show <thread>` has it whole.
fn cut(text: &str, thread: &str) -> String {
    if text.len() <= HELD_MAX {
        return text.to_string();
    }
    let mut end = HELD_MAX;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n[cut at {} KB: lnk agent chat show {thread} has it whole]",
        &text[..end],
        HELD_MAX >> 10
    )
}

/// What `lnk agent chat send --json` prints.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chatted {
    /// The conversation (`link:main`).
    pub thread: String,
    /// The agent's answer to the message.
    pub answer: String,
    /// The answers in the thread nobody waited for, handed over before
    /// it, oldest first.
    pub held: Vec<String>,
}

/// One chat: a thread of an agent's, wherever it runs.
pub struct Chat {
    way: Way,
    thread: String,
}

/// Where a chat's messages go.
enum Way {
    /// The agent's harness on this machine, through its bridge's way to
    /// it.
    Here {
        // Boxed, as the account is: each is large, and a command has
        // one chat.
        way_in: Box<WayIn>,
        held: Held,
        /// Whether its harness keeps the answers nobody took
        /// ([`crate::bridge::Plan::recover`]).
        recover: bool,
    },
    /// The cloud account hosting it, through its adapter's `agent-chat`
    /// ([`link_agent_place::chat`]): an answer nobody waited for there
    /// stays in its conversation there.
    Hosted {
        account: Box<link_agent_place::Account>,
        agent: link_agent_place::HostedAgent,
    },
    HostedTeam {
        account: Box<link_agent_place::Account>,
        team: String,
        member: Option<String>,
    },
}

/// What came of a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Said {
    /// The agent's answer, whole.
    Answered(String),
    /// Ctrl-C: the chat stopped waiting. The harness may finish the turn,
    /// whose answer is handed over at the next chat.
    Stopped,
}

impl Chat {
    /// Agent `agent`'s thread `thread`, its harness reached as `plan`
    /// says.
    pub fn new(
        agent: &crate::settings::Settings,
        plan: &crate::bridge::Plan,
        thread: String,
    ) -> Self {
        Self::with(
            WayIn::new(agent, plan),
            thread,
            Held::of(&agent.name),
            plan.recover,
        )
    }

    /// [`Chat::new`], from its parts.
    pub fn with(way_in: WayIn, thread: String, held: Held, recover: bool) -> Self {
        Self {
            way: Way::Here {
                way_in: Box::new(way_in),
                held,
                recover,
            },
            thread,
        }
    }

    /// Hosted agent `agent`'s thread `thread`, on the cloud account
    /// `account` that hosts it.
    pub fn hosted(
        account: link_agent_place::Account,
        agent: link_agent_place::HostedAgent,
        thread: String,
    ) -> Self {
        Self {
            way: Way::Hosted {
                account: Box::new(account),
                agent,
            },
            thread,
        }
    }

    pub fn thread(&self) -> &str {
        &self.thread
    }

    /// A team's owner chat, resolved again at the hosting service for
    /// every message, including after a lead or membership change.
    pub fn hosted_team(
        account: link_agent_place::Account,
        team: String,
        member: Option<String>,
        thread: String,
    ) -> Self {
        Self {
            way: Way::HostedTeam {
                account: Box::new(account),
                team,
                member,
            },
            thread,
        }
    }

    /// What the person at it sees when Ctrl-C stops waiting for an
    /// answer.
    pub fn stopped(&self) -> &'static str {
        match self.way {
            Way::Here { .. } => STOPPED,
            Way::Hosted { .. } | Way::HostedTeam { .. } => STOPPED_HOSTED,
        }
    }

    /// The answers in this thread nobody waited for, oldest first, no
    /// longer held: those its bridge held, then those its harness kept.
    /// None for an agent hosted elsewhere, whose host keeps none.
    pub async fn handed(&self) -> anyhow::Result<Vec<String>> {
        let Way::Here {
            way_in,
            held,
            recover,
        } = &self.way
        else {
            return Ok(Vec::new());
        };
        let _taking = held.taking(&self.thread)?;
        let mut handed = held.take(&self.thread)?;
        let _inbox = held.taking(CONNECTION)?;
        handed.extend(held.take(CONNECTION)?);
        if *recover {
            handed.extend(way_in.recover_for(Some(&self.thread), &[]).await?);
        }
        Ok(handed)
    }

    /// Sends `text` as the owner's, each piece of the answer to
    /// `progress` as it comes; the answer. A message that joined a turn
    /// already running (a turn whose chat stopped waiting) is answered by
    /// that turn's answer, once its harness has it.
    pub async fn ask(
        &self,
        text: &str,
        progress: &tokio::sync::watch::Sender<String>,
    ) -> anyhow::Result<String> {
        use link_connection_contract::Asked;
        match &self.way {
            Way::HostedTeam {
                account,
                team,
                member,
            } => {
                match link_agent_place::team_chat(
                    account,
                    team,
                    member.as_deref(),
                    &self.thread,
                    text,
                    progress,
                )
                .await?
                {
                    Asked::Answer(answer) => Ok(answer),
                    Asked::Steered => {
                        anyhow::bail!("the hosting service did not return the team member's answer")
                    }
                }
            }
            Way::Here {
                way_in, recover, ..
            } => {
                // What its harness keeps in the thread before it's sent:
                // none of it is the answer of a turn the message joins,
                // which runs.
                let before = match recover {
                    true => way_in.kept(&self.thread).await?,
                    false => Vec::new(),
                };
                let asked = link_connection_contract::ask_for(
                    way_in.endpoint(),
                    &self.thread,
                    text,
                    progress,
                )
                .await?;
                match asked {
                    Asked::Answer(answer) => Ok(answer),
                    Asked::Steered => self.joined(&before).await,
                }
            }
            Way::Hosted { account, agent } => {
                // Its adapter ends with this: Ctrl-C, dropping it, kills it.
                let asked =
                    link_agent_place::chat(account, agent, &self.thread, text, progress).await?;
                match asked {
                    Asked::Answer(answer) => Ok(answer),
                    Asked::Steered => anyhow::bail!(
                        "it joined the turn already running in {}, whose answer is in its \
                         conversation where it runs",
                        self.thread
                    ),
                }
            }
        }
    }

    /// The answer of the turn a message joined: kept by its harness when
    /// it ends, since the chat that started it stopped waiting. One kept
    /// before the message was sent (`before`) isn't it, and stays for the
    /// next chat; a schedule's answer is held apart ([`Held`]), never
    /// taken for it.
    #[expect(
        clippy::disallowed_methods,
        reason = "polls the thread for what was handed, which is written without notice"
    )]
    async fn joined(&self, before: &[String]) -> anyhow::Result<String> {
        let Way::Here {
            way_in,
            held,
            recover,
        } = &self.way
        else {
            anyhow::bail!("only a chat with an agent here joins a turn");
        };
        anyhow::ensure!(
            *recover,
            "it joined the turn already running in {}, whose answer goes where that turn's does",
            self.thread
        );
        let started = std::time::Instant::now();
        while started.elapsed() < JOINED_WAIT {
            let handed = {
                let _taking = held.taking(&self.thread)?;
                way_in.recover_for(Some(&self.thread), before).await?
            };
            if !handed.is_empty() {
                return Ok(handed.join("\n\n"));
            }
            tokio::time::sleep(LOOK_EVERY).await;
        }
        anyhow::bail!(
            "it joined the turn already running in {}, which hasn't ended in {} minutes",
            self.thread,
            JOINED_WAIT.as_secs() / 60
        )
    }

    /// Sends `text` and writes the answer to `out` as it comes, until it
    /// ends or `stop` resolves (Ctrl-C).
    pub async fn turn(
        &self,
        text: &str,
        out: &mut (dyn Write + Send),
        stop: impl std::future::Future<Output = ()>,
    ) -> anyhow::Result<Said> {
        self.turn_answering(text, out, stop, None).await
    }

    /// [`Self::turn`], meanwhile showing a question the agent's turn asks
    /// its owner, kept for this chat, and answering it with a line typed
    /// from `lines` that is its code. Any other line typed goes to
    /// `typed`, for after the turn.
    pub async fn turn_answering(
        &self,
        text: &str,
        out: &mut (dyn Write + Send),
        stop: impl std::future::Future<Output = ()>,
        mut lines: Option<(
            &mut tokio::sync::mpsc::Receiver<String>,
            &mut std::collections::VecDeque<String>,
        )>,
    ) -> anyhow::Result<Said> {
        let (progress, mut pieces) = tokio::sync::watch::channel(String::new());
        let asking = self.ask(text, &progress);
        tokio::pin!(asking, stop);
        let mut shown = Shown::default();
        let mut look = tokio::time::interval(LOOK_EVERY);
        loop {
            tokio::select! {
                asked = &mut asking => {
                    let answer = asked?;
                    shown.show(&answer, out)?;
                    shown.end(out)?;
                    return Ok(Said::Answered(answer));
                }
                Ok(()) = pieces.changed() => {
                    let so_far = pieces.borrow_and_update().clone();
                    shown.show(&so_far, out)?;
                }
                _ = look.tick(), if lines.is_some() => {
                    for question in self.questions() {
                        writeln!(out, "\n{}\n", printable(&question))?;
                    }
                }
                Some(line) = async {
                    match lines.as_mut() {
                        Some((lines, _)) => lines.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    // A question's code answers it here, as on any chat;
                    // anything else waits for the turn to end.
                    match self.answers(&line) {
                        Some(said) => writeln!(out, "{}", printable(&said))?,
                        None => {
                            if let Some((_, typed)) = lines.as_mut() {
                                typed.push_back(line);
                            }
                        }
                    }
                }
                () = &mut stop => {
                    shown.end(out)?;
                    return Ok(Said::Stopped);
                }
            }
        }
    }

    /// What `line` answers, if it's the code of a question asked on this
    /// chat ([`link_connection_contract::questions::caught`]).
    fn answers(&self, line: &str) -> Option<String> {
        let Way::Here { way_in, .. } = &self.way else {
            return None;
        };
        link_connection_contract::questions::caught(way_in.agent(), &self.thread, line)
    }

    /// The questions kept for this chat by the agent's bridge while its
    /// turn runs ([`crate::bridge`]'s asking), no longer kept.
    fn questions(&self) -> Vec<String> {
        let Way::Here { held, .. } = &self.way else {
            return Vec::new();
        };
        held.take(&self.thread).unwrap_or_default()
    }
}

/// What the person at a chat sees before an answer nobody waited for.
pub const WHILE_AWAY: &str = "While you were away:";

/// What the person at a chat sees when Ctrl-C stops waiting for an
/// answer.
pub const STOPPED: &str = "Stopped. Its answer, if it finishes, comes at your next message.";

/// [`STOPPED`], for an agent hosted by a cloud: its answer, if it
/// finishes, stays where it runs.
pub const STOPPED_HOSTED: &str =
    "Stopped. Its answer, if it finishes, is in its conversation where it runs.";

/// The answers nobody waited for, `handed`, under [`WHILE_AWAY`], so the
/// person at a chat tells them from the answer to what they sent: nothing
/// when there are none.
fn show_handed(out: &mut (dyn Write + Send), handed: &[String]) -> std::io::Result<()> {
    if !handed.is_empty() {
        writeln!(out, "{WHILE_AWAY}")?;
        for answer in handed {
            writeln!(out, "{}\n", printable(answer))?;
        }
    }
    Ok(())
}

/// One message, for a script: `text` sent in `chat`, its answer written
/// to `out` as it comes (none: only returned), after the answers nobody
/// waited for under [`WHILE_AWAY`], until it ends or `stop` resolves. An
/// empty answer is the caller's to refuse.
pub async fn once(
    chat: &Chat,
    text: &str,
    out: Option<&mut (dyn Write + Send)>,
    stop: impl std::future::Future<Output = ()>,
) -> anyhow::Result<Option<Chatted>> {
    let held = chat.handed().await?;
    let mut sink = std::io::sink();
    let out = match out {
        Some(out) => {
            show_handed(out, &held)?;
            out
        }
        None => &mut sink,
    };
    let Said::Answered(answer) = chat.turn(text, out, stop).await? else {
        return Ok(None);
    };
    Ok(Some(Chatted {
        thread: chat.thread.clone(),
        answer,
        held,
    }))
}

/// A conversation, in a terminal: each line from `lines` sent in `chat`,
/// its answer written to `out` as it comes, until the lines end (Ctrl-D)
/// or an interrupt (Ctrl-C) comes at the prompt. An interrupt while an
/// answer comes stops waiting for it. Meanwhile, each answer nobody
/// waited for (a reminder) shows as it's handed over.
#[expect(
    clippy::disallowed_methods,
    reason = "the chat's own period of looking"
)]
pub async fn converse(
    chat: &Chat,
    first: Option<String>,
    lines: &mut tokio::sync::mpsc::Receiver<String>,
    interrupts: &mut tokio::sync::mpsc::Receiver<()>,
    out: &mut (dyn Write + Send),
) -> anyhow::Result<()> {
    show_handed(out, &chat.handed().await?)?;
    // Lines typed while a turn ran, its questions' codes aside.
    let mut typed = std::collections::VecDeque::new();
    let mut first = first;
    loop {
        write!(out, "> ")?;
        out.flush()?;
        // The first message given, shown as though typed.
        let line = if let Some(first) = first.take() {
            writeln!(out, "{}", printable(&first))?;
            Some(first)
        } else {
            loop {
                // Shown already, as it was typed.
                if let Some(line) = typed.pop_front() {
                    break Some(line);
                }
                tokio::select! {
                    line = lines.recv() => break line,
                    Some(()) = interrupts.recv() => break None,
                    () = tokio::time::sleep(LOOK_EVERY * 4) => {
                        // A reminder come meanwhile, above a new prompt; the
                        // harness not answering is said at the next message.
                        if let Ok(handed) = chat.handed().await && !handed.is_empty() {
                            writeln!(out)?;
                            show_handed(out, &handed)?;
                            write!(out, "> ")?;
                            out.flush()?;
                        }
                    }
                }
            }
        };
        let Some(line) = line else {
            writeln!(out)?;
            return Ok(());
        };
        if line.trim() == "/quit" {
            return Ok(());
        }
        if line.trim().is_empty() {
            continue;
        }
        show_handed(out, &chat.handed().await.unwrap_or_default())?;
        let stop = async {
            interrupts.recv().await;
        };
        match chat
            .turn_answering(&line, out, stop, Some((&mut *lines, &mut typed)))
            .await
        {
            Ok(Said::Answered(answer)) if answer.trim().is_empty() => {
                writeln!(out, "(no answer: lnk agent log show shows why)")?
            }
            Ok(Said::Answered(_)) => {}
            Ok(Said::Stopped) => writeln!(out, "{}", chat.stopped())?,
            Err(err) => writeln!(out, "{}", printable(&format!("{err:#}")))?,
        }
    }
}

/// How much of an answer streamed has been written.
#[derive(Default)]
struct Shown {
    written: String,
}

impl Shown {
    /// Writes what of `so_far` isn't yet: the rest, or, when it isn't
    /// what was written and more (an answer not streamed), all of it on
    /// a line of its own.
    fn show(&mut self, so_far: &str, out: &mut (dyn Write + Send)) -> std::io::Result<()> {
        let rest = match so_far.strip_prefix(self.written.as_str()) {
            Some(rest) => rest,
            None => {
                writeln!(out)?;
                so_far
            }
        };
        if !rest.is_empty() {
            out.write_all(printable(rest).as_bytes())?;
            out.flush()?;
            self.written = so_far.to_string();
        }
        Ok(())
    }

    /// Ends the answer's last line.
    fn end(&self, out: &mut (dyn Write + Send)) -> std::io::Result<()> {
        if !self.written.is_empty() && !self.written.ends_with('\n') {
            writeln!(out)?;
        }
        out.flush()
    }
}

/// `text` as a terminal may show it: each line without control
/// characters (an escape that would move the cursor, set the title or
/// the clipboard), its line breaks kept.
pub fn printable(text: &str) -> String {
    text.split('\n')
        .map(link_format::clean)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thread_is_link_and_a_plain_name() {
        assert_eq!(thread("main").unwrap(), "link:main");
        assert_eq!(thread("qa-1.run_2").unwrap(), "link:qa-1.run_2");
        // A copy kept beside a thread, named for the harness it came from.
        assert_eq!(thread("main~hermes").unwrap(), "link:main~hermes");
        for bad in [
            "",
            "-x",
            "a:b",
            "a/b",
            "a b",
            "ü",
            &"a".repeat(65),
            "main~",
            "~x",
            "a~b~c",
        ] {
            assert!(thread(bad).is_err(), "{bad:?}");
        }
        assert!(is_link("link:main"));
        assert!(!is_link("telegram:42"));
        assert!(!is_link("linked:1"));
        assert!(!is_link("task-a1b2c3d4e5f6"));
    }

    #[test]
    fn held_answers_are_taken_by_their_thread_once() {
        let dir = link_test_kit::TempDir::new("chat-held");
        let held = Held::at(&dir.join("link/held.jsonl"));
        assert!(held.take("link:main").unwrap().is_empty());
        held.keep("link:main", "Stretch!").unwrap();
        held.keep("link:qa", "Other.").unwrap();
        held.keep("link:main", "Again.").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("link/held.jsonl"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0, "{mode:o}");
        }
        assert_eq!(held.take("link:main").unwrap(), ["Stretch!", "Again."]);
        assert!(held.take("link:main").unwrap().is_empty());
        assert_eq!(held.take("link:qa").unwrap(), ["Other."]);
        assert!(!dir.join("link/held.jsonl").exists());
    }

    #[test]
    fn only_the_latest_are_held() {
        let dir = link_test_kit::TempDir::new("chat-held-cap");
        let held = Held::at(&dir.join("held.jsonl"));
        for n in 0..HELD_KEPT + 2 {
            held.keep("link:main", &n.to_string()).unwrap();
        }
        let taken = held.take("link:main").unwrap();
        assert_eq!(taken.len(), HELD_KEPT);
        assert_eq!(taken[0], "2");
    }

    #[test]
    fn a_held_answer_is_cut_saying_where_it_is_whole() {
        assert_eq!(cut("short", "link:main"), "short");
        let long = "é".repeat(HELD_MAX);
        let held = cut(&long, "link:main");
        assert!(held.len() < HELD_MAX + 100, "{}", held.len());
        assert!(held.ends_with("\n[cut at 64 KB: lnk agent chat show link:main has it whole]"));
    }

    #[test]
    fn answers_nobody_waited_for_show_under_their_heading() {
        let mut out = Vec::new();
        show_handed(&mut out, &[]).unwrap();
        assert!(out.is_empty());
        show_handed(&mut out, &["Stretch!".into(), "Done: qa-bg".into()]).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            format!("{WHILE_AWAY}\nStretch!\n\nDone: qa-bg\n\n")
        );
    }

    #[test]
    fn an_answer_shows_without_escapes() {
        assert_eq!(printable("a\x1b]52;c;x\x07b\nnext"), "a]52;c;xb\nnext");
        let mut out = Vec::new();
        let mut shown = Shown::default();
        shown.show("I see ", &mut out).unwrap();
        shown.show("I see you", &mut out).unwrap();
        shown.end(&mut out).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "I see you\n");
    }
}
