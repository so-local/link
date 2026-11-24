//! A version 1 `actions.toml`, read through `link-actions`, as the view
//! every reader of [`super::Actions`] takes, until every module is
//! declared in version 1 and the view is the catalog's.

use std::collections::BTreeMap;

use anyhow::Context;
use link_actions::{ActionId, Arg, Role, Written, schema::Node};
use toml::{Table, Value};

/// What a module's groups say, as its generated parser says it, and the
/// sections explaining them: the module's own group its package's
/// description and its topic page's title, and each resource's group its
/// shape's description in `types.toml`.
#[derive(Default)]
pub(crate) struct Said {
    pub module: Option<String>,
    /// The docs section of the module's own group.
    pub docs: Option<String>,
    pub resources: BTreeMap<String, String>,
    /// The module's ID, its namespace where its file declares no action
    /// of its own and implements a contract's.
    pub id: Option<String>,
    /// Each contract it implements, its `actions.toml` text by its ID:
    /// none where nothing looked them up.
    pub contracts: Option<BTreeMap<String, String>>,
    /// Each spec kind a package it depends on declares, by its shape's
    /// name (`policy.policy`): a package owns no command, so a module
    /// binding its kind shows it, as the generated parser does.
    pub package_kinds: BTreeMap<String, String>,
}

impl Said {
    /// What the module in `directory` says, its package describing it as
    /// `description`.
    pub(crate) fn read(
        directory: &std::path::Path,
        file: &link_actions::ActionsFile,
        description: Option<&str>,
    ) -> anyhow::Result<Self> {
        let id = super::implements::module_id(directory)?;
        let namespace = match file.actions.keys().next() {
            Some(action) => action.namespace().to_string(),
            None => id.clone().unwrap_or_default(),
        };
        // Each resource described by its shape: the module's own, and
        // those of each contract it implements.
        let mut resources = BTreeMap::new();
        let mut contracts = BTreeMap::new();
        for (contract, folder) in super::implements::contracts(directory, &file.implements)? {
            resources.extend(described(&folder, &contract)?);
            let path = folder.join("actions.toml");
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            contracts.insert(contract, text);
        }
        resources.extend(described(directory, &namespace)?);
        let mut package_kinds = BTreeMap::new();
        for folder in super::implements::dependencies(directory)? {
            package_kinds.extend(package_kinds_in(&folder)?);
        }
        Ok(Self {
            module: description.map(str::to_string),
            docs: topic(directory)?,
            resources,
            id,
            contracts: Some(contracts),
            package_kinds,
        })
    }
}

/// The spec kinds the module in `directory` declares, by `<id>.<shape>`,
/// where it's a package: it declares no action, so no command of its own
/// shows them (`link_actions::package_kind`).
fn package_kinds_in(directory: &std::path::Path) -> anyhow::Result<BTreeMap<String, String>> {
    let Some(id) = super::implements::module_id(directory)? else {
        return Ok(BTreeMap::new());
    };
    let actions = directory.join("actions.toml");
    if actions.is_file() {
        let text = std::fs::read_to_string(&actions)
            .with_context(|| format!("reading {}", actions.display()))?;
        let table: toml::Table =
            toml::from_str(&text).with_context(|| format!("{}", actions.display()))?;
        if table
            .get("actions")
            .and_then(toml::Value::as_table)
            .is_some_and(|a| !a.is_empty())
        {
            return Ok(BTreeMap::new());
        }
    }
    let types = directory.join("types.toml");
    if !types.is_file() {
        return Ok(BTreeMap::new());
    }
    let text =
        std::fs::read_to_string(&types).with_context(|| format!("reading {}", types.display()))?;
    let table: toml::Table =
        toml::from_str(&text).with_context(|| format!("{}", types.display()))?;
    Ok(table
        .iter()
        .filter_map(|(name, shape)| {
            let kind = shape.get("x-link-spec")?.get("kind")?.as_str()?;
            Some((format!("{id}.{name}"), kind.to_string()))
        })
        .collect())
}

