//! A module's `actions.toml`: everything about each of its actions, by the
//! command line that runs it. The typed parser owns each argument's shape
//! (its type, spelling, default) and which actions are hidden; the file
//! keeps them so discovery reads them without compiling the module, and a
//! check holds the two equal. Everything else is written by hand.

use std::collections::BTreeMap;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::{Command, OptionDeclaration, Requirement, shared};

/// What an argument its command's own type declares is, which says where
/// it's documented: a shared flag once by its owner, a runtime argument in
/// its command's section. A spec field is neither: it comes from the spec
/// kind its action binds ([`Action::spec`]), documented once on that
/// kind's reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind<'a> {
    /// A flag of the shared type it names ([`shared`]).
    Shared(&'a str),
    /// What this run acts on or how it answers.
    Runtime,
}

/// Every action of a module, by its command line: `model list`, or `root`
/// for the program's own.
pub type Actions = BTreeMap<String, Action>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    /// What it does: its first line, then the rest of its help.
    pub help: String,
    /// Whether a command-backed tool only reads state.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub read_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    /// What it is to its module: what gives the module its function, what
    /// supports that, or plumbing Link runs and a person doesn't, which
    /// its parser hides and help never offers.
    pub role: Role,
    /// Where a primary or secondary command is explained: its page's
    /// website address and the section's anchor, `/compute/box#create-a-box`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
    /// Whether `lnk` hands this command and every one under it here.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dispatch: bool,
    /// The group's alias, from `[aliases]`: one word a person may type in
    /// place of its last.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    /// Whether its group, typed with no command after it, runs it, its
    /// flags read there (version 1's `default = true`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub default: bool,
    /// The spec kind it binds, by the name its declaration gives it
    /// (`box`): that kind's fields are the action's spec fields, and its
    /// shape their list. An action binds at most one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<String>,
    /// Whether it takes its bound kind's field flags: `false` for an
    /// action reading the kind's file as written, such as one whose yes is
    /// kept for that file, which its parser then refuses field flags for.
    /// Left out, it takes them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<bool>,
    /// Its arguments by the parser's names, each typed by the parser and
    /// described here.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub args: BTreeMap<String, OptionDeclaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<Requirement>,
    /// Where it's offered; absent, the CLI alone.
    #[serde(default = "cli", skip_serializing_if = "is_cli")]
    pub surfaces: Vec<Surface>,
    /// What another harness calls it, run as it is: `/model`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// Who may call a route or tool; the bridge supplies this identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub callers: Vec<Caller>,
    /// The module serving this action, or `harness` for its ACP agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bridge: Option<String>,
    /// The contract type answered by an action Link types.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    /// The protocol relayed by a carrier, instead of a typed answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carries: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub streams: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<Limits>,
    /// The Nexts it can give when it succeeds, each a command as `cmd!`
    /// writes it: what Next's graph reads.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next: Vec<String>,
    /// The Nexts it can give when it fails; `retry` is itself again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fail: Vec<String>,
    /// The kind of name (`agent`, `box`) this command diagnoses: given
    /// one, `<command> <name> --json` answers its state as typed blocks,
    /// what `lnk issue create` puts in an issue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnoses: Option<String>,
}

/// What an action is to its module, which says how prominently its pages
/// show it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// What gives its module its function: its section is a heading of
    /// its own.
    Primary,
    /// What supports a primary one.
    #[default]
    Secondary,
    /// What Link runs and a person doesn't: hidden, and on no page but
    /// under `/more`.
    Plumbing,
}

/// Where an action is offered: the CLI (`lnk model show`) or an agent's
/// commands (`/model:show`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Surface {
    Cli,
    Command,
    Route,
    Tool,
}

/// A bridge establishes the caller from authentication, never request data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Caller {
    Owner,
    Agent,
    Program,
    Trigger,
}

/// The answer budget of one action, applied by its bridge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub bytes: usize,
    pub time_ms: u64,
}

fn cli() -> Vec<Surface> {
    vec![Surface::Cli]
}

fn is_cli(surfaces: &Vec<Surface>) -> bool {
    *surfaces == cli()
}

mod declared;
mod implements;

