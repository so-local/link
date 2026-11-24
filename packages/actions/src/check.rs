//! The rules every declaration passes before a generator reads it.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, bail, ensure};

use crate::{
    Catalog,
    catalog::{Action, Module, implied},
    config::SETTINGS,
    names::{ActionId, TypeName, is_name},
    read::actions::{Role, Surface, Written},
    schema::{Node, Schema},
};

/// An action as written, checked against its own module and made whole.
pub(crate) fn action(module: &Module, id: ActionId, written: Written) -> anyhow::Result<Action> {
    let own = &module.identity.id;
    ensure!(
        id.namespace() == own,
        "an action of module {own} begins {own}."
    );
    ensure!(
        id.resource() == own || module.types.contains_key(id.resource()),
        "its resource {:?} is a shape in types.toml or the namespace's own name",
        id.resource()
    );
    let verb = id.verb();
    let declared_verb = module.verbs.contains_key(verb);
    let effect = match (SETTINGS.verbs.fixed.get(verb), module.verbs.get(verb)) {
        (Some(fixed), _) => fixed.effect,
        (None, Some(own)) => own.effect,
        (None, None) => bail!(
            "verb {verb:?} is one of Link's fixed verbs or one the module declares under [verbs]"
        ),
    };
    // A kind its module doesn't declare yet may be held, read by a
    // hand-written type until it does.
    let held = written
        .spec
        .as_deref()
        .filter(|name| !module.types.contains_key(*name) && crate::held_spec(name).is_some())
        .map(str::to_string);
    let spec = written
        .spec
        .as_deref()
        .filter(|_| held.is_none())
        .map(|name| TypeName::written(name, own))
        .transpose()?;
    // Its output is what it names, or else what its verb implies.
    let implied = match &written.output {
        Some(_) => None,
        None => implied(module, &id)?,
    };
    let output = match (&written.output, implied) {
        (Some(name), _) => Schema::new(Node::Named(TypeName::written(name, own)?)),
        (None, Some(implied)) => implied,
        (None, None) if declared_verb => {
            bail!("its verb {verb:?} is the module's own, so it names its output")
        }
        (None, None) => bail!(
            "its verb implies its resource as its output, and {:?} isn't a shape: it names its output",
            id.resource()
        ),
    };
    let stdin = written
        .stdin
        .as_deref()
        .map(|name| TypeName::written(name, own).map(|name| Schema::new(Node::Named(name))))
        .transpose()?;
    let surfaces = match (written.role, written.surfaces) {
        (Role::Plumbing, None) => vec![Surface::Cli],
        (Role::Plumbing, Some(s)) => {
            ensure!(
                s == [Surface::Cli],
                "a plumbing action is offered on the command line alone"
            );
            s
        }
        (_, None) => Surface::ALL.to_vec(),
        (_, Some(s)) => {
            // An action its route alone offers, as a command the agent's
            // harness runs itself, is on no other surface.
            ensure!(
                !s.is_empty() || written.route.is_some(),
                "surfaces names at least one, or none for an action its route alone offers"
            );
            let distinct: BTreeSet<_> = s.iter().collect();
            ensure!(distinct.len() == s.len(), "surfaces repeats one");
            // A service's own route is called by other principals alone,
            // never a person's command or a model's tool.
            ensure!(
                !s.contains(&Surface::Service) || s == [Surface::Service],
                "surfaces names service alone, a service's own route"
            );
            // The app's page calls the action through the API.
            ensure!(
                !s.contains(&Surface::App) || s.contains(&Surface::Rest),
                "surfaces names app only with rest, which its page calls"
            );
            s
        }
    };
    args(&written.args)?;
    hooks(&written.checks)?;
    ensure!(
        !written.next_modules || *own == crate::core().module,
        "next-modules is core's: only its commands lead into every module"
    );
    if let Some(docs) = &written.docs {
        ensure!(
            written.role != Role::Plumbing,
            "a plumbing action is on no page a person reads, so it names no docs"
        );
        ensure!(
            docs.starts_with('/')
                && docs
                    .split_once('#')
                    .is_some_and(|(_, anchor)| !anchor.is_empty()),
            "docs is a page's address and a section's anchor, /<address>#<anchor>"
        );
    }
    if let Some(kind) = &written.diagnoses {
        ensure!(
            is_name(kind),
            "diagnoses names a kind of thing, as agent or box"
        );
        ensure!(
            id.verb() == "show",
            "diagnoses is a show action's: it shows the thing's state"
        );
    }
    if written.control {
        ensure!(
            !surfaces.contains(&Surface::Tool),
            "it's control, so it's never a model's tool: its surfaces leave out tool"
        );
        ensure!(
            written
                .route
                .as_ref()
                .is_none_or(|route| route.callers.iter().all(|caller| matches!(
                    caller,
                    crate::Caller::Owner | crate::Caller::Program | crate::Caller::Trigger
                ))),
            "it's control, so only its owner, a program, or a trigger calls it: its route's callers say no other"
        );
    }
    if let Some(rest) = &written.rest_args {
        ensure!(
            surfaces.contains(&Surface::Rest),
            "rest-args names what REST accepts, so it's offered on rest"
        );
        for name in rest {
            let arg = written
                .args
                .get(name)
                .with_context(|| format!("rest-args names {name}, none of its arguments"))?;
            ensure!(
                arg.cli
                    .shared
                    .as_ref()
                    .is_none_or(|shared| !SETTINGS.rest.never.contains(shared)),
                "rest-args names {name}, which says where a machine runs it: a REST call is answered where its service runs"
            );
        }
    }
    ensure!(
        written.keeps.is_none() || effect == crate::Effect::Find,
        "keeps is a find's: only an answer that changes nothing may be kept"
    );
    // A model's call is read as the action's runtime arguments alone, so
    // a spec's fields would be refused at every call.
    ensure!(
        written.tool.is_none() || spec.is_none(),
        "a tool's action binds no spec: a model's call carries its arguments alone"
    );
    if let Some(resource) = &written.resource {
        ensure!(
            written.args.contains_key(resource),
            "resource names one of its arguments"
        );
    }
    Ok(Action {
        id,
        description: written.description,
        args: written.args,
        spec,
        held,
        effect,
        output,
        stdin,
        role: written.role,
        surfaces,
        next: written.next,
        fail: written.fail,
        checks: written.checks,
        docs: written.docs,
        resource: written.resource,
        default: written.default,
        next_modules: written.next_modules,
        stream: written.stream,
        keeps: written.keeps,
        rest_args: written.rest_args,
        control: written.control,
        route: written.route,
        tool: written.tool,
        diagnoses: written.diagnoses,
    })
}