/// The description of each shape `types.toml` in `directory` declares, by
/// its name, `namespace` the module's.
fn described(
    directory: &std::path::Path,
    namespace: &str,
) -> anyhow::Result<BTreeMap<String, String>> {
    let types = directory.join("types.toml");
    if !types.is_file() {
        return Ok(BTreeMap::new());
    }
    let text =
        std::fs::read_to_string(&types).with_context(|| format!("reading {}", types.display()))?;
    Ok(link_actions::read_types(&text, namespace)?
        .into_iter()
        .filter_map(|(name, shape)| Some((name, shape.schema.meta.description?)))
        .collect())
}

/// The section of the module's topic page (`module.toml`'s
/// `[documentation]`) its title heads: `/services/environments#environments`.
fn topic(directory: &std::path::Path) -> anyhow::Result<Option<String>> {
    let path = directory.join("module.toml");
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let table: Table = toml::from_str(&text).with_context(|| path.display().to_string())?;
    let Some(documentation) = table.get("documentation").and_then(Value::as_table) else {
        return Ok(None);
    };
    let (Some(topic), Some(root)) = (
        documentation.get("topic").and_then(Value::as_str),
        documentation.get("root").and_then(Value::as_str),
    ) else {
        return Ok(None);
    };
    let page = directory.join(root).join(topic).join("README.md");
    let Ok(text) = std::fs::read_to_string(&page) else {
        return Ok(None);
    };
    let title = text.lines().find_map(|line| line.strip_prefix("# "));
    Ok(title.map(|title| format!("/{topic}#{}", slug(title))))
}

/// A heading's anchor, as the website makes it: lowercase, its words
/// joined by hyphens, other punctuation dropped.
fn slug(title: &str) -> String {
    title
        .trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect()
}

