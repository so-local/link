//! `actions.toml`: the module's own verbs, and each action keyed by its ID.

use std::{collections::BTreeMap, num::NonZeroUsize};

use anyhow::{Context, bail, ensure};
use serde::{Deserialize, Serialize};

use super::schema::{Keys, schema};
use crate::{
    names::ActionId,
    schema::{Node, Schema},
};

/// Where an action's help lists it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// What gives its module its function.
    Primary,
    /// What supports a primary one.
    #[default]
    Secondary,
    /// What Link runs and a person doesn't: hidden, and offered on the
    /// command line alone.
    Plumbing,
}

/// Where an action is offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Surface {
    Cli,
    /// The REST API, the app's transport to a service.
    Rest,
    App,
    Tool,
    /// A service's own route, which other principals call: it makes no
    /// command, REST route, page, or tool, and is declared so the service
    /// serving it decides each call from the caller's assignments.
    Service,
}

impl Surface {
    /// Every surface an action is offered on when it names none: never
    /// [`Surface::Service`], which an action names alone.
    pub const ALL: [Surface; 4] = [Self::Cli, Self::Rest, Self::App, Self::Tool];
}

/// Who a bridge lets call a route: established from authentication,
/// never from the request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Caller {
    /// The person the agent works for.
    Owner,
    /// The agent itself.
    Agent,
    /// A program Link runs.
    Program,
    /// A trigger firing on its schedule or event.
    Trigger,
}

/// The answer budget of one call, which the bridge enforces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// The most its answer may hold.
    pub bytes: u64,
    /// The longest it may take, in milliseconds.
    pub time_ms: u64,
}

/// `[actions."<id>".route]`: how the agent's bridge offers the action,
/// declared by the module that owns it. Its answer is the action's output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Route {
    /// The module serving it, or `harness` for the agent's harness.
    pub bridge: String,
    /// Who may call it.
    pub callers: Vec<Caller>,
    /// Its answer budget: required, except on the harness's bridge, which
    /// runs the command itself and bounds no call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<Limits>,
    /// The protocol it relays, as `mcp`, in place of a typed answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carries: Option<String>,
    /// The path its bridge serves it at, where that isn't its command's
    /// words: a tool's route, `"browser click"`, served at
    /// `/_link/browser/click` and named `browser:click`, as it is today.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The section explaining it as its bridge offers it, `/<address>#<anchor>`,
    /// where that isn't its command's: only with its own `path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
    /// Whether an agent's chat offers it as a command, `/agent:chat:compact`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub command: bool,
    /// Its chat command's other names, as `/compact`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
}

/// `[verbs]`: a verb of the module's own, what it means and its effect.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verb {
    pub description: String,
    pub effect: crate::Effect,
}

/// `[tools]`: the closed sets a module's tool actions name, each the
/// program's to give at runtime ([`crate::tools::offer`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tools {
    /// The values a tool action's text or an argument's choices take
    /// from the program: `{name}` in a description, or `x-link-fill`.
    #[serde(default)]
    pub fills: Vec<String>,
    /// The conditions a tool action is offered on, when it names one.
    #[serde(default)]
    pub conditions: Vec<String>,
}

/// `[actions."<id>".tool]`: how an action offered on `tool` is named and
/// annotated as an MCP call. Whether it only reads, may destroy, or may
/// be called again is its effect's, never said here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    /// The call's own name, as the model calls it (`read`).
    pub name: String,
    /// Whether it reaches beyond the machine (MCP's `openWorldHint`),
    /// given exactly when declared: left out, MCP's default holds.
    #[serde(
        default,
        rename = "open-world",
        skip_serializing_if = "Option::is_none"
    )]
    pub open_world: Option<bool>,
    /// The condition it's offered on, one of `[tools].conditions`: none,
    /// always.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
}

/// `keeps`: how long a find's answer may be kept, and whether anyone's
/// request may be answered with it, in the declaration's own words. A
/// transport writes its caching headers from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keeps {
    /// How long: a duration, or `forever` for an answer that never
    /// changes, as a file fetched by its SHA-256.
    #[serde(rename = "for")]
    pub lasting: Lasting,
    /// Whether it's the same for every caller, so anything between may
    /// keep it. Left out, it's one caller's own, never kept between.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shared: bool,
}

/// How long an answer keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lasting {
    For(crate::value::Duration),
    Forever,
}

