//! Each action's typed runtime arguments and the command line that reads
//! them: commands from the action IDs, positionals and runtime flags from
//! `args`, the `--spec` file and its field flags from the bound spec kind,
//! and help from the declaration's text.

use std::{collections::BTreeMap, fmt::Write as _};

use anyhow::{Context, ensure};
use link_actions::{
    Action, Arg, Role, Surface, TypeName,
    schema::{Node, Schema},
};

use crate::{
    names::{field, method, pascal},
    types::Types,
};

mod shape;

/// The generated parser's words for the spec file every bound action takes.
/// The words of what core gives every module (`link-actions`' `format.toml`).
fn core() -> &'static link_actions::Core {
    link_actions::core()
}

pub(crate) struct Commands<'t, 'a> {
    pub types: &'t mut Types<'a>,
    pub out: String,
    /// Whether the parser's tables are written too, or the arguments' types alone.
    pub cli: bool,
    /// Each group that runs a default when typed bare, by its words, with
    /// the default's parser table and its command as typed
    /// (`lnk <group> <default>`).
    pub defaults: BTreeMap<Vec<String>, (String, String)>,
    /// Whether each arguments struct writes its command line, `argv()`.
    pub argv: bool,
    /// The crate whose generated runtime arguments these are, when
    /// another made them with [`crate::Parts::Args`]: its groups and
    /// arguments structs are named, never written again.
    pub over: Option<String>,
}

impl Commands<'_, '_> {
    /// Each of the module's argument groups as one type, flattened into
    /// every action taking it.
    fn groups(&mut self) -> anyhow::Result<()> {
        let module = self.types.module;
        for (name, group) in &module.groups {
            let args: Vec<(String, Arg)> = group
                .args
                .iter()
                .map(|(n, a)| (n.clone(), a.clone()))
                .collect();
            let label = format!(
                "the {name} group of {}",
                link_actions::program(&module.identity.id)
            );
            self.reused(&pascal(name), |me| {
                me.shape(&pascal(name), &label, &args, String::new(), true)
            })
            .with_context(|| format!("group {name}"))?;
        }
        Ok(())
    }

    /// Each action's arguments struct, with no command line.
    pub(crate) fn args_only(&mut self) -> anyhow::Result<()> {
        self.groups()?;
        for action in self.types.module.actions.values() {
            let stem = pascal(&method(&action.id));
            self.args(action, &stem)
                .with_context(|| format!("action {}", action.id))?;
        }
        Ok(())
    }

