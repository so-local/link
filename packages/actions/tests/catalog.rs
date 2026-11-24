//! The worked example's module loads into the catalog with what its
//! declarations imply filled in, and each rule refuses a declaration that
//! breaks it, naming why.

link_test_kit::program!(unit);

use std::path::{Path, PathBuf};

use link_actions::{
    ActionId, Catalog, Effect, Role, Surface, TypeName,
    schema::{Node, Schema},
};
use link_test_kit::temp::TempDir;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/declarations/module")
}

fn example() -> Catalog {
    Catalog::load(&[&fixture()]).unwrap_or_else(|e| panic!("{e:#}"))
}

fn id(text: &str) -> ActionId {
    text.parse().unwrap()
}

fn named(module: &str, name: &str) -> Schema {
    Schema::new(Node::Named(TypeName::new(module, name).unwrap()))
}

/// Each action's effect comes from its verb: a fixed verb's from
/// `format.toml`, a module's own from its `[verbs]`.
#[test]
fn every_action_takes_its_effect_from_its_verb() {
    let catalog = example();
    for (action, effect) in [
        ("env.env.create", Effect::Insert),
        ("env.approval.add", Effect::Insert),
        ("env.env.inspect", Effect::Find),
        ("env.env.steer", Effect::Update),
    ] {
        assert_eq!(
            catalog.action(&id(action)).unwrap().effect,
            effect,
            "{action}"
        );
    }
}

#[test]
fn the_example_loads_with_its_outputs_implied() {
    let catalog = example();
    let create = catalog.action(&id("env.env.create")).unwrap();
    assert_eq!(create.spec, Some(TypeName::new("env", "env").unwrap()));
    assert_eq!(create.output, named("env", "created"));
    assert_eq!(create.role, Role::Primary);
    assert_eq!(create.surfaces, Surface::ALL);
    assert_eq!(create.positionals()[0].0, "cloud");
    assert_eq!(create.args["yes"].cli.shared.as_deref(), Some("yes"));
    assert_eq!(
        create.args["yes"].schema.meta.description.as_deref(),
        Some("Say yes to what it would ask"),
        "core's shared flag, as declared"
    );
    let add = catalog.action(&id("env.approval.add")).unwrap();
    assert_eq!(
        add.output,
        named("env", "approval"),
        "add answers its resource"
    );
    assert_eq!(add.role, Role::Secondary);
    let route = catalog
        .action(&id("env.env.probe"))
        .unwrap()
        .route
        .clone()
        .unwrap();
    assert_eq!(route.names, ["/probe"]);
    assert_eq!(
        route.callers,
        [link_actions::Caller::Owner, link_actions::Caller::Agent]
    );
    let module = catalog.module("env").unwrap();
    assert!(matches!(
        &module.dependencies["filter"],
        link_actions::Dependency::Capability {
            min_abi: Some(1),
            ..
        }
    ));
    let env = catalog
        .json_schema(&TypeName::new("env", "env").unwrap())
        .unwrap();
    assert_eq!(
        env["properties"]["grant"]["x-link-unavailable"],
        "a sandbox run lends no agent anything"
    );
    let (name, _) = catalog.spec("env").unwrap();
    assert_eq!(name.to_string(), "env.env");
}

#[test]
fn a_shape_renders_as_json_schema_with_names_as_references() {
    let schema = example()
        .json_schema(&TypeName::new("env", "env").unwrap())
        .unwrap();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(
        schema["properties"]["stages"]["items"]["$ref"],
        "#/$defs/env.stage"
    );
    assert_eq!(
        schema["$defs"]["env.stage"]["properties"]["timeout"]["format"],
        "duration"
    );
    assert_eq!(schema["properties"]["workdir"]["x-link-path"], true);
    assert_eq!(schema["required"], serde_json::json!(["name"]));
}

