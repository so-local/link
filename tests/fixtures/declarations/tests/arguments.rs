//! The worked example's runtime arguments: each kind read from the
//! command line as declared, refused as declared, and written back by
//! `argv()` as its program's parser reads it.

link_test_kit::program!(unit);

mod support;

use link_declarations_fixture::module::{self, Command};
use serde_json::json;
use support::{parse, refused};

fn probe(words: &[&str]) -> Result<module::EnvProbeArgs, String> {
    let line: Vec<&str> = ["env", "probe"].iter().chain(words).copied().collect();
    match parse(&line)?.command.command {
        Command::EnvProbe(args) => Ok(args),
        _ => panic!("env probe"),
    }
}

#[test]
fn every_kind_of_argument_reads_as_declared() {
    let args = probe(&[
        "-v",
        "--count",
        "3",
        "--ratio",
        "0.5",
        "--tag",
        "a",
        "--tag",
        "b",
        "--ports",
        "80",
        "--ports",
        "8080",
        "--size",
        "large",
        "--owner",
        "user:k3m4n5p6q7r8",
        "--",
        "make",
        "-j",
    ])
    .unwrap();
    assert!(args.verbose);
    assert_eq!((args.count, args.ratio), (Some(3), Some(0.5)));
    assert_eq!(args.tag, ["a", "b"]);
    assert_eq!(args.ports, [module::Port(80), module::Port(8080)]);
    assert_eq!(args.size, Some(module::Size::Large));
    assert_eq!(args.owner, Some(module::UserId("user:k3m4n5p6q7r8".into())));
    assert_eq!(args.command, ["make", "-j"], "the rest, unparsed");
    let empty = probe(&[]).unwrap();
    assert!(!empty.verbose && empty.tag.is_empty() && empty.command.is_empty());
}

#[test]
fn each_argument_refuses_what_its_declaration_refuses() {
    for (words, why) in [
        (
            &["--count", "9"][..],
            "'--count <count>': 9 is not in 1..=5",
        ),
        (&["--count", "x"], "invalid value"),
        (&["--ports", "70000"], "70000 is not in 1..=65535"),
        (&["--size", "huge"], "huge"),
        (
            &["--owner", "box:k3m4n5p6q7r8"],
            "--owner: \"box:k3m4n5p6q7r8\" isn't a user ID",
        ),
        (&["--count", "4"], "four is refused by its hook"),
    ] {
        let refused = refused(
            &["env", "probe"]
                .iter()
                .chain(words)
                .copied()
                .collect::<Vec<_>>(),
        );
        assert!(refused.contains(why), "{words:?}: {refused}");
    }
}

#[test]
fn a_fallback_variable_and_a_short_flag_are_wired_as_declared() {
    let command = <module::Cli as clap::CommandFactory>::command();
    assert_eq!(
        command.get_name(),
        "lnk-env",
        "the program is the module crate's"
    );
    let probe = command.find_subcommand("probe").unwrap();
    let arg = |id: &str| probe.get_arguments().find(|a| a.get_id() == id).unwrap();
    assert_eq!(
        arg("region").get_env().and_then(|e| e.to_str()),
        Some("LNK_EXAMPLE_REGION")
    );
    assert_eq!(arg("verbose").get_short(), Some('v'));
}

#[test]
fn an_argument_every_action_takes_is_read_before_or_after_its_command() {
    for words in [
        &["env", "create", "aws", "-a", "work", "--spec.name", "web"][..],
        &[
            "env",
            "--agent",
            "work",
            "create",
            "aws",
            "--spec.name",
            "web",
        ],
    ] {
        let Command::EnvCreate(args) = parse(words).unwrap().command.command else {
            panic!("{words:?}: env create")
        };
        assert_eq!(args.agent.as_deref(), Some("work"), "{words:?}");
    }
    let Command::EnvProbe(args) = parse(&["env", "probe"]).unwrap().command.command else {
        panic!("env probe")
    };
    assert_eq!(args.agent, None, "every action has it, given or not");
}