fn args(args: &BTreeMap<String, crate::read::actions::Arg>) -> anyhow::Result<()> {
    for (name, arg) in args {
        for other in &arg.cli.conflicts {
            ensure!(
                other != name && args.contains_key(other),
                "argument {name} conflicts with {other:?}, which isn't another of its arguments"
            );
        }
        for other in &arg.cli.requires {
            ensure!(
                other != name && args.contains_key(other),
                "argument {name} requires {other:?}, which isn't another of its arguments"
            );
            ensure!(
                !arg.cli.conflicts.contains(other),
                "argument {name} both requires and conflicts with {other}"
            );
        }
    }
    let mut positions: Vec<usize> = args
        .values()
        .filter_map(|a| a.cli.position.map(usize::from))
        .collect();
    positions.sort_unstable();
    ensure!(
        positions.iter().enumerate().all(|(i, p)| *p == i + 1),
        "its positions run 1, 2, 3 with none skipped or repeated"
    );
    ensure!(
        args.values().filter(|a| a.cli.rest).count() <= 1,
        "at most one argument takes the rest"
    );
    let shorts: Vec<char> = args.values().filter_map(|a| a.cli.short).collect();
    ensure!(
        shorts.iter().collect::<BTreeSet<_>>().len() == shorts.len(),
        "two arguments share a short flag"
    );
    for short in &shorts {
        ensure!(
            short.is_ascii_alphanumeric(),
            "a short flag is a letter or digit"
        );
    }
    Ok(())
}

