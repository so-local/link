//! Tools that run outside: MCP servers you name (`lnk sandbox tool add`),
//! for what the sandbox keeps out (your logins, the desktop, `gh` or
//! `aws` with your credentials), served by the sandbox's bridge
//! ([`crate::bridges`]) behind the tools route.
//!
//! A tool's definition lives in Link's settings (`sandbox/tools/<name>.json`),
//! which no sandbox reads: the command, its folder, and the environment
//! it was added with. A sandbox whose spec names it (`[tools]`) reaches it
//! on its proxy at `http://127.0.0.1:40781/_link/url/<secret>/mcp/<name>`
//! (`$LNK_TOOL_<NAME>`), MCP's HTTP transport, which hands each call
//! here. The bridge, outside, runs the server over stdio, one for each
//! session a client starts (`initialize`), and passes on what the client
//! sends. A tool call (`tools/call`) goes through only if the tool's
//! rules allow it: every call asks you ([`crate::bridges::asks`]), but for
//! those allowed by name (`--add`), unless the tool asks for none
//! (`--ask none`). What a server says of its own tools (MCP's
//! `destructiveHint`) is shown when it asks, and never skips asking: the
//! server sets it itself.

mod gating;
mod metadata;
#[cfg(test)]
mod metadata_tests;
pub mod namespace;
mod route;
mod session;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::ensure;
use link_format::WithinExt;
use link_secretive::Duplicate;
use serde::{Deserialize, Serialize};

/// Who makes a namespace's calls (`lnk-sandbox mcp-namespace --by`).
pub use crate::cli::SandboxMcpNamespaceArgsBy as By;
pub use route::{Tools, question, serve, unanswered};
pub use session::{Session, calls};

/// The largest message passed on, each way.
const MAX_MESSAGE: usize = link_sandbox_client::routes::TOOL_MESSAGE_MAX;

/// How long a server may take to start and answer `initialize` (`npx`
/// may download it first).
const START_TIMEOUT: Duration = Duration::from_secs(120);

/// The environment a tool is added with, and so runs with: what finds
/// its programs and your logins, never the rest of your shell's.
const KEPT: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "TMPDIR",
    "SSH_AUTH_SOCK",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
];

/// Whether a tool's calls ask you.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ask {
    #[default]
    All,
    None,
}

/// A tool: an MCP server that runs outside, over stdio. Its file,
/// `tools/<name>.json`, names its schema; one with any other field (a
/// `cwd`, as tools were saved before they ran in `/`) is refused, never
/// read without it.
#[derive(Duplicate, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    pub name: String,
    /// The server's command, run in `/` ([`server`]): none of it written
    /// as a path from where it was added ([`from_here`]).
    pub argv: Vec<String>,
    /// Its environment: [`KEPT`] as it was when added, and `--env` (a
    /// name alone: its value then; a secret's always so). Each value held
    /// as a secret, as a token given with `--env` is: written as it is
    /// only in the tool's file (mode 600), and given to its server alone.
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub env: BTreeMap<String, link_secretive::Secret>,
    #[serde(default)]
    pub ask: Ask,
    /// The calls that don't ask (a tool's own names: `list_issues`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<String>,
    /// The calls that ask by one boolean argument, `<call>.<argument>`
    /// (`schedule.recurring`): asked unless that argument is given false,
    /// whatever `allow` says ([`Tool::asks_given`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ask_if: Vec<String>,
    /// What lets a call `ask_if` asks by through, in the words of whoever
    /// added it (`--ask-if-hint`): its refusal names it, as `--add`
    /// doesn't ([`Tool::lets_through`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask_if_hint: Option<String>,
    /// One server for every session of every sandbox naming it, not one
    /// each ([`crate::shared`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shared: bool,
}

link_files::specification!(Tool, include_str!("tools/tool.schema.toml"));

