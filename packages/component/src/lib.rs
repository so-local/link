//! A component's source declaration and its self-contained runtime answer.

use std::collections::BTreeMap;

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};

/// The schema of a component's source declaration, its `module.toml`.
pub const SCHEMA: u32 = 1;

/// The description protocol, as `description.toml` declares it: the
/// version a description ([`Resolved`]) and the registry are written in,
/// and the oldest this release accepts of a program it asks, or of a
/// program asking it.
pub static PROTOCOL: std::sync::LazyLock<link_version::Protocol> = std::sync::LazyLock::new(|| {
    link_version::Protocol::shipped_upgrade(include_str!("description.toml"))
});

/// The variable a program asking for a description or the registry states
/// its [`PROTOCOL`] version in ([`asking`]), which the answerer checks and
/// answers in ([`asked`]).
pub const PROTOCOL_VARIABLE: &str = "LNK_DESCRIPTION_PROTOCOL";

pub mod actions;
pub mod cargo;
pub mod languages;
pub mod release;
pub mod requirements;
pub mod shared;
pub mod tools;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Module,
    Harness,
    Connection,
    Tool,
    Store,
    Program,
    /// A contract crate: a protocol several modules implement, declared
    /// once, compiled into each, with no program of its own.
    Package,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Module {
    /// The module's identity, declared once: its program is `lnk-<id>`.
    pub id: String,
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// Whether its commands read control (the map, settings, grants or
    /// credentials). One saying `false` is run without core's folders
    /// check and is handed no control by core: a relaxation the security
    /// review keeps to the offline manual.
    #[serde(
        rename = "reads-control",
        default = "reads_control",
        skip_serializing_if = "said_by_default"
    )]
    pub reads_control: bool,
}

/// What a module that says nothing about control is taken to do: read it.
fn reads_control() -> bool {
    true
}

/// Whether `reads` is what saying nothing means, so it's left unwritten.
fn said_by_default(reads: &bool) -> bool {
    *reads
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Component {
    pub id: String,
    pub cargo_name: String,
    pub version: String,
    pub license: String,
    pub description: String,
    pub program: String,
    pub kind: Kind,
    /// Its module's [`Module::reads_control`], carried to core.
    #[serde(
        rename = "reads-control",
        default = "reads_control",
        skip_serializing_if = "said_by_default"
    )]
    pub reads_control: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(untagged)]
