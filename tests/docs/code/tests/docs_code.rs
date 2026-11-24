//! The docs' rules that need Link's own types: every `lnk` command a
//! page gives exists, and every TOML file a page shows is one Link reads.
//! They read the libraries that own those types and the core's registry,
//! never a built program, so a page's change runs them in seconds.

link_test_kit::program!(unit);

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use link_docs_check::md::{commands, shell_lines, toml_blocks};
use link_docs_check::{Check, Finding, Rule, Scope, Tree, fail};

const RULES: &[Rule] = &[
    Rule {
        id: "references",
        scope: Scope::Site,
        fix: "regenerate spec reference fields/defaults from their owning types and bind every reference",
        check: Check::Whole(link_docs_check_code::references::check),
    },
    Rule {
        id: "arguments",
        scope: Scope::Site,
        fix: "use values and runnable argv accepted by the real owning command parser",
        check: Check::Each(link_docs_check_code::arguments::check),
    },
    Rule {
        id: "environment-reads",
        scope: Scope::Site,
        fix: "declare every environment variable production code reads at its owning component or library",
        check: Check::Whole(link_docs_check_code::environment::check),
    },
    Rule {
        id: "json",
        scope: Scope::Site,
        fix: "bind each JSON example to its owning wire type or released schema and keep it valid",
        check: Check::Each(link_docs_check_code::json::check),
    },
    Rule {
        id: "flags",
        scope: Scope::Site,
        fix: "document flags that the referenced command declares; arguments after -- belong to the invoked program",
        check: Check::Each(link_docs_check_code::flags::check),
    },
    Rule {
        id: "inventories",
        scope: Scope::Site,
        fix: "run cargo run -p link-docs-check-code --bin lnk-docs-inventories -- --write; inventories enumerate their owning declarations and pages",
        check: Check::Whole(link_docs_check_code::inventories::check),
    },
    Rule {
        id: "files",
        scope: Scope::Site,
        fix: "give the file a kind in tests/docs/code/files.toml, then run cargo run -p link-docs-check-code --bin lnk-docs-inventories -- --write; headers and the Repository Files lists are generated from that catalogue",
        check: Check::Whole(link_docs_check_code::files::check),
    },
    Rule {
        id: "commands",
        scope: Scope::Site,
        fix: "commands that don't exist: name one a module's actions.toml declares",
        check: Check::Each(commands_exist),
    },
    Rule {
        id: "specs",
        scope: Scope::Site,
        fix: "a TOML block a page shows is a spec Link reads, by its type",
        check: Check::Each(specs),
    },
    Rule {
        id: "command-pages",
        scope: Scope::Site,
        fix: "a command a person runs names the section explaining it, `docs = \
              \"<address>#<anchor>\"`, which shows its actual CLI or namespace-command spelling; a primary command's \
              is a heading of its own",
        check: Check::Whole(link_docs_check_code::coverage::command_pages),
    },
    Rule {
        id: "owner-links",
        scope: Scope::Site,
        fix: "a page's first mention of a command another package owns links to the section \
              its docs names: run lnk-docs-inventories --write",
        check: Check::Whole(link_docs_check_code::owner_links::check),
    },
    Rule {
        id: "command-folders",
        scope: Scope::Site,
        fix: "a command group whose word is no approved verb (`lnk box image`) has a folder of \
              its own in its owner's docs, named its word, whose pages show it",
        check: Check::Whole(link_docs_check_code::coverage::command_folders),
    },
    Rule {
        id: "variable-pages",
        scope: Scope::Site,
        fix: "an LNK_ variable is named in a page's prose, beyond the inventory that lists \
              it: say what it changes where the command or package that reads it is explained",
        check: Check::Whole(link_docs_check_code::coverage::variable_pages),
    },
    Rule {
        id: "state-pages",
        scope: Scope::Site,
        fix: "a folder of Link's state is named in a page's prose, beyond the inventory that \
              lists it: say what it holds where its owner is explained",
        check: Check::Whole(link_docs_check_code::coverage::state_pages),
    },
    Rule {
        id: "rules-code",
        scope: Scope::Site,
        fix: "the Docs Rules page has a line for each rule, naming its id",
        check: Check::Whole(rules_page),
    },
];

/// Groups whose first word may be a name rather than a command: `lnk
/// measure <unit>`.
const NAMED: &[&str] = &[
    "help",
    "manual",
    "measure",
    "up",
    "folder",
    "upgrade",
    "uninstall",
];