impl Tool {
    /// Tool `name`, running `argv` here, with this environment and `extra`
    /// (`NAME=value`, or `NAME` for its value here).
    pub fn here(
        name: &str,
        argv: Vec<String>,
        extra: &[String],
        ask: Ask,
        allow: Vec<String>,
    ) -> anyhow::Result<Self> {
        link_sandbox_client::routes::check_tool_name(name)?;
        ensure!(
            !argv.is_empty(),
            "no command: lnk sandbox tool add {name} -- <command>"
        );
        for call in &allow {
            check_call(call)?;
        }
        let mut env: BTreeMap<String, link_secretive::Secret> =
            link_secretive::process::variables(|k| KEPT.contains(&k) || k.starts_with("LC_"))
                .map(|(k, v)| (k, v.into()))
                .collect();
        for pair in extra {
            let (k, v) = match pair.split_once('=') {
                // A secret's value never on the command line, where other
                // users of this machine see it: its name alone, its value
                // read from here.
                Some((k, _)) if link_secretive::process::names_a_secret(k) => anyhow::bail!(
                    "--env {k}=… puts a secret on the command line, where other users of \
                     this machine can see it: export {k}, then give --env {k}"
                ),
                Some((k, v)) => (k.to_string(), v.to_string().into()),
                // A name alone: its value here, or handed to this program
                // (a key `lnk` was given for the tool).
                None => (
                    pair.clone(),
                    link_secretive::Secret::from_env(pair)
                        .within(format!("${pair} isn't set here"))?,
                ),
            };
            ensure!(
                !k.is_empty() && !k.contains(char::is_whitespace),
                "{k:?} isn't a variable's name: --env takes NAME or NAME=value"
            );
            env.insert(k, v);
        }
        if let Some(arg) = argv.iter().find(|arg| from_here(arg)) {
            anyhow::bail!(
                "{arg:?} is a path from this folder, but tool {name}'s server runs in /, outside \
                 every sandbox: give its full path, in a folder no sandbox writes"
            );
        }
        Ok(Self {
            name: name.into(),
            argv,
            env,
            ask,
            allow,
            ask_if: Vec::new(),
            ask_if_hint: None,
            shared: false,
        })
    }

    /// It, its calls also asking by the boolean arguments `ask_if` names
    /// (`<call>.<argument>`), each checked.
    pub fn asking_if(mut self, ask_if: Vec<String>) -> anyhow::Result<Self> {
        for each in &ask_if {
            let (call, argument) = each
                .split_once('.')
                .within(format!("--ask-if is <call>.<argument>, not {each:?}"))?;
            check_call(call)?;
            ensure!(
                !argument.is_empty()
                    && argument
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
                "--ask-if {each:?}: its argument is a name"
            );
        }
        self.ask_if = ask_if;
        Ok(self)
    }

    /// It, saying `hint` of what lets a call `ask_if` asks by through
    /// (`--ask-if-hint`): only beside an `--ask-if`, on one line.
    pub fn hinting(mut self, hint: Option<String>) -> anyhow::Result<Self> {
        if let Some(hint) = &hint {
            ensure!(!self.ask_if.is_empty(), "--ask-if-hint comes with --ask-if");
            ensure!(
                !hint.trim().is_empty() && !hint.chars().any(char::is_control),
                "--ask-if-hint is a line of text"
            );
        }
        self.ask_if_hint = hint;
        Ok(self)
    }

    /// What lets call `call` through, for its refusal when no one answers:
    /// `--add` for a call that asks whatever its arguments; for one that
    /// asks by an argument (`ask_if`), which `--add` doesn't let
    /// through, the hint it was added with, else that argument given false.
    pub fn lets_through(&self, call: &str) -> String {
        if self.asks(call) {
            return format!(
                "lnk sandbox tool add {} --add {call} lets it through",
                self.name
            );
        }
        if let Some(hint) = &self.ask_if_hint {
            return hint.clone();
        }
        let argument = self
            .ask_if
            .iter()
            .filter_map(|each| each.split_once('.'))
            .find(|(asking, _)| *asking == call)
            .map_or("its argument", |(_, argument)| argument);
        format!("{call} asks by {argument} even when --add names it: given false, it doesn't ask")
    }

    /// Whether call `call` asks you first, whatever its arguments.
    pub fn asks(&self, call: &str) -> bool {
        self.ask == Ask::All && !self.allow.iter().any(|a| a == call)
    }

    /// Whether call `call`, with `arguments`, asks you first: as
    /// [`Tool::asks`] says, or by an argument `ask_if` names that isn't
    /// given false, even for a call `allow` lets through. `--ask none`
    /// asks nothing.
    pub fn asks_given(&self, call: &str, arguments: &serde_json::Value) -> bool {
        self.asks_given_by(call, arguments, By::Model)
    }

