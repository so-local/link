//! Each surface generated from two fixture modules, `box` and `env`, which
//! depends on it: what each generator writes, and the rules it holds.

link_test_kit::program!(unit);

use std::path::{Path, PathBuf};

use link_actions::Catalog;
use link_actions_surfaces::{completion, openapi, pages, reference, rust, tools, typescript};
use serde_json::{Value, json};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn catalog() -> Catalog {
    let root = fixtures();
    Catalog::load(&[&root.join("box"), &root.join("env")]).unwrap_or_else(|e| panic!("{e:#}"))
}

#[test]
fn each_api_action_takes_its_method_and_inputs_from_its_effect() {
    let document = openapi::document(&catalog()).unwrap();
    assert_eq!(document["openapi"], "3.1.0");
    let create = &document["paths"]["/_link/box/box/create"]["post"];
    assert_eq!(create["operationId"], "box.box.create");
    assert_eq!(create["x-link-command"], "lnk box create");
    assert_eq!(create["x-link-effect"], "insert");
    assert_eq!(
        create["requestBody"]["content"]["application/json"]["schema"],
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["cloud", "spec"],
            "properties": {
                "cloud": {"description": "The cloud account to make it on", "type": "string"},
                "spec": {"$ref": "#/components/schemas/box.machine"},
            },
        }),
        "an insert's inputs travel in its body, its spec beside its arguments"
    );
    assert_eq!(
        create["responses"]["200"]["content"]["application/json"]["schema"],
        json!({"$ref": "#/components/schemas/box.box"}),
        "create answers its resource"
    );
    assert!(
        create["parameters"].is_null(),
        "nothing names what it acts on, so nothing is in its query"
    );
    assert_eq!(
        create["responses"]["4XX"]["content"]["application/json"]["schema"],
        json!({"$ref": "#/components/schemas/core.error"}),
        "every failure is core's error"
    );
    let delete = &document["paths"]["/_link/box/box/delete"]["delete"];
    let parameters: Vec<&str> = delete["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        parameters,
        ["id", "on"],
        "a delete's inputs are its query, and REST never takes --yes"
    );
    assert_eq!(
        delete["responses"]["200"]["content"]["application/json"]["schema"]["x-link-kind"], "box",
        "delete answers its resource's ID"
    );
    let list = &document["paths"]["/_link/box/box/list"]["get"];
    assert_eq!(
        list["responses"]["200"]["headers"]["Cache-Control"]["schema"]["const"],
        "private, max-age=60",
        "a find keeps its answer as it declares"
    );
    let watch = &document["paths"]["/_link/box/box/watch"]["get"];
    let events = &watch["responses"]["200"]["content"]["text/event-stream"];
    assert_eq!(
        events["x-link-event"], "event",
        "each event named by its tag"
    );
    assert_eq!(events["schema"]["$ref"], "#/components/schemas/box.change");
    assert!(
        document["paths"]["/_link/box/box/serve"].is_null(),
        "plumbing is the command line's alone"
    );
    let env = &document["components"]["schemas"]["env.env"];
    assert_eq!(
        env["properties"]["box"]["$ref"], "#/components/schemas/box.machine",
        "a dependency's shape is its component"
    );
    assert_eq!(
        env["properties"]["stages"]["items"],
        json!({"$ref": "#/components/schemas/env.stage"})
    );
    let text = serde_json::to_string(&document).unwrap();
    assert!(!text.contains("#/$defs/"), "every reference is a component");
}

