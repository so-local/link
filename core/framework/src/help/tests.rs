use super::*;
use clap::{CommandFactory, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "lnk")]
struct Cli {
    #[command(subcommand)]
    group: Group,
}

#[derive(Subcommand)]
enum Group {
    Thing {
        #[command(subcommand)]
        command: Thing,
    },
}

#[derive(Subcommand)]
enum Thing {
    Start {
        #[command(flatten)]
        output: link_spec::shared::Output,
        name: Option<String>,
    },
}

const TOML: &str = r#"
[lnk-thing]
about = "The thing module"

[thing]
about = "Things"

[thing.start]
about = """
Start a thing.

It starts
at once.
"""
"--json" = "As JSON."
"<name>" = "Its name"
"#;

const HELP: Help = Help {
    toml: TOML,
    own: Some("lnk-thing"),
    under: "",
};

#[test]
fn help_is_read_as_clap_reads_a_doc_comment() {
    let mut command = HELP.apply(Cli::command());
    command.build();
    assert_eq!(command.get_about().unwrap().to_string(), "The thing module");
    let thing = command.find_subcommand("thing").unwrap();
    let start = thing.find_subcommand("start").unwrap();
    assert_eq!(start.get_about().unwrap().to_string(), "Start a thing");
    assert_eq!(
        start.get_long_about().unwrap().to_string(),
        "Start a thing.\n\nIt starts at once."
    );
    let json = start
        .get_arguments()
        .find(|a| a.get_id() == "json")
        .unwrap();
    assert_eq!(json.get_help().unwrap().to_string(), "As JSON");
    assert!(json.get_long_help().is_none());
}

/// An argument declared hidden leaves its command's help, a global
/// one from above included, and still parses.
#[test]
fn an_argument_declared_hidden_leaves_its_help() {
    #[derive(Parser)]
    #[command(name = "lnk")]
    struct Cli {
        #[arg(long, global = true)]
        agent: Option<String>,
        #[command(subcommand)]
        command: Sub,
    }
    #[derive(Subcommand)]
    enum Sub {
        Create {
            #[arg(long)]
            zone: Option<String>,
        },
        Start,
    }
    let help = Help {
        toml: "[create]\nabout = \"Make one\"\n\"<hidden>\" = [\"--zone\", \"--agent\"]\n\n[start]\nabout = \"Start one\"\n",
        own: None,
        under: "",
    };
    let mut command = help.apply(Cli::command());
    command.build();
    let create = command.find_subcommand("create").unwrap();
    for id in ["zone", "agent"] {
        let arg = create.get_arguments().find(|a| a.get_id() == id).unwrap();
        assert!(arg.is_hide_set(), "{id}");
    }
    let start = command.find_subcommand("start").unwrap();
    let agent = start
        .get_arguments()
        .find(|a| a.get_id() == "agent")
        .unwrap();
    assert!(!agent.is_hide_set());
    let parsed = help
        .apply(Cli::command())
        .try_get_matches_from(["lnk", "create", "--agent", "a", "--zone", "z"])
        .unwrap();
    let (_, create) = parsed.subcommand().unwrap();
    assert_eq!(
        create.get_one::<String>("zone").map(String::as_str),
        Some("z")
    );
}

#[test]
fn a_command_line_that_parses_reads_no_help() {
    let help = Help {
        toml: "not toml [",
        ..HELP
    };
    let cli: Cli = help.parse_from(["lnk", "thing", "start", "--json"].map(OsString::from));
    let Group::Thing {
        command:
            Thing::Start {
                output: link_spec::shared::Output { json },
                ..
            },
    } = cli.group;
    assert!(json);
}