/// The groups, and every `(group, command)` the components declare, those
/// hidden from `lnk --help` too: a doc may name `lnk agent state-send`, which an
/// agent's harness runs, not a person. A command's own commands are
/// `(group, "command verb")`: `("agent", "harness allow")`.
/// The groups, each `(group, command)`, and each group alias with the
/// group it stands for, from `[aliases]` (`man` for `manual`).
type Known = (
    BTreeSet<String>,
    BTreeSet<(String, String)>,
    BTreeMap<String, String>,
);

fn known() -> Known {
    let mut groups = BTreeSet::new();
    let mut known = BTreeSet::new();
    let registry = link_cli::registry();
    let mut declared = Vec::new();
    for descriptor in registry.values() {
        let routes: Vec<_> = descriptor
            .commands
            .iter()
            .filter(|(_, c)| c.route)
            .map(|(path, _)| path)
            .collect();
        declared.extend(descriptor.commands.iter().filter(|(path, _)| {
            routes
                .iter()
                .any(|route| *path == *route || path.starts_with(&format!("{route} ")))
        }));
    }
    // A group's alias is typed as the group: its declaration, a module's
    // in its descriptor and core's in its own `actions.toml`.
    let mut aliases: BTreeMap<String, String> = registry
        .values()
        .flat_map(|d| d.commands.iter())
        .filter(|(path, c)| c.route && !path.contains(' '))
        .filter_map(|(path, c)| Some((c.alias.clone()?, path.clone())))
        .collect();
    aliases.extend(
        link_actions::read_actions(link_cli::cli::HELP.toml)
            .expect("core declarations")
            .aliases
            .into_iter()
            .filter(|(group, _)| !group.contains(' '))
            .map(|(group, alias)| (alias, group)),
    );
    let core = link_component::actions::parse(link_cli::cli::HELP.toml).expect("core commands");
    let core = link_component::actions::commands(&core);
    declared.extend(core.iter().filter(|(path, _)| path.as_str() != "root"));
    groups.insert("help".into());
    for (path, _) in declared {
        match path.split_once(' ') {
            Some((group, rest)) => {
                groups.insert(group.to_string());
                known.insert((group.to_string(), rest.to_string()));
            }
            None => {
                groups.insert(path.clone());
            }
        }
    }
    (groups, known, aliases)
}

fn command_exists(known: &Known, group: &str, command: &str) -> bool {
    let group = known.2.get(group).map_or(group, String::as_str);
    known.0.contains(group)
        && (command.is_empty()
            || NAMED.contains(&group)
            || known.1.contains(&(group.to_owned(), command.to_owned())))
}

fn commands_exist(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    if !tree.is_doc(file) {
        return Vec::new();
    }
    static KNOWN: std::sync::LazyLock<Known> = std::sync::LazyLock::new(known);
    let (_, known, aliases) = &*KNOWN;
    let mut wrong = Vec::new();
    for line in shell_lines(text) {
        for (typed, command, next) in commands(&line) {
            let ok = command_exists(&KNOWN, &typed, &command);
            let group = aliases.get(&typed).cloned().unwrap_or(typed);
            if !ok {
                wrong.push(fail(format!("{}: lnk {group} {command}", tree.shown(file))));
            }
            // A command with commands of its own (`lnk agent harness
            // allow`): the word after it is one of them.
            let verbs = format!("{command} ");
            let has_verbs = known
                .iter()
                .any(|(g, c)| *g == group && c.starts_with(&verbs));
            if ok
                && has_verbs
                && !next.is_empty()
                && !known.contains(&(group.clone(), format!("{command} {next}")))
            {
                wrong.push(fail(format!(
                    "{}: lnk {group} {command} {next}",
                    tree.shown(file)
                )));
            }
        }
    }
    wrong
}

/// Whether a TOML text is a sandbox spec: it has one of a sandbox's
/// sections.
fn is_sandbox_spec(text: &str) -> bool {
    const SECTIONS: &[&str] = &[
        "files",
        "internet",
        "lan",
        "localhost",
        "tools",
        "browser",
        "commands",
    ];
    toml::from_str::<toml::Table>(text).is_ok_and(|t| SECTIONS.iter().any(|s| t.contains_key(*s)))
}