#[test]
fn typescript_declares_every_shape_and_a_client_by_the_ids_words() {
    let files = typescript::generate(&catalog()).unwrap();
    let types = files.get("types.ts").unwrap();
    assert!(types.contains("export type BoxMachine = {"), "{types}");
    assert!(
        types.contains("  \"size\": \"small\" | \"large\";"),
        "{types}"
    );
    assert!(types.contains("  \"box\"?: BoxMachine;"), "{types}");
    assert!(types.contains("  \"stages\"?: Array<EnvStage>;"), "{types}");
    assert!(types.contains("export type BoxBoxCreateArgs = {\n  \"cloud\": string;\n};"));
    assert!(types.contains("export type BoxBoxListOutput = Array<BoxBox>;"));
    assert!(types.contains("export type EnvApprovalAddOutput = EnvApproval;"));
    // An open record's other keys, and a choice told by its value.
    assert!(types.contains("} & { [key: string]: EnvTool };"), "{types}");
    assert!(
        types.contains("export type EnvCompactAt = number | \"never\";"),
        "{types}"
    );
    let client = files.get("client.ts").unwrap();
    assert!(
        client.contains(
            "\"create\": (args: T.BoxBoxCreateArgs, spec: T.BoxMachine): Promise<T.BoxBoxCreateOutput> =>"
        ),
        "{client}"
    );
    assert!(
        client.contains("\"list\": (args: T.BoxBoxListArgs = {}): Promise<T.BoxBoxListOutput> =>"),
        "an action with no required argument may be called with none: {client}"
    );
    assert!(client.contains("path: \"/_link/env/approval/add\""));
    assert!(
        client.contains(
            "transport.send({ action: \"box.box.create\", path: \"/_link/box/box/create\", method: \"POST\", query: {}, body: { \"cloud\": args[\"cloud\"], \"spec\": spec } })"
        ),
        "an insert's inputs go in its body: {client}"
    );
    assert!(
        client.contains(
            "method: \"DELETE\", query: { \"id\": args[\"id\"], \"on\": args[\"on\"] } }"
        ),
        "a delete's inputs are its query: {client}"
    );
    assert!(
        client.contains(
            "\"watch\": (args: T.BoxBoxWatchArgs): AsyncIterable<T.BoxBoxWatchOutput> =>"
        ) && client.contains("stream: \"event\" }"),
        "a stream yields its events, each named by its tag: {client}"
    );
    assert!(client.contains("  \"conflict\": 409,"), "{client}");
}

#[test]
fn two_names_typescript_would_read_alike_are_refused() {
    let temp = link_test_kit::temp::TempDir::new("surfaces-alike");
    for (file, text) in [
        (
            "module.toml",
            "schema = 1\n[module]\nid = \"a\"\nkind = \"module\"\nowner = \"modules/a\"\n",
        ),
        (
            "types.toml",
            "schema = 1\n[a-create-args]\ntype = \"string\"\n",
        ),
        (
            "actions.toml",
            "schema = 1\n[actions.\"a.a.create\"]\ndescription = \"Make one\"\noutput = \"a-create-args\"\n",
        ),
    ] {
        std::fs::write(temp.join(file), text).unwrap();
    }
    let catalog = Catalog::load(&[temp.as_ref()]).unwrap();
    let refused = typescript::generate(&catalog).unwrap_err().to_string();
    assert!(refused.contains("AACreateArgs"), "{refused}");
}

#[test]
fn each_app_action_has_a_page_naming_its_action() {
    let component = pages::Component {
        name: "ActionPage",
        import: "@/src/ActionPage",
    };
    let files = pages::generate(&catalog(), component).unwrap();
    let paths: Vec<_> = files.paths().map(|p| p.display().to_string()).collect();
    assert_eq!(
        paths,
        [
            "box/box/create.tsx",
            "box/box/list.tsx",
            "env/approval/add.tsx",
            "env/env/create.tsx"
        ],
        "delete isn't offered in the app, and serve is plumbing"
    );
    let page = files.get("env/env/create.tsx").unwrap();
    assert!(
        page.contains("<ActionPage action={\"env.env.create\"} />"),
        "{page}"
    );
}

#[test]
fn each_tool_takes_its_arguments_and_spec_as_one_object() {
    let tools = tools::tools(&catalog()).unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "box_box_create",
            "box_box_list",
            "env_approval_add",
            "env_env_create"
        ]
    );
    let create = &tools[3];
    assert_eq!(create.description, "Make a box from an environment file");
    let input = &create.input_schema;
    assert_eq!(input["required"], json!(["cloud", "spec"]));
    assert!(
        input["properties"]["yes"].is_null(),
        "a model never says yes for the person"
    );
    assert!(input["properties"]["json"].is_null());
    assert_eq!(input["properties"]["spec"]["$ref"], "#/$defs/env.env");
    let defs: Vec<&String> = input["$defs"].as_object().unwrap().keys().collect();
    assert_eq!(
        defs,
        ["box.machine", "box.port", "env.env", "env.stage"],
        "every shape the input reaches, a dependency's among them"
    );
    assert_eq!(input["additionalProperties"], Value::Bool(false));
    assert!(
        !input.to_string().contains("x-link-"),
        "no x-link-* keyword reaches a model's provider: {input}"
    );
}

