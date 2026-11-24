use super::*;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Start {
        name: String,
        #[arg(long)]
        json: bool,
    },
    Stop,
    Run {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    Job {
        place: Option<String>,
        #[arg(last = true)]
        command: Vec<String>,
    },
}

struct Example;

impl Program for Example {
    fn before(&self, _: &[OsString]) {}
    fn styled(&self, command: clap::Command) -> clap::Command {
        command
    }
    fn helped(&self, command: clap::Command) -> clap::Command {
        command
    }
    fn history(&self) -> History {
        History::parse(
            "[[gone]]\nwas = \"start --model\"\nnow = \"lnk start --model.name\"\nsince = \"0.1.0\"\nuntil = \"99.0.0\"\n[[gone]]\nwas = \"run --network\"\nwhy = \"Gone.\"\nsince = \"0.1.0\"\nuntil = \"99.0.0\"\n",
        )
        .unwrap()
    }
    fn binding(&self, words: &[String]) -> Option<Binding> {
        (words == ["start"] || words == ["job"]).then(Binding::of::<crate::fields::tests::Agent>)
    }
}

fn args(words: &[&str]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
}

#[test]
fn a_bound_commands_field_flags_are_taken_before_clap_reads_the_rest() {
    let parsed: Parsed<Cli> = try_run(
        &Example,
        args(&["lnk-example", "start", "main", "--model.thinking", "high"]),
        |c| c,
    )
    .unwrap();
    assert_eq!(parsed.words, ["start"]);
    assert_eq!(
        parsed.fields.table()["model"]["thinking"].as_str(),
        Some("high")
    );
    assert!(matches!(parsed.command.command, Command::Start { ref name, .. } if name == "main"));
}

/// A command whose program words follow `--` (`last`) takes a positional
/// before them, and the field flags after that positional are still its.
#[test]
fn field_flags_after_a_positional_are_taken_where_the_program_words_need_dashes() {
    let parsed: Parsed<Cli> = try_run(
        &Example,
        args(&[
            "lnk-example",
            "job",
            "here",
            "--model.thinking",
            "high",
            "--",
            "sh",
            "--model.thinking",
        ]),
        |c| c,
    )
    .unwrap();
    assert_eq!(
        parsed.fields.table()["model"]["thinking"].as_str(),
        Some("high")
    );
    let Command::Job { place, command } = parsed.command.command else {
        panic!("job")
    };
    assert_eq!(place.as_deref(), Some("here"));
    assert_eq!(command, ["sh", "--model.thinking"]);
}

#[test]
fn a_field_flag_on_a_command_binding_no_spec_is_a_usage_error() {
    let refused = try_run::<Cli>(
        &Example,
        args(&["lnk-example", "stop", "--spec.name", "x"]),
        |c| c,
    )
    .err()
    .unwrap();
    assert!(
        refused.to_string().contains("takes no field flags"),
        "{refused}"
    );
}

#[test]
fn a_retired_flag_is_answered_with_its_replacement() {
    let refused = try_run::<Cli>(
        &Example,
        args(&["lnk-example", "start", "main", "--model", "x"]),
        |c| c,
    )
    .err()
    .unwrap();
    assert!(
        refused.to_string().contains("Use lnk start --model.name"),
        "{refused}"
    );
    // A value that spells it, or anything after --, is never matched.
    let value = try_run::<Cli>(
        &Example,
        args(&["lnk-example", "start", "--", "--model"]),
        |c| c,
    );
    assert!(value.is_ok());
}

#[test]
fn json_is_chosen_from_the_parsed_command() {
    let parsed = Cli::try_parse_from(["x", "start", "a", "--json"]).unwrap();
    let _ = parsed;
    let matches = <Cli as clap::CommandFactory>::command()
        .try_get_matches_from(["x", "start", "a", "--json"])
        .unwrap();
    assert!(asks_json(&matches));
    let matches = <Cli as clap::CommandFactory>::command()
        .try_get_matches_from(["x", "start", "--", "--json"])
        .unwrap();
    assert!(!asks_json(&matches));
}