impl Lasting {
    const FOREVER: &str = "forever";
}

impl Serialize for Lasting {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::For(duration) => duration.serialize(serializer),
            Self::Forever => serializer.serialize_str(Self::FOREVER),
        }
    }
}

impl<'de> Deserialize<'de> for Lasting {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        match text == Self::FOREVER {
            true => Ok(Self::Forever),
            false => text.parse().map(Self::For).map_err(|_| {
                serde::de::Error::custom(format!(
                    "keeps for {text:?}: a duration, as 5m, or forever"
                ))
            }),
        }
    }
}

/// `x-link-cli`: how a runtime argument is typed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cli {
    /// A positional argument, at this place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<NonZeroUsize>,
    /// Its one-letter flag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short: Option<char>,
    /// Every argument after the others, passed on unparsed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rest: bool,
    /// A rest taken only after `--`, so a mistyped flag before it is
    /// refused, never handed on.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub last: bool,
    /// A variable, defined in a `variables.toml`, read when the flag is absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<String>,
    /// One of core's shared runtime arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared: Option<String>,
    /// Its value may be left out after its flag, `--sandbox` alone
    /// meaning the declared `default-missing`: an optional value.
    #[serde(
        default,
        rename = "default-missing",
        skip_serializing_if = "Option::is_none"
    )]
    pub default_missing: Option<String>,
    /// The arguments of the same action it can't be given with.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<String>,
    /// The arguments of the same action it's only given with.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
    /// Kept out of help: an argument Link passes and a person doesn't.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

/// A runtime argument: about this one invocation, never a spec field.
#[derive(Clone, Debug, PartialEq)]
pub struct Arg {
    pub schema: Schema,
    pub required: bool,
    pub cli: Cli,
    /// Declared once in the module's `[args]`, so every action takes it.
    pub every: bool,
    /// The module's argument group it comes from, taken whole by an
    /// argument of the group's name, and one Rust type in each action.
    pub group: Option<String>,
}

/// `[groups.<name>]`: arguments several actions of the module take
/// together, declared once, one Rust type wherever they're taken.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub description: Option<String>,
    pub args: BTreeMap<String, Arg>,
}

/// `[program]`: the program itself, run with no command, as `lnk
/// --version` is.
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub description: Option<String>,
    pub args: BTreeMap<String, Arg>,
}

/// An action as `actions.toml` writes it, before the catalog resolves it.
#[derive(Clone, Debug, PartialEq)]
pub struct Written {
    pub description: String,
    pub args: BTreeMap<String, Arg>,
    /// The module-wide arguments (`[args]`) it leaves out, each written
    /// `<name> = false` in its `args`.
    pub without: Vec<String>,
    pub spec: Option<String>,
    pub output: Option<String>,
    /// The name of the shape it reads on stdin, when it reads one.
    pub stdin: Option<String>,
    pub role: Role,
    pub surfaces: Option<Vec<Surface>>,
    pub next: Vec<ActionId>,
    pub fail: Vec<ActionId>,
    pub checks: Vec<String>,
    /// The guide section a person reads about it, `/<address>#<anchor>`.
    pub docs: Option<String>,
    pub resource: Option<String>,
    /// Whether its group, typed with no command after it, runs it.
    pub default: bool,
    /// Whether it leads into every module (`next-modules`), as `lnk up`
    /// does: core's alone.
    pub next_modules: bool,
    /// Whether its answer arrives as it's made, each event one choice of
    /// its output, a tagged union.
    pub stream: bool,
    /// How long a find's answer keeps.
    pub keeps: Option<Keeps>,
    /// The arguments REST accepts, where it narrows them: none, every one
    /// but those that say where or how a machine runs it.
    pub rest_args: Option<Vec<String>>,
    /// Whether it reads or writes a control file, which steers Local Link:
    /// never offered to a run, a model's tool, or a role a module ships.
    pub control: bool,
    /// How the agent's bridge offers it.
    pub route: Option<Route>,
    /// How it's named and annotated as an MCP call, offered on `tool`.
    pub tool: Option<Tool>,
    /// The kind of thing a show action diagnoses (`agent`, `box`): given
    /// one by name, its `--json` answers that thing's state as typed
    /// blocks, what `lnk issue create` puts in an issue.
    pub diagnoses: Option<String>,
}

