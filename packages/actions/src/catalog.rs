//! The catalog: every loaded module's declarations, each name resolved to
//! the shape it means and what the declarations imply filled in. A
//! generator reads only the catalog, never a file.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};

use crate::{
    config::{Implied, SETTINGS},
    names::{ActionId, TypeName},
    read::{
        self,
        actions::{Arg, Program, Role, Route, Surface, Tool, Tools},
        module::{Dependency, Identity},
        types::Declared,
        variables::Variables,
    },
    schema::{List, Node, Schema},
};

/// One module's declaration files as a program built them in: its
/// folder's name, and each file's text by its name.
#[derive(Clone, Copy, Debug)]
pub struct Embedded<'a> {
    pub directory: &'a str,
    pub files: &'a [(&'a str, &'a str)],
}

/// One module's declarations, resolved.
#[derive(Clone, Debug)]
pub struct Module {
    pub identity: Identity,
    /// Where its files were read from.
    pub directory: PathBuf,
    pub dependencies: BTreeMap<String, Dependency>,
    pub recommends: Vec<String>,
    /// The questions it answers, through its program's hidden
    /// `--link-answer <question>`, which core gives it.
    pub answers: Vec<String>,
    /// Whether its program runs a guest service, re-entered by the hidden
    /// `guest-service` command core gives it.
    pub guest_service: bool,
    /// Whether its `module.toml` declares `[removal]`: only then does core
    /// give it the hidden `uninstall` that reports and removes those parts.
    pub removal: bool,
    /// The actions that lead into it from `lnk up`, its module.toml's
    /// `next`: what Next offers after a person reaches the module.
    pub next: Vec<ActionId>,
    /// The verbs it declares beyond Link's fixed ones, with their meanings
    /// and effects.
    pub verbs: BTreeMap<String, crate::Verb>,
    /// The runtime arguments every action takes, declared once; each
    /// action's `args` holds them too.
    pub args: BTreeMap<String, Arg>,
    /// The program itself, run with no command.
    pub program: Option<Program>,
    pub actions: BTreeMap<ActionId, Action>,
    /// Its shapes, by their own names.
    pub types: BTreeMap<String, Declared>,
    pub variables: Variables,
    /// Each spec kind's versions and steps, by its kind: `schema.toml`
    /// for the kind named as the namespace, `<kind>-schema.toml` for
    /// another.
    pub schemas: BTreeMap<String, Versions>,
    /// Reserved for IAM: the roles it comes with, as written.
    pub roles: toml::Table,
    /// The actions of each contract it implements, as the contract
    /// declares them; each is among `actions` under its own namespace.
    pub implements: BTreeMap<String, Vec<ActionId>>,
    /// Its argument groups, each one Rust type wherever an action takes it.
    pub groups: BTreeMap<String, crate::read::actions::Group>,
    /// The closed sets its tool actions name.
    pub tools: Tools,
    /// Each group's alias, by the group's words as typed after `lnk`
    /// joined by a space: `module = "mod"`, `manual = "man"`.
    pub aliases: BTreeMap<String, String>,
}

impl Module {
    /// The alias of the group typed as `words` after `lnk`, where its
    /// `[aliases]` gives one.
    pub fn alias(&self, words: &[&str]) -> Option<&str> {
        self.aliases.get(&words.join(" ")).map(String::as_str)
    }
}

/// A spec kind's versions and steps, and the file they're read from.
#[derive(Clone, Debug)]
pub struct Versions {
    pub path: PathBuf,
    pub format: link_files::schema::Format,
}