impl Catalog {
    pub(crate) fn check(&self) -> anyhow::Result<()> {
        let mut kinds = BTreeMap::new();
        // A chat command's name is what an agent types, so one names one
        // action.
        let mut names: BTreeMap<&str, &ActionId> = BTreeMap::new();
        let mut paths: BTreeMap<&str, &ActionId> = BTreeMap::new();
        for action in self.actions() {
            if let Some(path) = action.route.as_ref().and_then(|r| r.path.as_deref())
                && let Some(other) = paths.insert(path, &action.id)
            {
                bail!("route path {path:?} is both {other}'s and {}'s", action.id);
            }
            for name in action.route.iter().flat_map(|r| &r.names) {
                if let Some(other) = names.insert(name, &action.id) {
                    bail!("chat command {name} names both {other} and {}", action.id);
                }
            }
        }
        self.first_words_check()?;
        for action in self.actions() {
            self.transport_check(action)
                .with_context(|| format!("action {}", action.id))?;
        }
        for module in self.modules() {
            let own = &module.identity.id;
            self.module_check(module)
                .with_context(|| format!("module {own}"))?;
            for (name, declared) in &module.types {
                if let Some(kind) = &declared.spec
                    && let Some(other) = kinds.insert(kind.clone(), format!("{own}.{name}"))
                {
                    bail!("spec kind {kind:?} is declared twice: {other} and {own}.{name}");
                }
            }
        }
        Ok(())
    }

    /// Each alias stands for one group a person types, and no word beside
    /// it is the same: the group is the start of a longer command on the
    /// command line, and the alias is neither its own word nor another
    /// word typed in its place.
    fn aliases_check(&self, module: &Module) -> anyhow::Result<()> {
        let typed: Vec<Vec<&str>> = module
            .actions
            .values()
            .filter(|a| a.surfaces.contains(&Surface::Cli))
            .map(|a| a.id.typed())
            .collect();
        for (group, alias) in &module.aliases {
            let words: Vec<&str> = group.split(' ').collect();
            ensure!(
                typed
                    .iter()
                    .any(|t| t.len() > words.len() && t.starts_with(&words)),
                "[aliases] {group}: no group of the module's command line is typed so"
            );
            let (last, parent) = words.split_last().context("a group has words")?;
            ensure!(
                alias != last,
                "[aliases] {group}: its alias is another word than its own"
            );
            // A namespace's word is beside every module's, which the
            // catalog checks across modules.
            if parent.is_empty() && module.identity.id != SETTINGS.core.module {
                continue;
            }
            let beside: Vec<&str> = typed
                .iter()
                .filter(|t| t.len() > parent.len() && t.starts_with(parent))
                .map(|t| t[parent.len()])
                .collect();
            ensure!(
                !beside.contains(&alias.as_str()),
                "[aliases] {group}: {alias} is a command typed there already"
            );
        }
        Ok(())
    }

    /// No two of the words typed first after `lnk` are the same: each
    /// module's namespace, core's groups, and the aliases of either.
    fn first_words_check(&self) -> anyhow::Result<()> {
        let mut first: BTreeMap<String, String> = BTreeMap::new();
        let mut take = |word: &str, whose: String| -> anyhow::Result<()> {
            match first.insert(word.to_string(), whose.clone()) {
                Some(other) if other != whose => {
                    bail!("lnk {word} is typed for both {other} and {whose}")
                }
                _ => Ok(()),
            }
        };
        for module in self.modules() {
            let own = &module.identity.id;
            for action in module
                .actions
                .values()
                .filter(|a| a.surfaces.contains(&Surface::Cli))
            {
                if let Some(word) = action.id.typed().first() {
                    take(word, format!("module {own}'s commands"))?;
                }
            }
            for (group, alias) in module.aliases.iter().filter(|(g, _)| !g.contains(' ')) {
                take(alias, format!("module {own}'s {group}"))?;
            }
        }
        Ok(())
    }