/// `--stream-json` asks for JSON lines, its argument named as its
/// declaration names it.
#[test]
fn stream_json_is_json() {
    let command = clap::Command::new("x").subcommand(
        clap::Command::new("send").arg(
            clap::Arg::new("stream-json")
                .long("stream-json")
                .action(clap::ArgAction::SetTrue),
        ),
    );
    let matches = command
        .clone()
        .try_get_matches_from(["x", "send", "--stream-json"])
        .unwrap();
    assert!(asks_json(&matches));
    let matches = command.try_get_matches_from(["x", "send"]).unwrap();
    assert!(!asks_json(&matches));
}

#[test]
fn a_retired_flag_clap_would_take_is_refused_before_it_parses() {
    // `run` takes the rest of its line, so clap would take --network.
    let refused = try_run::<Cli>(
        &Example,
        args(&["lnk-example", "run", "--network", "ls"]),
        |c| c,
    )
    .err()
    .unwrap();
    assert!(
        refused.to_string().contains("run --network is gone"),
        "{refused}"
    );
    // After the program's own name, its words are its own.
    assert!(
        try_run::<Cli>(
            &Example,
            args(&["lnk-example", "run", "ls", "--network"]),
            |c| c
        )
        .is_ok()
    );
}

#[test]
fn a_trailing_commands_own_dotted_flags_stay_its_programs() {
    // `run` takes the rest of its line: curl's --tlsv1.2 is curl's.
    let parsed = try_run::<Cli>(
        &Example,
        args(&[
            "lnk-example",
            "run",
            "curl",
            "--tlsv1.2",
            "https://example.com",
        ]),
        |c| c,
    )
    .unwrap();
    assert!(parsed.fields.is_empty());
    assert!(matches!(parsed.command.command, Command::Run { ref command } if command.len() == 3));
}

/// A global flag between a command's words, with its value, doesn't end
/// them, so the binding is the command's: `agent -a main start`.
#[test]
fn a_global_flag_between_words_keeps_the_command_bound() {
    #[derive(Parser, Debug)]
    struct Grouped {
        #[arg(short = 'a', long = "agent", global = true)]
        agent: Option<String>,
        #[command(subcommand)]
        command: Command,
    }
    let command = <Grouped as clap::CommandFactory>::command();
    for line in [
        &["lnk", "-a", "main", "start", "x"][..],
        &["lnk", "--agent", "main", "start"],
        &["lnk", "--agent=main", "start"],
        &["lnk", "-amain", "start"],
        &["lnk", "-a=main", "start"],
    ] {
        assert_eq!(leading_words(&command, &args(line)), ["start"], "{line:?}");
    }
    assert!(leading_words(&command, &args(&["lnk", "--json", "start"])).is_empty());
}

/// A bound command's help lists its field flags after its own; one
/// binding none lists none.
#[test]
fn a_bound_commands_help_lists_its_field_flags() {
    let said = try_run::<Cli>(&Example, args(&["lnk-example", "start", "--help"]), |c| c)
        .err()
        .unwrap()
        .to_string();
    assert!(said.contains("--model.thinking"), "{said}");
    assert!(said.contains("Spec fields"), "{said}");
    let said = try_run::<Cli>(&Example, args(&["lnk-example", "stop", "--help"]), |c| c)
        .err()
        .unwrap()
        .to_string();
    assert!(!said.contains("Spec fields"), "{said}");
}

#[test]
fn external_commands_keep_the_targets_flags_after_outer_global_flags() {
    #[derive(clap::Parser)]
    struct Outer {
        #[command(flatten)]
        output: crate::shared::Output,
        #[command(subcommand)]
        target: Target,
    }
    #[derive(clap::Subcommand)]
    enum Target {
        #[command(external_subcommand)]
        External(Vec<OsString>),
    }
    for line in [
        vec![
            "lnk",
            "module",
            "start",
            "--model.name",
            "chosen",
            "--",
            "--json",
        ],
        vec![
            "lnk",
            "--json",
            "module",
            "start",
            "--model.name=chosen",
            "--",
            "--json",
        ],
    ] {
        let parsed = try_run::<Outer>(&super::Bare, args(&line), |command| command).unwrap();
        assert!(parsed.fields.is_empty());
        let Target::External(words) = parsed.command.target;
        assert_eq!(words.first().unwrap(), "module");
        assert_eq!(words.last().unwrap(), "--json");
        assert!(
            words
                .iter()
                .any(|word| word.to_string_lossy().starts_with("--model.name"))
        );
    }
}

