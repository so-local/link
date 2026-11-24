use super::*;
use link_format::{
    Done, Exit, Foreign, Name, Out, Product, Prose, Refuser, Report, Sentence, WithinExt, cmd,
    phrase, prose, sentence,
};

/// What `connect` is asked: the agent's name and the bots other agents
/// have.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Request {
    pub agent: String,
    pub taken: Vec<Taken>,
}

/// Which of the connection protocol's commands a connection's program
/// was run with (`actions.toml`), with what each was given: what each
/// connection's generated parser reads.
pub enum Operation {
    /// `connect --agent <name> [--taken <json>]`, `--taken` a JSON list.
    Connect {
        agent: String,
        taken: String,
    },
    Describe,
    Rule,
    Bridge,
    Send,
    Owner,
    Share,
    Hook,
    Unhook,
    Sift,
    Receive,
    Hold,
    /// `link [--again]`.
    Link {
        again: bool,
    },
    State,
    /// Core's `uninstall`.
    Uninstall(link_core::uninstall::Flags),
}

impl Operation {
    /// The protocol's command at `words`, as a connection's generated
    /// parser read them, for a command given nothing: none for `connect`
    /// and `link`, whose arguments the parser holds, and for a command
    /// that isn't the protocol's.
    pub fn of(words: &[String]) -> Option<Self> {
        let [word] = words else {
            return None;
        };
        Some(match word.as_str() {
            "describe" => Self::Describe,
            "rule" => Self::Rule,
            "bridge" => Self::Bridge,
            "send" => Self::Send,
            "owner" => Self::Owner,
            "share" => Self::Share,
            "hook" => Self::Hook,
            "unhook" => Self::Unhook,
            "sift" => Self::Sift,
            "receive" => Self::Receive,
            "hold" => Self::Hold,
            "state" => Self::State,
            _ => return None,
        })
    }
}

/// What `connect` is asked, from its `--agent` and `--taken`, its
/// declared default `[]` when left out: no bots taken.
pub fn connect_request(agent: String, taken: &str) -> anyhow::Result<Request> {
    Ok(Request {
        agent,
        taken: serde_json::from_str(taken)?,
    })
}

/// Runs a connection module's `main`: `connect` (then `run` makes the
/// connection and pairs its owner) answers with what it made, and each
/// other `operation` as the protocol says. What it shows a person goes to
/// `out`; what it answers a program goes to stdout, only that.
pub fn main<F>(about: About, operation: Operation, run: F) -> std::process::ExitCode
where
    F: AsyncFnOnce(Request, &mut Out) -> Result<Connected, Report>,
{
    link_settings::output::run(|out| {
        run_main(
            out,
            about,
            operation,
            run,
            None::<fn(Bridge) -> NoBridge>,
            None::<fn(Bridge, bool, &mut Out) -> anyhow::Result<bool>>,
            None,
        )
    })
}

/// [`main`] for a connection whose module carries messages
/// ([`Describe::bridged`]): `bridge` runs `carry` with what it reads on
/// stdin, until stdin closes.
pub fn main_with_bridge<F, B>(
    about: About,
    operation: Operation,
    run: F,
    carry: impl FnOnce(Bridge) -> B,
) -> std::process::ExitCode
where
    F: AsyncFnOnce(Request, &mut Out) -> Result<Connected, Report>,
    B: std::future::Future<Output = anyhow::Result<()>>,
{
    link_settings::output::run(|out| {
        run_main(
            out,
            about,
            operation,
            run,
            Some(carry),
            None::<fn(Bridge, bool, &mut Out) -> anyhow::Result<bool>>,
            None,
        )
    })
}

