//! `lnk`: the command users install on their machine.
//!
//! This is only the core: it installs modules (`lnk up`, `lnk module`),
//! runs them, and updates itself and them. Each module is its own program
//! next to `lnk`, and `lnk <command>` runs the
//! module that provides the command, the way `git <command>` runs
//! `git-<command>`. Modules never link each other's code: one uses another
//! by running `lnk <command>` too.

use std::{
    ffi::OsString,
    io::IsTerminal,
    path::{Path, PathBuf},
};

use anyhow::{Context, bail, ensure};
use link_cli::cli;
use link_core::release::{self, Release};
use link_format::{
    self as format, Command, Done, Name, Out, Path as Shown, Product, Report, Section, Table,
    Value, Verbatim, Version, WithinExt, cmd, phrase, prose, sentence,
};

mod environment;
mod issue;
mod locations;
mod outside;
mod source;
mod staging;
mod uninstall;

mod registry;
mod root_programs;
use registry::{COMMANDS, GROUPS, MODULES, USES};
mod account_setup;
mod installing;
mod offers;
mod running;
mod upgrading;
use installing::{
    add, add_modules, add_options, goes_with, install_dir, missing, remove, up, with_goes_with,
};
use offers::{offer, offer_goes_with, typed_unasked};
use running::{run_module, up_choice, without_comment};
use upgrading::{check, installed_how, other_lnks, this_lnk, upgrade, version_of};

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_spec::entry::initialize();
    link_core::source!();
    // The files lnk itself writes, which an upgrade converts.
    link_core::saved::own(&[
        link_files::upgrade::Saved::map,
        link_files::upgrade::Saved::journal,
        link_files::upgrade::Saved::marker,
        link_files::upgrade::Saved::maintenance,
        link_core::update::saved,
        offers::saved,
        link_machine::installation::saved,
        link_settings::folders::saved_machine,
        link_settings::folders::saved_settings,
    ]);
    if let Some(link_spec::entry::Discovery::Upgrade(stage)) = link_spec::entry::discovery() {
        link_core::saved::answer(stage);
    }
    // Helpers (`tar`, `id`, `systemctl`) are found on the PATH, where an
    // empty entry is the current folder. lnk and every module it runs go
    // without those entries.
    if let Some(path) = std::env::var_os("PATH") {
        // SAFETY: no other thread is running yet.
        unsafe { std::env::set_var("PATH", link_files::absolute_path(&path)) };
    }
    link_settings::output::run(try_main)
}

/// A module, as a value.
fn module_name(module: &str) -> Name {
    Name::new("module", module)
}

/// Modules, as a list value: "box, cloud and auth".
fn modules_value(modules: &[&str]) -> Value {
    Value::List(modules.iter().map(|p| module_name(p).into()).collect())
}

/// `lnk` and `words`, as the command to type.
fn lnk_command<S: AsRef<str>>(words: impl IntoIterator<Item = S>) -> Command {
    Command::typed(
        std::iter::once("lnk".to_string()).chain(words.into_iter().map(|w| w.as_ref().to_string())),
    )
}

/// `lnk module add` and the modules to add.
fn module_add(modules: &[&str]) -> Command {
    lnk_command(["module", "add"].into_iter().chain(modules.iter().copied()))
}

/// Runs `future` to its end on the process's runtime.
fn wait<T>(future: impl std::future::Future<Output = Result<T, Report>>) -> Result<T, Report> {
    link_runtime::block_on(async { anyhow::Ok(future.await) })?
}

/// What a nested command found: its result shown with the command's own.
fn absorb(out: &mut Out, done: Done) {
    if let Some(success) = done.success {
        out.block(success);
    }
}

/// A failure the person can read: `headline`, and the error's own words
/// as its cause.
fn failed(headline: format::Sentence, err: anyhow::Error) -> Report {
    Report::error(headline)
        .why(format::Prose::from(format::Sentence::adopt(&format!(
            "{err:#}"
        ))))
        .cause(err)
}

