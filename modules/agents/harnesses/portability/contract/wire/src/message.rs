//! A harness message thread and its completion.
/// `link`, Link's own connection, with no account and no bot: `lnk agent
/// chat`'s, from the agent's owner alone. Its threads are `link:<name>`.
pub const LINK: &str = "link";

/// The `link` thread a chat is on unless it names one.
pub const LINK_MAIN: &str = "main";

/// The longest name of a `link` thread, after `link:`.
const LINK_NAME_MAX: usize = 64;

/// Thread `name` of the `link` connection: `link:<name>`. A name is 1 to
/// 64 letters, digits, `-`, `_` and `.`, starting with a letter or digit,
/// and at most one `~` before another such: a copy of a thread kept
/// beside it, named for the harness it came from (`main~hermes`).
pub fn link_thread(name: &str) -> anyhow::Result<String> {
    let plain = |part: &str| {
        part.starts_with(|c: char| c.is_ascii_alphanumeric())
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    };
    let (thread, copy) = name.split_once('~').unwrap_or((name, "a"));
    anyhow::ensure!(
        (1..=LINK_NAME_MAX).contains(&name.len()) && plain(thread) && plain(copy),
        "{:?} isn't a thread's name: 1 to {LINK_NAME_MAX} letters, digits, -, _ and ., starting \
         with a letter or digit",
        link_format::clean(name)
    );
    Ok(format!("{LINK}:{name}"))
}

/// Whether `thread` is one of the `link` connection's.
pub fn is_link_thread(thread: &str) -> bool {
    thread
        .split_once(':')
        .is_some_and(|(connection, _)| connection == LINK)
}

/// An error unless `thread` is a `link` thread, `link:<name>`, its name
/// as [`link_thread`] takes it.
pub fn check_link_thread(thread: &str) -> anyhow::Result<()> {
    use anyhow::Context;
    let name = thread
        .strip_prefix(LINK)
        .and_then(|rest| rest.strip_prefix(':'))
        .with_context(|| {
            format!(
                "{:?} isn't a thread of {LINK}'s",
                link_format::clean(thread)
            )
        })?;
    link_thread(name).map(drop)
}

/// What came of a message sent to the harness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    /// Its answer, whole.
    Answer(String),
    /// It joined a turn already running, whose answer, on the message
    /// that started it, answers it too ([`crate::acp::Said::steered`]).
    Steered,
}
