//! Every released public program is declared and checked against its real parser.
link_test_kit::program!(unit);

use link_docs_check_code::parsers::named;
use std::path::{Path, PathBuf};

fn programs() -> std::collections::BTreeMap<String, PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    link_test_kit::programs::declared(&[root]).unwrap()
}

#[test]
fn every_program_declares_its_commands() {
    let parsers = named();
    let count = parsers.len();
    let checked: std::collections::BTreeSet<_> = parsers
        .into_iter()
        .map(|(name, _)| name.to_string())
        .collect();
    assert_eq!(count, checked.len(), "a parser is registered twice");
    let programs: std::collections::BTreeSet<_> = programs().into_keys().collect();
    assert_eq!(
        programs, checked,
        "register every released program's parser, without exceptions"
    );
}

/// These CLI-only values are deliberately elided by serde. Assert the actual
/// owning parsed fields; a JSON/file snapshot cannot establish their values.
#[test]
fn runtime_only_booleans_are_read_by_the_actual_owning_parsers() {
    use clap::{Parser, error::ErrorKind};
    type SlotsReader = fn(&[&str]) -> Result<link_env::cli::Slots, clap::Error>;

    fn agent_slots(argv: &[&str]) -> Result<link_env::cli::Slots, clap::Error> {
        use link_agent::cli::{Cli, Command, slots};
        // lnk-agent is handed the words after its namespace, as lnk runs it.
        let argv = std::iter::once("lnk-agent").chain(argv.iter().copied().skip(2));
        let slots = match Cli::try_parse_from(argv)?.command {
            Command::AgentCreate(a) => slots(&a.on, &a.cloud, &a.zone, a.here, &a.place),
            Command::AgentMove(a) => slots(&a.on, &a.cloud, &a.zone, a.here, &a.place),
            Command::TeamMove(a) => slots(&a.on, &a.cloud, &a.zone, a.here, &a.place),
            _ => panic!("the fixture selects create, move or team move"),
        };
        Ok(slots)
    }
    fn box_slots(argv: &[&str]) -> Result<link_env::cli::Slots, clap::Error> {
        use link_box::cli::{Cli, Command};
        // lnk-box is handed the words after its namespace, as lnk runs it.
        let argv = std::iter::once("lnk-box").chain(argv.iter().copied().skip(2));
        let Command::BoxCreate(args) = Cli::try_parse_from(argv)?.command else {
            panic!("the fixture selects the actual box create command")
        };
        Ok(link_env::cli::Slots {
            place: args.place,
            on: args.on,
            cloud: args.cloud,
            zone: args.zone,
            here: args.here,
        })
    }
    fn env_slots(argv: &[&str]) -> Result<link_env::cli::Slots, clap::Error> {
        use link_env::cli::{Cli, Command, Slots};
        // lnk-env is handed the words after its namespace, as lnk runs it.
        let argv = std::iter::once("lnk-env").chain(argv.iter().copied().skip(2));
        let slots = match Cli::try_parse_from(argv)?.command {
            Command::ImageCreate(args) => Slots {
                place: args.place,
                cloud: args.cloud,
                here: args.here,
                ..Slots::default()
            },
            Command::EnvSetup(args) => Slots {
                place: args.place,
                on: args.on,
                here: args.here,
                ..Slots::default()
            },
            _ => panic!("the fixture selects env image create or setup"),
        };
        Ok(slots)
    }
    fn secret_file(argv: &[&str]) -> Result<bool, clap::Error> {
        use link_box::cli::{Cli, Command};
        let argv = std::iter::once("lnk-box").chain(argv.iter().copied().skip(2));
        let Command::KeyShow(args) = Cli::try_parse_from(argv)?.command else {
            panic!("the fixture selects the actual box key show command")
        };
        Ok(args.secret_file)
    }

    const BOTH: &[(&str, &str)] = &[("aws", "--cloud"), ("here", "--here")];
    // `env setup` takes a box or here, never a cloud.
    const HERE: &[(&str, &str)] = &[("here", "--here")];
    type Route = (
        &'static [&'static str],
        SlotsReader,
        &'static [(&'static str, &'static str)],
    );
    let routes: &[Route] = &[
        (&["lnk", "agent", "create", "fixture"], agent_slots, BOTH),
        (&["lnk", "agent", "move"], agent_slots, BOTH),
        (
            &["lnk", "agent", "team", "move", "fixture"],
            agent_slots,
            BOTH,
        ),
        (&["lnk", "box", "create"], box_slots, BOTH),
        (
            &["lnk", "env", "image", "create", "--spec", "fixture.toml"],
            env_slots,
            BOTH,
        ),
        (
            &["lnk", "env", "setup", "--spec", "fixture.toml"],
            env_slots,
            HERE,
        ),
    ];
    // Each composing command reads the cloud positional after its own,
    // and `here` as --here, through its actual parser.
    let target = |argv: &[&str], read: SlotsReader| read(argv).unwrap().target().unwrap();
    for (base, read, words) in routes {
        assert_eq!(target(base, *read), None, "{base:?}");
        for &(word, strict) in *words {
            let mut short = base.to_vec();
            short.push(word);
            let mut long = base.to_vec();
            long.push(strict);
            if strict == "--cloud" {
                long.push(word);
            }
            assert_eq!(target(&short, *read), target(&long, *read), "{short:?}");
        }
        let mut bad = base.to_vec();
        bad.push("--here=not-a-boolean");
        assert_eq!(
            read(&bad).unwrap_err().kind(),
            ErrorKind::TooManyValues,
            "{bad:?}"
        );
    }
    let key = ["lnk", "box", "key", "show"];
    assert!(!secret_file(&key).unwrap());
    assert!(secret_file(&["lnk", "box", "key", "show", "--secret-file"]).unwrap());
    assert_eq!(
        secret_file(&["lnk", "box", "key", "show", "--secret-file=not-a-boolean"])
            .unwrap_err()
            .kind(),
        ErrorKind::TooManyValues,
    );
}