/// The view of `text`, a version 1 `actions.toml`, written in the old
/// format's table and read by the old format's reader, so its checks hold.
pub(crate) fn view(text: &str, said: &Said) -> anyhow::Result<super::Actions> {
    let mut file = link_actions::read_actions(text)?;
    let mut out = Table::new();
    let Some(namespace) = file
        .actions
        .keys()
        .next()
        .map(|id| id.namespace().to_string())
        .or_else(|| said.id.clone())
    else {
        return Ok(super::Actions::new());
    };
    // A contract's actions it implements are its own commands.
    let imported = match &said.contracts {
        Some(contracts) => super::implements::import(&mut file, &namespace, contracts)?,
        None => Vec::new(),
    };
    if file.actions.is_empty() {
        return Ok(super::Actions::new());
    }
    let mut group = Table::new();
    let about = said
        .module
        .clone()
        .unwrap_or_else(|| format!("The {namespace} module's commands"));
    group.insert("help".into(), about.into());
    // A namespace Link alone runs, every action plumbing, as a contract's
    // protocol, is plumbing too: on no page a person reads. A tool's calls,
    // off the command line, are no command of it.
    let plumbing = file
        .actions
        .values()
        .filter(|action| {
            action
                .surfaces
                .as_ref()
                .is_none_or(|s| s.contains(&link_actions::Surface::Cli))
        })
        .all(|action| action.role == Role::Plumbing);
    if let Some(docs) = said.docs.as_ref().filter(|_| !plumbing) {
        group.insert("docs".into(), docs.clone().into());
    }
    let role = if plumbing { "plumbing" } else { "secondary" };
    group.insert("role".into(), role.into());
    group.insert("dispatch".into(), true.into());
    // Core's program is lnk itself: no group of its namespace, and its
    // resources' groups are their words alone (`module`).
    let core = namespace == link_actions::core().module;
    if !core {
        if let Some(alias) = file.aliases.get(&namespace) {
            group.insert("alias".into(), alias.clone().into());
        }
        out.insert(namespace.clone(), group.into());
    }
    let mut kinds = Vec::new();
    // A group is of the commands a person types: an action off the command
    // line makes none.
    let typed = |action: &Written| {
        action
            .surfaces
            .as_ref()
            .is_none_or(|s| s.contains(&link_actions::Surface::Cli))
    };
    for (id, _) in file.actions.iter().filter(|(_, action)| typed(action)) {
        let resource = id.resource().to_string();
        let path = match core {
            true => resource.clone(),
            false => format!("{namespace} {resource}"),
        };
        if resource != namespace
            && !out.contains_key(&path)
            && let Some(about) = said.resources.get(&resource)
        {
            let mut group = Table::new();
            group.insert("help".into(), about.clone().into());
            // The page most of its commands name explains it too (the
            // first such section, where two pages tie).
            let named: Vec<&String> = file
                .actions
                .iter()
                .filter(|(other, other_action)| other.resource() == resource && typed(other_action))
                .filter_map(|(_, other)| other.docs.as_ref())
                .collect();
            let page = |docs: &String| docs.split('#').next().unwrap_or_default().to_string();
            let count = |docs: &String| {
                named
                    .iter()
                    .filter(|other| page(other) == page(docs))
                    .count()
            };
            let most = named.iter().map(|docs| count(docs)).max().unwrap_or(0);
            let docs = named.iter().find(|docs| count(docs) == most);
            if let Some(docs) = docs {
                group.insert("docs".into(), (*docs).clone().into());
            }
            // A group of plumbing alone is plumbing, which help never shows.
            let plumbing = file
                .actions
                .iter()
                .filter(|(other, _)| other.resource() == resource)
                .all(|(_, other)| other.role == Role::Plumbing);
            let role = if plumbing { "plumbing" } else { "secondary" };
            group.insert("role".into(), role.into());
            if let Some(alias) = file.aliases.get(&path) {
                group.insert("alias".into(), alias.clone().into());
            }
            out.insert(path, group.into());
        }
    }
    for (id, action) in &file.actions {
        if let Some(spec) = &action.spec {
            kinds.push(spec.clone());
        }
        // Every action takes the module's own arguments too, but those it
        // leaves out.
        // A call a tool's program serves over MCP alone, off the command
        // line with no route, is no command of Link's.
        if !typed(action) && action.route.is_none() {
            continue;
        }
        let mut action = action.clone();
        let without = action.without.clone();
        action.args.extend(
            file.args
                .iter()
                .filter(|(name, _)| !without.contains(name))
                .map(|(name, arg)| (name.clone(), arg.clone())),
        );
        let reads = file
            .effect(id.verb())
            .is_some_and(link_actions::Effect::reads_only);
        let mut entry = entry(&action, reads)?;
        // A suggested action its group runs bare is that group's
        // command too, as `lnk measure` is `lnk measure list`.
        for (key, ids) in [("next", &action.next), ("fail", &action.fail)] {
            if let Some(Value::Array(said)) = entry.get_mut(key) {
                for id in ids
                    .iter()
                    .filter(|id| file.actions.get(*id).is_some_and(|a| a.default))
                {
                    let command = id.command();
                    if let Some((group, _)) = command.rsplit_once(' ') {
                        said.push(Value::String(group.to_string()));
                    }
                }
            }
        }
        // A contract's `show`, copied in, diagnoses nothing of the
        // implementer's: one kind of name has one diagnosis, which its
        // command declares (`diagnoses`), never one implied by its verb.
        if imported.contains(id) {
            entry.remove("diagnoses");
        }
        // A route served at a path of its own is two entries: its command,
        // and its route at that path, which the bridge reads as today.
        if let Some(path) = action.route.as_ref().and_then(|r| r.path.clone()) {
            let mut route = entry.clone();
            for key in [
                "bridge",
                "callers",
                "answer",
                "carries",
                "streams",
                "limits",
                "aliases",
                "read_only",
            ] {
                entry.remove(key);
            }
            entry.insert("surfaces".into(), Value::Array(vec!["cli".into()]));
            if let Some(Value::Array(surfaces)) = route.get_mut("surfaces") {
                surfaces.retain(|s| s.as_str() != Some("cli"));
            }
            route.remove("diagnoses");
            // Its route's own section, where it declares one.
            if let Some(docs) = action.route.as_ref().and_then(|r| r.docs.clone()) {
                route.insert("docs".into(), docs.into());
            }
            anyhow::ensure!(
                !out.contains_key(&path),
                "{id}: its route's path {path:?} is another command's"
            );
            out.insert(path, route.into());
            // Off the command line, its route is all of it.
            if !typed(&action) {
                continue;
            }
        }
        out.insert(id.typed().join(" "), entry.into());
    }
    // Each of its own kinds has its spec show, and each a package
    // declares that it binds; another module's kind is shown by its owner.
    let mut shown: Vec<String> = kinds
        .iter()
        .filter_map(|k| match k.contains('.') {
            false => Some(k.clone()),
            true => said.package_kinds.get(k).cloned(),
        })
        .collect();
    shown.sort();
    shown.dedup();
    for kind in &shown {
        let words = link_actions::spec_show(&namespace, kind);
        // A second kind's word is a group of its own where no resource
        // of the module is (`iam policy`), as the parser has it.
        if words.len() > 2 {
            let above = format!("{namespace} {}", words[0]);
            if !out.contains_key(&above) {
                let mut group = Table::new();
                group.insert("help".into(), format!("The {kind} spec").into());
                group.insert("role".into(), "secondary".into());
                out.insert(above, group.into());
            }
        }
        let group = format!("{namespace} {}", words[..words.len() - 1].join(" "));
        let path = format!("{namespace} {}", words.join(" "));
        let mut spec = Table::new();
        spec.insert(
            "help".into(),
            format!("The {kind} spec's own commands").into(),
        );
        let docs = link_actions::core().spec_docs.replace("{kind}", kind);
        spec.insert("docs".into(), docs.clone().into());
        spec.insert("role".into(), "secondary".into());
        out.insert(group, spec.into());
        let mut show = Table::new();
        show.insert(
            "help".into(),
            "Print the effective spec, each default written in"
                .to_string()
                .into(),
        );
        show.insert("spec".into(), kind.clone().into());
        show.insert("docs".into(), docs.into());
        show.insert("role".into(), "secondary".into());
        let mut args = Table::new();
        args.insert("spec".into(), runtime_spec(kind).into());
        let mut json = Table::new();
        json.insert("type".into(), "boolean".into());
        json.insert(
            "values".into(),
            Value::Array(vec![true.into(), false.into()]),
        );
        json.insert("long".into(), "json".into());
        json.insert("default".into(), false.into());
        json.insert("shared".into(), "output".into());
        args.insert("json".into(), json.into());
        // Core's `link_spec::Show`: an existing thing whose spec to print.
        let mut thing = Table::new();
        thing.insert("type".into(), "string".into());
        thing.insert(
            "help".into(),
            "An existing thing whose spec to print, as made again".into(),
        );
        thing.insert("position".into(), Value::Integer(1));
        thing.insert("runtime".into(), true.into());
        args.insert("thing".into(), thing.into());
        show.insert("args".into(), args.into());
        out.insert(path, show.into());
    }
    let text = toml::to_string(&out)?;
    super::checked(&text).context("the view of a version 1 actions.toml")
}