/// The actions of the module in `directory`, its package describing it
/// as `description` where it's known: its version 1 `actions.toml` read
/// through `link-actions`, its groups saying what its generated parser
/// says, each with the section explaining it.
pub fn read_module(
    directory: &std::path::Path,
    description: Option<&str>,
) -> anyhow::Result<Actions> {
    let path = directory.join("actions.toml");
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let file = link_actions::read_actions(&text)?;
    let said = declared::Said::read(directory, &file, description)?;
    declared::view(&text, &said)
}

/// A version 1 `actions.toml`'s actions, read through `link-actions`.
pub fn parse(text: &str) -> anyhow::Result<Actions> {
    declared::view(text, &declared::Said::default())
}

/// The view's own shape, as [`declared::view`] writes it, checked: an
/// unknown field is refused, and each action holds together.
pub(crate) fn checked(text: &str) -> anyhow::Result<Actions> {
    let actions: Actions = toml::from_str(text).context("actions.toml")?;
    for (path, action) in &actions {
        if let Some(docs) = &action.docs {
            anyhow::ensure!(
                !action.hidden(),
                "{path}: plumbing is on no page a person reads, so it names no docs section"
            );
            anyhow::ensure!(
                docs.starts_with('/') && docs.split_once('#').is_some_and(|(_, a)| !a.is_empty()),
                "{path}: docs is a page's address and a section's anchor, /<address>#<anchor>"
            );
        }
        if let Some(kind) = &action.spec {
            anyhow::ensure!(
                !kind.is_empty() && kind.bytes().all(|b| b.is_ascii_lowercase()),
                "{path}: spec names a spec kind by its one lowercase word, as its declaration does"
            );
        }
        for (name, arg) in &action.args {
            anyhow::ensure!(
                arg.kind().is_some(),
                "{path}: {name} says what it is with exactly one of shared and runtime"
            );
            let long = arg.long.as_deref();
            if let Some(ty) = &arg.shared {
                let flag = shared::declared(ty, name, long);
                let Some(flag) = flag else {
                    anyhow::bail!("{path}: {name} is shared, but {ty} has no such flag");
                };
                anyhow::ensure!(
                    flag.is(arg),
                    "{path}: {name} is {ty}'s, but isn't typed as its declaration says"
                );
            } else if let Some((ty, flag)) = long.and_then(shared::owner)
                && !shared::types()[ty].counted
            {
                anyhow::ensure!(
                    !flag.is(arg),
                    "{path}: --{} is {ty}'s: write shared = \"{ty}\" and leave its help to the declaration",
                    long.unwrap_or_default()
                );
            }
        }
        if action.bridge.as_deref() == Some("command") {
            anyhow::ensure!(
                !action.hidden() && !action.streams && action.carries.is_none(),
                "{path}: command tools are visible, finite actions"
            );
            anyhow::ensure!(
                matches!(action.answer.as_deref(), Some("Json" | "Text")),
                "{path}: command tools answer Json or Text"
            );
            if action.answer.as_deref() == Some("Json") {
                anyhow::ensure!(
                    action
                        .args
                        .values()
                        .any(|arg| arg.long.as_deref() == Some("json")),
                    "{path}: Json tools declare --json"
                );
            }
        }
        if action.on(Surface::Route) || action.on(Surface::Tool) {
            anyhow::ensure!(
                !action.callers.is_empty(),
                "{path}: a route or tool names its callers"
            );
            anyhow::ensure!(
                action.bridge.as_deref().is_some_and(|b| !b.is_empty()),
                "{path}: a route or tool names its bridge"
            );
            anyhow::ensure!(
                action.answer.is_some() != action.carries.is_some(),
                "{path}: exactly one of answer and carries is required"
            );
            anyhow::ensure!(
                action.limits.is_some_and(|l| l.bytes > 0 && l.time_ms > 0),
                "{path}: positive answer limits are required"
            );
        }
    }
    Ok(actions)
}

/// A spec kind an action may bind: its name, and the action its
/// declaration says prints it (`box spec`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpecKind {
    pub name: String,
    pub printed_by: String,
}