fn try_main(out: &mut Out) -> Result<Done, Report> {
    let _ = &*link_core::upgrades::POLICY;
    let history = &*link_cli::history::HISTORY;
    // Every module's commands, made only for a history to hold to them:
    // an empty one holds to nothing.
    if !history.is_empty() {
        let surface = link_cli::surface();
        history.validate(
            &surface,
            &surface,
            &link_cli::modules(),
            &link_version::version(),
        )?;
    }
    let parsed = link_spec::entry::current::<cli::Cli>(&CoreProgram, |command| command);
    let invocation = parsed.arguments;
    let command = parsed.command.command;
    let json = wants_json(&command);
    let flags = match &command {
        cli::Command::Program(flags) => flags.clone(),
        _ => cli::ProgramArgs::default(),
    };
    let internal = flags.link_history
        || flags.link_registry
        || flags.link_documentation
        || matches!(&command, cli::Command::CoreUpgrade(asked) if asked.check)
        || std::env::var_os(format::INNER).is_some()
        // A module running lnk: its notes are a person's, never in a
        // module's run.
        || std::env::var_os(link_core::LNK_EXE).is_some();
    if !internal {
        link_core::update::notice(json);
    }
    let is_version = flags.version || matches!(command, cli::Command::CoreVersion(_));
    // Two lnks on one PATH run different builds by turns, unnoticed: say
    // so, to a person (not to a module running `lnk`, which has LNK_EXE).
    if !is_version
        && std::env::var_os(link_core::LNK_EXE).is_none()
        && link_format::streams::stderr_is_terminal()
        && let Ok(exe) = this_lnk()
        && let Some(other) = other_lnks(&exe).first()
    {
        out.block(
            Report::warning(sentence!(
                "This is {} ({}), and {} ({}) is on your PATH too.",
                Shown::new(&exe),
                installed_how(&exe),
                Shown::new(other),
                installed_how(other)
            ))
            .why(prose!("{} shows them; keep one.", cmd!("lnk --version"))),
        );
    }
    if flags.link_history {
        let asked = link_spec::history::asked().within("answering the history")?;
        out.data(
            format!(
                "{}\n",
                serde_json::to_string(&link_cli::history::release().stating(asked))
                    .within("writing the release's history")?
            )
            .as_bytes(),
        );
        return Ok(Done::shown());
    }
    if flags.link_registry {
        let asked = link_component::asked().within("answering the registry")?;
        out.data(
            format!(
                "{}\n",
                serde_json::to_string(&link_component::registry_stating(
                    &registry::REGISTRY,
                    asked
                ))
                .within("writing the registry")?
            )
            .as_bytes(),
        );
        return Ok(Done::shown());
    }
    if flags.link_documentation {
        out.data(include_str!(concat!(env!("OUT_DIR"), "/documentation.json")).as_bytes());
        return Ok(Done::shown());
    }
    if is_version {
        // The first line alone is what `lnk upgrade` reads.
        let mut text = format!("lnk {}\n", env!("CARGO_PKG_VERSION"));
        if let Some(trusts) = link_core::BUILD.trusts() {
            text += &format!("  {trusts}\n");
        }
        if let Ok(exe) = this_lnk() {
            text += &format!("  {} ({})\n", exe.display(), installed_how(&exe));
            for other in other_lnks(&exe) {
                text += &format!(
                    "  also on your PATH: {} ({})\n",
                    other.display(),
                    installed_how(&other)
                );
            }
        }
        out.data(text.as_bytes());
        return Ok(Done::shown());
    }
    if flags.help {
        return Ok(help(out));
    }
    match command {
        cli::Command::Program(_) => return Ok(help(out)),
        cli::Command::CoreNexts(_) => out.reply(&cli::HELP.nexts()),
        cli::Command::CoreHelp(asked) => match asked.group.as_deref() {
            None | Some("help") => return Ok(help(out)),
            Some(group) if core_command().find_subcommand(group).is_some() => {
                out.data(core_help(group).as_bytes())
            }
            Some(group) => {
                let mut arguments: Vec<OsString> =
                    asked.arguments.into_iter().map(OsString::from).collect();
                arguments.push("--help".into());
                return run_module(out, registry::unaliased(group), &arguments);
            }
        },
        cli::Command::External(arguments) => {
            let (group, rest) = arguments
                .split_first()
                .expect("external command has its name");
            let typed = group
                .to_str()
                .within("the command group's name isn't text")?;
            let group = registry::unaliased(typed);
            if json_before(&invocation, typed) {
                return Err(
                    Report::error(sentence!("Put --json after the module's command."))
                        .exit(format::Exit::Usage)
                        .next(cmd!("lnk {} --help", group), "see the command's flags"),
                );
            }
            if running::reads_control(group) {
                locations::check(out)?;
            }
            return run_module(out, group, rest);
        }
        cli::Command::CoreVersion(_) => unreachable!("version handled above"),
        command => {
            if matches!(
                command,
                cli::Command::ModuleAdd(_)
                    | cli::Command::ModuleList(_)
                    | cli::Command::ModuleRemove(_)
                    | cli::Command::ModuleSetup(_)
                    | cli::Command::ModuleShow(_)
                    | cli::Command::CoreUpgrade(_)
            ) {
                locations::check(out)?;
            }
            return core(out, command);
        }
    }

    Ok(Done::shown())
}