fn entry(action: &Written, reads: bool) -> anyhow::Result<Table> {
    let mut entry = Table::new();
    entry.insert("help".into(), action.description.trim_end().into());
    if let Some(docs) = &action.docs {
        entry.insert("docs".into(), docs.clone().into());
    }
    let role = match action.role {
        Role::Primary => "primary",
        Role::Secondary => "secondary",
        Role::Plumbing => "plumbing",
    };
    entry.insert("role".into(), role.into());
    if action.default {
        entry.insert("default".into(), true.into());
    }
    let mut args = Table::new();
    for (name, arg) in &action.args {
        args.insert(name.replace('-', "_"), option(name, arg)?.into());
    }
    if let Some(spec) = &action.spec {
        // Another module's kind, `sandbox.sandbox`, by its kind's word, as
        // the old format names every kind.
        let kind = spec.rsplit('.').next().unwrap_or(spec);
        entry.insert("spec".into(), kind.into());
        if !action.args.contains_key("spec") {
            args.insert("spec".into(), runtime_spec(kind).into());
        }
    }
    if !args.is_empty() {
        entry.insert("args".into(), args.into());
    }
    let commands = |ids: &[ActionId]| -> Value {
        Value::Array(ids.iter().map(|id| Value::String(id.command())).collect())
    };
    if !action.next.is_empty() || action.next_modules {
        let mut next = match commands(&action.next) {
            Value::Array(next) => next,
            _ => Vec::new(),
        };
        // Leading into every module, as `lnk up` does: `lnk <module>`.
        if action.next_modules {
            let core = link_actions::core();
            next.push(format!("{} {}", core.program, core.module_next).into());
        }
        entry.insert("next".into(), Value::Array(next));
    }
    if !action.fail.is_empty() {
        entry.insert("fail".into(), commands(&action.fail));
    }
    if let Some(route) = &action.route {
        route_entry(&mut entry, action, route, reads);
    }
    if let Some(kind) = &action.diagnoses {
        entry.insert("diagnoses".into(), kind.clone().into());
    }
    Ok(entry)
}