/// One action, as every surface offers it.
#[derive(Clone, Debug)]
pub struct Action {
    pub id: ActionId,
    pub description: String,
    pub args: BTreeMap<String, Arg>,
    /// The one spec kind it reads.
    pub spec: Option<TypeName>,
    /// A spec kind it reads that is still read by a hand-written type
    /// (`held.toml`), until its module declares it; `spec` is then none.
    pub held: Option<String>,
    /// What it does to state, from its verb.
    pub effect: crate::Effect,
    /// What it answers: declared, or implied by its verb and resource.
    pub output: Schema,
    /// What it reads on stdin, when it reads anything: a declared shape.
    pub stdin: Option<Schema>,
    pub role: Role,
    pub surfaces: Vec<Surface>,
    pub next: Vec<ActionId>,
    pub fail: Vec<ActionId>,
    /// Named hooks the module writes, run after the declared checks pass.
    pub checks: Vec<String>,
    /// The guide section a person reads about it, `/<address>#<anchor>`.
    pub docs: Option<String>,
    /// Reserved for IAM: the argument naming the resource it acts on.
    pub resource: Option<String>,
    /// Whether its group, typed with no command after it, runs it.
    pub default: bool,
    /// Whether it leads into every module, as `lnk up` does: core's alone.
    pub next_modules: bool,
    /// Whether its answer arrives as it's made, each event one choice of
    /// its output, a union told apart by a tag.
    pub stream: bool,
    /// How long a find's answer keeps, and whether it's every caller's.
    pub keeps: Option<crate::Keeps>,
    /// The arguments REST accepts, where it narrows them.
    pub rest_args: Option<Vec<String>>,
    /// Whether it reads or writes a control file: never a run's.
    pub control: bool,
    /// How the agent's bridge offers it, its answer the output above.
    pub route: Option<Route>,
    /// How it's named and annotated as an MCP call, offered on `tool`.
    pub tool: Option<Tool>,
    /// The kind of thing it diagnoses for `lnk issue create`, said, never implied.
    pub diagnoses: Option<String>,
}

impl Action {
    /// Whether it is offered on `surface`: among its declared surfaces.
    pub fn offered_on(&self, surface: Surface) -> bool {
        self.surfaces.contains(&surface)
    }

    /// Whether REST accepts its argument `name`: one its `rest-args`
    /// names, or with none, any but those saying where or how a machine
    /// runs it.
    pub fn rest_accepts(&self, name: &str, arg: &Arg) -> bool {
        match &self.rest_args {
            Some(accepted) => accepted.iter().any(|one| one == name),
            None => arg
                .cli
                .shared
                .as_ref()
                .is_none_or(|shared| !SETTINGS.rest.never.contains(shared)),
        }
    }

    /// Its help's first line.
    pub fn summary(&self) -> &str {
        self.description.lines().next().unwrap_or_default()
    }

    /// The words of the group that runs it when typed bare, where it's
    /// its group's default.
    pub fn group(&self) -> Vec<&str> {
        let mut words = self.id.words();
        words.pop();
        words
    }

    /// Its positional arguments, in order.
    pub fn positionals(&self) -> Vec<(&str, &Arg)> {
        let mut out: Vec<_> = self
            .args
            .iter()
            .filter_map(|(name, arg)| Some((arg.cli.position?, (name.as_str(), arg))))
            .collect();
        out.sort_by_key(|(position, _)| *position);
        out.into_iter().map(|(_, arg)| arg).collect()
    }
}

/// Every loaded module's declarations, checked.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    modules: BTreeMap<String, Module>,
}

impl Catalog {
    /// The modules in `directories`, each a folder holding a `module.toml`.
    /// Core's declarations are always loaded beside them, so every
    /// module names its shapes (`core.os`).
    pub fn load(directories: &[&Path]) -> anyhow::Result<Self> {
        let read = |directory: &Path| {
            load(directory).with_context(|| format!("module at {}", directory.display()))
        };
        let all = directories
            .iter()
            .map(|directory| read(directory))
            .collect::<anyhow::Result<Vec<_>>>()?;
        Self::assemble(all)
    }