/// [`main_with_bridge`] for a linked connection ([`Describe::linked`]),
/// whose account is linked as a device: `link [--again]` runs `link`
/// with what it reads on stdin and whether to link anew; it returns
/// whether the connection is linked (false: it isn't, and there's no
/// terminal to link it on). `state` answers `state`: what of its folder
/// is the link.
pub fn main_linked<F, B>(
    about: About,
    operation: Operation,
    run: F,
    carry: impl FnOnce(Bridge) -> B,
    link: impl FnOnce(Bridge, bool, &mut Out) -> anyhow::Result<bool>,
    state: crate::harness::State,
) -> std::process::ExitCode
where
    F: AsyncFnOnce(Request, &mut Out) -> Result<Connected, Report>,
    B: std::future::Future<Output = anyhow::Result<()>>,
{
    link_settings::output::run(|out| {
        run_main(
            out,
            about,
            operation,
            run,
            Some(carry),
            Some(link),
            Some(state),
        )
    })
}

/// What a connection module exits with when the connection refused the bot's
/// token (`take`, `send`): it won't answer until the token changes.
pub const TOKEN_REFUSED: u8 = 11;

/// The connection refused the bot's token. A module's `take` or `send`
/// failing with it (or with an error it wraps) exits [`TOKEN_REFUSED`].
#[derive(Debug)]
pub struct TokenRefused;

impl std::fmt::Display for TokenRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the connection refused the bot's token")
    }
}

impl std::error::Error for TokenRefused {}

/// Whether `err` is, or wraps, [`TokenRefused`].
pub fn token_refused(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| cause.is::<TokenRefused>())
}

/// What a connection module's `take` exits with when the connection refused
/// its answer (the chat gone, the bot blocked or removed from it): the
/// message reached the agent, and its answer the chat never got.
pub const REPLY_REFUSED: u8 = 12;

/// The connection refused the answer to a message: it answered the send and
/// said no, as it does for a chat gone or a bot blocked, and for a request
/// of the module's own it can't take (a malformed one), never a connection
/// not reached, a refused token ([`TokenRefused`]) or a send told to
/// wait. A module's `take` failing with it (or with an error it wraps)
/// exits [`REPLY_REFUSED`].
#[derive(Debug)]
pub struct ReplyRefused;

impl std::fmt::Display for ReplyRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the connection refused the answer")
    }
}

impl std::error::Error for ReplyRefused {}

/// Whether `err` is, or wraps, [`ReplyRefused`].
pub fn reply_refused(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| cause.is::<ReplyRefused>())
}

/// What a connection module's `hook` exits with when the connection refused
/// to push to the address it was given: its pushes don't come.
pub const WEBHOOK_REFUSED: u8 = 13;

/// The connection refused the webhook `hook` set: it answered and said no
/// to the address or its secret (an address it can't reach or won't
/// take), never a connection not reached or a refused token
/// ([`TokenRefused`]). A module's `hook` failing with it (or with an
/// error it wraps) exits [`WEBHOOK_REFUSED`].
#[derive(Debug)]
pub struct WebhookRefused;

impl std::fmt::Display for WebhookRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the connection refused the webhook")
    }
}

impl std::error::Error for WebhookRefused {}

/// Whether `err` is, or wraps, [`WebhookRefused`].
pub fn webhook_refused(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| cause.is::<WebhookRefused>())
}

type NoBridge = std::future::Ready<anyhow::Result<()>>;

/// What an error a connection module's command ended with is: a refused
/// token, answer or webhook, each its own exit and report, else as every
/// module's.
fn report(err: anyhow::Error) -> Report {
    let (code, headline, why, next, because) = if token_refused(&err) {
        (
            TOKEN_REFUSED,
            sentence!("The connection refused the bot's token."),
            prose!("It won't answer until the token changes."),
            cmd!("lnk agent connect <connection>"),
            phrase!("connect it again with a token it accepts"),
        )
    } else if reply_refused(&err) {
        (
            REPLY_REFUSED,
            sentence!("The connection refused the answer."),
            prose!("The message reached the agent, and its answer the chat never got."),
            cmd!("lnk agent connect <connection>"),
            phrase!("check the connection"),
        )
    } else if webhook_refused(&err) {
        (
            WEBHOOK_REFUSED,
            sentence!("The connection refused the webhook."),
            prose!("Its pushes don't come."),
            cmd!("lnk agent connect <connection>"),
            phrase!("check the connection"),
        )
    } else {
        return Report::from(err);
    };
    let mut report = Report::refusal(Refuser::Policy("the connection".into()), headline)
        .exit(Exit::Module(code))
        .next(next, because)
        .cause(err.context("the connection said no"));
    report.why = Some(why);
    report
}