/// A route as the old format's bridge fields: offered as a route, an
/// agent's command where it has an alias and a tool where its surfaces
/// say, answering its output, or relaying what it carries.
/// `reads`: whether its effect only reads, which a command tool says.
fn route_entry(entry: &mut Table, action: &Written, route: &link_actions::Route, reads: bool) {
    let on = |surface| {
        action
            .surfaces
            .as_ref()
            .is_none_or(|s| s.contains(&surface))
    };
    // The harness runs its command itself: an agent's command alone,
    // never a bounded route or tool.
    let harness = route.limits.is_none();
    let mut surfaces = Vec::new();
    if on(link_actions::Surface::Cli) {
        surfaces.push(Value::from("cli"));
    }
    if !harness {
        surfaces.push("route".into());
    }
    if route.command {
        surfaces.push("command".into());
    }
    if on(link_actions::Surface::Tool) && !harness {
        surfaces.push("tool".into());
    }
    entry.insert("surfaces".into(), Value::Array(surfaces));
    entry.insert("bridge".into(), route.bridge.clone().into());
    let callers = route
        .callers
        .iter()
        .map(|c| Value::from(format!("{c:?}").to_lowercase()))
        .collect();
    entry.insert("callers".into(), Value::Array(callers));
    match &route.carries {
        Some(carries) => {
            entry.insert("carries".into(), carries.clone().into());
        }
        // A command run through lnk answers JSON where it takes --json,
        // and its text otherwise.
        None if route.bridge == "command" => {
            let json = action
                .args
                .values()
                .any(|arg| arg.cli.shared.as_deref() == Some("json"));
            entry.insert("answer".into(), if json { "Json" } else { "Text" }.into());
            if reads {
                entry.insert("read_only".into(), true.into());
            }
        }
        None => {
            let answer = action.output.clone().unwrap_or_else(|| "Json".into());
            entry.insert("answer".into(), answer.into());
        }
    }
    if action.stream {
        entry.insert("streams".into(), true.into());
    }
    if let Some(bounds) = &route.limits {
        let mut limits = Table::new();
        limits.insert("bytes".into(), Value::Integer(bounds.bytes as i64));
        limits.insert("time_ms".into(), Value::Integer(bounds.time_ms as i64));
        entry.insert("limits".into(), limits.into());
    }
    if !route.names.is_empty() {
        let aliases = route
            .names
            .iter()
            .map(|a| Value::from(a.as_str()))
            .collect();
        entry.insert("aliases".into(), Value::Array(aliases));
    }
}

