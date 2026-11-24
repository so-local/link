//! The worked example end to end: its command line read by the parser
//! generated from its declarations, its spec read from a file or its
//! field flags alike, and its actions dispatched with their arguments
//! and spec checked by the generated types.

link_test_kit::program!(unit);

mod support;

use link_actions::call::{Call, Outcome};
use link_declarations_fixture::{
    Handler,
    module::{self, Command, Env},
};
use link_test_kit::temp::TempDir;
use serde_json::json;
use support::{parse, refused};

fn spec_file(dir: &TempDir, text: &str) -> std::path::PathBuf {
    let path = dir.path().join("env.toml");
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn a_command_line_reads_into_its_typed_arguments_and_spec() {
    let dir = TempDir::new("generated");
    let file = spec_file(
        &dir,
        "kind = \"env\"\nschema = 2\nname = \"app\"\nworkdir = \"src\"\n",
    );
    let file_text = file.to_str().unwrap();
    let parsed = parse(&[
        "env",
        "create",
        "aws",
        "--spec",
        file_text,
        "--spec.name",
        "web",
        "--yes",
    ])
    .unwrap();
    assert_eq!(
        parsed.words,
        ["create"],
        "lnk-env never reads its namespace"
    );
    let Command::EnvCreate(args) = parsed.command.command else {
        panic!("env create")
    };
    assert_eq!(args.cloud, "aws");
    assert!(args.yes && !args.json);
    assert_eq!(args.spec.as_deref(), Some(file.as_path()));
    let made = link_spec::read_with::<Env>(args.spec.as_deref(), &parsed.fields).unwrap();
    assert_eq!(made.spec.name, "web", "the field flag over the file's name");
    assert_eq!(
        made.spec.workdir,
        Some(dir.path().join("src")),
        "a path, from the file's folder"
    );
}

#[test]
fn a_file_and_its_flags_are_refused_alike() {
    let dir = TempDir::new("generated");
    let bad = spec_file(&dir, "kind = \"env\"\nschema = 2\nname = \"Web\"\n");
    let from_file = link_spec::read_with::<Env>(Some(&bad), &Default::default())
        .err()
        .map(|e| format!("{e:#}"))
        .unwrap();
    let parsed = parse(&["env", "create", "aws", "--spec.name", "Web"]).unwrap();
    let from_flags = link_spec::read_with::<Env>(None, &parsed.fields)
        .err()
        .map(|e| format!("{e:#}"))
        .unwrap();
    let why = "name: \"Web\" doesn't match ^[a-z0-9-]+$";
    assert!(from_file.contains(why), "{from_file}");
    assert!(from_flags.contains(why), "{from_flags}");
}

#[test]
fn every_field_has_its_flag_and_runtime_flags_have_none() {
    let flags = parse(&[
        "env",
        "create",
        "aws",
        "--spec.name",
        "web",
        "--workdir",
        "src",
        "--variables.LOG",
        "info",
    ])
    .unwrap();
    let env = link_spec::read_with::<Env>(None, &flags.fields)
        .unwrap()
        .spec;
    assert_eq!(env.variables["LOG"], "info");
    let why = refused(&["env", "create", "aws", "--colour", "red"]);
    assert!(why.contains("colour"), "{why}");
    let why = refused(&[
        "env",
        "approval",
        "add",
        "token",
        "--file",
        "env.toml",
        "--spec.name",
        "web",
    ]);
    assert!(why.contains("takes no field flags"), "{why}");
}

#[test]
fn runtime_arguments_are_checked_by_their_declarations() {
    let missing = refused(&["env", "create"]);
    assert!(missing.contains("<cloud>"), "{missing}");
    let parsed = parse(&["env", "approval", "add", "token", "--file", "env.toml"]).unwrap();
    let Command::ApprovalAdd(args) = parsed.command.command else {
        panic!("approval add")
    };
    assert_eq!(
        (args.secret.as_str(), args.file.to_str()),
        ("token", Some("env.toml"))
    );
    let unknown = refused(&["env", "approval", "add", "token", "--file", "f", "--force"]);
    assert!(unknown.contains("--force"), "{unknown}");
}

#[test]
fn spec_show_prints_the_effective_spec() {
    let parsed = parse(&["env", "spec", "show", "--spec.name", "web"]).unwrap();
    let Command::SpecShow(show) = parsed.command.command else {
        panic!("spec show")
    };
    let shown = link_spec::show::<Env>(&show, &parsed.fields, None).unwrap();
    let table: toml::Table = toml::from_str(&shown).unwrap();
    assert_eq!(table["kind"].as_str(), Some("env"));
    assert_eq!(table["name"].as_str(), Some("web"));
}

fn call(
    action: &str,
    args: serde_json::Value,
    spec: serde_json::Value,
) -> Outcome<serde_json::Value> {
    let call = Call {
        action: action.parse().unwrap(),
        args,
        spec,
        caller: "user:k3m4n5p6q7r8".into(),
        resource: None,
    };
    module::dispatch(&Handler::default(), call)
}

#[test]
fn dispatch_checks_a_call_before_its_handler_sees_it() {
    let made = call(
        "env.env.create",
        json!({"cloud": "aws"}),
        json!({"name": "web"}),
    );
    assert_eq!(
        made,
        Outcome::Completed {
            output: json!({"box": "box:k3m4n5p6q7r8", "cloud": "aws for web"})
        }
    );
    for (args, spec, why) in [
        (
            json!({"cloud": "aws"}),
            json!({"name": "Web"}),
            "spec.name: \"Web\" doesn't match",
        ),
        (json!({}), json!({"name": "web"}), "missing field `cloud`"),
        (
            json!({"cloud": "aws"}),
            json!({"name": "web", "colour": "red"}),
            "unknown field `colour`",
        ),
        (
            json!({"cloud": "aws"}),
            json!({"name": "web", "stages": [{"name": "a", "run": "x", "timeout": "soon"}]}),
            "isn't a duration",
        ),
    ] {
        match call("env.env.create", args, spec) {
            Outcome::Refused { reason } => assert!(reason.contains(why), "{reason}"),
            other => panic!("{other:?}"),
        }
    }
    assert!(matches!(
        call("env.env.forgotten", json!({}), json!(null)),
        Outcome::Refused { .. }
    ));
}

#[test]
fn the_spec_kind_reads_its_shape_from_the_catalog() {
    let schema = link_spec::schema::<Env>();
    assert_eq!(
        schema["properties"]["stages"]["items"]["$ref"],
        "#/$defs/env.stage"
    );
    assert!(schema["$defs"]["env.stage"]["properties"]["run"].is_object());
}

#[test]
fn a_schema_step_reads_an_older_file_and_answers_its_old_flag() {
    let dir = TempDir::new("generated");
    let old = spec_file(
        &dir,
        "kind = \"env\"\nschema = 1\nname = \"app\"\ndir = \"src\"\n",
    );
    let made = link_spec::read_with::<Env>(Some(&old), &Default::default()).unwrap();
    assert_eq!(
        made.spec.workdir,
        Some(dir.path().join("src")),
        "dir, renamed workdir"
    );
    let retired = refused(&["env", "create", "aws", "--dir", "src"]);
    assert!(
        retired.contains("--workdir"),
        "the old flag names its replacement: {retired}"
    );
}

#[test]
fn a_choice_of_records_a_default_and_a_typed_id_read_from_a_file() {
    let dir = TempDir::new("generated");
    let file = spec_file(
        &dir,
        "kind = \"env\"\nschema = 2\nname = \"app\"\nowner = \"user:k3m4n5p6q7r8\"\nsize = \"small\"\n\
         [[stages]]\nname = \"serve\"\nrun = \"./serve\"\nready = { kind = \"port\", port = 8080 }\n",
    );
    let env = link_spec::read_with::<Env>(Some(&file), &Default::default())
        .unwrap()
        .spec;
    let stage = &env.stages.as_ref().unwrap()[0];
    assert_eq!(stage.retries, 1, "the declared default");
    assert_eq!(
        stage.ready,
        Some(module::Ready::Port(module::ReadyPort {
            port: module::Port(8080)
        }))
    );
    assert_eq!(env.owner, Some(module::UserId("user:k3m4n5p6q7r8".into())));
    for (stage, why) in [
        (
            "ready = { kind = \"port\", port = 0 }",
            "stages[1].ready.port: 0 is below 1",
        ),
        (
            "ready = { kind = \"run\", run = \"\" }",
            "stages[1].ready.run: is shorter than 1",
        ),
        ("ready = { kind = \"poll\" }", "poll"),
        ("retries = 10", "stages[1].retries: 10 is above 9"),
    ] {
        let bad = spec_file(
            &dir,
            &format!(
                "kind = \"env\"\nschema = 2\nname = \"app\"\n[[stages]]\nname = \"s\"\nrun = \"x\"\n{stage}\n"
            ),
        );
        let refused = link_spec::read_with::<Env>(Some(&bad), &Default::default())
            .err()
            .map(|e| format!("{e:#}"))
            .unwrap();
        assert!(refused.contains(why), "{stage}: {refused}");
    }
}

#[test]
fn a_call_never_names_its_spec_file() {
    match call(
        "env.env.create",
        json!({"cloud": "aws", "spec": "/etc/env.toml"}),
        json!({"name": "web"}),
    ) {
        Outcome::Refused { reason } => {
            assert!(reason.contains("never a file in its arguments"), "{reason}")
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn types_and_a_handler_generate_alone() {
    let stage: link_declarations_fixture::types::Stage =
        serde_json::from_value(json!({"name": "s", "run": "x"})).unwrap();
    assert_eq!(stage.retries, 1);
    let made = link_declarations_fixture::handler::EnvCreateArgs {
        cloud: "aws".into(),
        agent: None,
        yes: false,
        json: false,
        quiet: false,
        spec: None,
    };
    assert!(link_actions::value::Validate::validate(&made, "").is_ok());
}

#[test]
fn a_choice_told_apart_by_its_marking_key_reads_from_a_file() {
    let dir = TempDir::new("generated");
    let file = spec_file(
        &dir,
        "kind = \"env\"\nschema = 2\nname = \"app\"\n\
         [[steps]]\nname = \"build\"\nrun = \"make\"\n\
         [[steps]]\nparallel = [{ name = \"a\", run = \"x\" }, { name = \"b\", run = \"y\" }]\n",
    );
    let env = link_spec::read_with::<Env>(Some(&file), &Default::default())
        .unwrap()
        .spec;
    assert!(
        matches!(&env.steps.as_ref().unwrap()[0], module::Step::Run(stage) if stage.run == "make")
    );
    assert!(
        matches!(&env.steps.as_ref().unwrap()[1], module::Step::Parallel(group) if group.parallel.len() == 2)
    );
    for (step, why) in [
        ("name = \"lost\"", "holds none of run, parallel"),
        ("parallel = []", "steps[2].parallel: lists fewer than 1"),
    ] {
        let bad = spec_file(
            &dir,
            &format!(
                "kind = \"env\"\nschema = 2\nname = \"app\"\n[[steps]]\nname = \"s\"\nrun = \"x\"\n[[steps]]\n{step}\n"
            ),
        );
        let refused = link_spec::read_with::<Env>(Some(&bad), &Default::default())
            .err()
            .map(|e| format!("{e:#}"))
            .unwrap();
        assert!(refused.contains(why), "{step}: {refused}");
    }
}

#[test]
fn a_confined_path_stays_inside_the_files_folder() {
    let dir = TempDir::new("generated");
    let schema = link_spec::schema::<Env>();
    assert_eq!(
        schema["$defs"]["env.stage"]["properties"]["sandbox"]["format"],
        "confined-path"
    );
    let inside = spec_file(
        &dir,
        "kind = \"env\"\nschema = 2\nname = \"app\"\n[[stages]]\nname = \"s\"\nrun = \"x\"\nsandbox = \"build.toml\"\n",
    );
    let env = link_spec::read_with::<Env>(Some(&inside), &Default::default())
        .unwrap()
        .spec;
    assert_eq!(
        env.stages.as_ref().unwrap()[0].sandbox.as_deref(),
        Some(std::path::Path::new("build.toml")),
        "kept as written, inside the folder"
    );
    let outside = spec_file(
        &dir,
        "kind = \"env\"\nschema = 2\nname = \"app\"\n[[stages]]\nname = \"s\"\nrun = \"x\"\nsandbox = \"/etc/build.toml\"\n",
    );
    let refused = link_spec::read_with::<Env>(Some(&outside), &Default::default())
        .err()
        .map(|e| format!("{e:#}"))
        .unwrap();
    assert!(
        refused.contains("is outside the spec's folder"),
        "{refused}"
    );
}

#[test]
fn a_bound_actions_hook_checks_its_spec_on_every_read() {
    let dir = TempDir::new("generated");
    let why = "the name reserved is kept for Link";
    let file = spec_file(&dir, "kind = \"env\"\nschema = 2\nname = \"reserved\"\n");
    let from_file = link_spec::read_with::<Env>(Some(&file), &Default::default())
        .err()
        .map(|e| format!("{e:#}"))
        .unwrap();
    assert!(from_file.contains(why), "{from_file}");
    let parsed = parse(&["env", "create", "aws", "--spec.name", "reserved"]).unwrap();
    let from_flags = link_spec::read_with::<Env>(None, &parsed.fields)
        .err()
        .map(|e| format!("{e:#}"))
        .unwrap();
    assert!(from_flags.contains(why), "{from_flags}");
    match call(
        "env.env.create",
        json!({"cloud": "aws"}),
        json!({"name": "reserved"}),
    ) {
        Outcome::Refused { reason } => assert!(reason.contains(why), "{reason}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_bare_group_runs_its_declared_default() {
    let parsed = parse(&["env", "approval", "token", "--file", "env.toml"]).unwrap();
    let Command::ApprovalAdd(args) = parsed.command.command else {
        panic!("env approval runs approval add")
    };
    assert_eq!(args.secret, "token");
    let Command::ApprovalAdd(_) = parse(&["env", "approval", "add", "token", "--file", "env.toml"])
        .unwrap()
        .command
        .command
    else {
        panic!("env approval add")
    };
    // The default's flags are its alone: given before another command,
    // they're refused, never silently dropped.
    assert!(parse(&["env", "approval", "--file", "env.toml", "add", "token"]).is_err());
    // Its help says what it runs bare.
    let mut command = <module::Cli as clap::CommandFactory>::command();
    let help = command
        .find_subcommand_mut("approval")
        .unwrap()
        .render_help()
        .to_string();
    assert!(
        help.contains("`lnk env approval` is short for `lnk env approval add`."),
        "{help}"
    );
}

#[test]
fn cores_program_is_lnk_with_its_own_verbs_and_flags() {
    use clap::Parser;
    use link_declarations_fixture::core::{Cli, Command, NAMESPACE};
    assert_eq!(NAMESPACE, "core");
    let read = |argv: &[&str]| Cli::try_parse_from(argv.iter().copied()).map(|c| c.command);
    assert!(matches!(read(&["lnk", "check"]), Ok(Command::CoreCheck(_))));
    let Ok(Command::ModuleAdd(args)) = read(&["lnk", "module", "add", "env"]) else {
        panic!("lnk module add")
    };
    assert_eq!(args.name, "env");
    let Ok(Command::Program(flags)) = read(&["lnk", "--version"]) else {
        panic!("lnk --version")
    };
    assert!(flags.version && !flags.link_history);
    let Ok(Command::Program(flags)) = read(&["lnk", "-V"]) else {
        panic!("lnk -V")
    };
    assert!(flags.version, "its declared short flag");
    assert!(matches!(read(&["lnk"]), Ok(Command::Program(_))));
    // Core gives uninstall and never takes it: a namespace it doesn't
    // declare is another module's, its words passed on unparsed.
    let Ok(Command::External(words)) = read(&["lnk", "uninstall"]) else {
        panic!("lnk uninstall, undeclared, is forwarded")
    };
    assert_eq!(words, ["uninstall"]);
    let Ok(Command::External(words)) = read(&[
        "lnk",
        "agent",
        "start",
        "--model.thinking",
        "high",
        "--",
        "--json",
    ]) else {
        panic!("lnk agent start is the agent module's")
    };
    assert_eq!(
        words,
        ["agent", "start", "--model.thinking", "high", "--", "--json"]
    );
    // Its declared --help is its own, rendered by the program, and each
    // command keeps clap's.
    let Ok(Command::Program(flags)) = read(&["lnk", "-h"]) else {
        panic!("lnk -h")
    };
    assert!(flags.help);
    let help = read(&["lnk", "module", "add", "--help"]).err().unwrap();
    assert_eq!(help.kind(), clap::error::ErrorKind::DisplayHelp);
    // A usage error builds its command alone, with its one --help.
    let missing = read(&["lnk", "module", "add"]).err().unwrap();
    assert_eq!(
        missing.kind(),
        clap::error::ErrorKind::MissingRequiredArgument
    );
    // The argument every command takes goes before its words too.
    let Ok(Command::CoreCheck(args)) = read(&["lnk", "--json", "check"]) else {
        panic!("lnk --json check")
    };
    assert!(args.json);
    let command = <Cli as clap::CommandFactory>::command();
    assert_eq!(command.get_name(), "lnk");
    let history = command
        .get_arguments()
        .find(|a| a.get_id() == "link-history")
        .unwrap();
    assert!(history.is_hide_set());
}

#[test]
fn an_implementer_answers_its_contracts_actions_over_the_contracts_shapes() {
    use clap::Parser;
    use link_declarations_fixture::file_store::{Cli, Command, FileStoreActions};
    let read = |argv: &[&str]| Cli::try_parse_from(argv.iter().copied()).map(|c| c.command);
    assert!(matches!(
        read(&["lnk-file-store", "serve"]),
        Ok(Command::FileStoreServe(_))
    ));
    let Ok(Command::RecordShow(args)) = read(&["lnk-file-store", "record", "show", "notes"]) else {
        panic!("lnk-file-store record show")
    };
    assert_eq!(args.key, "notes");
    assert!(
        read(&["lnk-file-store", "record", "delete", "notes"]).is_err(),
        "a contract action it doesn't list isn't its"
    );
    // Its answers are the contract's own types.
    struct Store;
    impl FileStoreActions for Store {
        fn file_store_check(
            &self,
            _: Call<
                link_declarations_fixture::file_store::FileStoreCheckArgs,
                link_declarations_fixture::contract::Store,
            >,
        ) -> Outcome<link_declarations_fixture::contract::Served> {
            Outcome::Completed {
                output: link_declarations_fixture::contract::Served { answered: 0 },
            }
        }
        fn file_store_serve(
            &self,
            _: Call<link_declarations_fixture::file_store::FileStoreServeArgs>,
        ) -> Outcome<link_declarations_fixture::contract::Served> {
            Outcome::Completed {
                output: link_declarations_fixture::contract::Served { answered: 0 },
            }
        }
        fn record_show(
            &self,
            call: Call<link_declarations_fixture::file_store::RecordShowArgs>,
        ) -> Outcome<link_declarations_fixture::contract::Record> {
            Outcome::Completed {
                output: link_declarations_fixture::contract::Record {
                    key: call.args.key,
                    value: "kept".into(),
                },
            }
        }
    }
    let _ = Store;
}

#[test]
fn each_spec_kind_has_its_own_spec_show_and_field_flags() {
    // The kind named as its namespace is `lnk env spec show`.
    assert!(matches!(
        parse(&["env", "spec", "show"]).unwrap().command.command,
        Command::SpecShow(_)
    ));
    // Another kind is `lnk env profile spec show`.
    let parsed = parse(&["env", "profile", "spec", "show"]).unwrap();
    assert!(matches!(
        parsed.command.command,
        Command::ProfileSpecShow(_)
    ));
    assert!(module::binding(&parsed.words).is_some());
    // An action binding it takes that kind's field flags.
    let parsed = parse(&[
        "env",
        "profile",
        "set",
        "--spec.name",
        "big",
        "--size",
        "large",
    ])
    .unwrap();
    let profile = link_spec::read_with::<module::Profile>(None, &parsed.fields)
        .unwrap()
        .spec;
    assert_eq!(
        (profile.name.as_str(), profile.size),
        ("big", Some(module::Size::Large))
    );
}

#[test]
fn a_bound_kind_of_another_module_is_read_through_its_owners_types() {
    use clap::Parser;
    use link_declarations_fixture::file_store::{Cli, Command, binding};
    let parsed = Cli::try_parse_from(["lnk-file-store", "check", "--spec", "store.toml"]).unwrap();
    assert!(matches!(parsed.command, Command::FileStoreCheck(_)));
    let bound = binding(&["check".to_string()]).expect("its field flags are the contract's kind's");
    assert_eq!(bound.format.kind, "store");
    assert!(
        Cli::try_parse_from(["lnk-file-store", "spec", "show"]).is_err(),
        "the kind's owner shows it, never an implementer"
    );
}

#[test]
fn a_named_shape_carries_its_own_default_and_any_value_is_json() {
    let stage: module::Stage = serde_json::from_value(json!({"name": "s", "run": "x"})).unwrap();
    assert_eq!(
        serde_json::to_value(stage.ready_within).unwrap(),
        json!("1m"),
        "core.duration with the property's own default"
    );
    let catalog = link_actions::Catalog::load(&[std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/module"
    ))])
    .unwrap();
    let schema = catalog
        .json_schema(&link_actions::TypeName::new("env", "stage").unwrap())
        .unwrap();
    let within = &schema["properties"]["ready-within"];
    assert_eq!(within["$ref"], "#/$defs/core.duration");
    assert_eq!(within["default"], "1m");
    let probed: module::Probed =
        serde_json::from_value(json!({"count": 1, "said": {"any": ["shape", 2]}})).unwrap();
    assert_eq!(probed.said, Some(json!({"any": ["shape", 2]})));
    let report = catalog
        .json_schema(&link_actions::TypeName::new("core", "report").unwrap())
        .unwrap();
    assert_eq!(
        report["properties"]["blocks"]["items"]["description"],
        "One block, as link-format writes it"
    );
}

#[test]
fn core_gives_an_answering_module_its_question_flag_and_a_guest_its_entry() {
    let read = |argv: &[&str]| {
        <module::Cli as clap::Parser>::try_parse_from(argv.iter().copied()).map(|c| c.command)
    };
    let Ok(Command::Answer(question)) = read(&["lnk-env", "--link-answer", "measure.traffic"])
    else {
        panic!("lnk-env --link-answer")
    };
    assert_eq!(question, "measure.traffic");
    assert!(
        read(&[
            "lnk-env",
            "--link-answer",
            "measure.traffic",
            "create",
            "aws"
        ])
        .is_err(),
        "a protocol never shares the command space"
    );
    assert!(read(&["lnk-env"]).is_err(), "a command, or the question");
    let Ok(Command::GuestService(entry)) =
        read(&["lnk-env", "guest-service", "worker", "unit.service", "7"])
    else {
        panic!("lnk-env guest-service")
    };
    assert_eq!(
        (entry.unit.as_str(), entry.epoch.as_str()),
        ("unit.service", "7")
    );
    let command = <module::Cli as clap::CommandFactory>::command();
    assert!(
        command
            .find_subcommand("guest-service")
            .unwrap()
            .is_hide_set()
    );
    assert!(
        command
            .get_arguments()
            .find(|a| a.get_id() == "link-answer")
            .unwrap()
            .is_hide_set()
    );
}

#[test]
fn a_sensitive_string_is_a_secret_written_only_on_the_wire() {
    use link_declarations_fixture::contract::{Request, Token};
    let request: Request = serde_json::from_value(json!({
        "key": "notes",
        "credential": "hunter2-hunter2",
        "spare": ["second-secret"],
        "nested": "token-value",
    }))
    .unwrap();
    assert_eq!(request.credential.expose(), "hunter2-hunter2");
    assert!(!format!("{request:?}").contains("hunter2"), "never printed");
    // Written as its value on the wire, and read back.
    let wire = serde_json::to_value(&request).unwrap();
    assert_eq!(wire["credential"], "hunter2-hunter2");
    assert_eq!(wire["spare"][0], "second-secret");
    let Some(Token(token)) = &request.nested else {
        panic!("its token")
    };
    assert_eq!(token.expose(), "token-value");
    // Its checks read the value.
    let short: Request =
        serde_json::from_value(json!({"key": "k", "credential": "short", "nested": "t"})).unwrap();
    let why = link_actions::value::Validate::validate(&short, "").unwrap_err();
    assert!(why.at.contains("credential"), "{}: {}", why.at, why.why);
}

#[test]
fn a_choice_told_by_its_json_type_reads_a_number_a_text_or_a_record() {
    use link_actions::value::Validate;
    use module::{Amount, CompactAt};
    let at = |v: serde_json::Value| serde_json::from_value::<CompactAt>(v).unwrap();
    assert_eq!(at(json!(0.8)), CompactAt::Number(0.8));
    assert_eq!(at(json!("never")), CompactAt::Word("never".into()));
    assert!(at(json!(1.5)).validate("").is_err(), "above its most");
    assert!(
        at(json!("always")).validate("").is_err(),
        "not one of its words"
    );
    let amount = |v: serde_json::Value| serde_json::from_value::<Amount>(v).unwrap();
    assert_eq!(amount(json!(4)), Amount::Number(4));
    assert_eq!(amount(json!("8 GB")), Amount::Text("8 GB".into()));
    let Amount::Record(range) = amount(json!({"min": 2, "max": 8})) else {
        panic!("a range")
    };
    assert_eq!((range.min, range.max), (Some(2), Some(8)));
    assert!(
        amount(json!("lots")).validate("").is_err(),
        "its text's pattern"
    );
    assert!(
        amount(json!({"min": 0})).validate("").is_err(),
        "its record's bounds"
    );
}

#[test]
fn an_open_record_keeps_its_typed_keys_and_passes_on_the_rest() {
    use link_actions::value::Validate;
    let tools: module::Tools = serde_json::from_value(json!({
        "web": {"enabled": true},
        "outside": {"enabled": false, "limit": 3},
    }))
    .unwrap();
    assert_eq!(tools.web.as_ref().and_then(|t| t.enabled), Some(true));
    assert_eq!(tools.others["outside"].limit, Some(3));
    assert_eq!(
        serde_json::to_value(&tools).unwrap()["outside"]["limit"],
        3,
        "written back as given"
    );
    // Another key is of its shape, checked as it is.
    assert!(serde_json::from_value::<module::Tools>(json!({"outside": {"colour": 1}})).is_err());
    let low: module::Tools = serde_json::from_value(json!({"outside": {"limit": 0}})).unwrap();
    assert!(low.validate("").unwrap_err().at.contains("outside"));
    let catalog = link_actions::Catalog::load(&[std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/module"
    ))])
    .unwrap();
    let schema = catalog
        .json_schema(&link_actions::TypeName::new("env", "tools").unwrap())
        .unwrap();
    assert_eq!(schema["additionalProperties"]["$ref"], "#/$defs/env.tool");
}

#[test]
fn a_group_of_plumbing_runs_its_plumbing_default_and_is_hidden() {
    assert!(matches!(
        parse(&["env", "state"]).unwrap().command.command,
        Command::StateShow(_)
    ));
    assert!(matches!(
        parse(&["env", "state", "export"]).unwrap().command.command,
        Command::StateExport(_)
    ));
    let command = <module::Cli as clap::CommandFactory>::command();
    assert!(command.find_subcommand("state").unwrap().is_hide_set());
    assert!(!command.find_subcommand("approval").unwrap().is_hide_set());
}

#[test]
fn an_action_off_the_command_line_is_dispatched_and_never_parsed() {
    // A tool's call reaches its handler, checked as declared.
    assert_eq!(
        call("env.env.inspect", json!({"depth": 2}), json!(null)),
        Outcome::Completed {
            output: json!({"count": 2})
        }
    );
    assert!(matches!(
        call("env.env.inspect", json!({"depth": 0}), json!(null)),
        Outcome::Refused { .. }
    ));
    // No command reads it, and help never lists it.
    assert!(parse(&["env", "inspect", "--depth", "2"]).is_err());
    let env = <module::Cli as clap::CommandFactory>::command();
    assert!(env.find_subcommand("inspect").is_none());
    assert!(env.find_subcommand("probe").is_some());
}

#[test]
fn core_gives_uninstall_only_to_a_module_keeping_parts() {
    use clap::Parser;
    // env declares [removal]: core's hidden uninstall reports its parts.
    let Command::Uninstall(_) = parse(&["env", "uninstall", "--parts", "--json"])
        .unwrap()
        .command
        .command
    else {
        panic!("env uninstall")
    };
    // file-store keeps none, so it is never asked, and has no uninstall.
    assert!(
        link_declarations_fixture::file_store::Cli::try_parse_from(["lnk-file-store", "uninstall"])
            .is_err()
    );
}

#[test]
fn next_leads_from_the_module_and_from_each_command_a_person_types() {
    let row = |command: &str| {
        module::NEXTS
            .iter()
            .find(|(c, _, _)| *c == command)
            .map(|(_, next, fail)| (next.to_vec(), fail.to_vec()))
    };
    // module.toml's next leads into the module.
    assert_eq!(
        row("lnk env"),
        Some((vec!["lnk env create <cloud>", "lnk env probe"], vec![]))
    );
    // A suggestion filling a positional fits its template.
    let (_, next, _) = module::NEXTS
        .iter()
        .find(|(c, _, _)| *c == "lnk env")
        .unwrap();
    let next: Vec<String> = next.iter().map(|t| t.to_string()).collect();
    let typed = link_format::Command::typed(["lnk", "env"].map(String::from));
    let given = link_format::Command::typed(["lnk", "env", "create", "aws"].map(String::from));
    assert!(link_format::nexts::declares(&next, &given, &typed));
    // An action's next, and its fail on a failure.
    assert_eq!(
        row("lnk env create"),
        Some((
            vec!["lnk env probe", "lnk env spec show <arguments>"],
            vec![
                "lnk env create <cloud>",
                "lnk env status",
                "retry",
                "lnk env"
            ]
        ))
    );
    // Plumbing and what's off the command line are never offered.
    assert_eq!(row("lnk env state show"), None);
    assert_eq!(row("lnk env inspect"), None);
    // The program carries it.
    assert_eq!(module::PROGRAM.nexts.len(), module::NEXTS.len());
}

#[test]
fn a_size_or_a_rate_reads_typed_and_its_word_beside_it() {
    use link_actions::value::{Rate, Size, Validate};
    use module::{BodyLimit, Pace};
    let limit = |v: serde_json::Value| serde_json::from_value::<BodyLimit>(v);
    // The word is the word, never a bad size.
    assert_eq!(
        limit(json!("none")).unwrap(),
        BodyLimit::Word("none".into())
    );
    let BodyLimit::Text(size) = limit(json!("16 MB")).unwrap() else {
        panic!("a size")
    };
    assert_eq!(size, Size::bytes(16 << 20));
    assert_eq!(size.to_string(), "16 MB");
    assert!(limit(json!("lots")).is_err(), "neither the word nor a size");
    let pace = |v: serde_json::Value| serde_json::from_value::<Pace>(v);
    let Pace::Text(rate) = pace(json!("10/s")).unwrap() else {
        panic!("a rate")
    };
    assert_eq!((rate.count(), rate.per().as_secs()), (10, 1));
    assert_eq!(pace(json!("none")).unwrap(), Pace::Word("none".into()));
    assert!(pace(json!("0/s")).is_err(), "a rate is above none");
    assert!(BodyLimit::Word("none".into()).validate("").is_ok());
    let _: Rate = "600/m".parse().unwrap();
}

#[test]
fn a_maps_keys_are_their_typed_shape() {
    use link_actions::value::Validate;
    use module::{Domain, Opinions};
    let opinions: Opinions =
        serde_json::from_value(json!({"mail": "fine", "web-1": "slow"})).unwrap();
    // Keyed by the typed shape, ordered as its text.
    let keys: Vec<&Domain> = opinions.0.keys().collect();
    assert_eq!(keys, [&Domain("mail".into()), &Domain("web-1".into())]);
    assert!(opinions.validate("opinions").is_ok());
    let bad: Opinions = serde_json::from_value(json!({"Mail": "loud"})).unwrap();
    let refused = bad.validate("opinions").unwrap_err();
    assert!(refused.to_string().contains("Mail"), "{refused}");
    // The shape's own hook, as a key and alone.
    let local: Opinions = serde_json::from_value(json!({"localhost": "near"})).unwrap();
    let refused = local.validate("opinions").unwrap_err();
    assert!(
        refused.to_string().contains("localhost isn't a domain"),
        "{refused}"
    );
    assert!(Domain("localhost".into()).validate("domain").is_err());
    assert!(Domain("mail".into()).validate("domain").is_ok());
    // Its JSON Schema says its keys.
    let catalog = link_actions::Catalog::load(&[std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/module"
    ))])
    .unwrap();
    let schema = catalog
        .json_schema(&link_actions::TypeName::new("env", "opinions").unwrap())
        .unwrap();
    assert_eq!(schema["propertyNames"]["$ref"], "#/$defs/env.domain");
}

/// A closed choice of words names each word once, in its declaration:
/// every value in order, its word, the value a word names, and what each
/// means where the declaration says.
#[test]
fn a_choice_of_words_reads_its_words_from_its_declaration() {
    use link_actions::value::Words;
    use module::Size;
    assert_eq!(Size::all().collect::<Vec<_>>(), [Size::Small, Size::Large]);
    assert_eq!(Size::Large.word(), "large");
    assert_eq!(Size::from_word("small"), Some(Size::Small));
    assert_eq!(Size::from_word("huge"), None);
    assert_eq!(Size::Small.meaning(), Some("Two processors and 4 GB"));
}