/// A connection's `connect` that failed on the connection's side: `headline`
/// says what didn't happen, the error's words why, a refusal of the token
/// keeps its own exit, and an [`Expired`](super::Expired) code says so
/// with the command that makes a new one.
pub fn failed(connection: &str, headline: Sentence, err: anyhow::Error) -> Report {
    if token_refused(&err) || reply_refused(&err) || webhook_refused(&err) {
        return report(err);
    }
    if let Some(expired) = err.downcast_ref::<super::Expired>() {
        let connection = Name::new("connection", &expired.connection);
        return Report::error(sentence!("The pairing code expired."))
            .why(Sentence::adopt(&format!("{err:#}")))
            .cause(err)
            .next(
                cmd!("lnk agent connect {}", connection),
                phrase!("make a new code"),
            );
    }
    Report::error(headline)
        .why(Sentence::adopt(&format!("{err:#}")))
        .cause(err)
        .next(
            cmd!("lnk agent connect {}", Name::new("connection", connection)),
            phrase!("try again"),
        )
}

/// A command line `lnk-<name>` can't use: its usage is what `lnk agent
/// connect` runs it for.
fn usage(name: &str, headline: Sentence, why: Prose) -> Report {
    Report::error(headline).why(why).exit(Exit::Usage).next(
        cmd!("lnk agent connect {}", Name::new("connection", name)),
        phrase!("connect the connection"),
    )
}

/// An answer for a program as `T`, its declared output: `--json`'s
/// reply, and otherwise the same JSON as it is.
pub fn document<T>(out: &mut Out, answer: &impl serde::Serialize) -> Result<(), Report>
where
    T: serde::de::DeserializeOwned + serde::Serialize + link_actions::value::Validate,
{
    let typed: T = link_actions::answer::output(answer)?;
    out.reply(&typed);
    if !out.is_json() {
        let text = serde_json::to_string(&typed).within("writing its answer")?;
        out.data(format!("{text}\n").as_bytes());
    }
    Ok(())
}

/// `future` on this process's runtime, its error a report.
fn block_on<T>(future: impl std::future::Future<Output = anyhow::Result<T>>) -> Result<T, Report> {
    link_runtime::block_on(future).map_err(report)
}

/// `link` found the connection unlinked, with no terminal to link it on.
#[derive(Debug)]
struct NotLinked;

impl std::fmt::Display for NotLinked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("not linked, and no terminal to link it on")
    }
}

impl std::error::Error for NotLinked {}