fn option(name: &str, arg: &Arg) -> anyhow::Result<Table> {
    // A shared argument's owner in `shared.toml`: a composed group by
    // its name, or the type owning the flag, whose help it shows.
    let owner = arg
        .cli
        .shared
        .as_deref()
        .map(|flag| match crate::shared::types().get(flag) {
            Some(group) if group.composed => flag,
            _ => crate::shared::owner(name).map_or(flag, |(owner, _)| owner),
        });
    let mut out = Table::new();
    let (kind, multiple) = match &arg.schema.node {
        Node::Boolean => ("boolean", false),
        Node::Integer(_) => ("integer", false),
        Node::List(list) if matches!(list.items.node, Node::Integer(_)) => ("integer", true),
        Node::List(_) => ("string", true),
        _ => ("string", false),
    };
    out.insert("type".into(), kind.into());
    if let Some(help) = &arg.schema.meta.description {
        out.insert("help".into(), help.clone().into());
    }
    if let Node::Enum(values) = &arg.schema.node {
        let values = values
            .iter()
            .filter_map(|v| v.as_str())
            .map(|v| Value::String(v.into()))
            .collect();
        out.insert("values".into(), Value::Array(values));
    }
    match arg.cli.position {
        Some(position) => {
            let position = i64::try_from(usize::from(position))?;
            out.insert("position".into(), Value::Integer(position));
        }
        None => {
            out.insert("long".into(), name.into());
        }
    }
    if let Some(short) = arg.cli.short {
        out.insert("short".into(), short.to_string().into());
    }
    if let Some(env) = &arg.cli.env {
        out.insert("environment".into(), env.clone().into());
    }
    if arg.required {
        out.insert("required".into(), true.into());
    }
    if multiple || arg.cli.rest {
        out.insert("multiple".into(), true.into());
    }
    if kind == "boolean" {
        out.insert(
            "values".into(),
            Value::Array(vec![true.into(), false.into()]),
        );
        out.insert("default".into(), false.into());
    }
    match owner {
        Some(owner) => {
            out.remove("help");
            out.insert("shared".into(), owner.into());
        }
        None => {
            out.insert("runtime".into(), true.into());
        }
    }
    Ok(out)
}

fn runtime_spec(kind: &str) -> Table {
    let mut out = Table::new();
    out.insert("type".into(), "string".into());
    out.insert(
        "help".into(),
        format!("The {kind} spec file, its field flags laid over it").into(),
    );
    out.insert("long".into(), "spec".into());
    out.insert("runtime".into(), true.into());
    out
}