/// Whether `text` is a spec of `kind`, read strictly by its owning type.
fn spec_of(kind: &str, text: &str) -> anyhow::Result<()> {
    match kind {
        "box" => link_box::spec::BoxSpec::parse(text).map(|_| ()),
        "sandbox" => link_spec::parse_spec::<link_sandbox_client::Spec>(text).map(|_| ()),
        "env" => link_env_contract::file::parse(text).map(|_| ()),
        "policy" => link_policy::parse(text).map(|_| ()),
        "bundle" => link_bundle::spec::read(text).map(|_| ()),
        "agent" => link_agent_spec::parse(text)
            .map(|_| ())
            .or_else(|_| link_agent_spec::parse_team_definition(text).map(|_| ())),
        other => anyhow::bail!("no spec is a {other}'s"),
    }
}

/// Every TOML file the docs show is one Link reads: a box spec on the
/// boxes' pages (or a setup stage's sandbox spec), a sandbox spec on the
/// sandbox's, and elsewhere one of them, an agent's spec or a team's, an
/// agent's sandbox file, a model's, `/etc/lnk/kind.toml`, an agent's
/// `agent.toml`, the environments' `approvals.toml` or the secrets'
/// `secrets.toml` (its first line naming it), a `module.toml`, an
/// `actions.toml`, or a spec test case (its first line
/// `# tests/specs/<group>/<case>.toml`) whose `[spec]` is its group's.
fn specs(tree: &Tree, path: &Path, text: &str) -> Vec<Finding> {
    if !tree.is_doc(path) {
        return Vec::new();
    }
    let page = tree.shown(path);
    // A spec's own page, and its part's pages, show that spec; a page
    // elsewhere may show any.
    let kind = [
        ("box", "/config/specs/box.md"),
        ("box", "/services/boxes/"),
        ("sandbox", "/config/specs/sandbox.md"),
        ("sandbox", "/security/sandbox/"),
        ("agent", "/config/specs/agent.md"),
        ("env", "/config/specs/env.md"),
        ("env", "/services/environments/"),
        ("policy", "/config/specs/policy.md"),
        ("bundle", "/config/specs/bundle.md"),
    ]
    .into_iter()
    .find(|(_, at)| page.contains(at) && !page.ends_with("/permissions.md"))
    .map(|(kind, _)| kind);
    let mut wrong = Vec::new();
    for block in toml_blocks(text) {
        let first = block.lines().next().unwrap_or_default();
        let case = first
            .strip_prefix("# tests/specs/")
            .and_then(|l| l.split_once('/'))
            .map(|(group, _)| group.to_string());
        let result = match (&case, kind) {
            _ if first == "# module.toml" => link_component::Source::parse(&block).map(|_| ()),
            _ if first == "# actions.toml" => link_component::actions::parse(&block).map(|_| ()),
            // A declaration's file, read as its module's build reads it.
            _ if first == "# types.toml" => link_actions::read_types(&block, "example").map(|_| ()),
            _ if first == "# variables.toml" => link_actions::read_variables(&block).map(|_| ()),
            _ if first == "# schema.toml" => shown_format(&block),
            // The harness's tool namespaces, read as the contract reads them.
            _ if first == "# namespaces.toml" => {
                toml::from_str::<link_harness_contract::harness::namespaces::Namespaces>(&block)
                    .map(|_| ())
                    .map_err(anyhow::Error::from)
            }
            _ if first == "# history.toml" => {
                link_spec::history::History::parse(&block).map(|_| ())
            }
            _ if first == "# ~/.lnk/safe/installation.toml" => {
                toml::from_str::<link_machine::installation::Installation>(&block)
                    .map(|_| ())
                    .map_err(anyhow::Error::from)
            }
            _ if first == "# ~/Link/Safe/Agents/main/agent.toml" => {
                link_files::saved::parse::<link_agent::settings::Settings>(&block).map(|_| ())
            }
            _ if first.starts_with("# bundle.toml") => link_bundle::spec::read(&block).map(|_| ()),
            _ if first == "# archive.toml" => {
                link_files::saved::parse::<link_archive::Manifest>(&block).map(|_| ())
            }
            _ if first == "# layout.toml" => toml::from_str::<
                std::collections::BTreeMap<String, link_files::layout::Owner>,
            >(&block)
            .map(|_| ())
            .map_err(anyhow::Error::from),
            _ if first.starts_with("# outside.toml") => {
                toml::from_str::<link_bundle::outside::File>(&block)
                    .map(|_| ())
                    .map_err(anyhow::Error::from)
            }
            _ if first == "# ~/.lnk/safe/envs/approvals.toml" => {
                link_env_contract::approvals::parse(&block).map(|_| ())
            }
            _ if first == "# ~/.lnk/safe/secrets/secrets.toml" => {
                link_secret_contract::check_store(&block)
            }
            _ if first == "# ~/.lnk/safe/measures/settings.toml" => {
                toml::from_str::<link_measure::settings::Written>(&block)
                    .map(|_| ())
                    .map_err(anyhow::Error::from)
            }
            // What root says this machine is, by its type.
            _ if first.starts_with("# /etc/lnk/kind.toml") => {
                toml::from_str::<link_machine::machine::Kind>(&block)
                    .map(|_| ())
                    .map_err(anyhow::Error::from)
            }
            (Some(group), _) => toml::from_str::<toml::Table>(&block)
                .map_err(anyhow::Error::from)
                .and_then(|case| {
                    let spec = match case.get("spec") {
                        Some(toml::Value::Table(spec)) => spec.clone(),
                        _ => toml::Table::new(),
                    };
                    spec_of(group, &toml::to_string(&spec)?)
                }),
            // A box's setup stage, on the boxes' pages, is a sandbox spec.
            (None, Some("box" | "env")) if is_sandbox_spec(&block) => spec_of("sandbox", &block),
            (None, Some(kind)) => spec_of(kind, &block),
            (None, None) => spec_of("box", &block)
                .or_else(|_| spec_of("sandbox", &block))
                .or_else(|_| spec_of("agent", &block))
                .or_else(|_| spec_of("env", &block))
                .or_else(|_| spec_of("policy", &block))
                .or_else(|_| link_agent::sandbox_file::Sandbox::parse(&block).map(|_| ()))
                .or_else(|_| link_model_contract::parse(&block).map(|_| ())),
        };
        if let Err(err) = result {
            wrong.push(fail(format!("{page}: {err:#}\n{block}")));
        }
    }
    wrong
}