    /// [`Tool::asks_given`], for a call `by` makes: one the owner makes
    /// from their own command line skips each argument's ask, never a
    /// whole call's.
    pub fn asks_given_by(&self, call: &str, arguments: &serde_json::Value, by: By) -> bool {
        self.asks(call)
            || (by == By::Model
                && self.ask == Ask::All
                && self.ask_if.iter().any(|each| {
                    each.split_once('.').is_some_and(|(asking, argument)| {
                        asking == call
                            && arguments.get(argument) != Some(&serde_json::Value::Bool(false))
                    })
                }))
    }
}

/// Whether `arg` is written as a path from the folder a tool is added
/// in (`.`, `./server.js`, `../bin/serve`), which its server, run in `/`,
/// would read as one from there. Refused rather than made absolute: that
/// folder is one a sandbox may write, and a server run from it, or the
/// `node_modules` its interpreter loads beside it, would run outside
/// every sandbox. Anything else is kept as written: a program's name, a
/// flag, or a word the server reads its own way.
fn from_here(arg: &str) -> bool {
    arg == "." || arg == ".." || arg.starts_with("./") || arg.starts_with("../")
}

/// A call's name, or why it isn't one: what a server names its tools
/// (letters, digits, `_`, `-`, `.`, `/`), at most 128.
fn check_call(call: &str) -> anyhow::Result<()> {
    let plain = (1..=128).contains(&call.len())
        && call
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-./".contains(&b));
    ensure!(plain, "{call:?} isn't a tool call's name");
    Ok(())
}

fn tools_dir(dir: &Path) -> PathBuf {
    dir.join("tools")
}

/// `tool`'s server, as it's started (by a session or its hub): its
/// command, in `/`, a folder no sandbox writes, so nothing a sandbox
/// left where it was added is loaded as its own, with its environment
/// alone.
pub fn server(tool: &Tool) -> std::process::Command {
    let mut command = link_secretive::process::command(&tool.argv[0]);
    command.args(&tool.argv[1..]).current_dir("/").env_clear();
    // Its environment as you gave it (`--env`), a token among it: the
    // server reads it from there alone.
    for (name, value) in &tool.env {
        link_secretive::process::give(&mut command, name, value);
    }
    command
}

fn tool_file(dir: &Path, name: &str) -> PathBuf {
    tools_dir(dir).join(format!("{name}.json"))
}

/// Saves `tool` in `dir` (Link's `sandbox/`), in place of any of its
/// name; whether one was there.
pub fn save(dir: &Path, tool: &Tool) -> anyhow::Result<bool> {
    let _policy = policy_fence(dir)?;
    link_sandbox_client::routes::check_tool_name(&tool.name)?;
    link_files::create_private_dir(&tools_dir(dir))?;
    let path = tool_file(dir, &tool.name);
    let existed = path.exists();
    link_files::write_private(&path, &link_files::saved::print_json(tool)?)?;
    Ok(existed)
}

/// Tool `name` in `dir`.
pub fn load(dir: &Path, name: &str) -> anyhow::Result<Tool> {
    link_sandbox_client::routes::check_tool_name(name)?;
    // Never through a link: what it defines runs outside every sandbox.
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NOFOLLOW);
    let text = options
        .open(tool_file(dir, name))
        .and_then(std::io::read_to_string)
        .within(format!(
            "no tool {name}: lnk sandbox tool add {name} -- <command>"
        ))?;
    let tool: Tool = link_files::saved::parse_json(text.as_bytes()).within(format!(
        "reading tool {name} (lnk sandbox tool add {name} again saves it anew)"
    ))?;
    ensure!(tool.name == name, "tool {name}'s file names {}", tool.name);
    Ok(tool)
}

/// Every tool in `dir`, by name.
pub fn list(dir: &Path) -> Vec<Tool> {
    let Ok(entries) = std::fs::read_dir(tools_dir(dir)) else {
        return Vec::new();
    };
    let mut tools: Vec<Tool> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            load(dir, name.strip_suffix(".json")?).ok()
        })
        .collect();
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    tools
}

/// Removes tool `name` from `dir`; whether it was there.
pub fn remove(dir: &Path, name: &str) -> anyhow::Result<bool> {
    let _policy = policy_fence(dir)?;
    link_sandbox_client::routes::check_tool_name(name)?;
    match std::fs::remove_file(tool_file(dir, name)) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}