/// Closed CLI domains are asserted through the owning concrete parsed fields,
/// including alternatives the shared first-sample reader did not exercise.
#[test]
fn enum_alternatives_are_read_by_the_actual_owning_parsers() {
    use clap::{Parser, error::ErrorKind};
    use link_agent::cli::WebhookCreateArgsSender as Sender;
    use link_sandbox::bridges::tools::Ask;

    fn sender(value: &str) -> Result<Option<Sender>, clap::Error> {
        use link_agent::cli::{Cli, Command};
        let Command::WebhookCreate(args) = Cli::try_parse_from([
            "lnk-agent",
            "webhook",
            "create",
            "fixture",
            "--sender",
            value,
        ])?
        .command
        else {
            panic!("the fixture selects actual webhook create")
        };
        Ok(args.sender)
    }
    fn answer(value: &str) -> Result<String, clap::Error> {
        use link_sandbox::cli::{Cli, Command, SandboxAnswerArgsAnswer};
        let Command::SandboxAnswer(args) =
            Cli::try_parse_from(["lnk-sandbox", "answer", "fixture", value])?.command
        else {
            panic!("the fixture selects actual sandbox answer")
        };
        Ok(match args.answer {
            SandboxAnswerArgsAnswer::Yes => "yes",
            SandboxAnswerArgsAnswer::No => "no",
        }
        .into())
    }
    fn ask(value: &str) -> Result<Ask, clap::Error> {
        use link_sandbox::cli::{Cli, Command, ToolAddArgsAsk};
        let Command::ToolAdd(args) = Cli::try_parse_from([
            "lnk-sandbox",
            "tool",
            "add",
            "fixture",
            "--url",
            "https://fixture.invalid/mcp",
            "--ask",
            value,
        ])?
        .command
        else {
            panic!("the fixture selects actual sandbox tool add")
        };
        Ok(match args.ask {
            Some(ToolAddArgsAsk::None) => Ask::None,
            Some(ToolAddArgsAsk::All) | None => Ask::All,
        })
    }

    assert_eq!(sender("link").unwrap(), Some(Sender::Link));
    assert_eq!(sender("github").unwrap(), Some(Sender::Github));
    assert_eq!(answer("yes").unwrap(), "yes");
    assert_eq!(answer("no").unwrap(), "no");
    assert_eq!(ask("all").unwrap(), Ask::All);
    assert_eq!(ask("none").unwrap(), Ask::None);
    for invalid in ["true", "1", "not-a-declared-value"] {
        assert_eq!(sender(invalid).unwrap_err().kind(), ErrorKind::InvalidValue);
        assert_eq!(answer(invalid).unwrap_err().kind(), ErrorKind::InvalidValue);
        assert_eq!(ask(invalid).unwrap_err().kind(), ErrorKind::InvalidValue);
    }
}