/// A `schema.toml` a page shows, read as its owner's build reads it, the
/// migrations its steps name standing in for those its owner registers.
fn shown_format(block: &str) -> anyhow::Result<()> {
    fn unchanged(body: link_files::schema::Body) -> anyhow::Result<link_files::schema::Body> {
        Ok(body)
    }
    let table: toml::Table = toml::from_str(block)?;
    let named: Vec<link_files::schema::Migration> = table
        .get("steps")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|step| step.get("migration")?.as_str())
        .map(|name| link_files::schema::Migration {
            name: Box::leak(name.to_string().into_boxed_str()),
            run: unchanged,
        })
        .collect();
    link_files::schema::Format::parse_with(block, Box::leak(named.into_boxed_slice())).map(|_| ())
}

fn rules_page(tree: &Tree) -> Vec<Finding> {
    let rules: Vec<Rule> = RULES
        .iter()
        .filter(|r| r.id != "rules-code")
        .copied()
        .collect();
    link_docs_check::rules_page(tree, &rules)
}

#[test]
fn the_docs_follow_every_rule_of_the_code() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tree = Tree::read(
        &here.join("../../.."),
        true,
        &here.join("../large-files.toml"),
    );
    link_docs_check::run(&tree, RULES).assert();
}

/// The section rule alone, so owner declarations can be validated without
/// running every documentation rule or any operational program.
#[test]
fn declared_command_sections_cover_every_offered_surface() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tree = Tree::read(
        &here.join("../../.."),
        true,
        &here.join("../large-files.toml"),
    );
    let found = link_docs_check_code::coverage::command_pages(&tree);
    assert!(
        found.is_empty(),
        "{}",
        found
            .into_iter()
            .map(|finding| finding.text)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn the_registry_knows_every_command_hidden_ones_too() {
    let all = known();
    // A group's alias is typed as the group, from its declaration.
    assert_eq!(all.2.get("mod").map(String::as_str), Some("module"));
    assert_eq!(all.2.get("man").map(String::as_str), Some("manual"));
    assert!(command_exists(&all, "mod", "add"));
    assert!(command_exists(&all, "man", "services/boxes"));
    let (groups, known, aliases) = all;
    assert!(known.len() > 40, "the registry is incomplete: {known:?}");
    for group in ["agent", "box", "sandbox"] {
        assert!(groups.contains(group), "{group}");
    }
    let has = |g: &str, c: &str| known.contains(&(g.to_string(), c.to_string()));
    assert!(has("agent", "state-send"), "a command a harness runs");
    assert!(
        has("agent", "harness-permission add"),
        "a command's own command"
    );
    let wrong = |line: &str| {
        commands(line)
            .into_iter()
            .any(|(g, c, _)| !groups.contains(&g) || !has(&g, &c))
    };
    assert!(wrong("lnk agent no-such-command"));
    assert!(!wrong("lnk agent start"));
    let actual = (groups, known, aliases);
    assert!(command_exists(&actual, "module", "add"));
    assert!(!command_exists(&actual, "module", "made-up-command"));
    assert!(!command_exists(&actual, "made-up-group", ""));
    assert!(!command_exists(&actual, "root", ""));
    assert!(command_exists(&actual, "up", "agent"));
}

#[test]
fn a_spec_block_is_read_by_its_type() {
    assert!(spec_of("box", "schema = 1\nno-such-key = 1\n").is_err());
    assert!(spec_of("nothing", "").is_err());
    for kind in ["env", "policy"] {
        assert!(spec_of(kind, "schema = 1\nname = \"checks\"\nversion = 1\n").is_ok());
        assert!(
            spec_of(
                kind,
                "schema = 1\nname = \"checks\"\nversion = 1\nunknown = true\n"
            )
            .is_err()
        );
        // Its schema named, as every spec does.
        assert!(spec_of(kind, "name = \"checks\"\nversion = 1\n").is_err());
    }
    assert!(is_sandbox_spec("[internet]\n"));
    assert!(!is_sandbox_spec("[other]\n"));
}

#[test]
fn a_command_names_the_section_showing_it_and_a_held_one_leaves_once_it_does() {
    let root = link_test_kit::TempDir::new("command-pages");
    let write = |path: &str, text: &str| {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    let actions = [
        ("go", "secondary", ""),
        ("run", "secondary", "/p#run-it"),
        ("inner", "plumbing", ""),
        ("lost", "secondary", "/p#nowhere"),
        ("away", "secondary", "/q#q"),
        ("shown", "secondary", "/p#p"),
        ("first", "primary", "/p#p"),
        ("one", "primary", "/p#both"),
        ("two", "primary", "/p#both"),
        ("own", "primary", "/p#its-own"),
    ];
    let verbs: String = actions
        .iter()
        .map(|(verb, _, _)| format!("{verb} = {{ description = \"It\", effect = \"find\" }}\n"))
        .collect();
    let declared: String = actions
        .iter()
        .map(|(verb, role, docs)| {
            let docs = match docs.is_empty() {
                true => String::new(),
                false => format!("docs = \"{docs}\"\n"),
            };
            format!(
                "[actions.\"p.p.{verb}\"]\ndescription = \"It\"\nrole = \"{role}\"\n\
                 output = \"core.report\"\n{docs}\n"
            )
        })
        .collect();
    // The module's own group is explained by its topic page.
    write(
        "p/module.toml",
        "schema = 1\n[module]\nid = \"p\"\nkind = \"module\"\n\
         [documentation]\nroot = \"docs\"\ntopic = \"p\"\n",
    );
    write(
        "p/actions.toml",
        &format!("schema = 1\n\n[verbs]\n{verbs}\n{declared}"),
    );
    write(
        "p/docs/p/README.md",
        "# P\n\nRun `lnk p`, `lnk p shown`, `lnk p first`.\n\n## Run it\n\n`lnk p run`.\n\n\
         ## Both\n\n`lnk p one`, `lnk p two`.\n\n## Its own\n\n`lnk p own`, never `lnk p go`.\n",
    );
    write(
        "large-files.toml",
        "[lines]\n\n[held]\n\"command-pages\" = [\"p/actions.toml#p run\", \"p/actions.toml#p gone\"]\n",
    );
    let tree = Tree::read(&root, false, &root.join("large-files.toml"));
    let mut found: Vec<String> = link_docs_check_code::coverage::command_pages(&tree)
        .into_iter()
        .map(|f| f.text)
        .collect();
    found.sort();
    let primary = "is primary, so /p#both is a heading of its own: below its page's title, and \
                   no other primary command's";
    assert_eq!(
        found,
        [
            "[held] command-pages: p/actions.toml#p gone is gone".to_string(),
            "p/actions.toml#p away: names /q#q, but no page is at /q".to_string(),
            "p/actions.toml#p first: is primary, so /p#p is a heading of its own: below its \
             page's title, and no other primary command's"
                .to_string(),
            "p/actions.toml#p go: names no docs section (`docs = \"<address>#<anchor>\"`)"
                .to_string(),
            "p/actions.toml#p lost: names /p#nowhere, but its page has no heading #nowhere"
                .to_string(),
            format!("p/actions.toml#p one: {primary}"),
            "p/actions.toml#p run: keeps command-pages now; take it out of large-files.toml's [held]"
                .to_string(),
            format!("p/actions.toml#p two: {primary}"),
        ]
    );
}

#[test]
fn a_command_group_that_is_no_verb_gets_a_folder() {
    let root = link_test_kit::TempDir::new("command-folders");
    let write = |path: &str, text: &str| {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    write(
        "tests/repository/verbs.toml",
        "pairs = [[\"create\", \"delete\"]]\ncrossed = []\nalone = [\"list\"]\n\n[banned]\n\n[banned_flags]\n\n[held]\n",
    );
    // Each resource's group is explained by its shape.
    let words = ["image", "user", "create"];
    let group = |word: &str| {
        format!(
            "[actions.\"p.{word}.list\"]\ndescription = \"L\"\nrole = \"secondary\"\n\
             output = \"core.report\"\n\n"
        )
    };
    let shape = |word: &str| format!("[{word}]\ntype = \"string\"\ndescription = \"G\"\n\n");
    write(
        "p/module.toml",
        "schema = 1\n[module]\nid = \"p\"\nkind = \"module\"\n",
    );
    write(
        "p/actions.toml",
        &format!("schema = 1\n\n{}", words.map(group).concat()),
    );
    write(
        "p/types.toml",
        &format!("schema = 1\n\n{}", words.map(shape).concat()),
    );
    write(
        "p/docs/p/images/README.md",
        "# Images\n\nRun `lnk p image list`.\n",
    );
    write("p/docs/p/user.md", "# User\n\nRun `lnk p user list`.\n");
    write("large-files.toml", "[lines]\n");
    let tree = Tree::read(&root, false, &root.join("large-files.toml"));
    let found: Vec<String> = link_docs_check_code::coverage::command_folders(&tree)
        .into_iter()
        .map(|f| f.text)
        .collect();
    assert_eq!(
        found,
        ["p/actions.toml#p user: no folder named user under p's docs shows `lnk p user`"]
    );
}

#[test]
fn command_only_actions_require_their_surface_spelling_in_an_owner_section() {
    let root = link_test_kit::TempDir::new("command-surface");
    let write = |path: &str, text: &str| {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    write("large-files.toml", "[lines]\n");
    write(
        "p/module.toml",
        "schema = 1\n[module]\nid = 'chat'\nkind = 'module'\n",
    );
    // An agent's command alone, run by its harness.
    let action = "schema = 1\n[verbs]\nask = { description = 'Ask', effect = 'insert' }\n[actions.'chat.chat.ask']\ndescription = 'Ask'\n\
                  role = 'primary'\noutput = 'core.report'\nsurfaces = []\n\
                  route = { bridge = 'harness', callers = ['agent'], command = true }\n";
    write("p/actions.toml", action);
    let findings = || {
        let tree = Tree::read(&root, false, &root.join("large-files.toml"));
        link_docs_check_code::coverage::command_pages(&tree)
    };
    assert_eq!(
        findings().len(),
        1,
        "command-only actions need an owner section"
    );
    write("p/actions.toml", &format!("{action}docs = '/p#ask'\n"));
    write(
        "p/docs/p/README.md",
        "# P\n\nChat.\n\n## Ask\n\nUse `lnk chat ask`.\n",
    );
    assert_eq!(
        findings().len(),
        1,
        "a nonexistent CLI spelling cannot explain a chat-only action"
    );
    write(
        "p/docs/p/README.md",
        "# P\n\nChat.\n\n## Ask\n\nUse `/chat:ask:other`.\n",
    );
    assert_eq!(
        findings().len(),
        1,
        "a longer namespace command is not this action"
    );
    write(
        "p/docs/p/README.md",
        "# P\n\nChat.\n\n## Ask\n\nUse `/chat:ask question`.\n",
    );
    assert!(findings().is_empty());
}

#[test]
fn command_only_first_mentions_link_to_the_owner_by_their_real_surface() {
    let root = link_test_kit::TempDir::new("command-owner-link");
    let write = |path: &str, text: &str| {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    for owner in ["p", "other"] {
        write(
            &format!("{owner}/Cargo.toml"),
            &format!("[package]\nname = '{owner}'\nversion = '0.1.0'\n"),
        );
    }
    write("large-files.toml", "[lines]\n");
    write(
        "p/module.toml",
        "schema = 1\n[module]\nid = 'chat'\nkind = 'module'\n",
    );
    write(
        "p/actions.toml",
        "schema = 1\n[verbs]\nask = { description = 'Ask', effect = 'insert' }\n[actions.'chat.chat.ask']\ndescription = 'Ask'\n\
         role = 'secondary'\noutput = 'core.report'\nsurfaces = []\ndocs = '/p#ask'\n\
         route = { bridge = 'harness', callers = ['agent'], command = true }\n",
    );
    write(
        "p/docs/p/README.md",
        "# P\n\nChat.\n\n## Ask\n\nUse `/chat:ask question`.\n",
    );
    write(
        "other/docs/other/README.md",
        "# Other\n\nAnother owner.\n\n## Chat\n\nUse `/chat:ask question`.\n\n`lnk chat ask` is not this command's surface.\n",
    );
    let tree = Tree::read(&root, false, &root.join("large-files.toml"));
    let found = link_docs_check_code::owner_links::check(&tree);
    assert_eq!(
        found.len(),
        1,
        "an agent-only first mention needs its owner link: {found:?}"
    );
    let updates = link_docs_check_code::owner_links::updates(&tree);
    assert_eq!(updates.len(), 1);
    assert!(
        updates[0]
            .text
            .contains("[`/chat:ask question`](../../../p/docs/p/README.md#ask)")
    );
    assert!(updates[0].text.contains("`lnk chat ask`"));
}

#[test]
fn a_tools_command_uses_its_documented_owner_for_folders_and_links() {
    let root = link_test_kit::TempDir::new("command-owner");
    let write = |path: &str, text: &str| {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    write(
        "agent/Cargo.toml",
        "[package]\nname = 'agent'\nversion = '0.1.0'\n",
    );
    write(
        "browser/Cargo.toml",
        "[package]\nname = 'browser'\nversion = '0.1.0'\n",
    );
    write(
        "tests/repository/verbs.toml",
        "pairs = []\ncrossed = []\nalone = ['list']\n[banned]\n[banned_flags]\n[held]\n",
    );
    write("large-files.toml", "[lines]\n");
    write(
        "agent/module.toml",
        "schema = 1\n[module]\nid = 'agent'\nkind = 'module'\n",
    );
    // The browser's group is explained by its shape, at the page its
    // commands name.
    write(
        "agent/types.toml",
        "schema = 1\n[browser]\ntype = 'string'\ndescription = 'Browser'\n",
    );
    write(
        "agent/actions.toml",
        "schema = 1\n[actions.'agent.browser.list']\ndescription = 'List'\nrole = 'secondary'\n\
         output = 'core.report'\ndocs = '/agents/browser#terminal'\n",
    );
    write(
        "browser/docs/agents/browser/README.md",
        "# Browser\n\nControl a browser.\n\n## Terminal\n\nRun `lnk agent browser list`.\n",
    );
    write(
        "agent/docs/agents/README.md",
        "# Agent\n\nManage agents.\n\nUse `lnk agent browser list` to inspect the browser.\n",
    );
    let tree = Tree::read(&root, false, &root.join("large-files.toml"));
    assert!(link_docs_check_code::coverage::command_folders(&tree).is_empty());
    let links = link_docs_check_code::owner_links::updates(&tree);
    assert_eq!(links.len(), 1);
    assert!(links[0].path.ends_with("agent/docs/agents/README.md"));
    assert!(links[0].text.contains(
        "[`lnk agent browser list`](../../../browser/docs/agents/browser/README.md#terminal)"
    ));
    // A mention in the tool's own explanation must not acquire a self-link.
    assert!(
        !links
            .iter()
            .any(|update| update.path.starts_with(root.join("browser")))
    );
}