    pub(crate) fn all(&mut self) -> anyhow::Result<()> {
        self.groups()?;
        let module = self.types.module;
        let own = module.identity.id.clone();
        // Each spec kind `spec show` prints, by its kind: those the module
        // declares, the held ones its actions bind, and those it binds of
        // a package, which owns no command to show them. A bound kind of
        // another module is shown by its owner.
        let mut kinds: BTreeMap<String, String> = BTreeMap::new();
        for (name, declared) in &module.types {
            if let Some(kind) = &declared.spec {
                kinds.insert(
                    kind.clone(),
                    self.types.named_type(&TypeName::new(&own, name)?)?,
                );
            }
        }
        for action in module.actions.values() {
            if let (None, Some(kind), Some(rust)) =
                (&action.spec, &action.held, self.types.spec_type(action)?)
            {
                kinds.insert(kind.clone(), rust);
            }
            if let Some(spec) = action.spec.as_ref().filter(|s| s.module() != own)
                && let Some(kind) = link_actions::package_kind(self.types.catalog, spec)
            {
                kinds.insert(kind.to_string(), self.types.named_type(spec)?);
            }
        }
        let mut variants = String::new();
        let mut arms: BTreeMap<Vec<String>, String> = BTreeMap::new();
        let mut bindings = String::new();
        for action in module.actions.values() {
            let stem = pascal(&method(&action.id));
            self.args(action, &stem)
                .with_context(|| format!("action {}", action.id))?;
            let _ = writeln!(variants, "    {stem}({stem}Args),");
            // An action offered elsewhere alone (a tool, the app) keeps
            // its variant for dispatch, and no command reads it.
            if !action.surfaces.contains(&Surface::Cli) {
                continue;
            }
            let words: Vec<String> = action.id.words().into_iter().map(str::to_string).collect();
            let read = format!(
                "::link_actions::cli::read(command, m, {}.args).map(Command::{stem})",
                table(&stem).trim_start_matches('&')
            );
            // A module-wide argument it leaves out is refused wherever given.
            let left = left_out(action, module);
            let read = match left.is_empty() {
                true => read,
                false => format!(
                    "::link_actions::cli::left_out(command, &levels, &{left:?}).and_then(|()| {read})"
                ),
            };
            // Its group typed bare runs it, read from the group's matches.
            let mut said = vec![words.clone()];
            if action.default {
                let group: Vec<String> = action.group().into_iter().map(str::to_string).collect();
                arms.insert(group.clone(), read.clone());
                self.defaults
                    .insert(group.clone(), (table_args(&stem), action.id.command()));
                said.push(group);
            }
            arms.insert(words.clone(), read);
            if let Some(rust) = self.types.spec_type(action)? {
                for words in said {
                    let _ = writeln!(bindings, "        {words:?} => Some({}),", binding(&rust));
                }
            }
        }
        for (kind, rust) in &kinds {
            let words = link_actions::spec_show(&own, kind);
            let variant = spec_show_variant(&own, kind);
            let _ = writeln!(
                variants,
                "    /// `{} {own} {}`: the effective {kind} spec, every default written in.\n    {variant}(::link_spec::Show),",
                core().program,
                words.join(" "),
            );
            arms.insert(
                words.clone(),
                format!("<::link_spec::Show as ::clap::FromArgMatches>::from_arg_matches(m).map(Command::{variant})"),
            );
            let _ = writeln!(bindings, "        {words:?} => Some({}),", binding(rust));
        }
        if let Some(program) = &module.program {
            let args: Vec<(String, Arg)> = program
                .args
                .iter()
                .map(|(n, a)| (n.clone(), a.clone()))
                .collect();
            let label = link_actions::program(&own);
            let rows = self.shape("ProgramArgs", &label, &args, String::new(), false)?;
            let _ = write!(
                self.out,
                "const ARGS_PROGRAM: &[::link_actions::cli::Arg] = &[\n{rows}];\n\n"
            );
            let _ = writeln!(
                variants,
                "    /// `{label}` itself, given no command: its own flags.\n    Program(ProgramArgs),"
            );
            arms.insert(
                Vec::new(),
                "::link_actions::cli::read(command, m, ARGS_PROGRAM).map(Command::Program)".into(),
            );
        }
        // The arguments every action takes, for the program's root: clap
        // gives them to each command under it.
        let every: Vec<(String, Arg)> = module
            .args
            .iter()
            .map(|(n, a)| (n.clone(), a.clone()))
            .collect();
        let kept = std::mem::take(&mut self.out);
        let rows = self.shape("EveryArgs", "every command", &every, String::new(), false)?;
        self.out = kept;
        let _ = write!(
            self.out,
            "const EVERY: &[::link_actions::cli::Arg] = &[\n{rows}];\n\n"
        );
        // Core gives a module keeping parts (`[removal]`) its uninstall, and
        // declares its own; a module keeping none is never asked.
        if own != core().module && module.removal {
            let _ = writeln!(
                variants,
                "    /// The parts this module keeps, reported and removed.\n    Uninstall(::link_core::uninstall::Flags),"
            );
            arms.insert(
                vec![core().uninstall.clone()],
                "<::link_core::uninstall::Flags as ::clap::FromArgMatches>::from_arg_matches(m).map(Command::Uninstall)".into(),
            );
        }
        self.every_accessors()?;
        // Core's protocols for a module that answers questions or runs a
        // guest service: a hidden root flag, and a hidden command.
        if !module.answers.is_empty() {
            let _ = writeln!(
                variants,
                "    /// `--{}` <question>: a question of a module below this one, its\n    /// envelope on standard input ([`link_core::answers`]).\n    Answer(String),",
                core().answer
            );
        }
        // Core's program dispatches every other namespace to its module,
        // its words passed on unparsed.
        if own == core().module {
            let _ = writeln!(
                variants,
                "    /// Another namespace's command, for its module's program: its\n    /// namespace, then every word after it, unparsed.\n    External(Vec<::std::ffi::OsString>),"
            );
        }
        if module.guest_service {
            let _ = writeln!(
                variants,
                "    /// The guest service this program runs, re-entered.\n    GuestService(::link_service::cli::GuestEntry),"
            );
            arms.insert(
                vec![core().guest.clone()],
                "<::link_service::cli::GuestEntry as ::clap::FromArgMatches>::from_arg_matches(m).map(Command::GuestService)".into(),
            );
        }
        self.out += &crate::tool_calls::check_call(self.types.module);
        self.cli(&variants, &arms, &bindings)?;
        self.nexts();
        Ok(())
    }