#[test]
fn a_renamed_module_never_runs_through_an_external_command() {
    #[derive(clap::Parser)]
    struct Outer {
        #[command(subcommand)]
        command: Target,
    }
    #[derive(clap::Subcommand)]
    enum Target {
        #[command(external_subcommand)]
        External(Vec<OsString>),
    }
    struct Renamed;
    impl Program for Renamed {
        fn before(&self, _: &[OsString]) {}
        fn styled(&self, command: clap::Command) -> clap::Command {
            command
        }
        fn helped(&self, command: clap::Command) -> clap::Command {
            command
        }
        fn history(&self) -> History {
            History::parse(
                r#"[[renamed]]
module = "old"
now = "current"
since = "0.1.0"
until = "99.0.0"
"#,
            )
            .unwrap()
        }
    }
    let error = try_run::<Outer>(&Renamed, args(&["lnk", "old"]), |command| command)
        .err()
        .unwrap();
    assert!(error.to_string().contains("named current"), "{error}");
    let parsed = try_run::<Outer>(&Renamed, args(&["lnk", "current"]), |command| command).unwrap();
    let Target::External(words) = parsed.command.command;
    assert_eq!(words, [OsString::from("current")]);
}

fn history_of(text: &str) -> History {
    History::parse(text).unwrap()
}

/// A program whose retired `LNK_OLD_MODEL` is set where it runs.
struct WithOldVariable;

impl Program for WithOldVariable {
    fn before(&self, _: &[OsString]) {}
    fn styled(&self, command: clap::Command) -> clap::Command {
        command
    }
    fn helped(&self, command: clap::Command) -> clap::Command {
        command
    }
    fn history(&self) -> History {
        history_of(
            "[[gone]]\nwas = \"LNK_OLD_MODEL\"\nnow = \"lnk start --model.name\"\nsince = \"0.1.0\"\nuntil = \"99.0.0\"\n",
        )
    }
    fn variables_set<'h>(&self, history: &'h History) -> Vec<&'h crate::history::Gone> {
        history.gone.iter().filter(|g| g.variable()).collect()
    }
}

#[test]
fn a_retired_variable_that_is_set_is_a_warning_never_a_refusal() {
    // The command runs, its variable set.
    let parsed = try_run::<Cli>(&WithOldVariable, args(&["lnk-example", "stop"]), |c| c).unwrap();
    assert_eq!(parsed.words, ["stop"]);
    let history = WithOldVariable.history();
    let warnings = variable_warnings(WithOldVariable.variables_set(&history).into_iter());
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].kind, link_format::Kind::Warning);
    assert_eq!(warnings[0].exit, link_format::Exit::Success);
}

#[test]
fn a_retired_flag_is_a_usage_report_with_its_replacement_next() {
    let refused = try_run::<Cli>(
        &Example,
        args(&["lnk-example", "start", "main", "--model", "x"]),
        |c| c,
    )
    .err()
    .unwrap();
    let Refused::Retired(report) = refused else {
        panic!("a retired flag is a report, not a usage error: {refused}");
    };
    assert_eq!(report.exit, link_format::Exit::Usage);
    let next: Vec<String> = report.next.0.iter().map(|n| words(&n.command)).collect();
    assert_eq!(next, ["lnk start --model.name"], "{report:?}");
}

struct Renaming;

impl Program for Renaming {
    fn before(&self, _: &[OsString]) {}
    fn styled(&self, command: clap::Command) -> clap::Command {
        command
    }
    fn helped(&self, command: clap::Command) -> clap::Command {
        command
    }
    fn history(&self) -> History {
        history_of(
            "[[renamed]]\nmodule = \"memory\"\nnow = \"notes\"\nsince = \"0.1.0\"\nuntil = \"99.0.0\"\n[[renamed]]\nmodule = \"notes\"\nnow = \"stop\"\nsince = \"0.2.0\"\nuntil = \"99.0.0\"\n",
        )
    }
}