#[test]
fn a_programs_own_call_is_its_templates_alone() {
    let tools = tools::tools(&catalog()).unwrap();
    assert!(
        tools.iter().all(|t| t.name != "env_env_show"),
        "env.env.show has a tool table, so its program offers it"
    );
    let template = tools::template(&catalog(), "env").unwrap();
    let names: Vec<&str> = template.tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["environment"]);
    // Its one argument is optional: no `required` at all, never null,
    // which MCP clients (OpenClaw's among them) refuse a whole server for.
    let input = &template.tools[0].input_schema;
    assert!(input.get("required").is_none(), "{input}");
}

fn references() -> link_actions_surfaces::Files {
    reference::generate(&catalog(), &fixtures(), 1).unwrap()
}

#[test]
fn references_name_every_command_spec_field_and_variable() {
    let files = references();
    let commands = files.get("box/docs/more/reference/modules/box.md").unwrap();
    assert!(commands.contains("## `lnk box create`"), "{commands}");
    assert!(
        commands.contains("lnk box create --cloud <cloud> [--json] [--spec <file>] [field flags]"),
        "{commands}"
    );
    assert!(
        commands.contains(
            "It takes `--cloud`, `-c`, string, required, else `LNK_BOX_CLOUD`: The cloud account to make it on."
        ),
        "{commands}"
    );
    assert!(
        commands.contains("It reads the [`box` specification](../../../config/specs/box.md)"),
        "{commands}"
    );
    assert!(!commands.contains("serve"), "plumbing is hidden");
    assert!(
        !commands.contains("--on"),
        "an argument kept out of help is kept out of its usage and list: {commands}"
    );
    let env = files.get("env/docs/more/reference/modules/env.md").unwrap();
    assert!(
        !env.contains("compact"),
        "a command the harness runs is no command of its module's: {env}"
    );
    assert!(
        env.contains("[`env` specification](../../../config/specs/env.md)"),
        "{env}"
    );

    // Its own declared spec file, written once.
    assert!(
        env.contains("lnk env create <cloud> [--json] --spec <spec> [--yes] [field flags]"),
        "{env}"
    );
    let spec = files.get("env/docs/config/specs/env.md").unwrap();
    // One layout: how to use it, Every Option as TOML, then an H2 per
    // top-level key and an H3 per field, each opening with its quoted line.
    assert!(spec.contains("## Use It\n"), "{spec}");
    assert!(
        spec.contains("## Every Option\n\nEvery field the reader takes, with an example value.\n\n```toml\nkind = \"env\"\nschema = 1\n"),
        "{spec}"
    );
    assert!(
        spec.contains("## `name`\n\n> **Type** text · **Flag** `--spec.name`\n\nIts name"),
        "{spec}"
    );
    // A description of several paragraphs is written whole.
    assert!(
        spec.contains("from the home folder\n\nA relative path is the home folder's, and an absolute one is used as\nwritten."),
        "{spec}"
    );
    // Another spec kind held whole is linked to its own page, never
    // written out again.
    assert!(
        spec.contains(
            "Its fields are the [Box Specification](../../../../box/docs/config/specs/box.md)'s"
        ),
        "{spec}"
    );
    assert!(!spec.contains("### `gigabytes`"), "{spec}");
    assert!(
        spec.contains("### `run`\n\n> **Type** text · **Flag** in `--stages`, whole elements"),
        "a list's items have no flag: {spec}"
    );
    assert!(
        spec.contains("### `<name>`\n\n> **Type** text · **Flag** `--variables.<name>`"),
        "a map's values take a flag each, as the command line parses them: {spec}"
    );
    // Box's fields are on its own page.
    let machine = files.get("box/docs/config/specs/box.md").unwrap();
    assert!(machine.contains("## `token`"), "{machine}");
    assert!(
        machine.contains("### `gigabytes`") && machine.contains("`--disk.gigabytes`"),
        "{machine}"
    );
    assert!(
        machine.contains("## `boot`") && machine.contains("`image`"),
        "a choice's variants list their fields: {machine}"
    );
    let schema: Value =
        serde_json::from_str(files.get("env/docs/config/specs/env.schema.json").unwrap()).unwrap();
    assert_eq!(schema["$schema"], link_actions::dialect());
    assert!(schema["$defs"]["box.machine"].is_object());

    let hub = files.get("docs/more/reference/modules.md").unwrap();
    assert!(
        hub.contains("[Box](../../../box/docs/more/reference/modules/box.md) and [Env](../../../env/docs/more/reference/modules/env.md)."),
        "the hub introduces each module's page: {hub}"
    );
    let variables = files.get("docs/more/reference/variables.md").unwrap();
    assert!(
        variables.contains(
            "- `LNK_BOX_CLOUD`, string, from `box`: The cloud account a box is made on when no flag names one."
        ),
        "{variables}"
    );
}