/// The example with `file` replaced by `text`, loaded, and its refusal.
fn refused(file: &str, from: &str, to: &str) -> String {
    match edited(&[(file, from, to)]) {
        Ok(_) => panic!("{file} with {to:?} loaded"),
        Err(e) => format!("{e:#}"),
    }
}

/// The worked example with each `(file, from, to)` edit made, loaded.
fn edited(edits: &[(&str, &str, &str)]) -> anyhow::Result<Catalog> {
    let dir = TempDir::new("catalog");
    for name in [
        "module.toml",
        "actions.toml",
        "types.toml",
        "variables.toml",
        "schema.toml",
        "profile-schema.toml",
    ] {
        let mut text = std::fs::read_to_string(fixture().join(name)).unwrap();
        for (file, from, to) in edits {
            if name == *file {
                assert!(text.contains(from), "{from:?} isn't in {name}");
                text = text.replace(from, to);
            }
        }
        std::fs::write(dir.path().join(name), text).unwrap();
    }
    Catalog::load(&[dir.path()])
}

const PUSH: &str =
    "description = \"Send files, passing a copier's own flags on\"\noutput = \"probed\"";

/// A control action is known by its declaration's `control` key, and an
/// action the catalog doesn't declare is none.
#[test]
fn a_control_action_is_known_by_its_declaration() {
    let catalog = edited(&[(
        "actions.toml",
        "output = \"created\"\n",
        "output = \"created\"\ncontrol = true\nsurfaces = [\"cli\"]\n",
    )])
    .unwrap();
    assert!(catalog.is_control(&id("env.env.create")));
    assert!(!example().is_control(&id("env.env.create")));
    assert!(!catalog.is_control(&id("env.env.nothing")));
}

/// An action's output or stdin holds a secret exactly when its declared
/// shape reaches a value marked `x-link-sensitive`, through a name too;
/// a stdin naming no declared shape is refused.
#[test]
fn a_secret_in_what_an_action_prints_or_reads_is_found_from_its_shapes() {
    let reads = format!("{PUSH}\nstdin = \"login\"");
    let catalog = edited(&[
        ("actions.toml", PUSH, &reads),
        (
            "types.toml",
            "[probed]\n",
            "[login]\ntype = \"object\"\nrequired = [\"token\"]\nadditionalProperties = false\nproperties.token = \"token\"\n\n[token]\ntype = \"string\"\nx-link-sensitive = true\n\n[probed]\n",
        ),
    ])
    .unwrap_or_else(|e| panic!("{e:#}"));
    let push = catalog.action(&id("env.env.push")).unwrap();
    assert_eq!(push.stdin, Some(named("env", "login")));
    assert!(catalog.reads_secret(push));
    assert!(!catalog.prints_secret(push));
    let create = catalog.action(&id("env.env.create")).unwrap();
    assert_eq!(create.stdin, None);
    assert!(!catalog.reads_secret(create) && !catalog.prints_secret(create));
    assert!(catalog.sensitive(&named("env", "token")));
    let refusal = refused(
        "actions.toml",
        PUSH,
        &format!("{PUSH}\nstdin = \"nothing\""),
    );
    assert!(refusal.contains("stdin"), "{refusal}");
}

/// The store contract and its implementer, `file-store` written as `text`.
fn implementer(actions: &str, module: Option<&str>) -> anyhow::Result<Catalog> {
    let fixtures = fixture().join("..");
    let dir = TempDir::new("implementer");
    let written = std::fs::read_to_string(fixtures.join("file-store/module.toml")).unwrap();
    std::fs::write(dir.path().join("module.toml"), module.unwrap_or(&written)).unwrap();
    std::fs::write(dir.path().join("actions.toml"), actions).unwrap();
    Catalog::load(&[dir.path(), &fixtures.join("contract")])
}