const POLICY_FENCE: &str = "tool-policy.lock";
pub(super) fn policy_fence(dir: &Path) -> anyhow::Result<std::fs::File> {
    link_files::lock_file(&dir.join(POLICY_FENCE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_asks_unless_allowed_by_name_or_none_ask() {
        let mut tool = Tool {
            name: "gh".into(),
            argv: vec!["gh-mcp".into()],
            env: Default::default(),
            ask: Ask::All,
            allow: vec!["list_issues".into()],
            ask_if: Vec::new(),
            ask_if_hint: None,
            shared: false,
        };
        assert!(tool.asks("create_issue"));
        assert!(!tool.asks("list_issues"));
        assert!(tool.asks("list_issues2"));
        tool.ask = Ask::None;
        assert!(!tool.asks("create_issue"));
    }

    /// A call `--ask-if` names asks by its argument: given false, it's let
    /// through as `allow` says; true or left out, it asks, even allowed,
    /// unless the owner makes it; another call isn't touched, and `--ask
    /// none` asks nothing.
    #[test]
    fn a_call_asks_by_its_argument() {
        use serde_json::json;
        let mut tool = Tool::here(
            "scheduler",
            vec!["serve".into()],
            &[],
            Ask::All,
            vec!["schedule".into(), "task".into()],
        )
        .unwrap()
        .asking_if(vec!["schedule.recurring".into()])
        .unwrap();
        assert!(tool.asks_given("schedule", &json!({"cron": "0 9 * * *"})));
        assert!(tool.asks_given("schedule", &json!({"recurring": true})));
        assert!(!tool.asks_given("schedule", &json!({"recurring": false})));
        assert!(!tool.asks_given("task", &json!({"recurring": true})));
        // The owner's own call skips the argument's ask, never a whole
        // call's.
        assert!(!tool.asks_given_by("schedule", &json!({"recurring": true}), By::Owner));
        assert!(tool.asks_given_by("cancel", &json!({}), By::Owner));
        assert!(tool.asks_given_by("schedule", &json!({"recurring": true}), By::Model));
        tool.ask = Ask::None;
        assert!(!tool.asks_given("schedule", &json!({})));
        for bad in [
            "schedule",
            ".recurring",
            "schedule.",
            "sched ule.recurring",
            "schedule.re curring",
        ] {
            let refused = Tool::here("s", vec!["serve".into()], &[], Ask::All, vec![])
                .unwrap()
                .asking_if(vec![bad.into()]);
            assert!(refused.is_err(), "{bad}");
        }
    }

    /// A call refused with no one to answer names what lets it through:
    /// `--add` for one asking whatever its arguments, and for one asking
    /// by an argument, which `--add` doesn't let through, the hint it
    /// was added with, else that argument given false.
    #[test]
    fn a_refusal_names_what_lets_the_call_through() {
        let tool = |ask_if: Vec<String>| {
            Tool::here(
                "scheduler",
                vec!["serve".into()],
                &[],
                Ask::All,
                vec!["schedule".into()],
            )
            .unwrap()
            .asking_if(ask_if)
            .unwrap()
        };
        let plain = tool(vec!["schedule.recurring".into()]);
        assert_eq!(
            plain.lets_through("cancel"),
            "lnk sandbox tool add scheduler --add cancel lets it through"
        );
        let unhinted = plain.lets_through("schedule");
        assert!(unhinted.contains("recurring"), "{unhinted}");
        assert!(!unhinted.contains("lnk sandbox tool add"), "{unhinted}");
        let hint = "[tool.scheduler] recurring = \"allow\" lets it through";
        let hinted = tool(vec!["schedule.recurring".into()])
            .hinting(Some(hint.into()))
            .unwrap();
        assert_eq!(hinted.lets_through("schedule"), hint);
        assert!(hinted.lets_through("cancel").contains("--add cancel"));
        // Only beside an --ask-if, on one line.
        assert!(tool(vec![]).hinting(Some(hint.into())).is_err());
        for bad in ["", " ", "two\nlines"] {
            let refused = tool(vec!["schedule.recurring".into()]).hinting(Some(bad.into()));
            assert!(refused.is_err(), "{bad:?}");
        }
    }

    /// A server runs in `/`, never the folder it was added in, which a
    /// sandbox may write; what its command names there, by a path from
    /// it, is still what it runs.
    #[test]
    fn a_server_runs_outside_the_folder_it_was_added_in() {
        // Every way to name a file from where it's added, refused.
        for arg in ["./server.js", "../bin/serve", ".", ".."] {
            let argv = vec!["node".into(), arg.into(), "--write".into(), "work".into()];
            let err = Tool::here("files", argv, &[], Ask::All, vec![]).unwrap_err();
            assert!(format!("{err:#}").contains("full path"), "{arg}: {err:#}");
        }
        let argv: Vec<String> = ["node", "/opt/files/server.js", "--write", "work", ".x"]
            .map(String::from)
            .to_vec();
        let tool = Tool::here("files", argv.clone(), &[], Ask::All, vec![]).unwrap();
        assert_eq!(tool.argv, argv);
        assert_eq!(server(&tool).get_current_dir(), Some(Path::new("/")));
        // One saved before tools ran in `/`, with the folder it ran in, is
        // refused, never run there without it.
        let dir = link_test_kit::TempDir::new("tools-cwd");
        save(&dir, &tool).unwrap();
        let file = tool_file(&dir, "files");
        let mut old: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        old["cwd"] = "/home/someone/project".into();
        std::fs::write(&file, old.to_string()).unwrap();
        assert!(load(&dir, "files").is_err());
    }

    #[test]
    fn tools_are_saved_loaded_listed_and_removed() {
        let dir = link_test_kit::TempDir::new("tools");
        let tool = Tool::here(
            "gh",
            vec!["gh-mcp".into()],
            &["GH_HOST=x.example".into()],
            Ask::All,
            vec![],
        )
        .unwrap();
        assert_eq!(
            tool.env.get("GH_HOST").map(|v| v.expose().as_str()),
            Some("x.example")
        );
        // Held as a secret: never printed.
        assert!(!format!("{tool:?}").contains("x.example"));
        assert!(!tool.env.keys().any(|k| k.contains("PROXY") || k == "PWD"));
        assert!(!save(&dir, &tool).unwrap());
        assert!(save(&dir, &tool).unwrap());
        assert_eq!(load(&dir, "gh").unwrap(), tool);
        assert_eq!(list(&dir).len(), 1);
        assert!(remove(&dir, "gh").unwrap());
        assert!(!remove(&dir, "gh").unwrap());
        assert!(load(&dir, "gh").is_err());
        for bad in ["../x", "Gh", "", "-x", "a/b", "a.json"] {
            assert!(
                load(&dir, bad).is_err() && remove(&dir, bad).is_err(),
                "{bad:?}"
            );
        }
        assert!(Tool::here("gh", vec![], &[], Ask::All, vec![]).is_err());
        assert!(
            Tool::here(
                "gh",
                vec!["x".into()],
                &["LNK_UNSET_FOR_A_TEST".into()],
                Ask::All,
                vec![]
            )
            .is_err()
        );
        // A secret's value is refused on the command line, without it;
        // a setting's isn't.
        let refused = Tool::here(
            "gh",
            vec!["x".into()],
            &["GITHUB_TOKEN=ghp_on_the_command_line".into()],
            Ask::All,
            vec![],
        )
        .unwrap_err();
        let said = format!("{refused:#}");
        assert!(said.contains("export GITHUB_TOKEN"), "{said}");
        assert!(!said.contains("ghp_on_the_command_line"), "{said}");
        let profile = Tool::here(
            "aws",
            vec!["x".into()],
            &["AWS_PROFILE=work".into()],
            Ask::All,
            vec![],
        )
        .unwrap();
        assert_eq!(
            profile.env.get("AWS_PROFILE").map(|v| v.expose().clone()),
            Some("work".into())
        );
        // A name alone: its value here.
        let path = Tool::here("gh", vec!["x".into()], &["PATH".into()], Ask::All, vec![]).unwrap();
        assert_eq!(
            path.env.get("PATH").map(|v| v.expose().clone()),
            std::env::var("PATH").ok()
        );
        assert!(
            Tool::here(
                "gh",
                vec!["x".into()],
                &[],
                Ask::All,
                vec!["bad name".into()]
            )
            .is_err()
        );
    }
}