#[cfg(test)]
mod tests {
    /// A package's spec kind is found by its shape's name, and a module
    /// declaring actions owns its kinds' commands itself.
    #[test]
    fn a_packages_kinds_are_found_and_a_modules_are_not() {
        let dir = link_test_kit::TempDir::new("package-kinds");
        let write = |folder: &str, file: &str, text: &str| {
            let at = dir.path().join(folder);
            std::fs::create_dir_all(&at).unwrap();
            std::fs::write(at.join(file), text).unwrap();
        };
        let module = |id: &str| {
            format!("schema = 1\n[module]\nid = \"{id}\"\nkind = \"package\"\nowner = \"{id}\"\n")
        };
        let kind = "schema = 1\n[policy]\ntype = \"object\"\nx-link-spec = { kind = \"policy\" }\n";
        write("policy", "module.toml", &module("policy"));
        write("policy", "types.toml", kind);
        write("iam", "module.toml", &module("iam"));
        write("iam", "types.toml", kind);
        write(
            "iam",
            "actions.toml",
            "schema = 1\n[actions.\"iam.role.show\"]\ndescription = \"Show\"\n",
        );
        let found = super::package_kinds_in(&dir.path().join("policy")).unwrap();
        assert_eq!(
            found.get("policy.policy").map(String::as_str),
            Some("policy")
        );
        assert!(
            super::package_kinds_in(&dir.path().join("iam"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_version_1_file_reads_as_the_view() {
        let text = include_str!("../../../../tests/fixtures/declarations/module/actions.toml");
        let actions = super::super::parse(text).unwrap();
        let create = &actions["env create"];
        assert_eq!(create.spec.as_deref(), Some("env"));
        assert_eq!(
            create.docs.as_deref(),
            Some("/services/environments#create-a-box")
        );
        assert_eq!(create.args["cloud"].position, Some(1));
        assert_eq!(create.args["yes"].shared.as_deref(), Some("confirm"));
        assert_eq!(create.args["quiet"].shared.as_deref(), Some("quiet"));
        // A shared group's arguments, each its group's.
        let setup = &actions["env setup"];
        assert_eq!(setup.args["zone"].shared.as_deref(), Some("workplace"));
        assert_eq!(setup.args["place"].position, Some(1));
        assert_eq!(setup.args["request"].shared.as_deref(), Some("request"));
        assert!(create.args["spec"].runtime);
        assert!(actions["env"].dispatch);
        // A route reaches the bridge's fields, answering the output.
        let probe = &actions["env probe"];
        assert_eq!(probe.bridge.as_deref(), Some("agent"));
        assert_eq!(probe.answer.as_deref(), Some("probed"));
        assert_eq!(probe.aliases, ["/probe"]);
        assert_eq!(
            probe.limits.map(|l| (l.bytes, l.time_ms)),
            Some((65536, 30000))
        );
        assert!(probe.allows(super::super::Caller::Agent, super::super::Surface::Route));
        assert!(!probe.allows(super::super::Caller::Trigger, super::super::Surface::Route));
        // The harness's command is an agent's command, never a route.
        let compact = &actions["env compact"];
        assert!(
            compact.on(super::super::Surface::Command) && !compact.on(super::super::Surface::Route)
        );
        assert_eq!(compact.aliases, ["/compact"]);
        // A command route is a command tool, answering JSON or text, and a
        // show action keeps what it diagnoses.
        let show = &actions["env approval show"];
        let tool = show.command().tool.expect("a command tool");
        assert!(tool.read_only && tool.json);
        assert_eq!(show.diagnoses.as_deref(), Some("approval"));
        assert_eq!(actions["env status"].answer.as_deref(), Some("Text"));
        // A chat command because it says so, with or without an alias.
        assert!(actions["env status"].on(super::super::Surface::Command));
        assert!(!actions["env approval show"].on(super::super::Surface::Command));
        assert!(actions.contains_key("env spec show"));
        assert_eq!(create.diagnoses, None);
        // Read with its folder, a resource's group is plumbing when every
        // action of it is.
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/declarations/module");
        let file = link_actions::read_actions(text).unwrap();
        let said = super::Said::read(&folder, &file, None).unwrap();
        let viewed = super::view(text, &said).unwrap();
        assert_eq!(viewed["env state"].role, super::super::Role::Plumbing);
        assert_eq!(viewed["env profile"].role, super::super::Role::Secondary);
        assert_eq!(
            actions["env profile spec show"].spec.as_deref(),
            Some("profile")
        );
        assert_eq!(
            actions["env approval add"].args["file"].long.as_deref(),
            Some("file")
        );
    }

    /// A command diagnoses the kind it declares, never one its verb
    /// implies: a `show` that declares none diagnoses nothing.
    #[test]
    fn a_show_diagnoses_only_the_kind_it_declares() {
        let show = |said: &str| {
            format!(
                "schema = 1\n[actions.\"probe.probe.show\"]\ndescription = \"Show one\"\n{said}\
                 [actions.\"probe.probe.show\".args]\nname = {{ type = \"string\", required = true, x-link-cli = {{ position = 1 }} }}\n"
            )
        };
        let declared = super::super::parse(&show("diagnoses = \"probe\"\n")).unwrap();
        assert_eq!(declared["probe show"].diagnoses.as_deref(), Some("probe"));
        let other = super::super::parse(&show("diagnoses = \"account\"\n")).unwrap();
        assert_eq!(other["probe show"].diagnoses.as_deref(), Some("account"));
        let bare = super::super::parse(&show("")).unwrap();
        assert_eq!(bare["probe show"].diagnoses, None);
    }

    /// A route's path and an agent command's name are a wire, kept as
    /// today: the view's key, its namespace in it, never the program's
    /// words.
    #[test]
    fn route_paths_and_command_names_keep_the_namespace() {
        let agent = r#"schema = 1
[verbs]
compact = { description = "Make what was said shorter, keeping its sense", effect = "update" }
[actions."agent.chat.compact"]
description = "Compact the chat"
output = "core.report"
[actions."agent.chat.compact".route]
bridge = "agent"
callers = ["agent"]
limits = { bytes = 4096, time_ms = 2500 }
command = true
names = ["/compact"]
"#;
        let actions = super::super::parse(agent).unwrap();
        let (path, _) = super::super::route(
            &actions,
            "/_link/agent/chat/compact",
            super::super::Caller::Agent,
        )
        .expect("the route at its path of today");
        assert_eq!(super::super::command_name(path), "agent:chat:compact");
        let model = r#"schema = 1
[actions."model.model.show"]
description = "Show a model"
output = "core.report"
[actions."model.model.show".route]
bridge = "model"
callers = ["agent"]
limits = { bytes = 4096, time_ms = 2500 }
"#;
        let actions = super::super::parse(model).unwrap();
        let (path, _) =
            super::super::route(&actions, "/_link/model/show", super::super::Caller::Agent)
                .expect("the route at its path of today");
        assert_eq!(super::super::command_name(path), "model:show");
    }

    /// A route served at a path of its own keeps that path, as a tool's
    /// is served today, and its command stays the command line's.
    #[test]
    fn a_route_with_its_own_path_is_served_there() {
        let agent = r#"schema = 1
[verbs]
click = { description = "Click where the page says", effect = "update" }
[actions."agent.browser.click"]
description = "Click in the agent's browser"
output = "core.mcp"
docs = "/agents/browser#click"
[actions."agent.browser.click".route]
bridge = "agent"
callers = ["owner", "agent", "program"]
limits = { bytes = 8388608, time_ms = 600000 }
carries = "mcp"
path = "browser click"
docs = "/agents/link/commands/browser#click"
"#;
        let actions = super::super::parse(agent).unwrap();
        let (path, action) = super::super::route(
            &actions,
            "/_link/browser/click",
            super::super::Caller::Agent,
        )
        .expect("the route at its own path");
        assert_eq!(super::super::command_name(path), "browser:click");
        assert_eq!(action.carries.as_deref(), Some("mcp"));
        let command = &actions["agent browser click"];
        assert!(command.on(super::super::Surface::Cli));
        assert!(!command.on(super::super::Surface::Route));
        assert!(!actions["browser click"].on(super::super::Surface::Cli));
        // Each entry is explained where it's offered.
        assert_eq!(command.docs.as_deref(), Some("/agents/browser#click"));
        assert_eq!(
            actions["browser click"].docs.as_deref(),
            Some("/agents/link/commands/browser#click")
        );
    }

    /// A route off the command line is its route alone, and makes no group.
    #[test]
    fn a_route_off_the_command_line_is_no_command() {
        let agent = r#"schema = 1
[verbs]
plan = { description = "Plan the steps of a task", effect = "insert" }
[actions."agent.task.plan"]
description = "Plan a task"
output = "core.mcp"
surfaces = ["tool"]
[actions."agent.task.plan".route]
bridge = "agent"
callers = ["agent"]
limits = { bytes = 8388608, time_ms = 600000 }
carries = "mcp"
path = "task plan"
"#;
        let actions = super::super::parse(agent).unwrap();
        assert!(actions.contains_key("task plan"));
        assert!(!actions.contains_key("agent task plan"));
        assert!(!actions.contains_key("agent task"));
    }

    /// A call a tool's program serves over MCP, off the command line with
    /// no route, is no command of Link's, and leaves a module of plumbing
    /// plumbing.
    #[test]
    fn a_tools_own_call_is_no_command() {
        let tool = r#"schema = 1
[verbs]
serve = { description = "Serve the tool", effect = "insert" }
fetch = { description = "Fetch a page, for a model", effect = "find" }
[actions."web-tool.web-tool.serve"]
description = "Serve the tool"
role = "plumbing"
output = "core.mcp"
[actions."web-tool.web-tool.fetch"]
description = "Fetch a page"
surfaces = ["tool"]
output = "core.mcp"
[actions."web-tool.web-tool.fetch".tool]
name = "fetch"
"#;
        let actions = super::super::parse(tool).unwrap();
        assert!(actions.contains_key("web-tool serve"));
        assert!(!actions.contains_key("web-tool fetch"));
        assert_eq!(actions["web-tool"].role, super::super::Role::Plumbing);
    }

    /// A bound kind of another module reaches the view by its kind's word.
    #[test]
    fn a_bound_kind_of_another_module_is_named_by_its_word() {
        let agent = r#"schema = 1
[actions."agent.permission.add"]
description = "Let an agent reach more"
spec = "sandbox.sandbox"
output = "core.report"
"#;
        let actions = super::super::parse(agent).unwrap();
        let add = &actions["agent permission add"];
        assert_eq!(add.spec.as_deref(), Some("sandbox"));
        assert!(add.args["spec"].runtime);
        assert!(
            !actions.contains_key("agent spec show"),
            "its owner shows it"
        );
    }
}