#[test]
fn an_implementer_is_checked_against_its_contract() {
    let listed =
        "schema = 1\n[implements]\nstore = [\"store.store.serve\", \"store.record.show\"]\n";
    let catalog = implementer(listed, None).unwrap_or_else(|e| panic!("{e:#}"));
    let show = catalog.action(&id("file-store.record.show")).unwrap();
    assert_eq!(
        show.output,
        named("store", "record"),
        "the contract's output"
    );
    assert_eq!(show.id.command(), "lnk file-store record show");
    assert!(catalog.action(&id("file-store.file-store.serve")).is_some());
    assert!(catalog.action(&id("file-store.record.delete")).is_none());
    let cases = [
        (
            "schema = 1\n[implements]\nstore = [\"store.record.list\"]\n",
            None,
            "which contract store doesn't declare",
        ),
        (
            listed,
            Some("schema = 1\n[module]\nid = \"file-store\"\nkind = \"module\"\nowner = \"x\"\n"),
            "doesn't list as a dependency",
        ),
        (
            "schema = 1\n[implements]\nstore = [\"box.record.show\"]\n",
            None,
            "isn't one of its actions",
        ),
        (
            "schema = 1\n[implements]\nstore = [\"store.store.serve\"]\n[verbs]\nserve = { description = \"x\", effect = \"insert\" }\n[actions.\"file-store.file-store.serve\"]\ndescription = \"x\"\noutput = \"store.served\"\n",
            None,
            "[implements] lists it",
        ),
    ];
    for (actions, module, why) in cases {
        let refusal = format!("{:#}", implementer(actions, module).expect_err(why));
        assert!(refusal.contains(why), "{refusal}");
    }
}