#[test]
fn a_program_of_one_group_run_on_its_own_reads_its_group() {
    let cli: Cli = HELP.parse_from(["lnk-thing", "start", "--json"].map(OsString::from));
    let Group::Thing {
        command:
            Thing::Start {
                output: link_spec::shared::Output { json },
                ..
            },
    } = cli.group;
    assert!(json);
    let line =
        |args: &[&str]| grouped(&Cli::command(), args.iter().map(Into::into).collect(), None);
    assert_eq!(
        line(&["lnk-thing", "--help"]),
        ["lnk-thing", "thing", "--help"]
    );
    assert_eq!(
        line(&["lnk-thing", "thing", "start"]),
        ["lnk-thing", "thing", "start"]
    );
    assert_eq!(
        line(&["lnk-thing", "--version"]),
        ["lnk-thing", "--version"]
    );
    // An adapter's hidden contract words are no group either, and a line
    // starting with one is its own.
    let adapter = Cli::command().subcommand(clap::Command::new("tunnel").hide(true));
    let line = |args: &[&str]| grouped(&adapter, args.iter().map(Into::into).collect(), None);
    assert_eq!(
        line(&["lnk-thing", "start"]),
        ["lnk-thing", "thing", "start"]
    );
    assert_eq!(
        line(&["lnk-thing", "tunnel", "--id", "x"]),
        ["lnk-thing", "tunnel", "--id", "x"]
    );
}

#[test]
fn check_finds_what_is_missing_and_what_names_nothing() {
    assert_eq!(check(TOML, &[(Cli::command(), HELP)]), Vec::<String>::new());
    let missing = TOML.replace("\"<name>\" = \"Its name\"", "");
    let said = check(&missing, &[(Cli::command(), HELP)]).join("\n");
    assert!(said.contains("no thing.start.\"<name>\""), "{said}");
    let extra = format!("{TOML}\n[thing.stop]\nabout = \"Stop\"\n");
    let said = check(&extra, &[(Cli::command(), HELP)]).join("\n");
    assert!(said.contains("thing.stop names no command"), "{said}");
    let in_code = Cli::command().about("In the code");
    let said = check(TOML, &[(in_code, HELP)]).join("\n");
    assert!(said.contains("help in the code"), "{said}");
}

#[test]
fn a_program_without_clap_lists_its_commands() {
    let help = Help {
        toml: "[lnk-x]\nabout = \"Run by lnk.\"\n[lnk-x.install]\nabout = \"Install it.\"\n\
               [lnk-x.serve]\nabout = \"Serve.\"\nusage = \"serve <folder>\"\n",
        own: Some("lnk-x"),
        under: "lnk-x",
    };
    assert_eq!(
        help.usage(),
        "Run by lnk.\n\nUsage: lnk-x <COMMAND>\n\nCommands:\n  install         Install it\n  \
         serve <folder>  Serve\n"
    );
    assert_eq!(help.commands(), ["install", "serve"]);
    let long = Help {
        toml: "[lnk-x]\n[lnk-x.serve]\nabout = \"Serve the folder over stdio, its index and \
               model and notes as the flags name them, and stop when asked.\"\n\
               usage = \"serve <folder> --index <file> --embed <model>\"\n",
        ..help
    };
    assert_eq!(
        long.usage(),
        "Usage: lnk-x <COMMAND>\n\nCommands:\n  serve <folder> --index <file> --embed <model>\n          \
         Serve the folder over stdio, its index and model and notes as the\n          \
         flags name them, and stop when asked\n"
    );
    let source = "fn try_main() {\n    match x {\n        \"install\" => a(),\n        \
                  Some(\"serve\" | \"run\") => b(),\n        \"--version\" | \"-V\" => c(),\n    }\n}\n";
    assert_eq!(answered(source, "try_main"), ["install", "run", "serve"]);
}

#[test]
fn command_help_leaves_environment_variables_to_the_inventory() {
    let help = Help {
        toml: "[package.metadata.link.commands.serve]\nhelp = \"Serve.\"\nafter = \"More.\"\n\
               [package.metadata.link.commands.serve.environment.LNK_B]\nhelp = \"The b.\"\n\
               [package.metadata.link.commands.serve.options.port]\nenvironment = \"LNK_A\"\n\
               help = \"The\\nport.\"\n",
        own: None,
        under: "x",
    };
    let file = help.file().unwrap();
    assert_eq!(
        table_at(&file, "x.serve").unwrap()["after"].as_str(),
        Some("More.")
    );
    // Program variables do not expand the routed group's help either.
    let help = Help {
        toml: "[commands.root.environment.LNK_P]\nhelp = \"The p.\"\n\
               [commands.x]\nhelp = \"X.\"\nroute = true\n",
        own: Some("lnk-x"),
        under: "",
    };
    let file = help.file().unwrap();
    assert_eq!(table_at(&file, "x").unwrap().get("after"), None);
}