/// `actions.toml` alone.
pub struct File {
    pub verbs: BTreeMap<String, Verb>,
    /// The runtime arguments every action of the module takes.
    pub args: BTreeMap<String, Arg>,
    pub program: Option<Program>,
    pub actions: BTreeMap<ActionId, Written>,
    /// Reserved for IAM: read and kept, acted on by nothing yet.
    pub roles: toml::Table,
    /// The actions of each contract it implements, by the contract's ID.
    pub implements: BTreeMap<String, Vec<ActionId>>,
    /// Its argument groups, by name.
    pub groups: BTreeMap<String, Group>,
    /// The closed sets its tool actions name.
    pub tools: Tools,
    /// Each group's alias, a short form a person may type in its place,
    /// by the group's words as typed after `lnk`: `module = "mod"`.
    pub aliases: BTreeMap<String, String>,
}

impl File {
    /// What a verb does to state: a fixed verb's effect, or the one this
    /// file declares for a verb of its own.
    pub fn effect(&self, verb: &str) -> Option<crate::Effect> {
        crate::config::SETTINGS
            .verbs
            .fixed
            .get(verb)
            .map(|fixed| fixed.effect)
            .or_else(|| self.verbs.get(verb).map(|own| own.effect))
    }
}

pub(crate) fn read(text: &str, module: &str) -> anyhow::Result<File> {
    let table = super::versioned(text)?;
    let mut keys = Keys::new(&table);
    let verbs: BTreeMap<String, Verb> = keys
        .take("verbs")
        .map(|v| v.clone().try_into())
        .transpose()
        .context("[verbs] gives each verb its description and its effect: find, insert, update or delete")?
        .unwrap_or_default();
    let roles = match keys.take("roles") {
        Some(toml::Value::Table(roles)) => roles.clone(),
        Some(_) => bail!("[roles] is a table"),
        None => toml::Table::new(),
    };
    let implements: BTreeMap<String, Vec<ActionId>> = keys
        .take("implements")
        .map(|v| v.clone().try_into())
        .transpose()
        .context("[implements] lists each contract's action IDs by the contract's ID")?
        .unwrap_or_default();
    for (contract, ids) in &implements {
        ensure!(
            crate::names::is_name(contract) && contract != module,
            "[implements] {contract}: a contract is another module, by its ID"
        );
        ensure!(
            !ids.is_empty(),
            "[implements] {contract} lists the actions it implements"
        );
        for id in ids {
            ensure!(
                id.namespace() == contract,
                "[implements] {contract}: {id} isn't one of its actions"
            );
        }
    }
    let args = match keys.take("args") {
        Some(written) => {
            let args = arg_table(written, module, &BTreeMap::new()).context("[args]")?;
            for (name, arg) in &args {
                ensure!(
                    arg.cli.position.is_none() && !arg.cli.rest && !arg.required,
                    "[args] {name}: every action takes it, so it's an optional flag, never positional"
                );
            }
            args.into_iter()
                .map(|(name, arg)| (name, Arg { every: true, ..arg }))
                .collect()
        }
        None => BTreeMap::new(),
    };
    let program = match keys.take("program") {
        Some(toml::Value::Table(table)) => {
            let mut keys = Keys::new(table);
            let description = keys.string("description")?;
            let args = match keys.take("args") {
                Some(written) => {
                    arg_table(written, module, &BTreeMap::new()).context("[program.args]")?
                }
                None => BTreeMap::new(),
            };
            keys.done().context("[program]")?;
            for (name, arg) in &args {
                ensure!(
                    arg.cli.position.is_none() && !arg.cli.rest && !arg.required,
                    "[program.args] {name}: the program's own flags are optional and never positional"
                );
            }
            Some(Program { description, args })
        }
        Some(_) => bail!("[program] is a table"),
        None => None,
    };
    let tools: Tools = keys
        .take("tools")
        .map(|v| v.clone().try_into())
        .transpose()
        .context("[tools] holds fills and conditions, each a list of names")?
        .unwrap_or_default();
    for name in tools.fills.iter().chain(&tools.conditions) {
        ensure!(
            crate::names::is_name(name),
            "[tools] {name:?} is a lowercase name"
        );
    }
    let aliases: BTreeMap<String, String> = keys
        .take("aliases")
        .map(|v| v.clone().try_into())
        .transpose()
        .context("[aliases] gives a group's words, as typed after lnk, the one word that may stand for its last")?
        .unwrap_or_default();
    for (group, alias) in &aliases {
        ensure!(
            group.split(' ').all(crate::names::is_name),
            "[aliases] {group:?} is a group's words, each a lowercase name, as typed after lnk"
        );
        ensure!(
            crate::names::is_name(alias),
            "[aliases] {group}: {alias:?} is one lowercase name"
        );
    }
    let mut groups = BTreeMap::new();
    if let Some(written) = keys.take("groups") {
        let toml::Value::Table(written) = written else {
            bail!("[groups] is a table")
        };
        for (name, value) in written {
            ensure!(
                crate::names::is_name(name),
                "group {name:?} is a lowercase name"
            );
            let toml::Value::Table(table) = value else {
                bail!("group {name} is a table")
            };
            let mut group_keys = Keys::new(table);
            let description = group_keys.string("description")?;
            let args = arg_table(
                group_keys.take("args").context("a group lists its args")?,
                module,
                &BTreeMap::new(),
            )
            .with_context(|| format!("group {name}"))?;
            group_keys.done().with_context(|| format!("group {name}"))?;
            ensure!(!args.is_empty(), "group {name} lists its args");
            for (arg, declared) in &args {
                ensure!(
                    declared.cli.position.is_none() && !declared.cli.rest && !declared.required,
                    "group {name}'s argument {arg}: a group's arguments are optional flags"
                );
            }
            groups.insert(name.clone(), Group { description, args });
        }
    }
    let mut actions = BTreeMap::new();
    if let Some(written) = keys.take("actions") {
        let toml::Value::Table(written) = written else {
            bail!("[actions] is a table")
        };
        for (id, value) in written {
            let parsed: ActionId = id.parse()?;
            let action = action(value, module, &groups).with_context(|| format!("action {id}"))?;
            actions.insert(parsed, action);
        }
    }
    keys.done()?;
    Ok(File {
        verbs,
        args,
        program,
        actions,
        roles,
        implements,
        groups,
        tools,
        aliases,
    })
}