/// Every generated page is whole, however many commands or fields it has,
/// and every link it holds resolves to another generated page.
#[test]
fn every_reference_is_one_page_whose_links_resolve() {
    let files = references();
    for path in files.paths() {
        if path.extension().is_none_or(|e| e != "md") {
            continue;
        }
        let text = files.get(path).unwrap();
        assert!(!text.contains("Part 1"), "{}:\n{text}", path.display());
        for link in text.split("](").skip(1) {
            let target = &link[..link.find(')').unwrap()];
            if target.starts_with("http") || target.starts_with('#') {
                continue;
            }
            let resolved = normalize(&path.parent().unwrap().join(target));
            assert!(
                files.get(&resolved).is_some(),
                "{} links {target}, which isn't generated",
                path.display()
            );
        }
    }
}

/// A path with its `..` folded, as a link resolves.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// A module whose commands act on several resources has one page, a
/// heading for each resource and its commands beneath.
#[test]
fn a_module_of_several_resources_is_one_page_by_resource() {
    let root = link_test_kit::temp::TempDir::new("big");
    let dir = root.path().join("big");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("module.toml"),
        "schema = 1\n[module]\nid = \"big\"\nkind = \"module\"\nowner = \"big\"\n",
    )
    .unwrap();
    let mut types = String::from("schema = 1\n");
    let mut actions = String::from("schema = 1\n");
    for resource in ["alpha", "beta", "gamma"] {
        types += &format!(
            "[{resource}]\ntype = \"object\"\ndescription = \"An {resource}\"\nadditionalProperties = false\nproperties.name = {{ type = \"string\" }}\n"
        );
        for verb in ["show", "list"] {
            actions += &format!(
                "[actions.\"big.{resource}.{verb}\"]\ndescription = \"The {resource}, {verb}n\"\n"
            );
        }
    }
    std::fs::write(dir.join("types.toml"), types).unwrap();
    std::fs::write(dir.join("actions.toml"), actions).unwrap();
    let catalog = Catalog::load(&[&dir]).unwrap_or_else(|e| panic!("{e:#}"));
    let files = reference::generate(&catalog, root.path(), 1).unwrap();
    let page = files.get("big/docs/more/reference/modules/big.md").unwrap();
    for resource in ["Alpha", "Beta", "Gamma"] {
        assert!(page.contains(&format!("\n## {resource}\n")), "{page}");
    }
    assert!(page.contains("\n### `lnk big alpha show`\n"), "{page}");
    assert_eq!(
        files
            .paths()
            .filter(|p| p.starts_with("big/docs/more/reference"))
            .count(),
        1,
        "one page"
    );
}

