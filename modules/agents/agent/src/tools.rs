//! `lnk agent tool`: which of Link's tools an agent has, giving it one
//! and taking one away; and the choice of them at its first start, so a
//! tool is found on the command line, not only in the docs.

use std::io::IsTerminal;

use link_agent::sandbox_file::{self, Sandbox};
use link_agent::settings::{self, Settings};
use link_format::{
    Done, Exit, Name, Note, Out, Path, Prose, Question, Report, Sentence, Table, Value, WithinExt,
    cmd, prose, sentence, states,
};
use link_secretive::Secret;

use crate::{memory, run_file, screens};
mod setup;

/// What a tool command did: whether it changed what the agent has, so
/// that it restarts with it, and how it ended.
pub type Changed = (bool, Done);

states! {
    /// A tool of Link's, for an agent.
    enum ToolState {
        On("on") => Good,
        Off("off") => Quiet,
    }
}

/// One of Link's tools, as `lnk agent tool list` and the first start
/// offer it.
struct Offered {
    name: &'static str,
    about: &'static str,
    /// What it needs, said beside it.
    needs: &'static str,
    /// Whether its switch lives in the spec rather than the first-start picker.
    built_in: bool,
}

const OFFERED: [Offered; 9] = [
    Offered {
        name: "mcp",
        about: "Link documentation and declared commands; changes ask for approval",
        needs: "",
        built_in: true,
    },
    Offered {
        name: "memory",
        about: "Search over its files and conversations, and notes it keeps",
        needs: "",
        built_in: false,
    },
    Offered {
        name: "web",
        about: "Pages fetched for it, and a web search",
        needs: "search needs a Brave Search or Tavily key",
        built_in: false,
    },
    Offered {
        name: "browser",
        about: "A browser of its own: pages opened, clicked, typed into and seen",
        needs: "Chromium or Google Chrome on this computer",
        built_in: false,
    },
    Offered {
        name: "desktop",
        about: "A desktop of its own, seen and worked: the programs it runs show there",
        needs: "Linux, with Xvfb",
        built_in: false,
    },
    Offered {
        name: "scheduler",
        about: "Subagents, schedules, and messages from others",
        needs: "",
        built_in: true,
    },
    Offered {
        name: "run",
        about: "Programs it runs, each in a sandbox of its own",
        needs: "",
        built_in: true,
    },
    Offered {
        name: "file",
        about: "Reading and changing its files",
        needs: "",
        built_in: true,
    },
    Offered {
        name: "code",
        about: "Its project's language servers: problems, definitions, references",
        needs: "a language server its sandbox file allows",
        built_in: true,
    },
];

/// The variables Link Web reads a search key from, Brave Search's first.
const SEARCH_KEYS: [&str; 2] = ["BRAVE_API_KEY", "TAVILY_API_KEY"];

/// Whether `name` is one of Link's tools, which `lnk agent tool add`
/// gives, rather than one of your own.
pub(crate) fn offered(name: &str) -> bool {
    OFFERED.iter().any(|offered| offered.name == name)
}

/// Whether agent `settings` has tool `name` now.
pub(crate) fn has(settings: &Settings, name: &str) -> bool {
    match name {
        "memory" => settings.memory.is_some(),
        "scheduler" | "run" | "file" | "code" | "mcp" => settings
            .spec()
            .is_ok_and(|spec| spec.tool(name).enabled == Some(true)),
        "browser" | "desktop" => run_file::tool(settings, name).is_some(),
        other => settings
            .tools_of(settings.harness())
            .iter()
            .any(|t| t == other),
    }
}

/// `lnk agent tool list`.
pub fn list(out: &mut Out, settings: &Settings) -> Result<Done, Report> {
    let harness = settings.harness();
    let mut table = Table::new(
        ["Tool", "State", "What it does"],
        sentence!("There are no tools."),
    );
    for offered in &OFFERED {
        let state: Value = match has(settings, offered.name) {
            true => ToolState::On.into(),
            false => ToolState::Off.into(),
        };
        let about = match offered.needs {
            "" => offered.about.to_string(),
            needs => format!("{} ({needs})", offered.about),
        };
        table.row([Value::from(offered.name), state, Value::from(about)]);
    }
    // Tools of your own (`lnk sandbox tool add`) it may call.
    for tool in settings.tools_of(harness) {
        if !OFFERED.iter().any(|o| o.name == tool) {
            table.row([
                Name::new("tool", &tool).into(),
                ToolState::On.into(),
                Value::from("Yours"),
            ]);
        }
    }
    out.block(table);
    Ok(Done::shown()
        .next(cmd!("lnk agent tool add <tool>"), "give it one")
        .next(cmd!("lnk agent tool remove <tool>"), "take one away")
        .next(
            cmd!("lnk sandbox tool add <name> -- <command>"),
            "make your own MCP server, then add it here by its name",
        ))
}