fn action(
    value: &toml::Value,
    module: &str,
    groups: &BTreeMap<String, Group>,
) -> anyhow::Result<Written> {
    let toml::Value::Table(table) = value else {
        bail!("an action is a table")
    };
    let mut keys = Keys::new(table);
    let list = |value: Option<&toml::Value>, what: &str| -> anyhow::Result<Vec<ActionId>> {
        value
            .map(|v| v.clone().try_into::<Vec<ActionId>>())
            .transpose()
            .with_context(|| format!("{what} lists action IDs"))
            .map(Option::unwrap_or_default)
    };
    let description = keys.string("description")?.context("its description")?;
    ensure!(
        !description.trim().is_empty(),
        "its description says what it does"
    );
    // `<name> = false` leaves out a module-wide argument; the rest are its
    // own.
    let mut without: Vec<String> = Vec::new();
    let args = match keys.take("args") {
        Some(toml::Value::Table(written)) => {
            let mut own = written.clone();
            own.retain(|name, value| match value {
                toml::Value::Boolean(false) => {
                    without.push(name.to_string());
                    false
                }
                _ => true,
            });
            arg_table(&toml::Value::Table(own), module, groups)?
        }
        Some(written) => arg_table(written, module, groups)?,
        None => BTreeMap::new(),
    };
    let written = Written {
        description,
        args,
        without,
        spec: keys.string("spec")?,
        output: keys.string("output")?,
        stdin: keys.string("stdin")?,
        role: keys
            .take("role")
            .map(|v| v.clone().try_into())
            .transpose()
            .context("role is primary, secondary or plumbing")?
            .unwrap_or_default(),
        surfaces: keys
            .take("surfaces")
            .map(|v| v.clone().try_into())
            .transpose()
            .context("surfaces lists cli, rest, app or tool")?,
        next: list(keys.take("next"), "next")?,
        fail: list(keys.take("fail"), "fail")?,
        checks: keys
            .take("checks")
            .map(|v| v.clone().try_into())
            .transpose()
            .context("checks lists hooks by name")?
            .unwrap_or_default(),
        resource: keys.string("resource")?,
        docs: keys.string("docs")?,
        default: match keys.take("default") {
            Some(toml::Value::Boolean(default)) => *default,
            Some(_) => bail!("default is true or false"),
            None => false,
        },
        next_modules: match keys.take("next-modules") {
            Some(toml::Value::Boolean(on)) => *on,
            Some(_) => bail!("next-modules is true or false"),
            None => false,
        },
        diagnoses: keys.string("diagnoses")?,
        stream: match keys.take("stream") {
            Some(toml::Value::Boolean(on)) => *on,
            Some(_) => bail!("stream is true or false"),
            None => false,
        },
        control: match keys.take("control") {
            Some(toml::Value::Boolean(on)) => *on,
            Some(_) => bail!("control is true or false"),
            None => false,
        },
        rest_args: keys
            .take("rest-args")
            .map(|v| v.clone().try_into::<Vec<String>>())
            .transpose()
            .context("rest-args lists its arguments REST accepts, by name")?,
        keeps: keys
            .take("keeps")
            .map(|v| v.clone().try_into::<Keeps>())
            .transpose()
            .context("keeps holds for, a duration or forever, and shared")?,
        route: keys
            .take("route")
            .map(|v| v.clone().try_into::<Route>())
            .transpose()
            .context("route")?,
        tool: keys
            .take("tool")
            .map(|v| v.clone().try_into::<Tool>())
            .transpose()
            .context("tool holds name, open-world and when")?,
    };
    if let Some(route) = &written.route {
        route_check(route).context("route")?;
    }
    keys.done()?;
    Ok(written)
}