#[test]
fn an_optional_value_a_conflict_and_a_hidden_argument_are_wired_as_declared() {
    let sandbox = |words: &[&str]| match parse(words).unwrap().command.command {
        Command::EnvProbe(args) => args.sandbox,
        _ => panic!("env probe"),
    };
    assert_eq!(sandbox(&["env", "probe"]), None);
    assert_eq!(
        sandbox(&["env", "probe", "--sandbox"]),
        Some("sandbox.toml".into()),
        "the flag alone means its declared default"
    );
    assert_eq!(
        sandbox(&["env", "probe", "--sandbox", "mine.toml"]),
        Some("mine.toml".into())
    );
    let why = refused(&["env", "probe", "--sandbox", "--silent"]);
    assert!(why.contains("cannot be used with"), "{why}");
    let command = <module::Cli as clap::CommandFactory>::command();
    let probe = command.find_subcommand("probe").unwrap();
    let silent = probe
        .get_arguments()
        .find(|a| a.get_id() == "silent")
        .unwrap();
    assert!(
        silent.is_hide_set(),
        "a hidden argument is kept out of help"
    );
}

#[test]
fn argv_writes_the_command_line_its_programs_parser_reads_back() {
    use link_declarations_fixture::args;
    let given = args::EnvProbeArgs {
        agent: Some("work".into()),
        command: vec!["echo".into(), "--loud".into()],
        count: Some(3),
        owner: Some(args::UserId("user:k3m4n5p6q7r8".into())),
        ports: vec![args::Port(80), args::Port(443)],
        ratio: Some(0.5),
        region: Some("eu".into()),
        sandbox: Some("run.toml".into()),
        silent: false,
        size: Some(args::Size::Large),
        tag: vec!["a".into(), "b".into()],
        verbose: true,
        upstream: "proxy".into(),
        lines: 10,
        prefix: String::new(),
    };
    let argv = given.argv();
    assert_eq!(&argv[..1], ["probe"], "the program's words come first");
    let words: Vec<&str> = std::iter::once("env")
        .chain(argv.iter().map(String::as_str))
        .collect();
    let Command::EnvProbe(read) = parse(&words).unwrap().command.command else {
        panic!("{argv:?}: env probe")
    };
    assert_eq!(
        serde_json::to_value(&read).unwrap(),
        serde_json::to_value(&given).unwrap(),
        "{argv:?} reads back as what wrote it"
    );
    // A positional and a bound action's spec file.
    let add = args::ApprovalAddArgs {
        secret: "token".into(),
        file: "env.toml".into(),
        agent: None,
    };
    assert_eq!(
        add.argv(),
        ["approval", "add", "token", "--file", "env.toml"]
    );
    let create = args::EnvCreateArgs {
        cloud: "aws".into(),
        agent: None,
        yes: true,
        json: false,
        quiet: false,
        spec: Some("env.toml".into()),
    };
    assert_eq!(
        create.argv(),
        ["create", "aws", "--yes", "--spec", "env.toml"]
    );
}

#[test]
fn a_rest_after_a_positional_list_is_taken_only_after_the_separator() {
    let Command::EnvPush(args) = parse(&["env", "push", "a", "b", "--", "--fast", "-v"])
        .unwrap()
        .command
        .command
    else {
        panic!("env push")
    };
    assert_eq!(args.files, ["a", "b"]);
    assert_eq!(args.args, ["--fast", "-v"]);
    let Command::EnvPush(args) = parse(&["env", "push", "a"]).unwrap().command.command else {
        panic!("env push")
    };
    assert!(args.args.is_empty());
}

#[test]
fn a_runtime_argument_left_out_is_its_declared_default() {
    let Command::EnvProbe(args) = parse(&["env", "probe"]).unwrap().command.command else {
        panic!("env probe")
    };
    assert_eq!(
        (args.upstream.as_str(), args.lines, args.prefix.as_str()),
        ("direct", 40, ""),
        "each default, the empty one among them"
    );
    let Command::EnvProbe(args) = parse(&["env", "probe", "--upstream", "proxy", "--lines", "3"])
        .unwrap()
        .command
        .command
    else {
        panic!("env probe")
    };
    assert_eq!((args.upstream.as_str(), args.lines), ("proxy", 3));
    // Every surface reads the same default.
    let read: module::EnvProbeArgs = serde_json::from_value(json!({})).unwrap();
    assert_eq!((read.upstream.as_str(), read.lines), ("direct", 40));
    let why = refused(&["env", "probe", "--lines", "0"]);
    assert!(why.contains("--lines"), "{why}");
}