struct CoreProgram;
impl link_spec::Program for CoreProgram {
    fn before(&self, args: &[OsString]) {
        link_spec::Program::before(&cli::HELP, args);
    }
    fn styled(&self, command: clap::Command) -> clap::Command {
        link_spec::Program::styled(&cli::HELP, command)
    }
    fn helped(&self, _: clap::Command) -> clap::Command {
        core_command()
    }
    fn arranged(&self, _: &clap::Command, args: Vec<OsString>) -> Vec<OsString> {
        without_comment(args)
    }
    fn history(&self) -> link_spec::history::History {
        (*link_cli::history::HISTORY).clone()
    }
    fn release(&self) -> Option<fn() -> anyhow::Result<link_spec::history::Wire>> {
        Some(|| Ok(link_cli::history::release()))
    }
}

/// The core's own commands, as clap reads them: `module`'s help lists
/// Link's modules.
fn core_command() -> clap::Command {
    let modules: Vec<&str> = MODULES.iter().map(|(name, _)| *name).collect();
    link_spec::Program::styled(&cli::HELP, cli::command()).mut_subcommand("module", |module| {
        module.after_help(format!("Link's modules: {}", modules.join(", ")))
    })
}

/// `lnk <command> --help` for one of the core's own commands.
fn core_help(name: &str) -> String {
    let mut command = core_command();
    command.build();
    command
        .find_subcommand_mut(name)
        .map(|c| c.render_long_help().to_string())
        .unwrap_or_default()
}

/// `lnk up`, `module`, `location`, `upgrade` and `uninstall`, from `args`.
fn core(out: &mut Out, command: cli::Command) -> Result<Done, Report> {
    match command {
        cli::Command::Program(_)
        | cli::Command::CoreHelp(_)
        | cli::Command::CoreVersion(_)
        | cli::Command::CoreNexts(_)
        | cli::Command::External(_) => unreachable!("handled before core operations"),
        cli::Command::VariableList(asked) => environment::list(out, &asked),
        cli::Command::CoreUp(mut asked)
            if !asked.source.is_empty() || !asked.module.is_empty() || asked.stale =>
        {
            asked.module = source::modules_named(&asked.module);
            source::install(out, &source::with_checkout(asked))
        }
        cli::Command::CoreUp(asked) => {
            locations::check(out)?;
            wait(up(out, &asked.uses))
        }
        cli::Command::ModuleList(_) => module_list(out),
        cli::Command::ModuleShow(asked) => module_show(out, &asked.module),
        cli::Command::ModuleAdd(asked) if asked.dry => {
            let names: Vec<&str> = asked.modules.iter().map(String::as_str).collect();
            // One a line: what `link_core::ensure` reads.
            for module in missing(&names)? {
                out.data(format!("{module}\n").as_bytes());
            }
            Ok(Done::shown())
        }
        cli::Command::ModuleAdd(asked) => {
            let options = add_options(&asked.url, asked.yes, asked.signer)?;
            wait(add_modules(out, &asked.modules, &asked.url, &options))
        }
        cli::Command::ModuleRemove(asked) => {
            let names: Vec<&str> = asked.modules.iter().map(String::as_str).collect();
            remove(out, &names)
        }
        cli::Command::ModuleSetup(_) => wait(account_setup::run(out)),
        cli::Command::LocationList(_) => locations::list(out),
        cli::Command::LocationSet(asked) => locations::set(out, &asked.location, &asked.path),
        cli::Command::LocationCreate(_) => locations::create(),
        cli::Command::CoreUpgrade(asked) if asked.check => wait(check(out)),
        cli::Command::CoreUpgrade(asked) => {
            let version = match asked.version {
                Some(v) => Some(v.parse::<link_version::Version>().map_err(|_| {
                    Report::error(sentence!("{} isn't a version.", format::Foreign::new(&v)))
                        .next(
                            cmd!("lnk upgrade --check"),
                            phrase!("see which version is out"),
                        )
                        .exit(format::Exit::Usage)
                })?),
                None => None,
            };
            wait(upgrade(out, version, asked.yes))
        }
        cli::Command::CoreUninstall(asked) if asked.parts => {
            out.reply(&uninstall::parts()?);
            Ok(Done::shown())
        }
        cli::Command::CoreUninstall(asked) => {
            uninstall::uninstall(out, asked.yes, &asked.delete, asked.all)
        }
        cli::Command::IssueCreate(asked) => issue::run(out, asked.open),
        cli::Command::LocationCheck(asked) => staging::run(out, &asked),
    }
}