    /// What a transport derives from an action holds: a stream's events
    /// are a union told apart by a tag, which names each event.
    fn transport_check(&self, action: &Action) -> anyhow::Result<()> {
        if action.stream {
            let output = self.resolve(&action.output)?;
            ensure!(
                matches!(&output.node, Node::OneOf(choice) if matches!(choice.told, crate::schema::Told::Tag(_))),
                "it streams, so its output is a union of event records told apart by a tag, which names each event"
            );
        }
        Ok(())
    }

    fn module_check(&self, module: &Module) -> anyhow::Result<()> {
        let own = &module.identity.id;
        crate::iam::check(module)?;
        for (verb, declared) in &module.verbs {
            let meaning = &declared.description;
            ensure!(is_name(verb), "verb {verb:?} is a lowercase name");
            ensure!(
                !SETTINGS.verbs.fixed.contains_key(verb),
                "{verb} is a fixed verb: it's declared already"
            );
            if let Some(instead) = SETTINGS.verbs.banned.get(verb) {
                bail!("verb {verb:?} means a fixed one: say {instead}");
            }
            ensure!(!meaning.trim().is_empty(), "verb {verb} says what it means");
            ensure!(
                module.actions.keys().any(|id| id.verb() == verb),
                "verb {verb} is declared and no action uses it"
            );
        }
        let core = &SETTINGS.core;
        let words: Vec<(&ActionId, Vec<&str>)> =
            module.actions.keys().map(|id| (id, id.words())).collect();
        for (id, said) in &words {
            // The namespace's own resource leaves its word out, so only
            // the spec module's own actions may be on `spec`.
            ensure!(
                id.resource() != core.spec || id.resource() == own,
                "action {id}: its resource {:?} is core's, which gives {} spec show",
                core.spec,
                own
            );
            ensure!(
                !(module.guest_service && *said == [core.guest.as_str()]),
                "action {id}: its command is core's {}, which a module running a guest service has",
                core.guest
            );
            // Core gives every other module its uninstall, and declares its own.
            ensure!(
                *own == core.module || *said != [core.uninstall.as_str()],
                "action {id}: its command is core's {}, which every module has",
                core.uninstall
            );
            for (other, longer) in &words {
                ensure!(
                    longer.len() <= said.len() || !longer.starts_with(said),
                    "action {id}'s command, {}, is also the start of {other}'s",
                    said.join(" ")
                );
            }
        }
        self.aliases_check(module)?;
        // What leads into the module is a command a person types: its own,
        // on the command line, and never plumbing.
        for id in &module.next {
            let action = module.actions.get(id).with_context(|| {
                format!("module.toml's next names {id}, which isn't one of the module's actions")
            })?;
            ensure!(
                action.surfaces.contains(&Surface::Cli) && action.role != Role::Plumbing,
                "module.toml's next names {id}, which a person never types: it's off the command line or plumbing"
            );
        }
        // A group runs at most one default, never one of core's.
        let mut defaults: BTreeMap<Vec<&str>, &ActionId> = BTreeMap::new();
        for action in module.actions.values().filter(|a| a.default) {
            // A group runs its default when typed bare, on the command line.
            ensure!(
                action.surfaces.contains(&Surface::Cli),
                "action {}: a group's default is run on the command line, so its surfaces include cli",
                action.id
            );
            // A plumbing default only for a group of plumbing alone, which
            // Link runs bare and a person never sees.
            let group = action.group();
            ensure!(
                action.role != Role::Plumbing
                    || module
                        .actions
                        .values()
                        .filter(|a| a.id.words().starts_with(&group))
                        .all(|a| a.role == Role::Plumbing),
                "action {}: a plumbing action is a group's default only where every action of the group is plumbing",
                action.id
            );
            if let Some(other) = defaults.insert(action.group(), &action.id) {
                bail!(
                    "actions {other} and {} are both their group's default",
                    action.id
                );
            }
            if action.group().is_empty() && module.program.is_some() {
                bail!(
                    "action {}: a module with [program] runs it with no command, so no action is the program's default",
                    action.id
                );
            }
        }
        if let Some(program) = &module.program {
            super::check::args(&program.args).context("[program]")?;
            if let Some(name) = program.args.keys().find(|n| module.args.contains_key(*n)) {
                bail!("argument {name} is in both [program.args] and [args]");
            }
        }
        // Every runtime flag's fallback, wherever it's declared: an
        // action's own (below), the module's `[args]`, its program's and
        // each group's.
        let shared = module
            .args
            .values()
            .map(|arg| ("[args]".to_string(), arg))
            .chain(
                module
                    .program
                    .iter()
                    .flat_map(|p| p.args.values())
                    .map(|arg| ("[program]".to_string(), arg)),
            )
            .chain(module.groups.iter().flat_map(|(name, group)| {
                group
                    .args
                    .values()
                    .map(move |arg| (format!("group {name}"), arg))
            }));
        for (at, arg) in shared {
            if let Some(variable) = &arg.cli.env {
                self.variable(module, variable).context(at)?;
            }
        }
        for (name, declared) in &module.types {
            self.names(module, &declared.schema)
                .with_context(|| format!("shape {name}"))?;
            self.defaults(&declared.schema)
                .with_context(|| format!("shape {name}"))?;
            if let Some(field) = &declared.resource {
                let Node::Record(record) = &declared.schema.node else {
                    bail!("resource {name} is a record");
                };
                ensure!(
                    record.required.contains(field),
                    "resource {name}'s ID property {field:?} is a required property"
                );
            }
            if declared.spec.is_some() {
                ensure!(
                    matches!(declared.schema.node, Node::Record(_)),
                    "spec {name} is a record"
                );
            }
            hooks(&declared.checks).with_context(|| format!("shape {name}"))?;
            ensure!(
                declared.checks.is_empty()
                    || !matches!(declared.schema.node, Node::Enum(_) | Node::OneOf(_)),
                "shape {name}: x-link-checks is a record's or a plain value's: a choice is checked by its choices"
            );
        }
        for action in module.actions.values() {
            let id = &action.id;
            for (name, arg) in &action.args {
                self.names(module, &arg.schema)
                    .with_context(|| format!("action {id}"))?;
                self.argument(name, arg)
                    .with_context(|| format!("action {id}"))?;
                if let Some(variable) = &arg.cli.env {
                    self.variable(module, variable)
                        .with_context(|| format!("action {id}"))?;
                }
            }
            self.names(module, &action.output)
                .with_context(|| format!("action {id}'s output"))?;
            if let Some(stdin) = &action.stdin {
                self.names(module, stdin)
                    .with_context(|| format!("action {id}'s stdin"))?;
            }
            if let Some(spec) = &action.spec {
                let declared = self
                    .shape(spec)
                    .with_context(|| format!("action {id}'s spec {spec} isn't declared"))?;
                ensure!(
                    declared.spec.is_some(),
                    "action {id}'s spec {spec} isn't a spec kind (x-link-spec)"
                );
                self.dependency(module, spec)?;
            }
            for next in action.next.iter().chain(&action.fail) {
                // Its own action, or the spec show core gives the module's
                // kind named as its namespace: `env.spec.show`.
                // and of a package's kind its actions bind.
                let owned = module.types.values().filter_map(|d| d.spec.as_deref());
                let bound = module
                    .actions
                    .values()
                    .filter_map(|a| a.spec.as_ref())
                    .filter_map(|spec| crate::package_kind(self, spec));
                let given = owned
                    .chain(bound)
                    .any(|kind| crate::spec_show(own, kind) == next.words());
                if next.namespace() == own {
                    ensure!(
                        module.actions.contains_key(next) || given,
                        "action {id} suggests {next}, which isn't declared"
                    );
                }
            }
        }
        Ok(())
    }