impl SpecKind {
    /// The kind `name` whose declaration's print command is `command`,
    /// `lnk box spec --spec`: its words up to the first flag.
    pub fn new(name: &str, command: &str) -> Self {
        let words: Vec<&str> = command
            .split_whitespace()
            .skip(1)
            .take_while(|word| !word.starts_with('-'))
            .collect();
        Self {
            name: name.to_string(),
            printed_by: words.join(" "),
        }
    }
}

/// Every problem with the spec kinds `actions` bind, against the `kinds`
/// Link registers: a binding names a registered kind, a kind's print
/// command, where these actions have it, binds that kind, and an action
/// with an argument that takes a spec binds the kind it reads, so its
/// fields are flags ([`takes_spec`]).
pub fn check_bindings(actions: &Actions, kinds: &[SpecKind]) -> Vec<String> {
    let mut problems = Vec::new();
    for (path, action) in actions {
        match &action.spec {
            Some(kind) if !kinds.iter().any(|k| k.name == *kind) => problems.push(format!(
                "{path} binds the spec kind {kind}, which no spec registers"
            )),
            Some(_) => {}
            None => problems.extend(
                action
                    .args
                    .iter()
                    .filter(|(name, arg)| takes_spec(name, arg))
                    .map(|(name, _)| {
                        format!("{path} takes a spec in {name} but binds no spec kind")
                    }),
            ),
        }
    }
    for kind in kinds {
        if let Some(action) = actions.get(&kind.printed_by)
            && action.spec.as_deref() != Some(kind.name.as_str())
        {
            problems.push(format!(
                "{} prints the {} spec, so it binds spec = \"{}\"",
                kind.printed_by, kind.name, kind.name
            ));
        }
    }
    problems
}

/// Whether argument `name` takes a spec: it's named `spec` or `<what>_spec`,
/// or its flag is `--spec` or `--<what>-spec`.
pub fn takes_spec(name: &str, arg: &crate::OptionDeclaration) -> bool {
    name == "spec"
        || name.ends_with("_spec")
        || arg
            .long
            .as_deref()
            .is_some_and(|long| long == "spec" || long.ends_with("-spec"))
}

/// The file as the check writes it.
pub fn write(actions: &Actions) -> anyhow::Result<String> {
    Ok(toml::to_string_pretty(actions)?)
}

impl Default for Action {
    fn default() -> Self {
        Self {
            help: String::new(),
            read_only: false,
            usage: None,
            after: None,
            role: Role::Secondary,
            docs: None,
            dispatch: false,
            alias: None,
            default: false,
            spec: None,
            fields: None,
            args: BTreeMap::new(),
            requires: Vec::new(),
            surfaces: cli(),
            aliases: Vec::new(),
            callers: Vec::new(),
            bridge: None,
            answer: None,
            carries: None,
            streams: false,
            limits: None,
            next: Vec::new(),
            fail: Vec::new(),
            diagnoses: None,
        }
    }
}

impl Action {
    /// What it does, in a line: its help's first.
    pub fn what(&self) -> &str {
        self.help.lines().next().unwrap_or_default()
    }

    /// Whether it's plumbing: run by Link, never offered.
    pub fn hidden(&self) -> bool {
        self.role == Role::Plumbing
    }

    /// Whether it's offered on `surface`.
    pub fn on(&self, surface: Surface) -> bool {
        self.surfaces.contains(&surface)
    }

    /// Whether the authenticated caller may reach this surface.
    pub fn allows(&self, caller: Caller, surface: Surface) -> bool {
        !self.hidden() && self.on(surface) && self.callers.contains(&caller)
    }