fn run_main<F, B>(
    out: &mut Out,
    about: About,
    operation: Operation,
    run: F,
    carry: Option<impl FnOnce(Bridge) -> B>,
    link: Option<impl FnOnce(Bridge, bool, &mut Out) -> anyhow::Result<bool>>,
    state: Option<crate::harness::State>,
) -> Result<Done, Report>
where
    F: AsyncFnOnce(Request, &mut Out) -> Result<Connected, Report>,
    B: std::future::Future<Output = anyhow::Result<()>>,
{
    let About {
        name,
        mut describe,
        rule,
        send,
        pushed,
        share,
        owner,
    } = about;
    describe.bridged = carry.is_some();
    describe.share = share.is_some();
    describe.linked = link.is_some();
    describe.pushed = pushed.is_some_and(|p| p.hook.is_some());
    describe.held = pushed.is_some_and(|p| p.hold.is_some());
    let program = Product::new(format!("lnk-{name}"));
    match operation {
        Operation::Connect { agent, taken } => {
            let request = connect_request(agent, &taken).map_err(|err| {
                usage(
                    name,
                    Sentence::adopt(&err.to_string()),
                    prose!("It is run with the agent's name."),
                )
            })?;
            let connected = link_runtime::block_on(async {
                Ok::<_, std::convert::Infallible>(run(request, out).await)
            })
            .map_err(report)??;
            // Its settings hold the bot's token: only to another program.
            let connected: crate::declared::Connected = link_actions::answer::output(&connected)?;
            out.reply_private(&connected)?;
            Ok(Done::shown())
        }
        Operation::Rule => {
            document::<crate::declared::Ruled>(out, &Ruled { rule: rule.clone() })?;
            Ok(Done::shown())
        }
        Operation::Describe => {
            document::<crate::declared::Described>(out, &describe)?;
            Ok(Done::shown())
        }
        Operation::Bridge => {
            let Some(carry) = carry else {
                return Err(usage(
                    name,
                    sentence!("{} doesn't carry messages.", program),
                    prose!(
                        "Its harness speaks {} itself.",
                        Name::new("connection", name)
                    ),
                ));
            };
            let bridge = read_bridge(&mut std::io::stdin().lock()).map_err(report)?;
            block_on(async {
                // Until stdin closes: whoever runs it stopped, or died.
                let closed = tokio::task::spawn_blocking(|| {
                    let _ = std::io::copy(&mut std::io::stdin().lock(), &mut std::io::sink());
                });
                tokio::select! {
                    result = carry(bridge) => result,
                    _ = closed => Ok(()),
                }
            })?;
            Ok(Done::shown())
        }
        Operation::Send => {
            let Some(send) = send else {
                return Err(usage(
                    name,
                    sentence!("{} can't send a message nobody asked for.", program),
                    prose!("Only an answer goes back to a chat."),
                ));
            };
            let mut line = String::new();
            std::io::BufRead::read_line(
                &mut std::io::Read::take(std::io::stdin().lock(), MAX_WHOLE as u64 + 4096),
                &mut line,
            )
            .within("reading what to send")?;
            let sending: Sending = serde_json::from_str(&line).within("reading what to send")?;
            let receipt = block_on(send(sending.bridge, sending.chat, sending.text))?;
            out.data(&link_actions::answer::json_line::<crate::declared::Sent>(
                &receipt,
            )?);
            Ok(Done::shown())
        }
        Operation::Owner => {
            let Some(owner) = owner else {
                return Err(usage(
                    name,
                    sentence!("{} has no chat of the owner's to ask on.", program),
                    prose!("Only its own chats carry a question for the owner."),
                ));
            };
            let mut line = String::new();
            std::io::BufRead::read_line(
                &mut std::io::Read::take(std::io::stdin().lock(), 1 << 20),
                &mut line,
            )
            .within("reading the bridge")?;
            let bridge: Bridge = serde_json::from_str(&line).within("reading the bridge")?;
            document::<crate::declared::Owners>(out, &owner(&bridge).map_err(report)?)?;
            Ok(Done::shown())
        }
        Operation::Share => {
            let Some(share) = share else {
                return Err(usage(
                    name,
                    sentence!("{} can't share a chat.", program),
                    prose!("Its chats are not ones an agent joins."),
                ));
            };
            let shared = block_on(take_share(name, share, &mut std::io::stdin().lock()))?;
            document::<crate::declared::Shared>(out, &shared)?;
            Ok(Done::shown())
        }
        operation @ (Operation::Hook
        | Operation::Unhook
        | Operation::Sift
        | Operation::Receive
        | Operation::Hold) => {
            let command = match operation {
                Operation::Hook => "hook",
                Operation::Unhook => "unhook",
                Operation::Sift => "sift",
                Operation::Receive => "receive",
                _ => "hold",
            };
            let Some(pushed) = pushed else {
                return Err(usage(
                    name,
                    sentence!("The messages of {} aren't pushed.", program),
                    prose!("Only its bridge carries them."),
                ));
            };
            run_pushed(out, name, command, pushed)
        }
        Operation::Link { again } => {
            let Some(link) = link else {
                return Err(usage(
                    name,
                    sentence!("{} has nothing to link.", program),
                    prose!("Its connection has a token."),
                ));
            };
            let bridge = read_bridge(&mut std::io::stdin().lock()).map_err(report)?;
            match link(bridge, again, out).map_err(report)? {
                true => Ok(Done::shown()),
                // The harness's own code: it knows what to do with it.
                false => Err(Report::error(sentence!(
                    "The connection isn't linked, and there's no terminal to link it on."
                ))
                .cause(NotLinked.into())
                .exit(Exit::Module(crate::harness::NOT_LINKED as u8))
                .next(
                    cmd!("lnk agent connect {}", Name::new("connection", name)),
                    phrase!("connect it on a terminal"),
                )),
            }
        }
        Operation::State => {
            let Some(state) = state else {
                return Err(usage(
                    name,
                    sentence!("{} keeps no link to move.", program),
                    prose!("Its connection has a token."),
                ));
            };
            document::<crate::declared::State>(out, &state)?;
            Ok(Done::shown())
        }
        // It keeps nothing of its own here (the harness module keeps the
        // connection), but what it made stays the user's.
        Operation::Uninstall(given) => {
            crate::uninstall::run(
                &given,
                &crate::uninstall::Kept {
                    // One sentence for every connection, so it's said once.
                    stays: "What lnk agent connect made on a connection (a bot, an app, a linked \
                            device) stays yours until you delete or unlink it there."
                        .into(),
                    ..Default::default()
                },
                || Ok(()),
                |_| Ok(()),
            )
            .within("uninstalling")?;
            Ok(Done::shown())
        }
    }
}