    /// Every name in `schema` is declared, by this module or one it
    /// depends on, and each choice's variants tell themselves apart.
    fn names(&self, module: &Module, schema: &Schema) -> anyhow::Result<()> {
        let mut problem = None;
        schema.walk(&mut |schema| {
            if problem.is_some() {
                return;
            }
            let result = match &schema.node {
                Node::Named(name) => self.dependency(module, name).and_then(|()| {
                    self.shape(name)
                        .map(|_| ())
                        .with_context(|| format!("{name} isn't declared"))
                }),
                Node::OneOf(one) => self.choices(one),
                // A map's keys are text: a named key shape resolves to one.
                Node::Map(map) => match &map.names {
                    Some(keys) => self.resolve(keys).and_then(|resolved| {
                        anyhow::ensure!(
                            matches!(resolved.node, Node::String(_)),
                            "a map's keys (propertyNames) are a text shape"
                        );
                        Ok(())
                    }),
                    None => Ok(()),
                },
                _ => Ok(()),
            };
            if let Err(e) = result {
                problem = Some(e);
            }
        });
        problem.map_or(Ok(()), Err)
    }

    /// Every default `schema` declares, at any depth, is one its shape takes.
    fn defaults(&self, schema: &Schema) -> anyhow::Result<()> {
        let mut problem = None;
        schema.walk(&mut |schema| {
            if problem.is_none()
                && let Some(default) = &schema.meta.default
                && let Err(e) = self.conforms(schema, default)
            {
                problem = Some(e.context(format!("its default {default}")));
            }
        });
        problem.map_or(Ok(()), Err)
    }