    /// The command a component's answer declares: its arguments' help by
    /// how `--help` spells each, their shapes without it.
    pub fn command(&self) -> Command {
        let mut flags = BTreeMap::new();
        let mut options = BTreeMap::new();
        for (name, arg) in &self.args {
            // A counted type's command says the flag's help in full, with
            // the words its type holds it to, so its help is its own.
            let flag = arg
                .shared
                .as_deref()
                .filter(|ty| !shared::types()[*ty].counted)
                .and_then(|ty| shared::declared(ty, name, arg.long.as_deref()));
            let help = match flag {
                Some(flag) => Some(flag.help(arg.help.as_deref())),
                None => arg.help.clone(),
            };
            if let Some(help) = help {
                flags.insert(spelling(name, arg), help);
            }
            options.insert(
                name.clone(),
                OptionDeclaration {
                    help: None,
                    shared: None,
                    runtime: false,
                    ..arg.clone()
                },
            );
        }
        Command {
            tool: (self.bridge.as_deref() == Some("command")
                && self.allows(Caller::Agent, Surface::Tool))
            .then(|| crate::tools::CommandTool {
                read_only: self.read_only,
                json: self.answer.as_deref() == Some("Json"),
                limits: self.limits.expect("command tools declare limits"),
            }),
            help: self.help.clone(),
            usage: self.usage.clone(),
            after: self.after.clone(),
            route: self.dispatch,
            alias: self.alias.clone(),
            default: self.default,
            hidden: self.hidden(),
            flags,
            options,
            arguments: Vec::new(),
            environment: BTreeMap::new(),
            requires: self.requires.clone(),
            diagnoses: self.diagnoses.clone(),
            spec: self.spec.clone(),
            fields: self.fields,
        }
    }

    /// Whether it takes its bound kind's field flags ([`Action::fields`]).
    pub fn takes_fields(&self) -> bool {
        self.spec.is_some() && self.fields != Some(false)
    }
}

/// How an agent's commands spell the action at `path`: `model:show`.
pub fn command_name(path: &str) -> String {
    path.replace(' ', ":")
}

/// How `--help` spells an argument: `--<long>`, `-<short>`, or `<name>`.
pub fn spelling(name: &str, arg: &OptionDeclaration) -> String {
    match (&arg.long, &arg.short) {
        (Some(long), _) => format!("--{long}"),
        (None, Some(short)) => format!("-{short}"),
        (None, None) => format!("<{name}>"),
    }
}

/// Each action on the CLI as the command a component's answer declares.
pub fn commands(actions: &Actions) -> BTreeMap<String, Command> {
    actions
        .iter()
        .filter(|(_, action)| action.on(Surface::Cli))
        .map(|(path, action)| (path.clone(), action.command()))
        .collect()
}

/// The variables a crate's `variables.toml` (`text`) defines, and those it
/// reads at runtime, put on its program's own command, `root`, as the
/// inputs `lnk variable list` and the inventory show: a variable's one
/// home is that file, never `actions.toml`. A read variable another crate
/// defines carries no help here: the list takes its definition's.
pub fn add_variables(commands: &mut BTreeMap<String, Command>, text: &str) -> anyhow::Result<()> {
    let variables = link_actions::read_variables(text).context("variables.toml")?;
    let read: Vec<_> = variables
        .uses
        .iter()
        .filter(|one| {
            one.context == link_actions::When::Runtime
                && one.access.contains(&link_actions::Access::Read)
                && !variables.defines.contains_key(&one.name)
        })
        .map(|one| one.name.clone())
        .collect();
    if variables.defines.is_empty() && read.is_empty() {
        return Ok(());
    }
    let root = commands.entry("root".into()).or_insert_with(|| Command {
        hidden: true,
        ..Command::default()
    });
    let input = |kind: crate::ValueType, help: Option<String>| OptionDeclaration {
        r#type: kind,
        help,
        values: None,
        long: None,
        short: None,
        environment: None,
        required: false,
        default: None,
        position: None,
        multiple: false,
        optional_value: false,
        derived: false,
        hidden: false,
        shared: None,
        runtime: false,
    };
    for (name, definition) in variables.defines {
        let kind = match definition.kind {
            link_actions::Scalar::String => crate::ValueType::String,
            link_actions::Scalar::Boolean => crate::ValueType::Boolean,
            link_actions::Scalar::Integer => crate::ValueType::Integer,
        };
        root.environment
            .insert(name, input(kind, Some(definition.description)));
    }
    for name in read {
        root.environment
            .entry(name)
            .or_insert_with(|| input(crate::ValueType::String, None));
    }
    Ok(())
}