    /// Next's graph, as the program answers its hidden `nexts --json`: the
    /// module itself, leading to its module.toml's `next`, and each command
    /// a person types, leading to its `next` and, on a failure, its `fail`.
    /// Then the program itself, for its entry.
    fn nexts(&mut self) {
        let module = self.types.module;
        let catalog = self.types.catalog;
        let core = core();
        let own = format!("{} {}", core.program, module.identity.id);
        let is_core = module.identity.id == core.module;
        // A default is typed by its group's words too: `lnk manual <path>`.
        let grouped = |id: &link_actions::ActionId| -> String {
            let words = id.typed();
            std::iter::once(core.program.as_str())
                .chain(words[..words.len() - 1].iter().copied())
                .collect::<Vec<_>>()
                .join(" ")
        };
        // Each as the templates a suggestion fills: its command, then each
        // of its positionals as a placeholder, as `lnk box ssh <name>`.
        let templates = |ids: &[link_actions::ActionId]| -> Vec<String> {
            let mut out = Vec::new();
            for id in ids {
                let Some(action) = catalog.actions().find(|a| a.id == *id) else {
                    // Another module's, not loaded here: whatever it takes.
                    out.push(format!("{} <arguments>", id.command()));
                    continue;
                };
                let mut positionals: Vec<(std::num::NonZeroUsize, &String)> = action
                    .args
                    .iter()
                    .filter_map(|(name, arg)| arg.cli.position.map(|p| (p, name)))
                    .collect();
                positionals.sort();
                let tail: String = positionals
                    .iter()
                    .map(|(_, name)| format!(" <{name}>"))
                    .collect();
                out.push(format!("{}{tail}", id.command()));
                if action.default {
                    out.push(format!("{}{tail}", grouped(id)));
                }
            }
            out
        };
        let mut rows: BTreeMap<String, (Vec<String>, Vec<String>)> = BTreeMap::new();
        if !is_core {
            rows.insert(own.clone(), (templates(&module.next), Vec::new()));
        }
        for action in module
            .actions
            .values()
            .filter(|a| a.surfaces.contains(&Surface::Cli) && a.role != Role::Plumbing)
        {
            let mut next = templates(&action.next);
            // Leading into every module, as `lnk up` does: `lnk <module>`.
            if action.next_modules {
                next.push(format!("{} {}", core.program, core.module_next));
            }
            let mut fail = templates(&action.fail);
            // Naming itself is the old `retry`: the same command again,
            // which Next's check reads as that word.
            for (ids, list) in [(&action.next, &mut next), (&action.fail, &mut fail)] {
                if ids.contains(&action.id) {
                    list.push(link_format::nexts::RETRY.to_string());
                }
            }
            // A failure may always point at its module's help, as a usage
            // error does: `lnk env --help`.
            if !is_core {
                fail.push(own.clone());
            }
            let mut keys = vec![action.id.command()];
            if action.default {
                keys.push(grouped(&action.id));
            }
            for key in keys {
                let row = rows.entry(key).or_default();
                for template in &next {
                    if !row.0.contains(template) {
                        row.0.push(template.clone());
                    }
                }
                for template in &fail {
                    if !row.1.contains(template) {
                        row.1.push(template.clone());
                    }
                }
            }
        }
        let list = |templates: &[String]| -> String {
            let quoted: Vec<String> = templates.iter().map(|t| format!("{t:?}")).collect();
            format!("&[{}]", quoted.join(", "))
        };
        let rows: String = rows
            .iter()
            .map(|(command, (next, fail))| {
                format!("    ({command:?}, {}, {}),\n", list(next), list(fail))
            })
            .collect();
        let _ = write!(
            self.out,
            "/// Next's graph from this module's declarations: each command a person\n\
             /// types, the commands it leads to, and those a failure leads to.\n\
             pub const NEXTS: &[(&str, &[&str], &[&str])] = &[\n{rows}];\n\n\
             /// This program, for its entry: its commands' bindings, its namespace,\n\
             /// and its Next graph.\n\
             pub const PROGRAM: ::link_core::help::Declared = ::link_core::help::Declared {{\n    \
             binding,\n    namespace: NAMESPACE,\n    nexts: NEXTS,\n}};\n\n"
        );
    }