#[test]
fn a_shared_group_is_taken_whole_its_positional_after_the_actions_own() {
    let Command::EnvSetup(args) = parse(&[
        "env",
        "setup",
        "aws",
        "--zone",
        "eu-1",
        "--request",
        "r.json",
    ])
    .unwrap()
    .command
    .command
    else {
        panic!("env setup")
    };
    assert_eq!(
        (
            args.place.as_deref(),
            args.zone.as_deref(),
            args.request.as_deref()
        ),
        (Some("aws"), Some("eu-1"), Some("r.json"))
    );
    let Command::EnvSetup(args) = parse(&["env", "setup", "--here"]).unwrap().command.command
    else {
        panic!("env setup")
    };
    assert!(args.here && args.place.is_none() && args.on.is_none());
    let Command::EnvCreate(args) = parse(&["env", "create", "aws", "--quiet", "--spec.name", "w"])
        .unwrap()
        .command
        .command
    else {
        panic!("env create")
    };
    assert!(args.quiet);
}

#[test]
fn an_argument_every_action_takes_is_read_from_any_command() {
    let given = |words: &[&str]| parse(words).unwrap().command.command;
    assert_eq!(given(&["env", "-a", "work", "probe"]).agent(), Some("work"));
    assert_eq!(
        given(&[
            "env", "approval", "add", "t", "--file", "f", "--agent", "main"
        ])
        .agent(),
        Some("main")
    );
    assert_eq!(given(&["env", "probe"]).agent(), None);
    assert_eq!(
        given(&["env", "spec", "show"]).agent(),
        None,
        "core's own commands take none"
    );
}

#[test]
fn a_whole_number_is_its_bounds_width_and_a_table_declaring_its_default_is_never_optional() {
    let widths: module::Widths = serde_json::from_value(json!({
        "small": 255, "wide": 4_294_967_295_u64, "counted": 7, "offset": -5, "free": -1,
    }))
    .unwrap();
    let small: Option<u8> = widths.small;
    let wide: Option<u32> = widths.wide;
    let counted: Option<u64> = widths.counted;
    let offset: Option<i8> = widths.offset;
    let free: Option<i64> = widths.free;
    assert_eq!(
        (small, wide, counted, offset, free),
        (Some(255), Some(4_294_967_295), Some(7), Some(-5), Some(-1))
    );
    // Left out, a table declaring its default is its fields' defaults, and
    // one declaring none is absent: a left-out section isn't an empty one.
    let module::Isolation { network, files } = widths.isolation;
    assert!(network && files.is_none());
    assert!(widths.network.is_none());
    assert!(serde_json::from_value::<module::Widths>(json!({"small": 256})).is_err());
    let _: u16 = module::Port(80).0;
}

#[test]
fn a_rest_taken_last_refuses_a_mistyped_flag_before_the_separator() {
    let Command::EnvWrap(args) = parse(&["env", "wrap", "--quick", "--", "sh", "--bogus"])
        .unwrap()
        .command
        .command
    else {
        panic!("env wrap")
    };
    assert!(args.quick);
    assert_eq!(args.program, ["sh", "--bogus"]);
    let why = refused(&["env", "wrap", "--bogus", "sh"]);
    assert!(why.contains("--bogus"), "a mistyped flag is refused: {why}");
}

#[test]
fn a_group_of_the_modules_arguments_is_one_type_in_each_action() {
    let Command::EnvWrap(wrap) =
        parse(&["env", "wrap", "--times", "3", "--pause", "5s", "--", "sh"])
            .unwrap()
            .command
            .command
    else {
        panic!("env wrap")
    };
    let Command::EnvPush(push) = parse(&["env", "push", "a", "--times", "2"])
        .unwrap()
        .command
        .command
    else {
        panic!("env push")
    };
    // One type, the same in both.
    let timings: [&module::Timing; 2] = [&wrap.timing, &push.timing];
    assert_eq!((timings[0].times, timings[1].times), (Some(3), Some(2)));
    assert_eq!(wrap.program, ["sh"]);
    // Checked as its declaration says, naming the flag.
    let why = refused(&["env", "push", "a", "--times", "12"]);
    assert!(why.contains("'--times <times>'"), "{why}");
    // An unknown key is still refused, by the group's own type.
    assert!(
        serde_json::from_value::<module::EnvPushArgs>(json!({"files": ["a"], "colour": 1}))
            .is_err()
    );
    // Its command line written back, flat.
    let written = link_declarations_fixture::args::EnvPushArgs {
        files: vec!["a".into()],
        args: Vec::new(),
        agent: None,
        timing: link_declarations_fixture::args::Timing {
            times: Some(2),
            pause: None,
        },
    };
    assert_eq!(written.argv(), ["push", "a", "--times", "2"]);
}