    /// A runtime argument is one the command line can read: a scalar, a
    /// choice or a list of them, given where its kind can be, with no
    /// default, since leaving it out means its absence.
    fn argument(&self, name: &str, arg: &crate::read::actions::Arg) -> anyhow::Result<()> {
        let schema = self.resolve(&arg.schema)?;
        let cli = &arg.cli;
        // A default is the value its absence means: a scalar's or a
        // choice's, never a required one's, a switch's or a list's.
        if let Some(default) = arg.schema.meta.default.as_ref() {
            ensure!(
                !arg.required,
                "argument {name}: a required argument has no default"
            );
            ensure!(
                matches!(
                    schema.node,
                    Node::String(_) | Node::Integer(_) | Node::Number(_) | Node::Enum(_)
                ),
                "argument {name}: only a text, a number or a choice has a default"
            );
            self.conforms(&arg.schema, default)
                .with_context(|| format!("argument {name}: its default {default}"))?;
        }
        match &schema.node {
            Node::Boolean => {
                ensure!(
                    cli.position.is_none() && !cli.rest,
                    "argument {name}: a switch is a flag, never positional"
                );
                ensure!(!arg.required, "argument {name}: a switch is never required");
                ensure!(
                    cli.env.is_none(),
                    "argument {name}: a switch reads no variable"
                );
            }
            Node::List(list) => {
                let items = self.resolve(&list.items)?;
                ensure!(
                    matches!(
                        items.node,
                        Node::String(_) | Node::Enum(_) | Node::Integer(_)
                    ),
                    "argument {name}: a list argument lists text, choices or whole numbers"
                );
                if cli.rest {
                    ensure!(
                        matches!(items.node, Node::String(_)),
                        "argument {name}: the rest is a list of text"
                    );
                }
            }
            Node::String(_)
            | Node::Enum(_)
            | Node::Const(_)
            | Node::Integer(_)
            | Node::Number(_) => {
                ensure!(!cli.rest, "argument {name}: the rest is a list of text");
            }
            _ => bail!("argument {name}: an argument is a scalar, a choice or a list of them"),
        }
        ensure!(
            cli.short.is_none() || (cli.position.is_none() && !cli.rest),
            "argument {name}: a positional argument has no short flag"
        );
        Ok(())
    }