#[test]
fn each_rule_refuses_naming_why() {
    let cases = [
        (
            "actions.toml",
            "x-link-cli = { short = \"a\" } }",
            "x-link-cli = { short = \"a\", env = \"LNK_NOWHERE\" } }",
            "[args]: its fallback variable LNK_NOWHERE is defined in no variables.toml",
        ),
        (
            "actions.toml",
            "[verbs]\n",
            "[program]\ndescription = \"The example\"\n[program.args]\nquiet = { type = \"string\", description = \"Quiet\", x-link-cli = { env = \"LNK_NOWHERE\" } }\n\n[verbs]\n",
            "[program]: its fallback variable LNK_NOWHERE is defined in no variables.toml",
        ),
        (
            "types.toml",
            "items = \"stage\"",
            "items = \"stages\"",
            "env.stages isn't declared",
        ),
        (
            "types.toml",
            "items = \"stage\"",
            "items = \"box.id\"",
            "doesn't list as a dependency",
        ),
        (
            "types.toml",
            "items = \"stage\"",
            "items = { \"$ref\" = \"#/stage\" }",
            "$ref isn't read",
        ),
        (
            "types.toml",
            "type = \"object\"\nrequired = [\"name\", \"run\"]",
            "type = \"object\"\nrequired = [\"nam\"]",
            "isn't a property",
        ),
        (
            "types.toml",
            "additionalProperties = false\nproperties.name = { type = \"string\" }",
            "properties.name = { type = \"string\" }",
            "closes its properties",
        ),
        (
            "types.toml",
            "format = \"duration\"",
            "format = \"seconds\"",
            "format names a standard format",
        ),
        (
            "types.toml",
            "pattern = \"^[a-z0-9-]+$\"",
            "pattern = \"^[a-z\"",
            "pattern",
        ),
        (
            "types.toml",
            "schema = 1",
            "schema = 2",
            "isn't the format this build reads",
        ),
        (
            "actions.toml",
            "output = \"probed\"\n",
            "",
            "so it names its output",
        ),
        (
            "actions.toml",
            PUSH,
            &format!("{PUSH}\nsurfaces = [\"cli\", \"app\"]"),
            "names app only with rest",
        ),
        (
            "actions.toml",
            PUSH,
            &format!("{PUSH}\nsurfaces = [\"service\", \"cli\"]"),
            "names service alone",
        ),
        (
            "actions.toml",
            "spec = \"env\"\noutput = \"created\"\n",
            "spec = \"env\"\noutput = \"created\"\ntool = { name = \"create\", open-world = false }\n",
            "a tool's action binds no spec",
        ),
        (
            "actions.toml",
            "[actions.\"env.env.create\"]\n",
            "[actions.\"env.box.create\"]\ndescription = \"Make one\"\n\n[actions.\"env.env.create\"]\n",
            "is a shape in types.toml",
        ),
        (
            "actions.toml",
            "[actions.\"env.env.create\"]\n",
            "[actions.\"env.approval.new\"]\ndescription = \"Make one\"\n\n[actions.\"env.env.create\"]\n",
            "fixed verbs",
        ),
        (
            "actions.toml",
            "[verbs]\n",
            "[verbs]\nnew = { description = \"Make one\", effect = \"insert\" }\n",
            "say create",
        ),
        (
            "actions.toml",
            "x-link-cli = { position = 1 } }\nyes",
            "x-link-cli = { position = 2 } }\nyes",
            "positions run 1, 2, 3",
        ),
        (
            "actions.toml",
            "yes = { x-link-cli = { shared = \"yes\" } }",
            "yes = { x-link-cli = { shared = \"maybe\" } }",
            "isn't one of core's shared arguments",
        ),
        (
            "actions.toml",
            "secret = { type = \"string\"",
            "secret = { type = \"object\", additionalProperties = false",
            "an object written here is refused",
        ),
        (
            "actions.toml",
            "file = {",
            "file = { x-link-cli = { env = \"LNK_ENV_FILE\" },",
            "defined in no variables.toml",
        ),
        (
            "actions.toml",
            "spec = \"env\"",
            "spec = \"stage\"",
            "isn't a spec kind",
        ),
        (
            "actions.toml",
            "role = \"primary\"",
            "role = \"primary\"\nsurfaces = [\"web\"]",
            "surfaces lists",
        ),
        (
            "actions.toml",
            "role = \"primary\"",
            "role = \"primary\"\ncolour = \"red\"",
            "\"colour\" isn't a keyword",
        ),
        (
            "actions.toml",
            "docs = \"/services/environments#create-a-box\"",
            "docs = \"/services/environments\"",
            "docs is a page's address and a section's anchor",
        ),
        (
            "actions.toml",
            "x-link-cli = { position = 1 } }\nyes",
            "x-link-cli = { position = 1, short = \"c\" } }\nyes",
            "a positional argument has no short flag",
        ),
        (
            "actions.toml",
            "json = { x-link-cli = { shared = \"json\" } }",
            "json = { type = \"boolean\", x-link-cli = { position = 2 } }",
            "a switch is a flag, never positional",
        ),
        (
            "actions.toml",
            "json = { x-link-cli = { shared = \"json\" } }",
            "json = { type = \"string\", x-link-cli = { rest = true } }",
            "the rest is a list of text",
        ),
        (
            "actions.toml",
            "json = { x-link-cli = { shared = \"json\" } }",
            "json = { type = \"array\", items = { type = \"boolean\" } }",
            "lists text, choices or whole numbers",
        ),
        (
            "actions.toml",
            "json = { x-link-cli = { shared = \"json\" } }",
            "json = { type = \"string\", required = true, default = \"x\" }",
            "a required argument has no default",
        ),
        (
            "types.toml",
            "format = \"duration\",",
            "format = \"duration\", default = \"soon\",",
            "its default \"soon\"",
        ),
        (
            "types.toml",
            "x-link-markers = [\"run\", \"parallel\"]",
            "x-link-markers = [\"name\", \"parallel\"]",
            "marking key \"name\" is choice 1's too",
        ),
        (
            "types.toml",
            "x-link-markers = [\"run\", \"parallel\"]",
            "x-link-markers = [\"timeout\", \"parallel\"]",
            "marking key \"timeout\" is one it requires",
        ),
        (
            "types.toml",
            "x-link-markers = [\"run\", \"parallel\"]",
            "x-link-markers = [\"run\"]",
            "one marking key for each oneOf choice",
        ),
        (
            "types.toml",
            "x-link-markers = [\"run\", \"parallel\"]\n",
            "",
            "or the oneOf names each choice's marking key",
        ),
        (
            "types.toml",
            "x-link-path = \"confined\"",
            "x-link-path = \"outside\"",
            "x-link-path is true, or \"confined\"",
        ),
        (
            "actions.toml",
            "probe",
            "approval",
            "is also the start of env.approval.add",
        ),
        (
            "actions.toml",
            "x-link-cli = { short = \"a\" } }",
            "x-link-cli = { position = 1 } }",
            "never positional",
        ),
        (
            "actions.toml",
            "count = { type",
            "agent = { type = \"string\" }\ncount = { type",
            "argument agent is the module's",
        ),
        (
            "types.toml",
            "x-link-values = { small = \"Two processors and 4 GB\", large",
            "x-link-values = { tiny = \"Two processors and 4 GB\", large",
            "says the meaning of each of the enum's words, and only them",
        ),
        (
            "types.toml",
            "description = \"A domain, by its name\"",
            "description = \"A domain, by its name\"\nx-link-values = { a = \"b\" }",
            "x-link-values is an enum's",
        ),
        (
            "types.toml",
            "x-link-checks = [\"domain\"]",
            "x-link-checks = [\"Domain\"]",
            "names a hook",
        ),
        (
            "types.toml",
            "type = \"string\"\npattern = \"^[a-z][a-z0-9-]*$\"\nx-link-checks",
            "enum = [\"a\"]\nx-link-checks",
            "a choice is checked by its choices",
        ),
        (
            "types.toml",
            "propertyNames = \"domain\"",
            "propertyNames = \"stage\"",
            "a map's keys (propertyNames) are a text shape",
        ),
        (
            "actions.toml",
            "conflicts = [\"silent\"]",
            "conflicts = [\"loud\"]",
            "isn't another of its arguments",
        ),
        (
            "actions.toml",
            "x-link-cli = { requires = [\"here\"] }",
            "x-link-cli = { requires = [\"there\"] }",
            "requires \"there\", which isn't another of its arguments",
        ),
        (
            "actions.toml",
            "description = \"What to run\", x-link-cli = { rest = true }",
            "description = \"What to run\", x-link-cli = { rest = true, default-missing = \"x\" }",
            "default-missing is an optional flag's",
        ),
        (
            "actions.toml",
            "checks = [",
            "default = true\nchecks = [",
            "are both their group's default",
        ),
        (
            "actions.toml",
            "surfaces = [\"tool\"]",
            "surfaces = [\"tool\"]\ndefault = true",
            "its surfaces include cli",
        ),
        (
            "actions.toml",
            "[actions.\"env.env.status\".args]\nagent = false",
            "[actions.\"env.env.status\".args]\nnonesuch = false",
            "[args] declares no nonesuch",
        ),
        (
            "module.toml",
            "next = [\"env.env.create\", \"env.env.probe\"]",
            "next = [\"env.env.forgotten\"]",
            "isn't one of the module's actions",
        ),
        (
            "module.toml",
            "next = [\"env.env.create\", \"env.env.probe\"]",
            "next = [\"env.state.show\"]",
            "a person never types",
        ),
        (
            "actions.toml",
            "names = [\"/probe\"]",
            "names = [\"/probe\"]\ndocs = \"/services/environments#probe\"",
            "docs needs its own path",
        ),
        (
            "actions.toml",
            "surfaces = [\"tool\"]",
            "surfaces = []",
            "or none for an action its route alone offers",
        ),
        (
            "actions.toml",
            "[actions.\"env.env.probe\"]",
            "[actions.\"env.env.uninstall\"]\ndescription = \"x\"\noutput = \"probed\"\n[actions.\"env.env.probe\"]",
            "uninstall",
        ),
        (
            "actions.toml",
            "names = [\"/probe\"]",
            "names = [\"probe\"]",
            "is a slash and a lowercase name",
        ),
        (
            "actions.toml",
            "callers = [\"owner\", \"agent\"]",
            "callers = []",
            "callers names who may call it",
        ),
        (
            "actions.toml",
            "callers = [\"owner\", \"agent\"]",
            "callers = [\"stranger\"]",
            "route",
        ),
        (
            "actions.toml",
            "time_ms = 30000",
            "time_ms = 0",
            "its limits are positive",
        ),
        (
            "actions.toml",
            "bridge = \"harness\"",
            "bridge = \"harness\"\nlimits = { bytes = 1, time_ms = 1 }",
            "leave out limits",
        ),
        (
            "actions.toml",
            "command = true\nnames = [\"/probe\"]",
            "names = [\"/probe\"]",
            "say command = true",
        ),
        (
            "actions.toml",
            "quick = { type = \"boolean\", description = \"Skip the slow parts\" }\ntiming = { group = \"timing\" }",
            "quick = { type = \"boolean\", description = \"Skip the slow parts\" }\ntiming = { group = \"timings\" }",
            "a group is taken by an argument of its own name",
        ),
        (
            "actions.toml",
            "description = \"Skip the slow parts\" }",
            "description = \"Skip the slow parts\", x-link-cli = { last = true } }",
            "last is a rest's",
        ),
        (
            "actions.toml",
            "[actions.\"env.state.export\"]\ndescription = \"Write the environment's state, for Link\"\nrole = \"plumbing\"",
            "[actions.\"env.state.export\"]\ndescription = \"Write the environment's state, for Link\"",
            "only where every action of the group is plumbing",
        ),
        (
            "actions.toml",
            "default = 40 }",
            "default = 0 }",
            "its default 0",
        ),
        (
            "actions.toml",
            "description = \"Say more\", x-link-cli",
            "description = \"Say more\", default = true, x-link-cli",
            "only a text, a number or a choice has a default",
        ),
        (
            "actions.toml",
            "bridge = \"agent\"\ncallers = [\"owner\", \"agent\"]\nlimits = { bytes = 65536, time_ms = 30000 }",
            "bridge = \"agent\"\ncallers = [\"owner\", \"agent\"]",
            "names its limits",
        ),
        (
            "actions.toml",
            "surfaces = [\"tool\"]\noutput = \"probed\"\n\n[actions.\"env.env.inspect\".tool]",
            "surfaces = [\"cli\"]\noutput = \"probed\"\n\n[actions.\"env.env.inspect\".tool]",
            "its surfaces name tool",
        ),
        (
            "actions.toml",
            "inspect = { description = \"Look inside the environment, for whoever works in it\", effect = \"find\" }",
            "inspect = { description = \"Look inside the environment, for whoever works in it\" }",
            "its description and its effect",
        ),
        (
            "actions.toml",
            "name = \"inspect\"\nwhen",
            "name = \"Inspect\"\nwhen",
            "is a lowercase name",
        ),
        (
            "actions.toml",
            "when = \"inside\"",
            "when = \"outside\"",
            "one of [tools] conditions",
        ),
        (
            "actions.toml",
            "conditions = [\"inside\"]",
            "conditions = [\"inside\", \"idle\"]",
            "condition idle is declared and no tool action",
        ),
        (
            "actions.toml",
            "fills = [\"deepest\", \"places\"]",
            "fills = [\"deepest\", \"places\", \"most\"]",
            "fill most is declared and nothing names it",
        ),
        (
            "actions.toml",
            "at most {deepest} deep",
            "at most {deeper} deep",
            "{deeper} is one of [tools] fills",
        ),
        (
            "actions.toml",
            "at most {deepest} deep",
            "at most {deepest deep",
            "a { with no }",
        ),
        (
            "actions.toml",
            "x-link-fill = \"places\"",
            "x-link-fill = \"spots\"",
            "x-link-fill \"spots\" is one of [tools] fills",
        ),
        (
            "actions.toml",
            "place = { type = \"string\", x-link-fill = \"places\", description = \"Where it looks\", x-link-summary = 1 }",
            "place = { type = \"string\", enum = [\"here\"], x-link-fill = \"places\", description = \"Where it looks\", x-link-summary = 1 }",
            "a string with no enum",
        ),
        (
            "actions.toml",
            "depth = { type = \"integer\", minimum = 1, maximum = 9, description = \"How deep it looks\", x-link-summary = 2 }",
            "depth = { type = \"integer\", minimum = 1, maximum = 9, description = \"How deep it looks\", x-link-summary = 3 }",
            "places are 1, 2 and on",
        ),
        (
            "actions.toml",
            "place = { type = \"string\", x-link-fill = \"places\", description = \"Where it looks\", x-link-summary = 1 }",
            "place = { type = \"string\", x-link-fill = \"places\", description = \"Where it looks\", x-link-summary = 1, x-link-sensitive = true }",
            "is sensitive",
        ),
        (
            "actions.toml",
            "surfaces = [\"tool\"]\noutput = \"probed\"\n\n[actions.\"env.env.inspect\".tool]",
            "surfaces = [\"tool\"]\noutput = \"probed\"\ncontrol = true\n\n[actions.\"env.env.inspect\".tool]",
            "never a model's tool",
        ),
        (
            "actions.toml",
            "output = \"created\"\n",
            "output = \"created\"\nrest-args = [\"yes\"]\n",
            "which says where a machine runs it",
        ),
        (
            "actions.toml",
            "output = \"created\"\n",
            "output = \"created\"\nrest-args = [\"nothing\"]\n",
            "none of its arguments",
        ),
        (
            "actions.toml",
            "output = \"created\"\n",
            "output = \"created\"\nkeeps = { for = \"5m\" }\n",
            "keeps is a find's",
        ),
        (
            "actions.toml",
            "output = \"created\"\n",
            "output = \"created\"\nstream = true\n",
            "it streams, so its output is a union of event records told apart by a tag",
        ),
        (
            "actions.toml",
            "at most {deepest} deep\"",
            "at most {deepest} deep, in {places}\"",
            "is a description's text and an argument's choices",
        ),
        (
            "actions.toml",
            "quick = { type = \"boolean\", description = \"Skip the slow parts\" }",
            "quick = { type = \"boolean\", description = \"Skip the slow parts\", x-link-summary = 1 }",
            "it has no tool table",
        ),
    ];
    for (file, from, to, why) in cases {
        let refusal = refused(file, from, to);
        assert!(refusal.contains(why), "{file} with {to:?}: {refusal}");
    }
}

/// Core's shared shapes given in a copy, as a tree copied to another
/// machine gives them, are core's: told by what they declare, never by
/// where link-actions was built, which may not exist where it runs.
#[test]
fn core_shapes_given_elsewhere_are_core_shapes() {
    let dir = TempDir::new("core-shapes");
    let copy = dir.path().join("core");
    std::fs::create_dir(&copy).unwrap();
    for entry in std::fs::read_dir(link_actions::core_directory()).unwrap() {
        let path = entry.unwrap().path();
        std::fs::copy(&path, copy.join(path.file_name().unwrap())).unwrap();
    }
    let catalog = Catalog::load(&[&fixture(), &copy]).unwrap_or_else(|e| panic!("{e:#}"));
    let core = catalog.module("core").expect("core's shapes are loaded");
    assert_eq!(core.directory, copy, "the copy given, and no other");
    assert_eq!(
        catalog.action(&id("env.env.create")).unwrap().args["yes"]
            .cli
            .shared
            .as_deref(),
        Some("yes"),
        "the shared flag comes from the copy's shapes"
    );
}