    /// The modules `embedded` holds, each its folder's name and its
    /// declaration files' texts by file name, as a program built them in:
    /// read wherever it runs, never from a path of the machine that built
    /// it. Core's declarations are loaded beside them, as [`Self::load`]
    /// loads them.
    pub fn embedded(embedded: &[Embedded<'_>]) -> anyhow::Result<Self> {
        let all = embedded
            .iter()
            .map(|module| {
                from(Path::new(module.directory), |name| {
                    Ok(module
                        .files
                        .iter()
                        .find(|(file, _)| *file == name)
                        .map(|(_, text)| text.to_string()))
                })
                .with_context(|| format!("module at {}, as built in", module.directory))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Self::assemble(all)
    }

    /// The catalog of the modules read.
    fn assemble(mut all: Vec<Module>) -> anyhow::Result<Self> {
        let mut modules = BTreeMap::new();
        // Core's shapes are known by what they declare, so a tree read
        // anywhere, as a copy on another machine, gives its own; else the
        // ones built in, never a path of the machine that built it.
        if !all.iter().any(is_core_shapes) {
            all.push(core_shapes()?);
        }
        let core_id = &SETTINGS.core.module;
        for module in all {
            let id = module.identity.id.clone();
            // Core is one module: its program's declarations, `lnk`'s,
            // and the shapes every module shares, kept with link-actions.
            let module = match modules.remove(&id) {
                Some(other) if id == *core_id => merge_core(other, module)?,
                Some(_) => bail!("module {id} is loaded twice"),
                None => module,
            };
            modules.insert(id, module);
        }
        let mut catalog = Self { modules };
        catalog.implement()?;
        catalog.settle_tags()?;
        catalog.check()?;
        Ok(catalog)
    }

    /// Each tagged choice's tag, now its choices' names resolve: the one
    /// property every choice holds as a `const`.
    fn settle_tags(&mut self) -> anyhow::Result<()> {
        let names = self.clone();
        let mut problem = None;
        let mut settle = |schema: &mut Schema| {
            let Node::OneOf(one) = &mut schema.node else {
                return;
            };
            let crate::schema::Told::Tag(tag) = &mut one.told else {
                return;
            };
            // A choice of a number, words or a text, and at most one
            // record, told apart by its value's JSON type.
            let nodes: Vec<_> = one
                .variants
                .iter()
                .map(|variant| names.resolve(variant).map(|s| &s.node))
                .collect();
            let records = nodes
                .iter()
                .filter(|n| matches!(n, Ok(Node::Record(_))))
                .count();
            if records < nodes.len()
                && nodes.iter().all(|node| {
                    matches!(
                        node,
                        Ok(Node::Integer(_)
                            | Node::Number(_)
                            | Node::String(_)
                            | Node::Record(_)
                            | Node::Const(serde_json::Value::String(_)))
                    )
                })
            {
                one.told = crate::schema::Told::Value;
                return;
            }
            let mut common: Option<std::collections::BTreeSet<String>> = None;
            for variant in &one.variants {
                let consts = match names.resolve(variant).map(|s| &s.node) {
                    Ok(Node::Record(record)) => record
                        .properties
                        .iter()
                        .filter(|(_, s)| matches!(s.node, Node::Const(_)))
                        .map(|(name, _)| name.clone())
                        .collect(),
                    _ => Default::default(),
                };
                common = Some(match common {
                    None => consts,
                    Some(common) => common.intersection(&consts).cloned().collect(),
                });
            }
            match common
                .filter(|c| c.len() == 1)
                .and_then(|c| c.into_iter().next())
            {
                Some(found) => *tag = found,
                None => {
                    problem.get_or_insert_with(|| {
                        anyhow::anyhow!(
                            "each oneOf choice is a record holding one property as a const, the same property in each, or the oneOf names each choice's marking key (x-link-markers)"
                        )
                    });
                }
            }
        };
        for module in self.modules.values_mut() {
            for declared in module.types.values_mut() {
                declared.schema.walk_mut(&mut settle);
            }
            for action in module.actions.values_mut() {
                action.output.walk_mut(&mut settle);
                if let Some(stdin) = &mut action.stdin {
                    stdin.walk_mut(&mut settle);
                }
                for arg in action.args.values_mut() {
                    arg.schema.walk_mut(&mut settle);
                }
            }
        }
        problem.map_or(Ok(()), Err)
    }

    /// Each contract action a module implements, made its own: the
    /// contract's declaration under the module's namespace, so
    /// `store.store.serve` implemented by `file-store` is
    /// `file-store.file-store.serve`, run as `lnk-file-store serve`.
    fn implement(&mut self) -> anyhow::Result<()> {
        let mut imported = Vec::new();
        for module in self.modules.values() {
            let own = &module.identity.id;
            for (contract, ids) in &module.implements {
                let declared = self.modules.get(contract).with_context(|| {
                    format!(
                        "module {own} implements {contract}, whose declarations aren't loaded: its build names the contract's folder"
                    )
                })?;
                anyhow::ensure!(
                    module.dependencies.values().any(|d| d.id() == contract),
                    "module {own} implements {contract}, which module.toml doesn't list as a dependency"
                );
                for id in ids {
                    let action = declared.actions.get(id).with_context(|| {
                        format!("module {own} implements {id}, which contract {contract} doesn't declare")
                    })?;
                    let resource = match id.resource() == contract {
                        true => own.as_str(),
                        false => id.resource(),
                    };
                    let mine = ActionId::new(own, resource, id.verb())?;
                    anyhow::ensure!(
                        !module.actions.contains_key(&mine),
                        "module {own} declares {mine}, which is contract {contract}'s {id}: [implements] lists it, and the contract declares it"
                    );
                    let mut action = action.clone();
                    action.id = mine;
                    for (name, arg) in &module.args {
                        action
                            .args
                            .entry(name.clone())
                            .or_insert_with(|| arg.clone());
                    }
                    imported.push((own.clone(), action));
                }
            }
        }
        for (own, action) in imported {
            if let Some(module) = self.modules.get_mut(&own) {
                module.actions.insert(action.id.clone(), action);
            }
        }
        Ok(())
    }

    pub fn modules(&self) -> impl Iterator<Item = &Module> {
        self.modules.values()
    }

    pub fn module(&self, id: &str) -> Option<&Module> {
        self.modules.get(id)
    }

    /// Every action of every module, in ID order within each module.
    pub fn actions(&self) -> impl Iterator<Item = &Action> {
        self.modules.values().flat_map(|m| m.actions.values())
    }

    pub fn action(&self, id: &ActionId) -> Option<&Action> {
        self.modules.get(id.namespace())?.actions.get(id)
    }

    /// Whether `id` is a control action, one no run is ever given: false
    /// for one the catalog doesn't declare, which no grant gives anyway.
    pub fn is_control(&self, id: &ActionId) -> bool {
        self.action(id).is_some_and(|action| action.control)
    }

    /// The shape a name means.
    pub fn shape(&self, name: &TypeName) -> Option<&Declared> {
        self.modules.get(name.module())?.types.get(name.name())
    }

    /// The schema a name means, followed through names to one that isn't.
    pub fn resolve<'a>(&'a self, mut schema: &'a Schema) -> anyhow::Result<&'a Schema> {
        for _ in 0..=SETTINGS.files.max_depth {
            match &schema.node {
                Node::Named(name) => {
                    schema = &self
                        .shape(name)
                        .with_context(|| format!("{name} isn't declared"))?
                        .schema;
                }
                _ => return Ok(schema),
            }
        }
        bail!("names refer to each other with no shape at the end")
    }

    /// Whether `schema` holds a secret: it, or a shape it reaches through
    /// fields, items, choices and names, is marked `x-link-sensitive`.
    pub fn sensitive(&self, schema: &Schema) -> bool {
        let mut seen = std::collections::BTreeSet::new();
        let mut todo = vec![schema];
        while let Some(schema) = todo.pop() {
            let mut found = false;
            schema.walk(&mut |inner| {
                found |= inner.meta.sensitive;
                if let Node::Named(name) = &inner.node
                    && seen.insert(name.clone())
                    && let Some(declared) = self.shape(name)
                {
                    todo.push(&declared.schema);
                }
            });
            if found {
                return true;
            }
        }
        false
    }

    /// Whether what `action` prints holds a secret, by its declared output.
    pub fn prints_secret(&self, action: &Action) -> bool {
        self.sensitive(&action.output)
    }

    /// Whether what `action` reads on stdin holds a secret, by its
    /// declared `stdin`.
    pub fn reads_secret(&self, action: &Action) -> bool {
        action.stdin.as_ref().is_some_and(|s| self.sensitive(s))
    }

    /// The spec kind named `kind`, wherever it is declared.
    pub fn spec(&self, kind: &str) -> Option<(TypeName, &Declared)> {
        self.modules.values().find_map(|module| {
            module.types.iter().find_map(|(name, declared)| {
                (declared.spec.as_deref() == Some(kind))
                    .then(|| (TypeName::new(&module.identity.id, name).ok(), declared))
                    .and_then(|(name, declared)| Some((name?, declared)))
            })
        })
    }
}

/// Core's two halves as one module: the program's declarations, and the
/// shared shapes from link-actions, each shape declared once.
fn merge_core(a: Module, b: Module) -> anyhow::Result<Module> {
    let (mut program, shapes) = match is_core_shapes(&a) {
        true => (b, a),
        false => (a, b),
    };
    anyhow::ensure!(
        is_core_shapes(&shapes) && !is_core_shapes(&program),
        "core is its program's declarations and its shapes, one of each"
    );
    for (name, declared) in shapes.types {
        anyhow::ensure!(
            program.types.insert(name.clone(), declared).is_none(),
            "core's shape {name} is declared twice"
        );
    }
    Ok(program)
}

/// Whether `module` is core's half holding the shapes every module shares.
fn is_core_shapes(module: &Module) -> bool {
    module.identity.id == SETTINGS.core.module && module.identity.kind == SETTINGS.core.shapes
}

/// Core's shapes as link-actions was built with them, its `core/` folder's
/// files embedded, so a program reads them wherever it runs.
const CORE_SHAPES: &[(&str, &str)] = &[
    ("module.toml", include_str!("../core/module.toml")),
    ("types.toml", include_str!("../core/types.toml")),
];

/// Core's shapes from the files embedded at build time.
pub(crate) fn core_shapes() -> anyhow::Result<Module> {
    from(crate::core_directory(), |name| {
        Ok(CORE_SHAPES
            .iter()
            .find(|(file, _)| *file == name)
            .map(|(_, text)| text.to_string()))
    })
    .context("core's shapes, as built")
}

fn load(directory: &Path) -> anyhow::Result<Module> {
    from(directory, |name| read::text(directory, name))
}

/// A module from the files `text` gives by name, named as in `directory`.
fn from(
    directory: &Path,
    text: impl Fn(&str) -> anyhow::Result<Option<String>>,
) -> anyhow::Result<Module> {
    let files = &SETTINGS.files;
    let need = |name: &str| -> anyhow::Result<String> {
        text(name)?.with_context(|| format!("its {name}"))
    };
    let module = read::module::read(&need(&files.module)?).context(files.module.clone())?;
    let id = module.identity.id.clone();
    let types = match text(&files.types)? {
        Some(text) => read::types::read(&text, &id).context(files.types.clone())?,
        None => BTreeMap::new(),
    };
    let actions = match text(&files.actions)? {
        Some(text) => Some(read::actions::read(&text, &id).context(files.actions.clone())?),
        None => None,
    };
    let variables = match text(&files.variables)? {
        Some(text) => read::variables::read(&text).context(files.variables.clone())?,
        None => Variables::default(),
    };
    let mut schemas = BTreeMap::new();
    for declared in types.values() {
        let Some(kind) = &declared.spec else { continue };
        let file = crate::schema_file(&id, kind);
        if let Some(text) = text(&file)? {
            let format = link_files::schema::Format::parse(&text).context(file.clone())?;
            anyhow::ensure!(
                format.kind == *kind,
                "{file} is kind {:?}, and holds spec kind {kind:?}'s versions",
                format.kind
            );
            schemas.insert(
                kind.clone(),
                Versions {
                    path: directory.join(&file),
                    format,
                },
            );
        }
    }
    // A schema.toml naming a kind the module doesn't declare is a saved
    // file's, kept for that file's reader.
    if !schemas.values().any(|v| v.path.ends_with(&files.schema))
        && let Some(text) = text(&files.schema)?
    {
        let format = link_files::schema::Format::parse(&text).context(files.schema.clone())?;
        anyhow::ensure!(
            !types
                .values()
                .any(|d| d.spec.as_ref() == Some(&format.kind)),
            "{} holds spec kind {:?}, whose versions are in {}",
            files.schema,
            format.kind,
            crate::schema_file(&id, &format.kind)
        );
        schemas.insert(
            format.kind.clone(),
            Versions {
                path: directory.join(&files.schema),
                format,
            },
        );
    }
    let mut out = Module {
        identity: module.identity,
        directory: directory.to_path_buf(),
        dependencies: module.dependencies,
        recommends: module.recommends,
        answers: module.answers,
        guest_service: module.guest_service,
        removal: module.removal,
        next: module.next,
        verbs: BTreeMap::new(),
        args: BTreeMap::new(),
        program: None,
        actions: BTreeMap::new(),
        types,
        variables,
        schemas,
        roles: toml::Table::new(),
        implements: BTreeMap::new(),
        groups: BTreeMap::new(),
        tools: Tools::default(),
        aliases: BTreeMap::new(),
    };
    if let Some(file) = actions {
        out.verbs = file.verbs;
        out.roles = file.roles;
        out.args = file.args;
        out.program = file.program;
        out.implements = file.implements;
        out.groups = file.groups;
        out.tools = file.tools;
        out.aliases = file.aliases;
        for (id, mut written) in file.actions {
            if let Some(name) = written.without.iter().find(|n| !out.args.contains_key(*n)) {
                anyhow::bail!(
                    "action {id}: args.{name} = false leaves out a module-wide argument, and [args] declares no {name}"
                );
            }
            for (name, arg) in out
                .args
                .iter()
                .filter(|(n, _)| !written.without.contains(n))
            {
                anyhow::ensure!(
                    !written.args.contains_key(name),
                    "action {id}: argument {name} is the module's, in [args], which every action takes"
                );
                written.args.insert(name.clone(), arg.clone());
            }
            let action = crate::check::action(&out, id.clone(), written)
                .with_context(|| format!("action {id}"))?;
            out.actions.insert(id, action);
        }
    }
    crate::tools::check(&out)?;
    Ok(out)
}

/// What an action answers when it declares nothing: the resource its ID
/// names, a list of it or its ID, as its verb says.
pub(crate) fn implied(module: &Module, id: &ActionId) -> anyhow::Result<Option<Schema>> {
    let Some(implied) = SETTINGS
        .verbs
        .fixed
        .get(id.verb())
        .map(|fixed| fixed.output)
    else {
        return Ok(None);
    };
    let Some(declared) = module.types.get(id.resource()) else {
        return Ok(None);
    };
    let name = Schema::new(Node::Named(TypeName::new(
        &module.identity.id,
        id.resource(),
    )?));
    Ok(Some(match implied {
        Implied::Resource => name,
        Implied::List => Schema::new(Node::List(List {
            items: Box::new(name),
            min_items: None,
            max_items: None,
        })),
        Implied::Id => {
            let field = declared.resource.as_deref().with_context(|| {
                format!(
                    "{} answers {}'s ID, but {} has no x-link-resource",
                    id,
                    id.resource(),
                    id.resource()
                )
            })?;
            match &declared.schema.node {
                Node::Record(record) => {
                    record.properties.get(field).cloned().with_context(|| {
                        format!("{}'s ID property {field} isn't declared", id.resource())
                    })?
                }
                _ => bail!("resource {} is a record", id.resource()),
            }
        }
        Implied::None => return Ok(None),
    }))
}

#[cfg(test)]
mod core_shapes_tests {
    use super::*;

    /// The shapes built in are the `core/` folder's files, all of them and
    /// as they are, so no runtime read of that folder is ever needed.
    #[test]
    fn the_shapes_built_in_are_the_core_folders() {
        let mut files: Vec<String> = std::fs::read_dir(crate::core_directory())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        files.sort();
        let mut built: Vec<&str> = CORE_SHAPES.iter().map(|(file, _)| *file).collect();
        built.sort();
        assert_eq!(files, built, "embed every file of core/ in CORE_SHAPES");
        for (file, text) in CORE_SHAPES {
            let on_disk = std::fs::read_to_string(crate::core_directory().join(file)).unwrap();
            assert_eq!(on_disk, *text, "{file}");
        }
        let module = core_shapes().unwrap();
        assert!(is_core_shapes(&module));
        assert!(!module.types.is_empty());
    }
}

#[cfg(test)]
mod embedded_tests {
    use super::*;

    /// A module built in reads as the same module read from its folder:
    /// its actions, and which are control.
    #[test]
    fn a_module_built_in_reads_as_its_folder_does() {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules/storage/safe");
        let files: Vec<(String, String)> = ["module.toml", "actions.toml", "types.toml"]
            .iter()
            .map(|name| {
                let text = std::fs::read_to_string(folder.join(name)).unwrap();
                (name.to_string(), text)
            })
            .collect();
        let files: Vec<(&str, &str)> = files
            .iter()
            .map(|(name, text)| (name.as_str(), text.as_str()))
            .collect();
        let built = Catalog::embedded(&[Embedded {
            directory: "safe",
            files: &files,
        }])
        .unwrap();
        let read = Catalog::load(&[&folder]).unwrap();
        let ids = |catalog: &Catalog| -> Vec<(String, bool)> {
            catalog
                .actions()
                .map(|a| (a.id.to_string(), a.control))
                .collect()
        };
        assert!(!ids(&built).is_empty());
        assert_eq!(ids(&built), ids(&read));
    }
}