    /// For each argument every action takes (`[args]`), an accessor on
    /// `Command` reading it from whichever action was given, so a module
    /// never matches every command for it: `fn agent(&self) -> Option<&str>`.
    fn every_accessors(&mut self) -> anyhow::Result<()> {
        let module = self.types.module;
        if module.args.is_empty() {
            return Ok(());
        }
        let mut methods = String::new();
        for (name, arg) in &module.args {
            // Each action taking it: one leaving it out answers none.
            let stems: Vec<String> = module
                .actions
                .values()
                .filter(|a| a.args.contains_key(name))
                .map(|a| pascal(&method(&a.id)))
                .collect();
            let resolved = self.types.catalog.resolve(&arg.schema)?.clone();
            let ident = field(name);
            let (returns, read, none) = match &resolved.node {
                Node::Boolean => ("bool".to_string(), format!("args.{ident}"), "false"),
                Node::String(text)
                    if text.kind.is_none()
                        && text.path.is_none()
                        && text.format.is_none()
                        && arg.schema.meta.default.is_none() =>
                {
                    (
                        "Option<&str>".to_string(),
                        format!("args.{ident}.as_deref()"),
                        "None",
                    )
                }
                _ if arg.schema.meta.default.is_some()
                    || matches!(resolved.node, Node::List(_)) =>
                {
                    let rust = self
                        .types
                        .rust(&arg.schema, &format!("Every{}", pascal(name)))?;
                    (
                        format!("Option<&{rust}>"),
                        format!("Some(&args.{ident})"),
                        "None",
                    )
                }
                _ => {
                    let rust = self
                        .types
                        .rust(&arg.schema, &format!("Every{}", pascal(name)))?;
                    (
                        format!("Option<&{rust}>"),
                        format!("args.{ident}.as_ref()"),
                        "None",
                    )
                }
            };
            let arms: String = stems
                .iter()
                .map(|stem| format!("            Self::{stem}(args) => {read},\n"))
                .collect();
            let _ = write!(
                methods,
                "    /// `--{name}`, whichever action was given: {}.\n    \
                 #[allow(unreachable_patterns, reason = \"core's own commands take none\")]\n    \
                 pub fn {ident}(&self) -> {returns} {{\n        match self {{\n{arms}            _ => {none},\n        }}\n    }}\n",
                arg.schema.meta.description.clone().unwrap_or_default()
            );
        }
        let _ = write!(self.out, "impl Command {{\n{methods}}}\n\n");
        Ok(())
    }