/// Whether the command asks for its answer as JSON: `--json`, which
/// every core command takes before or after its words.
fn wants_json(command: &cli::Command) -> bool {
    use cli::Command as C;
    match command {
        C::LocationCheck(a) => a.json,
        C::CoreHelp(a) => a.json,
        C::IssueCreate(a) => a.json,
        C::CoreNexts(a) => a.json,
        C::CoreUninstall(a) => a.json,
        C::CoreUp(a) => a.json,
        C::CoreUpgrade(a) => a.json,
        C::CoreVersion(a) => a.json,
        C::LocationCreate(a) => a.json,
        C::LocationList(a) => a.json,
        C::LocationSet(a) => a.json,
        C::ModuleAdd(a) => a.json,
        C::ModuleList(a) => a.json,
        C::ModuleRemove(a) => a.json,
        C::ModuleSetup(a) => a.json,
        C::ModuleShow(a) => a.json,
        C::VariableList(a) => a.json,
        C::Program(_) | C::External(_) => false,
    }
}

/// Whether `--json` came before `group`, a module's word on the command
/// line `arguments`: it's the module's to read, after its command.
fn json_before(arguments: &[OsString], group: &str) -> bool {
    arguments
        .iter()
        .take_while(|arg| *arg != group)
        .any(|arg| arg == "--json")
}

/// The core's own commands, for `lnk --help`.
const CORE: &[&str] = &[
    "up",
    "module",
    "upgrade",
    "uninstall",
    "issue",
    "variable",
    "location",
];

/// `lnk --help`: one screen, the groups and the core's own commands.
/// Each group's commands are its own help (`lnk help <group>`), and the
/// modules are `lnk module list`.
fn help(out: &mut Out) -> Done {
    let help = cli::HELP;
    out.block(
        Verbatim::new()
            .line(format!(
                "lnk {}: {}",
                env!("CARGO_PKG_VERSION"),
                help.about("lnk")
            ))
            // `--version` is said here, not in a row of its own, so the
            // screen stays one with every group.
            .line(
                "Usage: lnk <GROUP> <COMMAND>; lnk help <group> [<command>] for its help, \
                 lnk --version for where it's installed",
            ),
    );
    let mut groups = Table::new(["Command", "About"], sentence!("No groups are known."));
    // Link's own groups, which it runs and a person never types, are not offered.
    for (group, owner) in GROUPS
        .iter()
        .filter(|(group, owner)| !registry::REGISTRY[*owner].commands[*group].hidden)
    {
        let about = registry::REGISTRY[*owner].commands[*group]
            .help
            .split("\n\n")
            .next()
            .unwrap_or_default()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let alias = registry::REGISTRY[*owner].commands[*group].alias.as_deref();
        groups.row([
            Value::from(lnk_command([*group])),
            Value::from(also(about.trim_end_matches('.'), alias)),
        ]);
    }
    out.block(Section::new(phrase!("Groups")).with(groups));
    let mut setting = Table::new(["Command", "About"], sentence!("Nothing is here."));
    // Each as its declaration says it, a command's description or a
    // group's resource, as its own help gives it.
    let core = cli::command();
    for c in CORE {
        let about = core
            .find_subcommand(c)
            .and_then(clap::Command::get_about)
            .map(|about| about.to_string())
            .unwrap_or_default();
        let alias = core
            .find_subcommand(c)
            .and_then(|group| group.get_visible_aliases().next());
        setting.row([
            Value::from(lnk_command([*c])),
            Value::from(also(about.trim_end_matches('.'), alias)),
        ]);
    }
    out.block(Section::new(phrase!("Setting up lnk")).with(setting));
    Done::shown()
        .next(cmd!("lnk up"), phrase!("set Link up on this computer"))
        .next(
            cmd!("lnk module list"),
            phrase!("see the modules, and which are installed"),
        )
}

/// A group's help line in `lnk --help`, saying its alias after it, where
/// its declaration gives one: the full command first, then its short form.
fn also(about: &str, alias: Option<&str>) -> String {
    match alias {
        Some(alias) => format!("{about} (short: lnk {alias})"),
        None => about.to_string(),
    }
}