pub enum Value {
    Boolean(bool),
    Integer(i64),
    String(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValueType {
    String,
    Boolean,
    Integer,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionDeclaration {
    pub r#type: ValueType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<usize>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub multiple: bool,
    /// Whether a value-taking flag also accepts its bare spelling.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional_value: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub derived: bool,
    /// Whether it's left out of its command's help: declared where it
    /// means nothing, such as a shared type's flag a command doesn't take,
    /// or a global flag a command names its own way. It's still parsed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// An argument the command's own type declares is one of two kinds,
    /// said by exactly one of `shared` and `runtime`, which the typed
    /// parser writes ([`actions::Kind`]). A spec field is never one: it
    /// comes from the spec kind its action binds ([`actions::Action::spec`]).
    /// `shared`: the shared type owning a flag more than one module takes
    /// ([`shared`]), `output` for `--json`: its help is the type's, and
    /// `help` here is only what is this action's to add.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared: Option<String>,
    /// An argument that steers only this run: what the action acts on, or
    /// how it answers, positional arguments among them.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub runtime: bool,
}

impl OptionDeclaration {
    /// Its kind, when exactly one field says it.
    pub fn kind(&self) -> Option<actions::Kind<'_>> {
        match (&self.shared, self.runtime) {
            (Some(ty), false) => Some(actions::Kind::Shared(ty)),
            (None, true) => Some(actions::Kind::Runtime),
            _ => None,
        }
    }

    /// The kind `kind` written into its fields, replacing any other.
    pub fn set_kind(&mut self, kind: actions::Kind<'_>) {
        self.shared = None;
        self.runtime = false;
        match kind {
            actions::Kind::Shared(ty) => self.shared = Some(ty.to_string()),
            actions::Kind::Runtime => self.runtime = true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct When {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub os: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arch: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub libc: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, untagged)]
pub enum Binding {
    Literal { literal: Value },
    From { from: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    pub dependency: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, Binding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<When>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "kebab-case")]
pub enum Dependency {
    Module {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        protocol: Option<u32>,
    },
    External {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
    },
    Capability {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min_abi: Option<u32>,
    },
    /// A package compiled in, such as a contract whose shapes the module
    /// names: nothing to install or probe.
    Package { id: String },
}

impl Dependency {
    pub fn id(&self) -> &str {
        match self {
            Self::Module { id, .. }
            | Self::External { id, .. }
            | Self::Capability { id, .. }
            | Self::Package { id } => id,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Argument {
    pub name: String,
    pub help: String,
    pub r#type: ValueType,
    #[serde(default)]
    pub required: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Command {
    /// A command explicitly exposed to MCP, projected from its action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<tools::CommandTool>,
    pub help: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    /// Whether the core dispatches this complete command prefix here.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub route: bool,
    /// The group's alias, from `[aliases]` in `actions.toml`: one word a
    /// person may type in place of its last, as `lnk man` for `lnk manual`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    /// Whether its group, typed with no command after it, runs it, its
    /// flags read there.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub default: bool,
    /// Whether the command is Link's, not a person's: Link runs it and never
    /// offers it, so `lnk --help` does not list it and Next's graph does
    /// not count it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// Authored help, indexed by the spelling clap displays.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub flags: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, OptionDeclaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<Argument>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub environment: BTreeMap<String, OptionDeclaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<Requirement>,
    /// The kind of name it diagnoses: `<command> <name> --json` is that
    /// name's state as typed blocks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnoses: Option<String>,
    /// The spec kind it consumes, its action's binding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<String>,
    /// Whether it takes that kind's field flags: `Some(false)` where it
    /// reads the file as written, its action's `fields`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub file: String,
    pub pointer: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Menu {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub default: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Removal {
    #[serde(default)]
    pub parts: Vec<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub help: Option<String>,
}

/// Required program closure. Recommendations never enter it implicitly.
pub fn required_programs<'a>(
    registry: &'a BTreeMap<String, Resolved>,
    roots: &[&str],
) -> anyhow::Result<Vec<&'a str>> {
    let mut programs = vec![];
    for root in roots {
        for module in requirements::required(
            registry,
            root,
            None,
            &BTreeMap::new(),
            &requirements::Environment::here(),
        )?
        .modules
        {
            if !programs.contains(&module) {
                programs.push(module);
            }
        }
    }
    Ok(programs)
}

/// Migration-only additions are separate from operational requirements. A new
/// split component can preserve an installed feature without a runtime cycle.
pub fn upgrade_programs<'a>(
    registry: &'a BTreeMap<String, Resolved>,
    roots: &[&str],
) -> anyhow::Result<Vec<&'a str>> {
    let mut selected = required_programs(registry, roots)?;
    let mut index = 0;
    while index < selected.len() {
        let descriptor = &registry[selected[index]];
        for dependency in &descriptor.upgrade_requires {
            for needed in required_programs(registry, &[dependency])? {
                if !selected.contains(&needed) {
                    selected.push(needed);
                }
            }
        }
        index += 1;
    }
    Ok(selected)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Documentation {
    pub topic: String,
    pub root: String,
    /// Whether its first page opens with a quickstart: false for a part
    /// only Link runs, such as a store driver, which a user never acts on.
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub quickstart: bool,
}

fn yes() -> bool {
    true
}

fn is_yes(value: &bool) -> bool {
    *value
}

/// Authored build inputs stay out of installed answers and release indexes.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub schema: u32,
    pub module: Module,
    #[serde(default)]
    pub dependencies: BTreeMap<String, Dependency>,
    #[serde(default)]
    pub requires: Vec<Requirement>,
    #[serde(default)]
    pub recommends: Vec<Requirement>,
    /// A module's are in its `actions.toml`.
    #[serde(default)]
    pub commands: BTreeMap<String, Command>,
    #[serde(default)]
    pub menu: Option<Menu>,
    #[serde(default)]
    pub removal: Option<Removal>,
    #[serde(default)]
    pub documentation: Option<Documentation>,
    /// Additional components needed when upgrading an existing installation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub upgrade_requires: Vec<String>,
    #[serde(default)]
    pub pins: BTreeMap<String, Reference>,
    /// The questions it answers ([`check_question`]), each through its
    /// program's hidden `--link-answer` flag.
    #[serde(default)]
    pub answers: Vec<String>,
    /// Account services whose successful sign-in sets this component up.
    /// These are local release declarations, never a service's install request.
    #[serde(default)]
    pub account_services: Vec<String>,
    /// Whether its program runs a guest service, re-entered by the hidden
    /// `guest-service` command its generated parser has.
    #[serde(default)]
    pub guest_service: bool,
    /// A version 1 module's actions that lead into it from `lnk up`, by
    /// their IDs, which `link-actions` reads.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub path: String,
    pub markdown: String,
}

/// The variable a program's build script hands it the sources it's built
/// from in ([`Resolved::source`]), read as it's compiled.
pub const SOURCE_VARIABLE: &str = "LNK_BUILT_FROM";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolved {
    /// The [`PROTOCOL`] version it's written in.
    pub protocol: u32,
    pub component: Component,
    #[serde(default)]
    pub dependencies: BTreeMap<String, Dependency>,
    #[serde(default)]
    pub requires: Vec<Requirement>,
    #[serde(default)]
    pub recommends: Vec<Requirement>,
    pub commands: BTreeMap<String, Command>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub menu: Option<Menu>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removal: Option<Removal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation: Option<Documentation>,
    /// Additional components needed when upgrading an existing installation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub upgrade_requires: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pages: Vec<Page>,
    /// The questions it answers, each through its program's hidden
    /// `--link-answer` flag: how a module below learns what this one has,
    /// without naming it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub answers: Vec<String>,
    /// Its retired commands, runtime flags and variables, from its
    /// `history.toml`, which its own parser answers.
    #[serde(default, skip_serializing_if = "link_spec::history::History::is_empty")]
    pub history: link_spec::history::History,
    /// The spec kinds it owns, as its program exports them when it
    /// answers `--link-describe`: never in a build's description, which
    /// the types aren't built for.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<link_spec::Exported>,
    /// The sources a program built from source was built from, its
    /// checkout's commit (`link_core::source`): two programs whose sources
    /// differ were built apart, whatever version they say. Nothing for a
    /// released build, whose version is exact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub account_services: Vec<String>,
}

impl Source {
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let source: Self = toml::from_str(text).context("component source declaration")?;
        link_files::layout::check_owner(&source.module.id)?;
        ensure!(
            source.schema == SCHEMA,
            "unsupported component schema {}",
            source.schema
        );
        Ok(source)
    }

    pub fn resolve(self, component: Component, pages: Vec<Page>) -> anyhow::Result<Resolved> {
        ensure!(
            component.kind == self.module.kind,
            "component kind differs from source"
        );
        ensure!(
            component.id == self.module.id,
            "component identity differs from source"
        );
        let resolved = Resolved {
            protocol: PROTOCOL.current,
            component,
            dependencies: self.dependencies,
            requires: self.requires,
            recommends: self.recommends,
            commands: self.commands,
            menu: self.menu,
            removal: self.removal,
            documentation: self.documentation,
            upgrade_requires: self.upgrade_requires,
            pages,
            answers: self.answers,
            account_services: self.account_services,
            history: Default::default(),
            kinds: Vec::new(),
            source: None,
        };
        resolved.validate()?;
        Ok(resolved)
    }
}

impl Resolved {
    /// A description, its protocol checked before its shape is read.
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let stated: Stated = serde_json::from_str(text).context("component runtime answer")?;
        stated.check()?;
        let value: Self = serde_json::from_str(text).context("component runtime answer")?;
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        let value = self;
        PROTOCOL.read(&value.component.program, value.protocol)?;
        let id = &value.component.id;
        link_files::layout::check_owner(id)?;
        for service in &value.account_services {
            ensure!(
                service.strip_prefix("https://").is_some_and(|authority| {
                    !authority.is_empty()
                        && !authority.starts_with('.')
                        && authority
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b".-:[]".contains(&b))
                }),
                "account setup needs an exact HTTPS origin"
            );
        }
        ensure!(
            value.account_services.is_empty() || !value.answers.is_empty(),
            "account setup must declare the capabilities it answers"
        );
        for (n, question) in value.answers.iter().enumerate() {
            check_question(question)?;
            ensure!(
                !value.answers[..n].contains(question),
                "{question} is listed twice in its answers"
            );
        }
        ensure!(
            !id.is_empty()
                && !id.starts_with('-')
                && !id.ends_with('-')
                && id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
            "invalid component identity"
        );
        if value.component.kind != Kind::Program {
            ensure!(
                value.component.program == format!("lnk-{id}"),
                "component identity and program disagree"
            );
        }
        ensure!(
            plain_program(&value.component.program),
            "invalid component program"
        );
        Ok(())
    }

    /// This description, written in the version `asked`, the asker's
    /// ([`asked`]): this release's own where it stated none or a newer
    /// one. A version that changes the shape keeps a writer for each it
    /// still accepts.
    pub fn stating(self, asked: Option<link_version::Accepted>) -> Resolved {
        Resolved {
            protocol: PROTOCOL.answering(asked),
            ..self
        }
    }

    /// Exact origin matching: a sign-in cannot select an executable or
    /// capability. Only this release's local declaration can select one.
    pub fn after_sign_in(&self, account_service: &str) -> bool {
        self.account_services
            .iter()
            .any(|origin| origin == account_service.trim_end_matches('/'))
    }
}

/// An error unless `question` is a question's name: `<module>.<what>`,
/// the module that asks it and what it asks, each lowercase words
/// joined by `-`.
pub fn check_question(question: &str) -> anyhow::Result<()> {
    let word = |w: &str| plain_program(w) && !w.bytes().any(|b| b.is_ascii_digit());
    ensure!(
        question
            .split_once('.')
            .is_some_and(|(module, what)| word(module) && word(what)),
        "{question:?} isn't a question's name (<module>.<what>)"
    );
    Ok(())
}

fn plain_program(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// What every description states whatever its protocol, read before its
/// shape: the version it's written in, and the program it describes, so
/// one of a protocol this release doesn't read, older or newer, is refused
/// by name, never by a field it doesn't know. A program answers in the
/// version its asker stated ([`Resolved::stating`]), so a newer one is a program
/// that didn't.
#[derive(Deserialize)]
struct Stated {
    protocol: Option<u32>,
    #[serde(default)]
    component: Option<StatedComponent>,
}

#[derive(Deserialize)]
struct StatedComponent {
    program: String,
}

impl Stated {
    fn check(&self) -> anyhow::Result<()> {
        let peer = self
            .component
            .as_ref()
            .map_or("a component", |c| c.program.as_str());
        let protocol = self
            .protocol
            .with_context(|| format!("{peer}'s description states no description protocol"))?;
        PROTOCOL.read(peer, protocol)?;
        Ok(())
    }
}

/// A registry, as `lnk --link-registry` prints it: each entry's protocol
/// checked before its shape is read, then the whole validated
/// ([`validate_registry`]).
pub fn parse_registry(text: &[u8]) -> anyhow::Result<BTreeMap<String, Resolved>> {
    let stated: BTreeMap<String, Stated> = serde_json::from_slice(text).context("a registry")?;
    for entry in stated.values() {
        entry.check()?;
    }
    let registry = serde_json::from_slice(text).context("a registry")?;
    validate_registry(&registry)?;
    Ok(registry)
}

/// `command`, which asks a program for its description or `lnk` for its
/// registry, stating this release's [`PROTOCOL`] version.
pub fn asking(command: &mut std::process::Command) -> &mut std::process::Command {
    command.env(PROTOCOL_VARIABLE, PROTOCOL.current.to_string())
}

/// The version the program asking this one for its description or
/// registry stated, which the answer is written in ([`Resolved::stating`]): refused,
/// naming its upgrade, when it's below the oldest this release accepts.
/// None for a person asking by hand, who reads the version the answer
/// states.
pub fn asked() -> anyhow::Result<Option<link_version::Accepted>> {
    asker(std::env::var(PROTOCOL_VARIABLE).ok().as_deref())
}

fn asker(stated: Option<&str>) -> anyhow::Result<Option<link_version::Accepted>> {
    let Some(stated) = stated else {
        return Ok(None);
    };
    let version: u32 = stated.parse().map_err(|_| {
        anyhow::anyhow!("{PROTOCOL_VARIABLE} is {stated:?}, not a protocol version")
    })?;
    Ok(Some(PROTOCOL.accept(
        "the program asking for this description",
        version,
    )?))
}

/// A registry, each description written in the version `asked`, the
/// asker's ([`Resolved::stating`]).
pub fn registry_stating(
    registry: &BTreeMap<String, Resolved>,
    asked: Option<link_version::Accepted>,
) -> BTreeMap<String, Resolved> {
    registry
        .iter()
        .map(|(id, description)| (id.clone(), description.clone().stating(asked)))
        .collect()
}

/// Signed release metadata still has to describe unambiguous local basenames.
pub fn validate_registry(registry: &BTreeMap<String, Resolved>) -> anyhow::Result<()> {
    let mut programs = BTreeMap::new();
    for (id, descriptor) in registry {
        descriptor.validate()?;
        ensure!(
            id == &descriptor.component.id,
            "registry identity differs from its entry"
        );
        for dependency in &descriptor.upgrade_requires {
            ensure!(
                registry.contains_key(dependency),
                "unknown upgrade dependency {dependency}"
            );
        }
        let program = &descriptor.component.program;
        if let Some(other) = programs.insert(program, id) {
            anyhow::bail!("registry program {program} is owned by both {other} and {id}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
link_test_kit::program!(system);