    /// The action's arguments struct and its table for the parser.
    fn args(&mut self, action: &Action, stem: &str) -> anyhow::Result<()> {
        let mut args: Vec<(String, Arg)> = action
            .args
            .iter()
            .map(|(n, a)| (n.clone(), a.clone()))
            .collect();
        let bound = action.spec.is_some() || action.held.is_some();
        if bound && !action.args.contains_key(&core().spec) {
            args.push((core().spec.clone(), spec_arg(action.spec.as_ref())));
        }
        let name = format!("{stem}Args");
        let mut checks = String::new();
        // A bound action's hooks check its spec, on every read of it
        // (`types.rs`); a spec-less action's check its arguments.
        ensure!(
            action.held.is_none() || action.checks.is_empty(),
            "a held spec's checks are its hand-written type's"
        );
        for hook in action.checks.iter().filter(|_| !bound) {
            let _ = writeln!(
                checks,
                "        crate::checks::{hook}(self).map_err(|why| ::link_actions::value::Refusal {{ at: String::new(), why }})?;"
            );
        }
        let label = action.id.command();
        let rows = self.reused(&name, |me| me.shape(&name, &label, &args, checks, false))?;
        // An action off the command line has no command to read or write.
        let typed = action.surfaces.contains(&Surface::Cli);
        if self.argv && typed {
            let words: Vec<&str> = action.id.words();
            let _ = write!(
                self.out,
                "const {table}: &[::link_actions::cli::Arg] = &[\n{rows}];\n\n\
                 impl {name} {{\n    \
                 /// The words and flags after `{program}` that give these arguments.\n    \
                 pub fn argv(&self) -> Vec<String> {{\n        \
                 let value = ::serde_json::to_value(self).expect(\"runtime arguments are JSON\");\n        \
                 ::link_actions::cli::argv(&{words:?}, {table}, &value)\n    }}\n\n    \
                 /// A line `argv` wrote, read back by the same table.\n    \
                 pub fn from_argv(line: &[String]) -> Result<Self, ::link_actions::cli::Error> {{\n        \
                 ::link_actions::cli::from_argv(&{words:?}, {table}, line)\n    }}\n}}\n\n",
                table = table_args(stem),
                program = action.id.program(),
            );
        }
        if self.cli && typed {
            let _ = write!(
                self.out,
                "const {}: &[::link_actions::cli::Arg] = &[\n{rows}];\n\n\
                 const {}: ::link_actions::cli::Action = ::link_actions::cli::Action {{ word: {:?}, description: {:?}, hidden: {}, args: {} }};\n\n",
                table_args(stem),
                table(stem).trim_start_matches('&'),
                action.id.verb(),
                action.description,
                action.role == Role::Plumbing,
                table_args(stem),
            );
        }
        Ok(())
    }

