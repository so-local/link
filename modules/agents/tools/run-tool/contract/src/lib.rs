//! What Link run, `lnk-run-tool`, takes: its command line ([`cli`]),
//! which the agent module builds and the tool reads back, the rule its
//! `serve` holds across flags ([`background`]), and the protected
//! context a server is started with ([`context`]).

/// The calls a model makes of the tool, as its declarations render them:
/// the template its program fills ([`link_actions::tools::offer`]).
pub const TOOLS: &str = include_str!(concat!(env!("OUT_DIR"), "/tools.json"));

/// Every call the tool declares, by the name a model calls it.
pub fn calls() -> anyhow::Result<Vec<String>> {
    Ok(link_actions::tools::Template::parse(TOOLS)?.calls())
}

mod checks;
pub mod cli;
pub mod context;

/// `serve`'s background flags, given together or not at all: `--state`
/// needs `--way-in`, `--as` and every background cap, and each of those,
/// or `--keyring`, needs `--state`. The check hook `background` of the
/// tool's declarations, for both the arguments a caller builds and those
/// the tool reads; `given` is each of them but `--state` and `--keyring`.
pub fn background(state: bool, given: &[bool], keyring: bool) -> Result<(), String> {
    let all = given.iter().all(|g| *g);
    let any = given.iter().any(|g| *g) || keyring;
    match (state, all, any) {
        (true, false, _) => Err(
            "--state needs --way-in, --as, --background, --background-timeout, \
             --background-output, --background-keep, --background-per-day and --hops"
                .into(),
        ),
        (false, _, true) => {
            Err("--way-in, --as, --keyring and the background flags are for --state".into())
        }
        _ => Ok(()),
    }
}

/// The shell a command runs in, as `programs` (those a command may
/// start, a sandbox file's `[commands]` this machine has) name it: the
/// shell's name ([`Limits::shell`]) or an absolute path ending in it, or
/// its name where they allow `any` program. None: `run` offers no call.
pub fn shell(programs: &[String], any: bool) -> Option<String> {
    let name = &limits().shell;
    if any {
        return Some(name.clone());
    }
    programs
        .iter()
        .find(|entry| {
            *entry == name
                || (entry.starts_with('/')
                    && !entry.ends_with('/')
                    && std::path::Path::new(entry).file_name() == Some(std::ffi::OsStr::new(name)))
        })
        .cloned()
}

/// Link run's limits, as `limits.toml` sets them.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// The most bytes kept of each of a command's output and its errors.
    pub output_most: u64,
    /// The bytes of a command's answer beside its two streams.
    pub answer_room: u64,
    /// The most calls one session runs at once.
    pub calls_at_once: usize,
    /// The most bytes of a call's command.
    pub command_max: usize,
    /// The shell a command runs in, by its name.
    pub shell: String,
    drain_ms: u64,
    stop_wait_ms: u64,
    tick_ms: u64,
    tell_for_secs: u64,
}

impl Limits {
    /// How long a command's output is waited for once it exited: a
    /// program it left running in the background may hold it open.
    pub fn drain(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.drain_ms)
    }

    /// How long `stop` waits for a background command's supervisor to
    /// record its end.
    pub fn stop_wait(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.stop_wait_ms)
    }

    /// How often a background command's supervisor looks at its command.
    pub fn tick(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.tick_ms)
    }

    /// How long a sweep keeps trying to tell of a background command's
    /// end while the agent's way in isn't there.
    pub fn tell_for(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.tell_for_secs)
    }
}

static LIMITS: std::sync::LazyLock<Limits> = std::sync::LazyLock::new(|| {
    let limits: Limits = toml::from_str(include_str!("limits.toml")).expect("Link run's limits");
    assert!(
        limits.output_most >= 2
            && limits.calls_at_once > 0
            && limits.command_max > 0
            && !limits.shell.is_empty()
            && limits.tick_ms > 0
            && limits.tick_ms < limits.stop_wait_ms
    );
    limits
});

/// Link run's limits ([`Limits`]).
pub fn limits() -> &'static Limits {
    &LIMITS
}

/// What's kept of each of a command's output and its errors, in bytes,
/// when a spec asks `asked` and the harness cuts its answer at `answer`
/// bytes: at most half the answer less its room, so the errors' end
/// survives the cut, and at most [`Limits::output_most`], so the answer
/// stays under the tools bridge's message cap.
pub fn output_within(asked: u64, answer: u64) -> u64 {
    let limits = limits();
    let half = answer.saturating_sub(limits.answer_room) / 2;
    asked.min(half).min(limits.output_most).max(2)
}

#[cfg(test)]
mod tests {
    /// Each stream is kept to half the answer less its room, and never
    /// past the most an answer under the message cap holds.
    #[test]
    fn output_is_clamped_to_the_answer_and_the_message() {
        let limits = super::limits();
        let room = limits.answer_room;
        assert_eq!(
            super::output_within(16 << 20, 32 << 10),
            ((32 << 10) - room) / 2
        );
        assert_eq!(super::output_within(4096, 32 << 10), 4096);
        assert_eq!(super::output_within(16 << 20, 16 << 20), limits.output_most);
        assert_eq!(super::output_within(16 << 20, 0), 2);
        // Both streams, each byte shown as up to three, and the room fit
        // in one message.
        assert!(
            (6 * limits.output_most + room) as usize
                <= link_sandbox_client::routes::TOOL_MESSAGE_MAX
        );
    }

    /// `limits.toml` reads, each limit within what the rest hold to: a
    /// call's input fits a message, and a supervisor looks many times
    /// within the wait for its end and its output's drain.
    #[test]
    fn the_limits_read() {
        let limits = super::limits();
        assert!(limits.command_max < link_sandbox_client::routes::TOOL_MESSAGE_MAX);
        assert!(!limits.shell.contains('/'), "the shell is a name");
        assert!(limits.tick() < limits.drain() && limits.drain() < limits.stop_wait());
        assert!(limits.tell_for() > limits.stop_wait());
    }

    /// The shell is found by its name or a path ending in it, never a
    /// program whose name only holds it, and any program allowed is the
    /// shell by name.
    #[test]
    fn the_shell_is_named_or_a_path_to_it() {
        let shell = |programs: &[&str], any: bool| {
            let programs: Vec<String> = programs.iter().map(|p| p.to_string()).collect();
            super::shell(&programs, any)
        };
        assert_eq!(shell(&["git", "sh"], false).as_deref(), Some("sh"));
        assert_eq!(shell(&["/bin/sh"], false).as_deref(), Some("/bin/sh"));
        assert_eq!(shell(&["*"], true).as_deref(), Some("sh"));
        for none in [
            &["bash", "zsh"][..],
            &["/bin/sh/"],
            &["sh-tool"],
            &["shared/sh"],
        ] {
            assert_eq!(shell(none, false), None, "{none:?}");
        }
    }

    /// `--state` comes with all of `serve`'s background flags or none, as
    /// clap held them before: the declaration's check hook.
    #[test]
    fn background_flags_come_together() {
        let all = [true; 8];
        assert!(super::background(false, &[false; 8], false).is_ok());
        assert!(super::background(true, &all, true).is_ok());
        let mut one_short = all;
        one_short[3] = false;
        assert!(super::background(true, &one_short, false).is_err());
        assert!(super::background(false, &[false; 8], true).is_err());
        let mut stray = [false; 8];
        stray[0] = true;
        assert!(super::background(false, &stray, false).is_err());
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