#[test]
fn conditional_omissions_are_read_by_the_actual_owning_parsers() {
    use clap::Parser;
    let link_box::cli::Command::BoxDelete(deleted) = link_box::cli::Cli::try_parse_from([
        "lnk-box",
        "delete",
        "--id",
        "fixture",
        "--cloud",
        "aws",
        "--zone",
        "fixture-zone",
    ])
    .unwrap()
    .command
    else {
        panic!("the fixture selects actual box delete")
    };
    assert!(deleted.name.is_none());
    assert_eq!(deleted.id.as_deref(), Some("fixture"));
    assert_eq!(deleted.cloud.as_deref(), Some("aws"));
    assert_eq!(deleted.zone.as_deref(), Some("fixture-zone"));

    let link_sandbox::cli::Command::ToolAdd(tool) =
        <link_sandbox::cli::Cli as clap::Parser>::try_parse_from([
            "lnk-sandbox",
            "tool",
            "add",
            "fixture",
            "--url",
            "https://fixture.invalid/mcp",
        ])
        .unwrap()
        .command
    else {
        panic!("the fixture selects actual sandbox tool add")
    };
    assert!(tool.add.is_empty());
    assert_eq!(tool.url.as_deref(), Some("https://fixture.invalid/mcp"));
}

/// No runtime flag of a command that binds a spec shares a top-level
/// field's flag, `--os` both the spec's and the command's: the field's
/// flag is its own name, so the declaration renames one.
#[test]
fn no_runtime_flag_shares_a_fields_name() {
    let programs = programs();
    let kinds = link_docs_check_code::references::kinds::kinds();
    let mut problems = Vec::new();
    for (name, command) in named() {
        let actions = std::fs::read_to_string(programs[name].join("actions.toml")).unwrap();
        for (path, action) in &link_component::actions::parse(&actions).unwrap() {
            let Some(kind) = &action.spec else { continue };
            let Some(page) = kinds.iter().find(|page| page.kind.kind == *kind) else {
                continue;
            };
            let words: Vec<String> = path.split_whitespace().map(str::to_string).collect();
            let runtime = link_spec::entry::runtime_flags(&command, &words);
            for clash in link_spec::clashes(&page.kind.shape, &runtime) {
                problems.push(format!("{name} {path}: {clash}"));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Every public module's `history.toml` holds to its commands today: no
/// entry is a current command, tool call or flag of its module, each
/// replacement is a command or tool call of the release's whole surface,
/// every module's and the core's (a bound command's field flags among its
/// flags, a tool module's calls by the names a model calls), so a command
/// moved names its new home, and none has reached the release that
/// deletes it. Link 0.1 is the baseline every history starts from, so a
/// module has a file only once a later release retires something; the
/// mechanism's own cases are `link-spec`'s fixtures.
#[test]
fn every_modules_history_holds_to_its_commands() {
    let programs = programs();
    let kinds = link_docs_check_code::references::kinds::kinds();
    let version = link_version::version();
    let mut problems = Vec::new();
    let surfaces: Vec<_> = named()
        .into_iter()
        .map(|(name, command)| {
            let folder = &programs[name];
            let mut surface = link_spec::entry::surface(&command);
            let actions = std::fs::read_to_string(folder.join("actions.toml")).unwrap();
            for (path, action) in &link_component::actions::parse(&actions).unwrap() {
                let Some(kind) = &action.spec else { continue };
                let Some(page) = kinds.iter().find(|page| page.kind.kind == *kind) else {
                    continue;
                };
                let flags = surface.entry(path.clone()).or_default();
                for field in link_spec::fields::fields(&page.kind.shape) {
                    flags.insert(format!("--{}", field.flag), true);
                }
            }
            // Each call a model makes of a tool module, `<module> <call>`
            // by the name it calls (`memory-tool search`): an agent tool
            // is a command of its module, so its history names its calls.
            let raw: toml::Table = toml::from_str(&actions).unwrap();
            for action in raw
                .get("actions")
                .and_then(toml::Value::as_table)
                .into_iter()
                .flat_map(|a| a.values())
            {
                let on_tool = action
                    .get("surfaces")
                    .and_then(toml::Value::as_array)
                    .is_some_and(|s| s.iter().any(|s| s.as_str() == Some("tool")));
                let call = action
                    .get("tool")
                    .and_then(|t| t.get("name"))
                    .and_then(toml::Value::as_str);
                if let (true, Some(call)) = (on_tool, call) {
                    surface.entry(format!("{name} {call}")).or_default();
                }
            }
            (name, surface)
        })
        .collect();
    let mut catalog = link_spec::history::Surface::new();
    for (path, flags) in surfaces.iter().flat_map(|(_, surface)| surface) {
        catalog
            .entry(path.clone())
            .or_default()
            .extend(flags.clone());
    }
    for (name, surface) in &surfaces {
        let Ok(text) = std::fs::read_to_string(programs[*name].join("history.toml")) else {
            continue;
        };
        let history = match link_spec::history::History::parse(&text) {
            Ok(history) => history,
            Err(e) => {
                problems.push(format!("{name}: {e:#}"));
                continue;
            }
        };
        if let Err(e) = history.validate(surface, &catalog, &[], &version) {
            problems.push(format!("{name}: {e:#}"));
        }
        problems.extend(
            history
                .check_retirement(&version)
                .into_iter()
                .map(|p| format!("{name}: {p}")),
        );
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Every `lnk` command a box's setup script runs is one its program's
/// parser takes, as a person's would be: the script is generated text, so
/// only parsing its lines catches a command that has since been renamed.
#[test]
fn a_boxs_setup_runs_commands_its_programs_take() {
    use link_box::setup::{Checked, Releases, install_script};
    let machine = link_machine::installation::Installation {
        id: "k3v9q2x7m1ab".into(),
        name: "aws-1".into(),
    };
    let version: link_version::Version = "0.14.0".parse().unwrap();
    let releases = Releases {
        source: "https://builds.example/releases".parse().unwrap(),
        version: Some(version.clone()),
        followed: true,
        signers: String::new(),
    };
    let checked = Checked {
        version,
        script: "#!/bin/sh\n".into(),
    };
    let script = install_script(&machine, &releases, &checked).unwrap();
    let parsers = named();
    let program = "\"$HOME/.lnk/exec/bin/lnk\" ";
    let mut ran = 0;
    for line in script.lines() {
        // A line that runs lnk, after the variables it's given.
        let Some((before, words)) = line.split_once(program) else {
            continue;
        };
        if before.contains('$') || words.contains('|') {
            continue;
        }
        let words: Vec<String> = words
            .split_whitespace()
            .map(|w| w.trim_matches('\'').to_string())
            .collect();
        let parser = parsers
            .iter()
            .find(|(name, _)| *name == words[0])
            .or_else(|| parsers.iter().find(|(name, _)| *name == "core"))
            .map(|(_, parser)| parser.clone())
            .unwrap();
        let argv = std::iter::once("lnk".to_string()).chain(words.iter().cloned());
        let parsed = parser.try_get_matches_from(argv);
        assert!(parsed.is_ok(), "lnk {}: {parsed:?}", words.join(" "));
        ran += 1;
    }
    // module add, secret keeper create and sandbox workers, at least.
    assert!(ran >= 3, "{script}");
}