    /// `Command`, `Cli` and its clap traits, and the field flags' bindings.
    fn cli(
        &mut self,
        variants: &str,
        arms: &BTreeMap<Vec<String>, String>,
        bindings: &str,
    ) -> anyhow::Result<()> {
        let module = self.types.module;
        let typed = typed(&module.identity.id);
        let namespace = &module.identity.id;
        let forwarding = match module.identity.id == core().module {
            true => "if let Some((word, sub)) = matches.subcommand()\n            \
                     && group_root().find_subcommand(word).is_none()\n        {\n            \
                     let mut words = vec![::std::ffi::OsString::from(word)];\n            \
                     words.extend(sub.get_many::<::std::ffi::OsString>(\"\").into_iter().flatten().cloned());\n            \
                     return Ok(Cli { command: Command::External(words) });\n        }\n        "
                .to_string(),
            false => String::new(),
        };
        let answering = match module.answers.is_empty() {
            true => String::new(),
            false => format!(
                "if let Some(question) = matches.try_get_one::<String>({answer:?}).ok().flatten() {{\n            \
                 if matches.subcommand().is_some() {{\n                \
                 return Err(group_root().error(::clap::error::ErrorKind::ArgumentConflict, \"--{answer} is asked alone\"));\n            }}\n            \
                 return Ok(Cli {{ command: Command::Answer(question.clone()) }});\n        }}\n        ",
                answer = core().answer
            ),
        };
        // The command tree: each word's subcommands, from the action IDs.
        let mut tree = String::new();
        let mut groups: BTreeMap<Vec<String>, Vec<String>> = BTreeMap::new();
        for words in arms.keys() {
            for depth in 0..words.len() {
                let entry = groups.entry(words[..depth].to_vec()).or_default();
                if !entry.contains(&words[depth]) {
                    entry.push(words[depth].clone());
                }
            }
        }
        for (path, children) in groups.iter().rev() {
            let name = if path.is_empty() {
                "root".to_string()
            } else {
                path.join("_").replace('-', "_")
            };
            let mut built = if path.is_empty() {
                format!(
                    "::clap::Command::new({:?}).bin_name({typed:?}).version(env!(\"CARGO_PKG_VERSION\")).about(env!(\"CARGO_PKG_DESCRIPTION\"))",
                    link_actions::program(&module.identity.id),
                )
            } else {
                format!(
                    "::clap::Command::new({:?})",
                    path.last().cloned().unwrap_or_default()
                )
            };
            // A group's alias, from `[aliases]`, by its words as typed
            // after `lnk`: the root's is its namespace's, which a parser
            // nesting the program under `lnk` reads.
            let typed_group: Vec<&str> = match module.identity.id == core().module {
                true => path.iter().map(String::as_str).collect(),
                false => std::iter::once(namespace.as_str())
                    .chain(path.iter().map(String::as_str))
                    .collect(),
            };
            if let Some(alias) = module.alias(&typed_group) {
                built += &format!(".visible_alias({alias:?})");
            }
            // A group of plumbing alone is hidden, as each of its actions is.
            let under: Vec<_> = module
                .actions
                .values()
                .filter(|a| {
                    let words = a.id.words();
                    a.surfaces.contains(&Surface::Cli)
                        && !path.is_empty()
                        && words.len() > path.len()
                        && words.starts_with(&path.iter().map(String::as_str).collect::<Vec<_>>())
                })
                .collect();
            if !under.is_empty() && under.iter().all(|a| a.role == Role::Plumbing) {
                built += ".hide(true)";
            }
            if path.last() == Some(&core().spec) {
                let kind = match path.len() {
                    1 => module.identity.id.as_str(),
                    _ => path[0].as_str(),
                };
                built += &format!(".about(\"The {kind} spec's own commands\")");
            }
            // A second kind's own word, where no resource of the module
            // is (`iam policy`), says its kind, as the registry does.
            if path.len() == 1
                && !module.types.contains_key(&path[0])
                && children.iter().all(|c| *c == core().spec)
            {
                built += &format!(".about(\"The {} spec\")", path[0]);
            }
            // A resource's group says what its resource is.
            if path.len() == 1
                && let Some(said) = module
                    .types
                    .get(&path[0])
                    .and_then(|d| d.schema.meta.description.as_deref())
            {
                built += &format!(".about({said:?})");
            }
            if path.is_empty() {
                built += ".args(::link_actions::cli::globals(EVERY))";
                if module.identity.id == core().module {
                    built += ".allow_external_subcommands(true).external_subcommand_value_parser(::clap::value_parser!(::std::ffi::OsString))";
                }
                if !module.answers.is_empty() {
                    built += &format!(
                        ".arg(::clap::Arg::new({answer:?}).long({answer:?}).value_name(\"QUESTION\").hide(true).exclusive(true))",
                        answer = core().answer
                    );
                }
            }
            // A root with the answer flag runs with no command beside it.
            let asks = path.is_empty() && !module.answers.is_empty();
            let program = path.is_empty().then_some(module.program.as_ref()).flatten();
            match (self.defaults.get(path), program) {
                // A default's flags are its alone: given before another
                // command, they are refused, never dropped.
                // Its help says what the bare group runs: `lnk <group>`
                // is short for `lnk <group> <default>`.
                (Some((args, command)), _) => {
                    let bare = command
                        .rsplit_once(' ')
                        .map_or(command.as_str(), |(bare, _)| bare);
                    let short = format!("`{bare}` is short for `{command}`.");
                    built += &format!(
                        ".after_help({short:?}).subcommand_negates_reqs(true).args_conflicts_with_subcommands(true).args(::link_actions::cli::args({args}))"
                    );
                }
                (None, Some(program)) => {
                    built += ".subcommand_negates_reqs(true).args(::link_actions::cli::args(ARGS_PROGRAM))";
                    // A declared --version is the program's own.
                    if program.args.contains_key(&core().version) {
                        built += ".disable_version_flag(true)";
                    }
                    // So is a declared --help: the program renders its
                    // own, and each command under it keeps clap's.
                    if program.args.contains_key(&core().help) {
                        built += ".disable_help_flag(true).disable_help_subcommand(true)";
                    }
                }
                (None, None) if asks => {}
                (None, None) => built += ".subcommand_required(true)",
            }
            for child in children {
                let mut full = path.clone();
                full.push(child.clone());
                built += &if groups.contains_key(&full) {
                    format!(".subcommand(group_{}())", full.join("_").replace('-', "_"))
                } else {
                    format!(".subcommand({})", leaf(&full, module)?)
                };
            }
            let declares_help = program.is_some_and(|p| p.args.contains_key(&core().help));
            if declares_help {
                built = format!("::link_actions::cli::nested_help({built})");
            }
            let _ = writeln!(
                tree,
                "fn group_{name}() -> ::clap::Command {{\n    {built}\n}}\n"
            );
        }
        // A module binding no spec answers none, matching nothing.
        let binds = match bindings.is_empty() {
            true => "    let _ = words;\n    None\n".to_string(),
            false => format!(
                "    let words: Vec<&str> = words.iter().map(String::as_str).collect();\n    \
                 match words.as_slice() {{\n{bindings}        _ => None,\n    }}\n"
            ),
        };
        let mut matched = String::new();
        for (words, read) in arms {
            let pattern: Vec<String> = words.iter().map(|w| format!("{w:?}")).collect();
            let _ = writeln!(matched, "        [{}] => {read},", pattern.join(", "));
        }
        let _ = write!(
            self.out,
            "/// The namespace `lnk` dispatches here, which this program never reads.\n\
             pub const NAMESPACE: &str = {namespace:?};\n\n\
             /// Every command of this module, with its typed arguments.\n\
             #[allow(clippy::large_enum_variant, reason = \"one command is parsed per process, and handlers take each action's arguments as they are\")]\n\
             pub enum Command {{\n{variants}}}\n\n\
             /// The module's whole command line, read from its declarations.\n\
             pub struct Cli {{\n    pub command: Command,\n}}\n\n\
             {tree}\
             impl ::clap::CommandFactory for Cli {{\n    \
             fn command() -> ::clap::Command {{\n        group_root()\n    }}\n    \
             fn command_for_update() -> ::clap::Command {{\n        group_root()\n    }}\n}}\n\n\
             impl ::clap::FromArgMatches for Cli {{\n    \
             fn from_arg_matches(matches: &::clap::ArgMatches) -> Result<Self, ::clap::Error> {{\n        \
             {answering}{forwarding}\
             let mut words = Vec::new();\n        let mut m = matches;\n        let mut levels = vec![matches];\n        let mut command = group_root();\n        \
             while let Some((word, sub)) = m.subcommand() {{\n            \
             command = command.find_subcommand(word).cloned().unwrap_or_else(|| ::clap::Command::new({typed:?}));\n            \
             words.push(word.to_string());\n            m = sub;\n            levels.push(m);\n        }}\n        \
             let _ = &levels;\n        \
             let command = &command.bin_name(format!(\"{typed} {{}}\", words.join(\" \")));\n        let words: Vec<&str> = words.iter().map(String::as_str).collect();\n        \
             let command = match words.as_slice() {{\n{matched}        \
             _ => Err(command.clone().error(::clap::error::ErrorKind::MissingSubcommand, \"a command\")),\n        }}?;\n        \
             Ok(Cli {{ command }})\n    }}\n    \
             fn update_from_arg_matches(&mut self, matches: &::clap::ArgMatches) -> Result<(), ::clap::Error> {{\n        \
             *self = Self::from_arg_matches(matches)?;\n        Ok(())\n    }}\n}}\n\n\
             impl ::clap::Parser for Cli {{}}\n\n\
             /// The spec kind the command at `words` binds, whose fields are its field flags.\n\
             pub fn binding(words: &[String]) -> Option<::link_spec::Binding> {{\n{binds}}}\n\n"
        );
        Ok(())
    }
}