/// `lnk agent tool add <tool>`: whether it changed what the agent has.
pub fn add(
    out: &mut Out,
    settings: &mut Settings,
    tool: &str,
    yes: bool,
) -> Result<Changed, Report> {
    match tool {
        "memory" => memory::start_engine(out, settings, None),
        "web" => add_web(out, settings),
        "browser" | "desktop" => add_screen(settings, tool),
        "scheduler" | "run" | "file" | "code" | "mcp" => {
            set_builtin(out, settings, tool, true, yes)
        }
        other => allow(settings, other),
    }
}

/// `lnk agent tool remove <tool>`: whether it changed what the agent has.
pub fn remove(out: &mut Out, settings: &mut Settings, tool: &str) -> Result<Changed, Report> {
    let agent = Name::new("agent", &settings.name);
    match tool {
        "memory" => memory::off(out, settings),
        "browser" | "desktop" => {
            let had = screens::remove(settings, tool);
            // A spec that gave it is changed too, so a start doesn't again.
            let switched = settings.set_tool_enabled(tool, false)?;
            Ok(match had || switched {
                true => (
                    true,
                    Done::success(sentence!(
                        "{} has no {} now.",
                        &agent,
                        Name::new("tool", tool)
                    )),
                ),
                false => {
                    out.block(Note(sentence!(
                        "{} has no {}.",
                        &agent,
                        Name::new("tool", tool)
                    )));
                    (false, Done::shown())
                }
            })
        }
        "scheduler" | "run" | "file" | "code" | "mcp" => {
            set_builtin(out, settings, tool, false, false)
        }
        other => {
            let name = settings.name.clone();
            let tool = Name::new("tool", other);
            let mut sandbox = sandbox_file::load_or_warn(&name);
            if !has_tool(&sandbox, other) {
                out.block(Note(sentence!("{} doesn't have {}.", &agent, &tool)));
                return Ok((false, Done::shown()));
            }
            if let Some(why) = sandbox.spec.widened_by_taking(&tool_spec(other))? {
                return Err(Report::error(sentence!(
                    "{} is still allowed, since taking it away would widen.",
                    &tool
                ))
                .why(Sentence::adopt(&why.to_string())));
            }
            sandbox.spec = sandbox.spec.without(&tool_spec(other));
            sandbox_file::save(&name, &sandbox)?;
            Ok((true, removed_tool(&name, other, &sandbox_file::path(&name))))
        }
    }
}

fn removed_tool(agent: &str, tool: &str, grant: &std::path::Path) -> Done {
    let tool = Name::new("tool", tool);
    Done::success(sentence!(
        "{} can't call {} anymore.",
        Name::new("agent", agent),
        &tool
    ))
    .changed("Taken from its sandbox file", Path::new(grant))
    .next(
        cmd!("lnk sandbox tool remove {}", &tool),
        "remove it for your other agents too",
    )
}

fn set_builtin(
    out: &mut Out,
    settings: &Settings,
    tool: &str,
    enabled: bool,
    yes: bool,
) -> Result<Changed, Report> {
    let granted = if enabled {
        let granted = match setup::prepare(out, settings, tool, yes)? {
            setup::Prepared::Cancelled => return Ok((false, Done::shown())),
            setup::Prepared::Unchanged => false,
            setup::Prepared::Changed => true,
        };
        // Before the spec turns it on, so the restart finds it.
        setup::install(tool)?;
        granted
    } else {
        false
    };
    let changed = settings.set_tool_enabled(tool, enabled)?;
    let state = match enabled {
        true => ToolState::On,
        false => ToolState::Off,
    };
    let mut done = Done::success(sentence!(
        "{}: {} is {}.",
        Name::new("agent", &settings.name),
        Name::new("tool", tool),
        Value::from(state)
    ));
    if changed {
        done = done.changed(
            "Written to its spec",
            Path::new(Settings::path_of(&settings.name)),
        );
    }
    if granted {
        done = done.changed(
            "Sandbox access saved in",
            Path::new(sandbox_file::path(&settings.name)),
        );
    }
    Ok((changed || granted, done))
}