    /// Each choice of `one` says which it is: by its tag, the same
    /// property each requires as a `const` with its own value, or by its
    /// marking key, which it requires and no other choice has.
    fn choices(&self, one: &crate::schema::OneOf) -> anyhow::Result<()> {
        use crate::schema::Told;
        if one.told == Told::Value {
            // Told apart by its value: at most one number, one record and
            // one text, and words, each read before the text, so `"none"`
            // beside a size is the word.
            let (mut numbers, mut texts, mut records) = (0, 0, 0);
            let mut words = BTreeSet::new();
            for variant in &one.variants {
                match &self.resolve(variant)?.node {
                    Node::Integer(_) | Node::Number(_) => numbers += 1,
                    Node::String(_) => texts += 1,
                    Node::Record(_) => records += 1,
                    Node::Const(value) => ensure!(
                        words.insert(value.to_string()),
                        "two oneOf choices are both the word {value}"
                    ),
                    _ => {
                        bail!("a choice told by its value is a number, a word, a text or a record")
                    }
                }
            }
            ensure!(
                numbers <= 1 && records <= 1 && texts <= 1,
                "a choice told by its value has at most one number, one record and one text, beside its words"
            );
            return Ok(());
        }
        let records = one
            .variants
            .iter()
            .map(|variant| match &self.resolve(variant)?.node {
                Node::Record(record) => Ok(record),
                _ => bail!("each oneOf choice is a record"),
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        match &one.told {
            Told::Tag(tag) => {
                let mut values = BTreeSet::new();
                for record in &records {
                    let value = match record.properties.get(tag).map(|s| &s.node) {
                        Some(Node::Const(value)) if record.required.contains(tag) => value,
                        _ => bail!("each oneOf choice requires {tag:?} as its const"),
                    };
                    ensure!(
                        values.insert(value.to_string()),
                        "two oneOf choices are both {tag} = {value}"
                    );
                }
            }
            Told::Value => unreachable!("checked above"),
            Told::Markers(markers) => {
                for (i, (record, marker)) in records.iter().zip(markers).enumerate() {
                    ensure!(
                        record.required.contains(marker),
                        "oneOf choice {i}'s marking key {marker:?} is one it requires"
                    );
                    for (j, other) in records.iter().enumerate() {
                        ensure!(
                            i == j || !other.properties.contains_key(marker),
                            "oneOf choice {i}'s marking key {marker:?} is choice {j}'s too: a marking key is one choice's alone"
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// A name of another module is one this module depends on.
    fn dependency(&self, module: &Module, name: &TypeName) -> anyhow::Result<()> {
        let own = &module.identity.id;
        if name.module() == own || name.module() == SETTINGS.core.module {
            return Ok(());
        }
        ensure!(
            module
                .dependencies
                .values()
                .any(|d| d.id() == name.module()),
            "{name} is module {}'s, which module.toml doesn't list as a dependency",
            name.module()
        );
        ensure!(
            self.module(name.module()).is_some(),
            "{name} is module {}'s, whose declarations aren't in this catalog",
            name.module()
        );
        Ok(())
    }

    /// A runtime flag's fallback variable is defined in a loaded
    /// `variables.toml`, or this module declares reading it.
    fn variable(&self, module: &Module, name: &str) -> anyhow::Result<()> {
        let defined = self.modules().any(|m| m.variables.defining(name).is_some());
        let read =
            module.variables.uses.iter().any(|u| {
                u.name == name && u.access.contains(&crate::read::variables::Access::Read)
            });
        ensure!(
            defined || read,
            "its fallback variable {name} is defined in no variables.toml, and this module declares no read of it"
        );
        Ok(())
    }
}

/// Each hook a declaration names: lowercase letters, digits and `_`.
fn hooks(checks: &[String]) -> anyhow::Result<()> {
    for check in checks {
        ensure!(
            check
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                && check.starts_with(|c: char| c.is_ascii_lowercase()),
            "check {check:?} names a hook: lowercase letters, digits and _"
        );
    }
    Ok(())
}