/// The command at `words`: an action's, or a core-provided one.
fn leaf(words: &[String], module: &link_actions::Module) -> anyhow::Result<String> {
    let core = core();
    if module.guest_service && words == [core.guest.clone()] {
        return Ok(format!(
            "<::link_service::cli::GuestEntry as ::clap::Args>::augment_args(::clap::Command::new({:?}).hide(true).about(\"Run the selected guest service process\"))",
            core.guest
        ));
    }
    if words == [core.uninstall.clone()] && module.identity.id != core.module && module.removal {
        return Ok(format!(
            "<::link_core::uninstall::Flags as ::clap::Args>::augment_args(::clap::Command::new({:?}).hide(true).about(\"Report and remove the parts this module keeps\"))",
            core.uninstall
        ));
    }
    if words.len() >= 2 && words[words.len() - 2..] == [core.spec.clone(), core.show.clone()] {
        return Ok(format!(
            "<::link_spec::Show as ::clap::Args>::augment_args(::clap::Command::new({:?}).about(\"Print the effective spec, each default written in\"))",
            core.show
        ));
    }
    let action = module
        .actions
        .values()
        .find(|a| a.id.words() == words)
        .with_context(|| format!("no action is {}", words.join(" ")))?;
    let command = format!(
        "::link_actions::cli::command(&{})",
        table(&pascal(&method(&action.id))).trim_start_matches('&')
    );
    // A module-wide argument it leaves out has a hidden stand-in, so the
    // root's global never reaches its command.
    let left = left_out(action, module);
    Ok(match left.is_empty() {
        true => command,
        false => format!("{command}.args(::link_actions::cli::stand_ins(EVERY, &{left:?}))"),
    })
}