/// `[tools] names = [tool]`.
fn tool_spec(tool: &str) -> link_sandbox_client::Spec {
    link_sandbox_client::Spec {
        tools: Some(link_sandbox_contract::spec::Tools {
            names: Some(vec![tool.to_string()]),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Whether `sandbox` lets its agent call `tool` here.
fn has_tool(sandbox: &Sandbox, tool: &str) -> bool {
    sandbox
        .spec
        .covers(&tool_spec(tool), link_sandbox_contract::spec::Os::here())
        .unwrap_or(false)
}

/// A tool `lnk sandbox tool add` made, allowed for the agent.
fn allow(settings: &mut Settings, tool: &str) -> Result<Changed, Report> {
    sandbox_file::check_tool(tool)?;
    let shown = Name::new("tool", tool);
    if !crate::confine::defined_tools()?.iter().any(|t| t == tool) {
        return Err(unknown_tool(tool));
    }
    let agent = settings.name.clone();
    let mut sandbox = sandbox_file::load_or_warn(&agent);
    if has_tool(&sandbox, tool) {
        return Ok((
            false,
            Done::success(sentence!(
                "{} has {} already.",
                Name::new("agent", &agent),
                &shown
            )),
        ));
    }
    sandbox.spec = sandbox.spec.join(&tool_spec(tool));
    sandbox_file::save(&agent, &sandbox)?;
    Ok((
        true,
        Done::success(sentence!(
            "{} has {} now.",
            Name::new("agent", &agent),
            &shown
        ))
        .changed(
            "Written to its sandbox file",
            Path::new(sandbox_file::path(&agent)),
        ),
    ))
}

fn unknown_tool(tool: &str) -> Report {
    let shown = Name::new("tool", tool);
    Report::error(sentence!("There is no tool {}.", &shown))
        .why(prose!("Link's are memory, web, browser and desktop."))
        .exit(Exit::Usage)
        .next(cmd!("lnk agent tool list"), "see them")
        .next(
            cmd!("lnk sandbox tool add {} -- <command>", &shown),
            "make one of your own",
        )
}

/// Link Web: its module installed, defined with a search key if one is
/// given (from its variable, or asked for), and allowed for the agent.
fn add_web(out: &mut Out, settings: &mut Settings) -> Result<Changed, Report> {
    let program = link_core::modules::add("web-tool", "Link Web")?;
    let had_web = crate::defined::list_tools()?
        .into_iter()
        .find(|t| t.name == "web");
    let defined = had_web.is_some();
    // What it had: whether it searched, and its own arguments (the
    // private hosts it may reach), carried over to a new definition.
    let searched = had_web
        .as_ref()
        .is_some_and(|t| t.allow.iter().any(|call| call == "search"));
    let carried = match had_web {
        Some(tool) => served(&tool.argv).map_err(|refused| {
            Report::error(sentence!("Web's definition isn't a serve line Link reads.")).why(
                Sentence::adopt(&link_format::clean(refused.to_string().trim())),
            )
        })?,
        None => link_web_tool_contract::WebToolServeArgs {
            private_host: Vec::new(),
        },
    };
    let key = search_key(out, searched)?;
    // Defined again only with a new key, or for the first time: adding it
    // again without its key would drop the search it had.
    if key.is_some() || !defined {
        // The key on a pipe to `lnk sandbox tool add`, which keeps it as
        // the tool's: never in a command's environment on the way.
        let mut add = match &key {
            Some((var, key)) => link_core::lnk_with([(*var, key)]),
            None => link_core::lnk(),
        };
        add.args(["sandbox", "tool", "add", "web"]);
        // Each call the tool offers, search only with its key.
        for call in link_web_tool_contract::calls_with(key.is_some())? {
            add.args(["--add", &call]);
        }
        if let Some((var, _)) = &key {
            add.args(["--env", var]);
        }
        add.arg("--").arg(&program).args(carried.argv());
        let output = add.output().within("running lnk sandbox tool add web")?;
        if !output.status.success() {
            return Err(Report::error(sentence!("Adding the tool web failed.")).why(
                Sentence::adopt(&link_format::clean(
                    String::from_utf8_lossy(&output.stderr).trim(),
                )),
            ));
        }
        crate::confine::forget_defined_tools();
    }
    let agent = settings.name.clone();
    let mut sandbox = sandbox_file::load_or_warn(&agent);
    let had = has_tool(&sandbox, "web");
    if !had {
        sandbox.spec = sandbox.spec.join(&tool_spec("web"));
        sandbox_file::save(&agent, &sandbox)?;
    }
    let (search, offer_search) = web_search(key.as_ref().map(|(var, _)| *var), searched);
    Ok((
        !had || key.is_some(),
        added_web(&agent, search, &sandbox_file::path(&agent), offer_search),
    ))
}

/// What adding Web says of its search, and whether it offers a key:
/// by the variable of the key given (`key`), else by whether its
/// definition already searched (`searched`).
fn web_search(key: Option<&str>, searched: bool) -> (&'static str, bool) {
    match key {
        Some("TAVILY_API_KEY") => ("it searches with your Tavily key", false),
        Some(_) => ("it searches with your Brave Search key", false),
        None if searched => ("it searches as it did", false),
        None => ("it fetches pages without search", true),
    }
}

/// The `serve` line of Web's definition `argv`, from its program on,
/// read by the contract's own table: what a new definition keeps, such
/// as its private hosts.
fn served(
    argv: &[String],
) -> Result<link_web_tool_contract::WebToolServeArgs, link_actions::cli::Error> {
    let at = argv
        .iter()
        .position(|arg| arg == "serve")
        .unwrap_or(argv.len());
    link_web_tool_contract::WebToolServeArgs::from_argv(&argv[at..])
}

fn added_web(agent: &str, search: &str, grant: &std::path::Path, offer_search: bool) -> Done {
    let mut done = Done::success(sentence!(
        "{} has Web: {}.",
        Name::new("agent", agent),
        search
    ))
    .changed("Its grant is in", Path::new(grant))
    .next(
        cmd!("lnk sandbox tool list"),
        "see the tool, with its key outside your agent",
    );
    if offer_search {
        done = done.next(
            cmd!("lnk agent tool add web"),
            "add it again with a key, for search",
        );
    }
    done
}

/// A screen, Link browser or Link desktop: its module installed, and
/// defined for the agent alone.
fn add_screen(settings: &mut Settings, tool: &str) -> Result<Changed, Report> {
    let agent = Name::new("agent", &settings.name);
    if !screens::runs_here(tool) {
        return Err(no_desktop(&agent));
    }
    let added = screens::add(settings, tool)?;
    // Its spec says it, so a start gives it again wherever the agent goes.
    let switched = settings.set_tool_enabled(tool, true)?;
    if !added && !switched {
        return Ok((
            false,
            Done::success(sentence!(
                "{} has a {} already.",
                &agent,
                Name::new("tool", tool)
            )),
        ));
    }
    let (has, kept) = match tool {
        "desktop" => (
            "a desktop: it sees and works it, and the programs it runs show there",
            "Its display takes only programs with its cookie, kept in",
        ),
        _ => (
            "a browser: it opens, reads, clicks and sees pages",
            "It runs in a sandbox of its own, reaching the internet only, and keeps its sign-ins in",
        ),
    };
    let dir = match tool {
        "desktop" => screens::desktop_dir(settings),
        _ => screens::profile(settings),
    };
    let mut done =
        Done::success(sentence!("{} has {}.", &agent, has)).changed(kept, Path::new(dir));
    if switched {
        done = done.changed(
            "Written to its spec",
            Path::new(Settings::path_of(&settings.name)),
        );
    }
    Ok((true, done))
}

/// Why `agent` can't have a desktop on a computer that isn't Linux.
fn no_desktop(agent: &Name) -> Report {
    Report::error(sentence!("Couldn't give {} a desktop.", agent))
        .why(prose!(
            "Its desktop is a virtual screen, made by Linux's Xvfb, and this computer isn't Linux."
        ))
        .next(
            cmd!("lnk agent tool list"),
            "see the tools it can have here",
        )
        .next(
            cmd!("lnk agent move"),
            "move it to a Linux machine, where it can",
        )
}

/// What the question for Web's search key says, for a tool that has
/// one already (`searches`) or not.
fn search_why(searches: bool) -> Prose {
    match searches {
        true => prose!(
            "Web fetches pages for free. To search too, it needs a key from Brave Search \
             (https://brave.com/search/api) or Tavily (https://tavily.com). No keeps the one it has."
        ),
        false => prose!(
            "Web fetches pages for free. To search too, it needs a key from Brave Search \
             (https://brave.com/search/api) or Tavily (https://tavily.com). No gives it pages only."
        ),
    }
}

/// A search key for Link Web, with its variable: from the environment,
/// else asked for in a terminal. None for fetch only, or to keep the key
/// the tool searches with (`searches`).
fn search_key(out: &mut Out, searches: bool) -> Result<Option<(&'static str, Secret)>, Report> {
    for var in SEARCH_KEYS {
        let key = Secret::from_env(var).map(|k| k.expose_map(|k| k.trim().to_string()));
        if let Some(key) = key.filter(|k| !k.is_empty()) {
            return Ok(Some((var, key)));
        }
    }
    if !std::io::stdin().is_terminal() {
        return Ok(None);
    }
    let why = search_why(searches);
    let wants = Question::confirm(sentence!("Give Web a search key?"), false).why(why);
    if !out.ask(wants)? {
        return Ok(None);
    }
    let key = out
        .ask(Question::hidden(sentence!("What is the search key?")))?
        .expose_map(|k| k.trim().to_string());
    Ok(match key.expose().as_str() {
        k if k.starts_with("tvly-") => Some(("TAVILY_API_KEY", key)),
        _ => Some(("BRAVE_API_KEY", key)),
    })
}

/// At an agent's first start, in a terminal: which of Link's tools it
/// gets, memory picked unless you say otherwise. Each one's failure is
/// said and skipped: the agent still starts.
pub fn pick(out: &mut Out, settings: &mut Settings) -> Result<(), Report> {
    let picks: Vec<&Offered> = OFFERED
        .iter()
        .filter(|o| !o.built_in && !has(settings, o.name))
        .collect();
    // Link Harness's own, those its spec leaves on.
    let built_in: Vec<&str> = match settings.harness() == settings::DEFAULT_HARNESS {
        true => {
            let spec = settings.spec()?;
            OFFERED
                .iter()
                .filter(|o| o.built_in && spec.tool(o.name).enabled == Some(true))
                .map(|o| o.name)
                .collect()
        }
        false => Vec::new(),
    };
    if !built_in.is_empty() {
        out.block(Note(sentence!(
            "Tools already on: {}.",
            Value::List(built_in.iter().map(|b| Value::from(*b)).collect())
        )));
    }
    let mut chosen = Vec::new();
    for (i, offered) in picks.iter().enumerate() {
        let needs = match offered.needs {
            "" => Value::from(""),
            needs => Value::from(format!(" ({needs})")),
        };
        // Memory unless you say otherwise.
        let give = Question::confirm(
            sentence!(
                "Give your agent {}: {}{}?",
                Name::new("tool", offered.name),
                offered.about,
                needs
            ),
            i == 0,
        );
        if out.ask(give)? {
            chosen.push(offered.name);
        }
    }
    if chosen.is_empty() {
        out.block(Note(sentence!(
            "{} adds one later.",
            cmd!("lnk agent tool add <tool>")
        )));
    }
    for tool in chosen {
        if let Err(err) = add(out, settings, tool, false) {
            out.block(
                Report::warning(sentence!("{} wasn't added.", Name::new("tool", tool)))
                    .why(Sentence::adopt(&err.headline.to_string()))
                    .next(
                        cmd!("lnk agent tool add {}", Name::new("tool", tool)),
                        "try again",
                    ),
            );
        }
    }
    Ok(())
}

/// What a start does about Link's tools agent `settings` doesn't have:
/// asks once, in a terminal ([`pick`]), remembering it asked whatever
/// the answer; without one, says once, at its `first` start, what adds
/// them, which isn't an answer, so a later start in a terminal asks.
pub fn offer(out: &mut Out, settings: &mut Settings, first: bool) -> Result<(), Report> {
    let picks: Vec<&Offered> = OFFERED
        .iter()
        .filter(|o| !o.built_in && !has(settings, o.name))
        .collect();
    match offering(settings.tools_offered, picks.is_empty(), first, may_ask()) {
        Offer::Ask => {
            pick(out, settings)?;
            settings.tools_offered = true;
        }
        Offer::Say => {
            let names: Vec<Value> = picks.iter().map(|o| Value::from(o.name)).collect();
            out.block(Note(sentence!(
                "Your agent can also have {}, and {} adds one.",
                Value::List(names),
                cmd!("lnk agent tool add <tool>")
            )));
        }
        Offer::Nothing => {}
    }
    Ok(())
}

#[derive(Debug, PartialEq)]
enum Offer {
    Ask,
    Say,
    Nothing,
}

/// [`offer`]'s choice: asked once if it can be, else said at the first
/// start, else nothing.
fn offering(offered: bool, none_left: bool, first: bool, may_ask: bool) -> Offer {
    match (offered || none_left, may_ask, first) {
        (true, ..) => Offer::Nothing,
        (false, true, _) => Offer::Ask,
        (false, false, true) => Offer::Say,
        (false, false, false) => Offer::Nothing,
    }
}

/// Whether a first start may ask which tools: in a terminal, and not
/// told yes to everything (`--yes`, a script).
pub fn may_ask() -> bool {
    std::io::stdin().is_terminal()
        && !crate::permissions::YES.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_tool_changes_have_declared_navigation() {
        let actions: toml::Value = crate::navigation::navigation();
        let grant = std::path::Path::new("/fixture/sandbox.toml");
        for (action, typed, done) in [
            (
                "agent tool add",
                cmd!("lnk agent tool add web"),
                added_web("main", "fetches pages", grant, true),
            ),
            (
                "agent tool add",
                cmd!("lnk agent tool add web"),
                added_web("main", "searches with your key", grant, false),
            ),
            (
                "agent tool remove",
                cmd!("lnk agent tool remove web"),
                removed_tool("main", "web", grant),
            ),
        ] {
            let declared: Vec<String> = actions[action]["next"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap().to_owned())
                .collect();
            for next in &done.next.0 {
                assert!(
                    link_format::nexts::declares(&declared, &next.command, &typed),
                    "{action} offers undeclared navigation: {}",
                    next.command.text()
                );
            }
        }
        let declared: Vec<String> = actions["agent tool add"]["fail"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_owned())
            .collect();
        let refused = [
            unknown_tool("not-defined"),
            no_desktop(&Name::new("agent", "main")),
        ];
        for next in refused.iter().flat_map(|report| &report.next.0) {
            assert!(
                link_format::nexts::declares(
                    &declared,
                    &next.command,
                    &cmd!("lnk agent tool add not-defined")
                ),
                "an unknown tool offers undeclared navigation: {}",
                next.command.text()
            );
        }
    }

    /// Adding Web again carries its arguments over, its private hosts
    /// among them, and says it searches only when its definition did.
    #[test]
    fn adding_web_again_keeps_what_it_had() {
        let argv = |line: &str| line.split(' ').map(String::from).collect::<Vec<_>>();
        // The hosts it had, read back by the contract and written again.
        let had = served(&argv(
            "/x/lnk-web-tool serve --private-host wiki.lan --private-host=10.0.0.2",
        ))
        .unwrap();
        assert_eq!(had.private_host, ["wiki.lan", "10.0.0.2"]);
        assert_eq!(
            had.argv(),
            argv("serve --private-host wiki.lan --private-host 10.0.0.2")
        );
        assert!(
            served(&argv("/x/lnk-web-tool serve"))
                .unwrap()
                .private_host
                .is_empty()
        );
        // A line it can't read is refused, never half read.
        assert!(served(&argv("/x/lnk-web-tool serve --colour red")).is_err());
        assert!(served(&argv("/x/lnk-web-tool")).is_err());
        assert_eq!(
            web_search(None, false),
            ("it fetches pages without search", true)
        );
        assert_eq!(web_search(None, true), ("it searches as it did", false));
        assert_eq!(
            web_search(Some("TAVILY_API_KEY"), false),
            ("it searches with your Tavily key", false)
        );
        assert_eq!(
            web_search(Some("BRAVE_API_KEY"), true),
            ("it searches with your Brave Search key", false)
        );
    }

    /// The question for Web's search key ends on a whole sentence either
    /// way, as a debug build checks.
    #[test]
    fn the_search_key_question_says_what_no_means() {
        assert!(
            search_why(true)
                .to_string()
                .ends_with("keeps the one it has.")
        );
        assert!(
            search_why(false)
                .to_string()
                .ends_with("gives it pages only.")
        );
    }

    #[test]
    fn the_tools_are_asked_once_and_said_once_without_a_terminal() {
        // In a terminal, asked until it has been, whichever start.
        assert_eq!(offering(false, false, true, true), Offer::Ask);
        assert_eq!(offering(false, false, false, true), Offer::Ask);
        // Asked once, or nothing left to offer: never again.
        assert_eq!(offering(true, false, false, true), Offer::Nothing);
        assert_eq!(offering(false, true, true, true), Offer::Nothing);
        // Without a terminal: said at the first start only, not asked.
        assert_eq!(offering(false, false, true, false), Offer::Say);
        assert_eq!(offering(false, false, false, false), Offer::Nothing);
    }
}