/// A group's argument with a declared default stays unset when it's
/// left out, so a join sees only what's given; its accessor gives the
/// default, and its help tells it without the parser setting it.
#[test]
fn a_groups_default_is_read_through_its_accessor_never_set() {
    use link_declarations_fixture::args::Timing;
    let Command::EnvPush(push) = parse(&["env", "push", "a"]).unwrap().command.command else {
        panic!("env push")
    };
    assert_eq!(push.timing.times, None);
    assert_eq!(push.timing.times(), 1);
    let given = Timing {
        times: Some(4),
        pause: None,
    };
    assert_eq!(given.times(), 4);
    let mut help = Vec::new();
    <module::Cli as clap::CommandFactory>::command()
        .find_subcommand_mut("push")
        .expect("env push")
        .write_long_help(&mut help)
        .unwrap();
    let help = String::from_utf8(help).unwrap();
    assert!(
        help.contains("How many times it runs [default: 1]"),
        "{help}"
    );
}

/// A program over its contract's arguments reads its lines into the
/// contract's own types, and a line the contract's `argv` writes reads
/// back through the contract's `from_argv`.
#[test]
fn a_program_over_its_contracts_arguments_reads_into_the_contracts_types() {
    use clap::Parser;
    use link_declarations_fixture::{args, over};
    let read = over::Cli::try_parse_from(["lnk-env", "push", "a", "--times", "2"])
        .unwrap()
        .command;
    let over::Command::EnvPush(push) = read else {
        panic!("env push")
    };
    // One type: the contract's.
    let push: args::EnvPushArgs = push;
    assert_eq!(push.timing.times, Some(2));
    let line = push.argv();
    assert_eq!(args::EnvPushArgs::from_argv(&line).unwrap(), push);
    assert!(args::EnvPushArgs::from_argv(&["wrap".to_string()]).is_err());
}

#[test]
fn an_action_leaving_out_a_module_wide_argument_has_no_such_flag() {
    use clap::CommandFactory;
    // Its command neither shows nor takes it, before or after its words.
    for line in [
        &["env", "status", "-a", "work"][..],
        &["env", "-a", "work", "status"][..],
        &["env", "approval", "show", "t", "--agent", "work"][..],
        &["env", "approval", "-a", "work", "show", "t"][..],
        &["env", "--agent=work", "approval", "show", "t"][..],
    ] {
        // Refused, however the parser says so.
        refused(line);
    }
    assert!(
        refused(&["env", "-a", "work", "status"])
            .contains("--agent isn't one of this command's arguments")
    );
    let mut env = module::Cli::command();
    let help = env
        .find_subcommand_mut("status")
        .unwrap()
        .render_long_help()
        .to_string();
    assert!(!help.contains("--agent"), "{help}");
    // Given nothing, it reads, and answers no agent.
    let status = parse(&["env", "status"]).unwrap().command.command;
    assert!(matches!(status, Command::EnvStatus(_)));
    assert_eq!(status.agent(), None);
    // The others still take it anywhere.
    assert_eq!(
        parse(&["env", "-a", "work", "probe"])
            .unwrap()
            .command
            .command
            .agent(),
        Some("work")
    );
}

#[test]
fn an_argument_requiring_another_is_refused_without_it() {
    // native-only requires here, a member of the shared workplace group.
    assert!(parse(&["env", "setup", "--here", "--native-only"]).is_ok());
    let refused = refused(&["env", "setup", "--native-only"]);
    assert!(refused.contains("--here"), "{refused}");
}