/// The module-wide arguments (`[args]`) `action` leaves out.
fn left_out<'a>(action: &Action, module: &'a link_actions::Module) -> Vec<&'a str> {
    module
        .args
        .keys()
        .filter(|name| !action.args.contains_key(*name))
        .map(String::as_str)
        .collect()
}

/// The command variant of a spec kind's `spec show`: `SpecShow` for the
/// kind named as the namespace, `TeamSpecShow` for `team`.
fn spec_show_variant(namespace: &str, kind: &str) -> String {
    match namespace == kind {
        true => "SpecShow".into(),
        false => format!("{}SpecShow", pascal(kind)),
    }
}

/// What a person types before the program's words: `lnk env`, or `lnk`
/// for core's own program.
fn typed(namespace: &str) -> String {
    match namespace == core().module {
        true => core().program.clone(),
        false => format!("{} {namespace}", core().program),
    }
}

fn table(stem: &str) -> String {
    format!("&ACTION_{}", stem.to_uppercase())
}

fn table_args(stem: &str) -> String {
    format!("ARGS_{}", stem.to_uppercase())
}

fn binding(rust: &str) -> String {
    format!(
        "::link_spec::Binding {{ schema: ::link_spec::schema::<{rust}>(), format: <{rust} as ::link_files::schema::Specification>::format().clone() }}"
    )
}

/// The `--spec` file a bound action reads, before its field flags.
fn spec_arg(spec: Option<&TypeName>) -> Arg {
    let mut schema = Schema::new(Node::String(link_actions::schema::Text {
        path: Some(link_actions::schema::PathKind::Plain),
        ..Default::default()
    }));
    schema.meta.description = Some(format!(
        "The {} spec file, its field flags laid over it",
        spec.map(TypeName::name).unwrap_or("bound")
    ));
    Arg {
        schema,
        required: false,
        cli: link_actions::Cli::default(),
        every: false,
        group: None,
    }
}

fn doc(schema: &Schema) -> String {
    crate::types::doc(schema)
        .lines()
        .map(|l| format!("    {l}\n"))
        .collect()
}