/// `hook`, `unhook`, `sift`, `take` or `hold`, as `pushed` makes them.
fn run_pushed(
    out: &mut Out,
    name: &'static str,
    command: &str,
    pushed: pushed::Pushed,
) -> Result<Done, Report> {
    fn line<T: serde::de::DeserializeOwned>(what: &'static str) -> Result<T, Report> {
        let mut line = String::new();
        std::io::BufRead::read_line(
            &mut std::io::Read::take(
                std::io::stdin().lock(),
                (2 * pushed::UPDATE_MAX + 64 * 1024) as u64,
            ),
            &mut line,
        )
        .within(what)?;
        // Said by where, never by what's in it.
        serde_json::from_str(&line)
            .within(what)
            .map_err(Report::from)
    }
    let missing = || {
        usage(
            name,
            sentence!(
                "{} has no {} command.",
                Product::new(format!("lnk-{name}")),
                Foreign::new(command)
            ),
            prose!("Its messages are not pushed that way."),
        )
    };
    match command {
        "sift" => pushed::sift_lines(
            pushed.sift,
            std::io::stdin().lock(),
            link_format::streams::protocol(),
        )
        .map_err(report)?,
        "hook" => {
            let hook = pushed.hook.ok_or_else(missing)?;
            let hooking: pushed::Hooking = line("reading what to hook on stdin")?;
            let hooked = block_on(hook(hooking))?;
            document::<crate::declared::Hooked>(out, &hooked)?;
        }
        "unhook" => {
            let unhook = pushed.unhook.ok_or_else(missing)?;
            let unhooking: pushed::Unhooking = line("reading what to unhook on stdin")?;
            block_on(unhook(unhooking))?;
        }
        "receive" => {
            let taking: pushed::Taking = line("reading what to take on stdin")?;
            block_on((pushed.take)(taking))?;
        }
        _ => {
            let hold = pushed.hold.ok_or_else(missing)?;
            block_on(async {
                let (lines, read) = tokio::sync::mpsc::channel(64);
                let (write, mut held) = tokio::sync::mpsc::channel::<pushed::Held>(256);
                // Lines in, until stdin closes: then every hold is let go.
                std::thread::spawn(move || {
                    use std::io::BufRead as _;
                    for line in std::io::stdin().lock().lines() {
                        let Ok(line) = line else { break };
                        match serde_json::from_str::<pushed::HoldLine>(&line) {
                            Ok(line) => {
                                if lines.blocking_send(line).is_err() {
                                    break;
                                }
                            }
                            Err(err) => {
                                warn(&format!("lnk-{name} hold: a line it can't read: {err}"))
                            }
                        }
                    }
                });
                let out = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                    use std::io::Write as _;
                    let mut stdout = link_format::streams::protocol();
                    while let Some(held) = held.blocking_recv() {
                        // Each line as its declaration writes it.
                        let line =
                            link_actions::answer::json_line::<crate::declared::HeldLine>(&held)?;
                        stdout.write_all(&line)?;
                        stdout.flush()?;
                    }
                    Ok(())
                });
                let held = hold(read, write).await;
                out.await??;
                held
            })?;
        }
    }
    Ok(Done::shown())
}