#[test]
fn a_renamed_module_is_answered_with_its_name_today_through_every_rename() {
    let refused = try_run::<Cli>(&Renaming, args(&["lnk", "memory"]), |c| c)
        .err()
        .unwrap();
    let Refused::Retired(report) = refused else {
        panic!("a renamed module is a report: {refused}");
    };
    let said = refused_text(&report);
    assert!(said.contains("named stop"), "{said}");
    let next: Vec<String> = report.next.0.iter().map(|n| words(&n.command)).collect();
    assert_eq!(next, ["lnk stop"]);
}

#[test]
fn a_renamed_modules_next_step_is_the_command_given_with_its_name_today() {
    for (given, again) in [
        (
            &["lnk", "memory", "list", "--", "memory"][..],
            "lnk stop list -- memory",
        ),
        (
            &["lnk", "module", "add", "memory"][..],
            "lnk module add stop",
        ),
    ] {
        let Err(Refused::Retired(report)) = try_run::<Cli>(&Renaming, args(given), |c| c) else {
            panic!("{given:?} is a renamed module's report");
        };
        let next: Vec<String> = report.next.0.iter().map(|n| words(&n.command)).collect();
        assert_eq!(next, [again]);
    }
}

struct Removing;

impl Program for Removing {
    fn before(&self, _: &[OsString]) {}
    fn styled(&self, command: clap::Command) -> clap::Command {
        command
    }
    fn helped(&self, command: clap::Command) -> clap::Command {
        command
    }
    fn history(&self) -> History {
        history_of(
            "[[removed]]\nmodule = \"memory\"\nwhy = \"Use lnk stop.\"\nsince = \"0.2.0\"\nuntil = \"99.0.0\"\n",
        )
    }
}

/// A removed module's name is a usage report with why, never run.
#[test]
fn a_removed_module_is_answered_by_name_with_why() {
    for given in [
        &["lnk", "memory", "list"][..],
        &["lnk", "module", "add", "memory"],
    ] {
        let Err(Refused::Retired(report)) = try_run::<Cli>(&Removing, args(given), |c| c) else {
            panic!("{given:?} is a removed module's report");
        };
        assert_eq!(report.exit, link_format::Exit::Usage);
        let said = refused_text(&report);
        assert!(
            said.contains("memory module is removed since 0.2.0"),
            "{said}"
        );
        assert!(said.contains("Use lnk stop."), "{said}");
    }
    assert!(try_run::<Cli>(&Removing, args(&["lnk-example", "stop"]), |c| c).is_ok());
}

#[test]
fn a_declared_replacements_placeholder_stays_one() {
    let history = history_of(
        "[[gone]]\nwas = \"start --model\"\nnow = \"lnk start --variables.<name>\"\nsince = \"0.1.0\"\nuntil = \"99.0.0\"\n",
    );
    let report = retired_report(
        &history,
        &surface(&<Cli as clap::CommandFactory>::command()),
        &args(&["lnk-example", "start", "main", "--model", "x"]),
    )
    .unwrap();
    let shown = report.next.0[0].command.text();
    assert_eq!(shown, "lnk start --variables.<name>");
}

fn refused_text(report: &link_format::Report) -> String {
    Refused::Retired(Box::new(report.clone())).to_string()
}