fn route_check(route: &Route) -> anyhow::Result<()> {
    let words = &crate::config::SETTINGS.route;
    ensure!(
        words.kinds.contains(&route.bridge),
        "bridge is one of {}",
        words.kinds.join(", ")
    );
    ensure!(!route.callers.is_empty(), "callers names who may call it");
    let distinct: std::collections::BTreeSet<_> = route.callers.iter().collect();
    ensure!(distinct.len() == route.callers.len(), "callers repeats one");
    let harness = route.bridge == crate::config::SETTINGS.route.harness;
    match (&route.limits, harness) {
        (Some(_), true) => bail!(
            "the harness runs its command itself, so its route bounds no call: leave out limits"
        ),
        (None, false) => bail!("a route names its limits, bytes and time_ms"),
        (Some(limits), false) => ensure!(
            limits.bytes > 0 && limits.time_ms > 0,
            "its limits are positive"
        ),
        (None, true) => {}
    }
    if let Some(carries) = &route.carries {
        ensure!(
            crate::names::is_name(carries),
            "carries names a protocol, as mcp"
        );
    }
    if let Some(path) = &route.path {
        ensure!(
            !path.is_empty() && path.split(' ').all(crate::names::is_name),
            "path {path:?} is words, lowercase names with one space between, as \"browser click\""
        );
    }
    if let Some(docs) = &route.docs {
        ensure!(
            route.path.is_some(),
            "a route served at its command's words is its command's section: docs needs its own path"
        );
        ensure!(
            docs.starts_with('/')
                && docs
                    .split_once('#')
                    .is_some_and(|(_, anchor)| !anchor.is_empty()),
            "docs is a page's address and a section's anchor, /<address>#<anchor>"
        );
    }
    ensure!(
        route.names.is_empty() || route.command,
        "names are a chat command's: say command = true"
    );
    for name in &route.names {
        ensure!(
            name.strip_prefix('/').is_some_and(crate::names::is_name),
            "name {name:?} is a slash and a lowercase name, as /whoami"
        );
    }
    Ok(())
}