format::states! {
    /// Whether a module is on this machine.
    enum Installed {
        Yes("installed") => Good,
        No("not installed") => Quiet,
    }
}

/// Whether `module` runs here, for a person reading the table or `show`:
/// one built apart is asked, said aside and not installed. The JSON every
/// question reads asks none ([`modules_json`]).
fn installed(module: &str) -> Installed {
    match link_core::find_module(module) {
        Some(_) => Installed::Yes,
        None => Installed::No,
    }
}

/// `lnk module list`: Link's modules, then those from outside Link; with
/// `--json`, `link_core::modules::Listed` for each.
fn module_list(out: &mut Out) -> Result<Done, Report> {
    // Every question asks this list, so it runs no module: the table's
    // check of what each was built from is for a person reading it.
    if out.is_json() {
        out.reply(&modules_json()?);
        return Ok(Done::shown());
    }
    let pinned = outside::Pinned::load().unwrap_or_default();
    let mut ours = Table::new(
        ["Module", "State", "About"],
        sentence!("No modules are known."),
    );
    for (module, about) in MODULES.iter() {
        ours.row([
            Value::from(module_name(module)),
            Value::from(installed(module)),
            Value::from(about.to_string()),
        ]);
    }
    out.block(ours);
    if !pinned.modules.is_empty() {
        let mut theirs = Table::new(
            ["Module", "State", "From", "Key"],
            sentence!("No modules from outside Link are added."),
        );
        for (module, from) in &pinned.modules {
            let key = link_core::release::fingerprint(&from.signer).unwrap_or_default();
            theirs.row([
                Value::from(module_name(module)),
                Value::from(installed(module)),
                Value::from(from.source.clone()),
                Value::from(key),
            ]);
        }
        out.block(Section::new(phrase!("From outside Link")).with(theirs));
    }
    Ok(Done::shown().next(cmd!("lnk module add <module>"), phrase!("install a module")))
}

/// `lnk module show <module>`: where one module comes from and whether
/// it's installed; `lnk issue create` puts its blocks in an issue.
fn module_show(out: &mut Out, module: &str) -> Result<Done, Report> {
    let pinned = outside::Pinned::load().unwrap_or_default();
    let mut fields = format::Fields::new().row("Module", module_name(module));
    if let Some((_, about)) = MODULES.iter().find(|(known, _)| *known == module) {
        fields = fields
            .row("State", installed(module))
            .row("About", about.to_string());
    } else if let Some(from) = pinned.modules.get(module) {
        let key = link_core::release::fingerprint(&from.signer).unwrap_or_default();
        fields = fields
            .row("State", installed(module))
            .row("From", from.source.clone())
            .row("Key", key);
    } else {
        return Err(Report::error(sentence!(
            "{} isn't a module Link knows.",
            format::Foreign::new(module)
        ))
        .next(cmd!("lnk module list"), phrase!("see the modules"))
        .exit(format::Exit::Usage));
    }
    out.block(fields);
    Ok(Done::shown().next(cmd!("lnk module list"), phrase!("see the modules")))
}

/// `lnk module list --json`: Link's modules, then those from outside
/// Link with where each comes from (`link_core::modules::Listed`). A
/// module is installed when its program is there; none is run to say
/// what it was built from, since every question asks this list.
fn modules_json() -> anyhow::Result<Vec<link_core::modules::Listed>> {
    let pinned = outside::Pinned::load()?;
    let ours = MODULES
        .iter()
        .map(|(module, _)| link_core::modules::Listed {
            name: module.to_string(),
            installed: link_core::module_present(module),
            kind: Some(registry::REGISTRY[*module].component.kind.clone()),
            description: Some(registry::REGISTRY[*module].component.description.clone()),
            answers: registry::REGISTRY[*module].answers.clone(),
            ..Default::default()
        });
    let theirs = pinned
        .modules
        .into_iter()
        .map(|(module, from)| link_core::modules::Listed {
            installed: link_core::module_present(&module),
            name: module,
            source: Some(from.source),
            signer: Some(from.signer),
            ..Default::default()
        });
    Ok(ours.chain(theirs).collect())
}

#[cfg(all(test, target_os = "linux"))]
mod spawns;

#[cfg(test)]
mod location_tests;

#[cfg(all(test, unix))]
mod install_script_tests;

#[cfg(test)]
link_test_kit::program!(unit);