/// A route offered to `caller`, from the same registry as CLI and commands.
/// Only canonical paths match; queries are parsed by the action's own type.
pub fn route<'a>(
    actions: &'a Actions,
    path: &str,
    caller: Caller,
) -> Option<(&'a str, &'a Action)> {
    actions.iter().find_map(|(name, action)| {
        (action.allows(caller, Surface::Route)
            && path == format!("/_link/{}", name.replace(' ', "/")))
        .then_some((name.as_str(), action))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crates_variables_are_its_programs_inputs() {
        let mut commands = BTreeMap::new();
        add_variables(
            &mut commands,
            "schema = 1\n[defines.LNK_A]\ntype = \"string\"\ndescription = \"A\"\n\
             [[uses]]\nname = \"HOME\"\naccess = [\"read\"]\ncontext = \"runtime\"\n\
             [[uses]]\nname = \"LNK_T\"\naccess = [\"read\"]\ncontext = \"test\"\n\
             [[uses]]\nname = \"PATH\"\naccess = [\"forward\"]\ncontext = \"runtime\"\n",
        )
        .unwrap();
        let root = &commands["root"].environment;
        assert_eq!(root.keys().collect::<Vec<_>>(), ["HOME", "LNK_A"]);
        assert_eq!(root["LNK_A"].help.as_deref(), Some("A"));
        assert_eq!(root["HOME"].help, None);
    }

    const ACTION: &str = r#"
["model show"]
help = "Show a model"
role = "secondary"
surfaces = ["cli", "route"]
callers = ["owner", "program"]
bridge = "model"
answer = "Shown"
limits = { bytes = 4096, time_ms = 2500 }
"#;

    #[test]
    fn route_discovery_is_filtered_to_the_authenticated_caller() {
        let actions = checked(ACTION).unwrap();
        let (_, action) = route(&actions, "/_link/model/show", Caller::Program).unwrap();
        assert_eq!(action.answer.as_deref(), Some("Shown"));
        assert_eq!(action.limits.unwrap().bytes, 4096);
        assert!(route(&actions, "/_link/model/show", Caller::Agent).is_none());
        assert!(route(&actions, "/_link/model/%73how", Caller::Program).is_none());
        assert!(route(&actions, "/_link/model/show/../set", Caller::Owner).is_none());
    }

    #[test]
    fn routes_require_one_answer_shape_and_positive_limits() {
        for bad in [
            ACTION.replace("answer = \"Shown\"", ""),
            ACTION.replace("bytes = 4096", "bytes = 0"),
            ACTION.replace("bridge = \"model\"", ""),
            ACTION.replace(
                "answer = \"Shown\"",
                "answer = \"Shown\"\ncarries = \"openai\"",
            ),
        ] {
            assert!(checked(&bad).is_err(), "{bad}");
        }
        assert!(
            checked(&ACTION.replace("answer = \"Shown\"", "carries = \"openai\"\nstreams = true"))
                .is_ok()
        );
    }

    const JSON: &str = r#"
["model show"]
help = "Show a model"
role = "secondary"

["model show".args.json]
type = "boolean"
long = "json"
shared = "output"
"#;

    #[test]
    fn a_shared_flag_shows_the_shared_help_and_then_its_own_detail() {
        let shown = |text: &str| {
            let actions = checked(text).unwrap();
            commands(&actions)["model show"].flags["--json"].clone()
        };
        assert_eq!(shown(JSON), shared::flag("output", "json").unwrap().help);
        let detail = format!("{JSON}help = \"{{\\\"name\\\"}}\"\n");
        assert_eq!(
            shown(&detail),
            "Print the answer as JSON for other programs: {\"name\"}"
        );
    }

    #[test]
    fn a_shared_flag_is_declared_once() {
        let own = JSON.replace(
            "shared = \"output\"",
            "help = \"Print JSON\"\nruntime = true",
        );
        assert!(
            checked(&own)
                .unwrap_err()
                .to_string()
                .contains("is output's: write shared")
        );
        let wrong = JSON.replace("boolean", "string");
        assert!(
            checked(&wrong)
                .unwrap_err()
                .to_string()
                .contains("isn't typed")
        );
        let unknown = JSON.replace("json", "jsonl");
        assert!(
            checked(&unknown)
                .unwrap_err()
                .to_string()
                .contains("output has no such flag")
        );
        // Another type of flag of the same name is its own.
        let other = JSON
            .replace("boolean", "string")
            .replace("shared = \"output\"", "runtime = true");
        assert!(checked(&other).is_ok());
    }

    #[test]
    fn docs_name_a_section_of_a_command_a_person_runs() {
        let with = |docs: &str| JSON.replace("role = \"secondary\"", docs);
        assert!(checked(&with("role = \"primary\"\ndocs = \"/models#show-a-model\"")).is_ok());
        for (bad, says) in [
            ("role = \"plumbing\"\ndocs = \"/models#show\"", "plumbing"),
            ("role = \"secondary\"\ndocs = \"models.md\"", "address"),
            ("role = \"secondary\"\ndocs = \"/models#\"", "address"),
        ] {
            let error = checked(&with(bad)).unwrap_err().to_string();
            assert!(error.contains(says), "{bad}: {error}");
        }
        assert!(checked(&JSON.replace("role = \"secondary\"\n", "")).is_err());
    }

    #[test]
    fn an_argument_says_exactly_one_kind() {
        let kindless = JSON.replace("shared = \"output\"", "");
        let both = JSON.replace("shared = \"output\"", "shared = \"output\"\nruntime = true");
        for bad in [kindless, both] {
            assert!(
                checked(&bad)
                    .unwrap_err()
                    .to_string()
                    .contains("exactly one of shared and runtime"),
                "{bad}"
            );
        }
        // A field's path is no argument's: fields come from the binding.
        let field = JSON.replace("shared = \"output\"", "spec = \"files.write\"");
        let error = format!("{:#}", checked(&field).unwrap_err());
        assert!(error.contains("unknown field `spec`"), "{error}");
    }

    const BOUND: &str = r#"
["box create"]
help = "Make a box"
role = "primary"
spec = "box"

["box spec"]
help = "Print a box spec"
role = "secondary"
spec = "box"
"#;

    fn kinds() -> Vec<SpecKind> {
        vec![SpecKind::new("box", "lnk box spec --spec")]
    }

    #[test]
    fn a_binding_names_one_registered_kind_and_a_print_command_binds_its_own() {
        assert_eq!(
            SpecKind::new("box", "lnk box spec --spec").printed_by,
            "box spec"
        );
        assert_eq!(
            check_bindings(&checked(BOUND).unwrap(), &kinds()),
            Vec::<String>::new()
        );
        let unknown =
            checked(&BOUND.replace("spec = \"box\"\n\n", "spec = \"boxes\"\n\n")).unwrap();
        let problems = check_bindings(&unknown, &kinds());
        assert!(
            problems.iter().any(
                |p| p.contains("box create binds the spec kind boxes, which no spec registers")
            ),
            "{problems:?}"
        );
        let missing = checked(&BOUND.replace(
            "role = \"secondary\"\nspec = \"box\"",
            "role = \"secondary\"",
        ))
        .unwrap();
        let problems = check_bindings(&missing, &kinds());
        assert!(
            problems
                .iter()
                .any(|p| p.contains("box spec prints the box spec, so it binds spec = \"box\"")),
            "{problems:?}"
        );
        // An argument that takes a spec, by its name or its flag, binds one.
        for arg in [
            "[\"box tune\".args.spec]\ntype = \"string\"\nposition = 1\nruntime = true",
            "[\"box tune\".args.file]\ntype = \"string\"\nlong = \"box-spec\"\nruntime = true",
        ] {
            let unbound = checked(&format!(
                "{BOUND}\n[\"box tune\"]\nhelp = \"Tune a box\"\nrole = \"secondary\"\n\n{arg}\n"
            ))
            .unwrap();
            let problems = check_bindings(&unbound, &kinds());
            assert!(
                problems
                    .iter()
                    .any(|p| p.starts_with("box tune takes a spec in ")
                        && p.ends_with("binds no spec kind")),
                "{problems:?}"
            );
        }
        // One kind an action: a second binding is no file at all.
        let twice = BOUND.replace(
            "spec = \"box\"\n\n",
            "spec = \"box\"\nspec = \"sandbox\"\n\n",
        );
        let error = format!("{:#}", checked(&twice).unwrap_err());
        assert!(error.contains("duplicate key"), "{error}");
        let worded = BOUND.replace("spec = \"box\"\n\n", "spec = \"Box Spec\"\n\n");
        assert!(
            checked(&worded)
                .unwrap_err()
                .to_string()
                .contains("one lowercase word")
        );
    }
}