/// A kind a package owns names no `spec show`, since a package owns no
/// command, and with no command reading it, says no command at all.
#[test]
fn a_package_owned_kinds_page_names_no_command_it_lacks() {
    let root = fixtures();
    let catalog = Catalog::load(&[&root.join("box"), &root.join("env"), &root.join("rules")])
        .unwrap_or_else(|e| panic!("{e:#}"));
    let files = reference::generate(&catalog, &root, 1).unwrap();
    let page = files.get("rules/docs/config/specs/rules.md").unwrap();
    assert!(!page.contains("spec show"), "{page}");
    assert!(!page.contains("lnk rules"), "{page}");
    assert!(page.contains("`allow`"), "{page}");
    // A module's kind still names its own.
    let env = files.get("env/docs/config/specs/env.md").unwrap();
    assert!(env.contains("lnk env spec show"), "{env}");
}

/// A kind a package owns is shown by each module binding it, with that
/// module's own `spec show`, and its page names that command.
#[test]
fn a_package_owned_kind_is_shown_by_the_module_binding_it() {
    let root = fixtures();
    let catalog = Catalog::load(&[
        &root.join("box"),
        &root.join("env"),
        &root.join("rules"),
        &root.join("guard"),
    ])
    .unwrap_or_else(|e| panic!("{e:#}"));
    let files = reference::generate(&catalog, &root, 1).unwrap();
    let page = files.get("rules/docs/config/specs/rules.md").unwrap();
    assert!(page.contains("`lnk guard check`"), "{page}");
    assert!(
        page.contains("`lnk guard rules spec show` prints the spec"),
        "{page}"
    );
    assert!(!page.contains("lnk rules"), "{page}");
}

/// Completion offers each command a person types, its runtime flags and
/// its field flags, a closed set's values among them, and no plumbing.
#[test]
fn completion_offers_commands_flags_and_values_but_no_plumbing() {
    let root = link_actions::core_directory().parent().unwrap();
    let files = completion::generate(&catalog(), root).unwrap();
    assert_eq!(files.len(), 2);
    for path in files.paths() {
        let text = files.get(path).unwrap();
        assert!(text.contains("'lnk box'"), "{text}");
        assert!(!text.contains("serve"), "plumbing offered: {text}");
        assert!(text.contains("--cloud") && text.contains("-c"), "{text}");
        assert!(
            text.contains("--size") && text.contains("--disk.gigabytes"),
            "{text}"
        );
        assert!(
            text.contains("'lnk box create --size' 'small large'")
                || text.contains("['lnk box create --size']='small large'"),
            "{text}"
        );
        // A runtime flag's named enum, read through its name.
        assert!(
            text.contains("'lnk box list --order' 'newest oldest'")
                || text.contains("['lnk box list --order']='newest oldest'"),
            "{text}"
        );
    }
    // A workspace without core writes none: the one holding it does.
    assert!(
        completion::generate(&catalog(), &fixtures())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn rust_declares_each_reached_shape_and_a_request_per_action() {
    let files = rust::generate(&catalog()).unwrap();
    let types = files.get("types.rs").unwrap();
    assert!(types.contains("pub struct BoxMachine {"), "{types}");
    assert!(
        types.contains("#[serde(tag = \"event\")]\npub enum BoxChange {"),
        "a stream's events are a tagged enum: {types}"
    );
    let client = files.get("client.rs").unwrap();
    assert!(
        client.contains("pub fn box_box_create(args: &BoxBoxCreateArgs, spec: &BoxMachine) -> crate::Request<BoxBoxCreateOutput> {"),
        "{client}"
    );
    assert!(
        client.contains("crate::Method::Delete, args, &[\"id\", \"on\"], &[], None, None)"),
        "a delete's inputs are its query: {client}"
    );
    assert!(
        client.contains("Some(\"event\"))"),
        "a stream names its tag: {client}"
    );
    let handler = files.get("handler.rs").unwrap();
    assert!(
        handler.contains("fn box_box_watch(&self, caller: &Self::Caller, args: BoxBoxWatchArgs) -> impl ::std::future::Future<Output = Result<crate::Events<BoxBoxWatchOutput>, ::link_types::Thrown>> + Send {"),
        "a server's method for a stream answers its events: {handler}"
    );
    assert!(
        handler.contains("        \"box.box.create\" => {"),
        "{handler}"
    );
    assert!(
        handler.contains("crate::Served { action: \"box.box.list\", path: \"/_link/box/box/list\", method: crate::Method::Get, stream: None, cache: Some(\"private, max-age=60\") },"),
        "{handler}"
    );
}