/// A table of runtime arguments, by name.
fn arg_table(
    written: &toml::Value,
    module: &str,
    own: &BTreeMap<String, Group>,
) -> anyhow::Result<BTreeMap<String, Arg>> {
    let toml::Value::Table(written) = written else {
        bail!("args is a table")
    };
    let mut args = BTreeMap::new();
    let mut groups = Vec::new();
    for (name, value) in written {
        ensure!(
            crate::names::is_name(name),
            "argument {name:?}: a runtime argument's name is a lowercase name with no dot"
        );
        // A group of the module's own, taken whole.
        if let Some(group) = value.get("group") {
            let group = group.as_str().context("group names one of [groups]")?;
            ensure!(
                group == name && value.as_table().is_some_and(|t| t.len() == 1),
                "argument {name}: a group is taken by an argument of its own name, {group} = {{ group = {group:?} }} alone"
            );
            let declared = own
                .get(group)
                .with_context(|| format!("argument {name}: no group {group} is declared"))?;
            for (member, arg) in &declared.args {
                let mut arg = arg.clone();
                arg.group = Some(group.to_string());
                ensure!(
                    args.insert(member.clone(), arg).is_none(),
                    "argument {member} is also group {group}'s"
                );
            }
            continue;
        }
        if let Some(group) = shared_group(value) {
            ensure!(
                group == name,
                "argument {name}: a shared group is taken by an argument of its own name, {group}"
            );
            groups.push(group.to_string());
            continue;
        }
        args.insert(
            name.clone(),
            arg(value, module).with_context(|| format!("argument {name}"))?,
        );
    }
    // A group's arguments, each its own, a positional after the action's.
    let after = args
        .values()
        .filter_map(|a: &Arg| a.cli.position.map(usize::from))
        .max()
        .unwrap_or(0);
    for group in groups {
        let declared = crate::config::SHARED[&group]
            .get("args")
            .and_then(toml::Value::as_table)
            .with_context(|| format!("shared group {group} lists its args"))?;
        for (name, value) in declared {
            let mut one = arg(value, module)
                .with_context(|| format!("shared group {group}'s argument {name}"))?;
            one.cli.shared = Some(group.clone());
            one.cli.position = one
                .cli
                .position
                .and_then(|p| NonZeroUsize::new(usize::from(p) + after));
            ensure!(
                args.insert(name.clone(), one).is_none(),
                "argument {name} is also shared group {group}'s"
            );
        }
    }
    Ok(args)
}

/// The shared group `value` takes whole, where it names one: an entry of
/// core's shared arguments with `args` of its own.
fn shared_group(value: &toml::Value) -> Option<&str> {
    let shared = value.get("x-link-cli")?.get("shared")?.as_str()?;
    crate::config::SHARED
        .get(shared)?
        .get("args")
        .is_some()
        .then_some(shared)
}

fn arg(value: &toml::Value, module: &str) -> anyhow::Result<Arg> {
    let table = match value {
        toml::Value::Table(table) => table,
        name @ toml::Value::String(_) => {
            return Ok(Arg {
                schema: schema(name, module, 0)?,
                required: false,
                cli: Cli::default(),
                every: false,
                group: None,
            });
        }
        _ => bail!("an argument is a table or a shape's name"),
    };
    let mut table = table.clone();
    let required = match table.remove("required") {
        Some(toml::Value::Boolean(required)) => required,
        Some(_) => bail!("required is true or false"),
        None => false,
    };
    let cli: Cli = table
        .remove("x-link-cli")
        .map(toml::Value::try_into)
        .transpose()
        .context("x-link-cli")?
        .unwrap_or_default();
    if let Some(shared) = &cli.shared {
        ensure!(
            table.is_empty()
                && !required
                && cli
                    == Cli {
                        shared: cli.shared.clone(),
                        ..Cli::default()
                    },
            "a shared argument means the same everywhere: it says x-link-cli = {{ shared = {shared:?} }} alone"
        );
        let declared = crate::config::SHARED
            .get(shared)
            .with_context(|| format!("{shared:?} isn't one of core's shared arguments"))?;
        let mut shared_arg = arg(declared, module)?;
        shared_arg.cli.shared = cli.shared.clone();
        return Ok(shared_arg);
    }
    let schema = schema(&toml::Value::Table(table), module, 0)?;
    ensure!(
        !matches!(schema.node, Node::Record(_) | Node::Map(_) | Node::OneOf(_)),
        "an argument is a scalar, a list or a choice, or a shape's name: an object written here is refused"
    );
    if let Node::List(list) = &schema.node {
        ensure!(
            !matches!(
                list.items.node,
                Node::Record(_) | Node::Map(_) | Node::List(_) | Node::OneOf(_)
            ),
            "a list argument lists scalars or choices"
        );
    }
    ensure!(
        !(cli.rest && cli.position.is_some()),
        "rest takes what's left, at no position"
    );
    ensure!(
        !cli.last || cli.rest,
        "last is a rest's: taken only after --"
    );
    if let Some(missing) = &cli.default_missing {
        ensure!(
            cli.position.is_none() && !cli.rest && !required,
            "default-missing is an optional flag's, the value its flag alone means"
        );
        ensure!(
            !missing.is_empty(),
            "default-missing names the value its flag alone means"
        );
    }
    Ok(Arg {
        schema,
        required,
        cli,
        every: false,
        group: None,
    })
}