/// The [`Bridge`] on the first line of `input`.
fn read_bridge(input: &mut impl std::io::BufRead) -> anyhow::Result<Bridge> {
    let mut line = String::new();
    input.read_line(&mut line)?;
    serde_json::from_str(&line).map_err(|err| anyhow::anyhow!("reading the bridge on stdin: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A refused token is found however it's wrapped, and ends in
    /// [`TOKEN_REFUSED`]; any other failure doesn't.
    #[test]
    fn a_refused_token_is_its_own_exit() {
        let refused = anyhow::Error::from(TokenRefused)
            .context("Telegram refused getMe")
            .context("answering");
        assert!(token_refused(&refused));
        assert_eq!(report(refused).exit, Exit::Module(TOKEN_REFUSED));
        let other = anyhow::anyhow!("Telegram didn't answer sendMessage");
        assert!(!token_refused(&other));
        assert_eq!(report(other).exit, Exit::Failure);
    }

    /// A pairing code that expired says so, however it's wrapped, and
    /// names the command that makes a new one, for its connection.
    #[test]
    fn an_expired_code_names_the_command_that_makes_another() {
        let expired = anyhow::Error::from(Expired {
            connection: "telegram".into(),
            waited: std::time::Duration::from_secs(600),
        })
        .context("check the bot");
        let report = failed("telegram", sentence!("The bot wasn't paired."), expired);
        assert_eq!(report.headline.to_string(), "The pairing code expired.");
        let why = report
            .why
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default();
        assert!(why.contains("10 minutes"), "{why}");
        assert_eq!(
            report.next.0[0].command.text(),
            "lnk agent connect telegram"
        );
        let other = failed(
            "telegram",
            sentence!("The bot wasn't paired."),
            anyhow::anyhow!("Telegram didn't answer getUpdates"),
        );
        assert_eq!(other.headline.to_string(), "The bot wasn't paired.");
    }

    /// A refused answer is found however it's wrapped, and ends in
    /// [`REPLY_REFUSED`], apart from a refused token and any other
    /// failure.
    #[test]
    fn a_refused_answer_is_its_own_exit() {
        let refused = anyhow::Error::from(ReplyRefused)
            .context("Telegram refused sendMessage: Forbidden: bot was blocked by the user")
            .context("answering");
        assert!(reply_refused(&refused) && !token_refused(&refused));
        assert_eq!(report(refused).exit, Exit::Module(REPLY_REFUSED));
        let token = anyhow::Error::from(TokenRefused).context("answering");
        assert!(!reply_refused(&token));
        let other = anyhow::anyhow!("Telegram didn't answer sendMessage");
        assert!(!reply_refused(&other));
        assert_eq!(report(other).exit, Exit::Failure);
    }

    /// A refused webhook is found however it's wrapped, and ends in
    /// [`WEBHOOK_REFUSED`], apart from a refused token and any other
    /// failure.
    #[test]
    fn a_refused_webhook_is_its_own_exit() {
        let refused = anyhow::Error::from(WebhookRefused)
            .context("Telegram refused setWebhook: Bad Request: bad webhook")
            .context("hooking");
        assert!(webhook_refused(&refused) && !token_refused(&refused));
        assert!(!reply_refused(&refused));
        assert_eq!(report(refused).exit, Exit::Module(WEBHOOK_REFUSED));
        let token = anyhow::Error::from(TokenRefused).context("hooking");
        assert!(!webhook_refused(&token));
        assert_eq!(report(token).exit, Exit::Module(TOKEN_REFUSED));
    }

    /// The codes are the format's list, and an unlinked connection exits as
    /// the harness says.
    #[test]
    fn the_codes_are_the_lists() {
        for (code, meaning) in [
            (
                crate::harness::NOT_LINKED as u8,
                "a connection's harness isn't linked",
            ),
            (TOKEN_REFUSED, "a connection's service refused its token"),
            (REPLY_REFUSED, "a connection's service refused a reply"),
            (WEBHOOK_REFUSED, "a webhook was refused"),
        ] {
            assert_eq!(Exit::Module(code).meaning(), meaning);
        }
    }

    /// What a program is answered is the JSON alone on stdout, with or
    /// without `--json`; a person's blocks never join it.
    #[test]
    fn an_answer_is_the_json_alone() {
        let answer = serde_json::json!({"title": "Mini"});
        for json in [false, true] {
            let (mut out, captured) = Out::capture(json, &["lnk-connection-fixture", "describe"]);
            document::<serde_json::Value>(&mut out, &answer).unwrap();
            assert_eq!(out.finish(Ok(Done::shown())), 0);
            assert_eq!(captured.stdout(), "{\"title\":\"Mini\"}\n", "{json}");
        }
    }

    /// A connection that failed says why, and a refusal of the token keeps
    /// its exit.
    #[test]
    fn a_failed_connect_keeps_a_refused_tokens_exit() {
        let said = failed(
            "mini",
            sentence!("Mini didn't accept the token."),
            anyhow::anyhow!("no route"),
        );
        assert_eq!(said.exit, Exit::Failure);
        assert!(!said.next.is_empty());
        let refused = failed(
            "mini",
            sentence!("Mini didn't accept the token."),
            anyhow::Error::from(TokenRefused),
        );
        assert_eq!(refused.exit, Exit::Module(TOKEN_REFUSED));
    }

    #[test]
    fn connect_takes_the_agent_and_the_bots_taken() {
        let request =
            connect_request("main".into(), r#"[{"id": "1", "agent": "moltbot-2"}]"#).unwrap();
        assert_eq!(request.agent, "main");
        assert_eq!(request.taken[0].agent, "moltbot-2");
        assert!(
            connect_request("main".into(), "[]")
                .unwrap()
                .taken
                .is_empty()
        );
        assert!(connect_request("main".into(), "not json").is_err());
    }

    #[test]
    fn a_bridge_is_one_line_and_never_prints_its_token() {
        let bridge = Bridge {
            agent: "main".into(),
            identity: crate::bridge::Identity {
                agent: "abcdefghijk1".into(),
                service: String::new(),
                owner: "dana@laptop".into(),
                teams: Vec::new(),
            },
            endpoint: "http://127.0.0.1:18800/acp".into(),
            key: "k".to_string().into(),
            proof: true,
            parts: false,
            settings: serde_json::json!({"bot_token": "123:secret", "allowed_users": [42]}).into(),
            state: "/home/dana/.lnk/safe/agents/main/connections/telegram".into(),
            spec: Some(crate::policy::Connection {
                pace: Some(link_actions::value::Duration::seconds(2)),
                ..Default::default()
            }),
            shared: Vec::new(),
        };
        let line = format!("{}\n", serde_json::to_string(&bridge).unwrap());
        assert_eq!(read_bridge(&mut line.as_bytes()).unwrap(), bridge);
        assert!(!format!("{bridge:?}").contains("secret"));
        assert!(read_bridge(&mut "".as_bytes()).is_err());
    }
}