fn words(command: &link_format::Command) -> String {
    command
        .words()
        .iter()
        .map(|w| match w {
            link_format::Word::Fixed(w)
            | link_format::Word::Placeholder(w)
            | link_format::Word::Value(w) => w.as_str(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A program whose `start` binds a kind one of whose fields a step
/// renamed.
struct Stepped;

impl Program for Stepped {
    fn before(&self, _: &[OsString]) {}
    fn styled(&self, command: clap::Command) -> clap::Command {
        command
    }
    fn helped(&self, command: clap::Command) -> clap::Command {
        command
    }
    fn history(&self) -> History {
        History::default()
    }
    fn binding(&self, words: &[String]) -> Option<Binding> {
        let format = crate::schema::Format::parse(
            "schema = 1\nkind = 'k'\ncurrent = 2\ncommand = 'lnk k spec show --spec'\n\
             [[steps]]\nfrom = 1\nsince = '0.1.0'\nuntil = '99.0.0'\nrename = { 'model.name' = 'model.thinking' }\n",
        )
        .unwrap();
        (words == ["start"]).then(|| Binding {
            schema: crate::schema::<crate::fields::tests::Agent>(),
            format,
        })
    }
}

#[test]
fn a_field_flag_a_step_retired_is_a_report_with_the_flag_that_replaced_it() {
    let refused = try_run::<Cli>(
        &Stepped,
        args(&["lnk-example", "start", "main", "--model.name", "high"]),
        |c| c,
    )
    .err()
    .unwrap();
    let Refused::Retired(report) = refused else {
        panic!("a step's retired field flag is a report, not a usage error: {refused}");
    };
    assert_eq!(report.exit, link_format::Exit::Usage);
    assert!(refused_text(&report).contains("Use --model.thinking"));
    let next: Vec<String> = report.next.0.iter().map(|n| n.command.text()).collect();
    assert_eq!(next, ["lnk start --model.thinking <value>"]);
}

#[derive(Parser, Debug)]
struct Agent {
    #[arg(short, long, global = true)]
    agent: Option<String>,
    #[command(subcommand)]
    command: AgentCommand,
}

#[derive(Subcommand, Debug)]
enum AgentCommand {
    Permission {
        #[command(subcommand)]
        command: PermissionCommand,
    },
}

#[derive(Subcommand, Debug)]
enum PermissionCommand {
    Add,
}

/// `lnk-agent`, whose `allow` became `permission add`.
struct Namespaced;

impl Program for Namespaced {
    fn before(&self, _: &[OsString]) {}
    fn styled(&self, command: clap::Command) -> clap::Command {
        command
    }
    fn helped(&self, command: clap::Command) -> clap::Command {
        command
    }
    fn history(&self) -> History {
        history_of(
            "[[gone]]\nwas = \"agent allow --read\"\nnow = \"lnk agent permission add --files.read\"\nsince = \"0.1.0\"\nuntil = \"99.0.0\"\n[[gone]]\nwas = \"agent allow\"\nnow = \"lnk agent permission add\"\nsince = \"0.1.0\"\nuntil = \"99.0.0\"\n",
        )
    }
    fn namespace(&self) -> Option<&str> {
        Some("agent")
    }
}

#[test]
fn a_retired_command_is_found_by_its_words_whatever_flags_it_carries() {
    for line in [
        &["lnk-agent", "allow", "--files.read", "~"][..],
        &["lnk-agent", "-a", "x", "allow"][..],
        &["lnk-agent", "--agent=x", "allow", "--files.read", "~"][..],
        &["lnk-agent", "-ax", "allow"][..],
    ] {
        let refused = try_run::<Agent>(&Namespaced, args(line), |c| c)
            .err()
            .unwrap();
        let Refused::Retired(report) = refused else {
            panic!("{line:?} is its history's: {refused}");
        };
        let next: Vec<String> = report.next.0.iter().map(|n| words(&n.command)).collect();
        assert_eq!(next, ["lnk agent permission add"], "{line:?}");
    }
    // Its retired flag still finds its own entry, after a global flag.
    let refused = try_run::<Agent>(
        &Namespaced,
        args(&["lnk-agent", "-a", "x", "allow", "--read", "~"]),
        |c| c,
    )
    .err()
    .unwrap();
    let Refused::Retired(report) = refused else {
        panic!("a retired flag is its history's: {refused}");
    };
    let next: Vec<String> = report.next.0.iter().map(|n| words(&n.command)).collect();
    assert_eq!(next, ["lnk agent permission add --files.read"]);
}

#[derive(Parser, Debug)]
struct Team {
    #[arg(short, long, global = true)]
    agent: Option<String>,
    #[command(subcommand)]
    command: TeamCommand,
}

#[derive(Subcommand, Debug)]
enum TeamCommand {
    Add {
        name: String,
        /// The stand-in of the global this command leaves out.
        #[arg(long = "agent", id = "agent", hide = true)]
        left_out: Option<String>,
        #[arg(long)]
        member: Option<String>,
    },
}

/// `lnk-agent`, whose `team add --agent` became `--member`.
struct Retiring;

impl Program for Retiring {
    fn before(&self, _: &[OsString]) {}
    fn styled(&self, command: clap::Command) -> clap::Command {
        command
    }
    fn helped(&self, command: clap::Command) -> clap::Command {
        command
    }
    fn history(&self) -> History {
        history_of(
            "[[gone]]\nwas = \"agent add --agent\"\nnow = \"lnk agent add --member\"\nsince = \"0.1.0\"\nuntil = \"99.0.0\"\n",
        )
    }
    fn namespace(&self) -> Option<&str> {
        Some("agent")
    }
}

#[test]
fn a_global_a_command_leaves_out_is_no_flag_of_its_surface_and_its_history_answers() {
    use clap::CommandFactory;
    let flags = &surface(&Team::command())["add"];
    assert!(!flags.contains_key("--agent"), "{flags:?}");
    assert!(flags.contains_key("--member"));
    let refused = try_run::<Team>(
        &Retiring,
        args(&["lnk-agent", "add", "t", "--agent", "x"]),
        |c| c,
    )
    .err()
    .unwrap();
    let Refused::Retired(report) = refused else {
        panic!("a retired flag its stand-in catches is its history's: {refused}");
    };
    let next: Vec<String> = report.next.0.iter().map(|n| words(&n.command)).collect();
    assert_eq!(next, ["lnk agent add --member"]);
}

#[derive(Parser, Debug)]
struct Boxes {
    #[command(subcommand)]
    command: BoxCommand,
}

#[derive(Subcommand, Debug)]
enum BoxCommand {
    Create {
        #[arg(long)]
        cloud: Option<String>,
        #[arg(long)]
        bundle: Option<String>,
    },
}

/// `lnk-box`, which `lnk` runs for `lnk box`.
struct BoxProgram;

impl Program for BoxProgram {
    fn before(&self, _: &[OsString]) {}
    fn styled(&self, command: clap::Command) -> clap::Command {
        command
    }
    fn helped(&self, command: clap::Command) -> clap::Command {
        command
    }
    fn history(&self) -> History {
        History::default()
    }
    fn namespace(&self) -> Option<&str> {
        Some("box")
    }
}

#[test]
fn a_replay_typed_for_another_lnk_names_the_programs_namespace_first() {
    let line = ["lnk-box", "create", "--cloud", "aws", "--bundle", "devbox"];
    let parsed = try_run::<Boxes>(&BoxProgram, args(&line), |c| c).unwrap();
    let typed: Vec<OsString> = ["box", "create", "--cloud", "aws"]
        .map(OsString::from)
        .to_vec();
    assert_eq!(parsed.replay.typed_without("bundle").unwrap(), typed);
    // The program's own replay holds only what it was given.
    assert_eq!(parsed.replay.without("bundle").unwrap(), typed[1..]);
}

thread_local! {
    /// How many times this test's thread made [`Counted`]'s command tree.
    static TREES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// [`Cli`], counting each making of its command tree, which a module's
/// is made from its declarations, hundreds of commands.
#[derive(Debug)]
struct Counted(Cli);

impl clap::CommandFactory for Counted {
    fn command() -> clap::Command {
        TREES.with(|trees| trees.set(trees.get() + 1));
        <Cli as clap::CommandFactory>::command()
    }
    fn command_for_update() -> clap::Command {
        Self::command()
    }
}

impl clap::FromArgMatches for Counted {
    fn from_arg_matches(matches: &clap::ArgMatches) -> Result<Self, clap::Error> {
        <Cli as clap::FromArgMatches>::from_arg_matches(matches).map(Counted)
    }
    fn update_from_arg_matches(&mut self, matches: &clap::ArgMatches) -> Result<(), clap::Error> {
        clap::FromArgMatches::update_from_arg_matches(&mut self.0, matches)
    }
}

impl clap::Parser for Counted {}

/// A command line is read with one command tree, made once: its field
/// flags, its history, its parse and its replay all read that one.
#[test]
fn a_command_line_makes_its_command_tree_once() {
    let before = TREES.with(std::cell::Cell::get);
    let parsed = try_run::<Counted>(
        &Example,
        args(&["lnk-example", "start", "main", "--model.thinking", "high"]),
        |c| c,
    )
    .unwrap();
    assert!(matches!(parsed.command.0.command, Command::Start { ref name, .. } if name == "main"));
    assert_eq!(TREES.with(std::cell::Cell::get) - before, 1);
}
