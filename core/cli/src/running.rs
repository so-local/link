//! `lnk <group> ...`: running the module that provides a group, or saying
//! how to install it, what replaced it, or the command meant.

use link_format::{Exit, Question};

use super::*;
use crate::offers::it_or_them;

/// `args` without a pasted `# comment`. zsh, the Mac's shell, passes one
/// on as arguments (`lnk example start # a comment` is `start`, `#`,
/// `asks`, ...) unless `interactive_comments` is set, and the docs' lines
/// end in one. No lnk argument is `#` alone; after `--` the arguments are
/// another program's (`lnk example run -- ...`), so they stay as they are.
pub(super) fn without_comment(mut args: Vec<OsString>) -> Vec<OsString> {
    let end = args
        .iter()
        .take_while(|a| *a != "--")
        .position(|a| a == "#");
    if let Some(end) = end {
        args.truncate(end);
    }
    args
}

/// The `lnk up` choice that installs `module`, and its modules.
pub(super) fn up_choice(module: &str) -> Option<(&'static str, &'static [&'static str])> {
    USES.iter()
        .find(|(_, _, modules, _)| modules.contains(&module))
        .map(|(choice, _, modules, _)| (*choice, *modules))
}

/// `lnk <group>`'s module isn't installed: the `lnk up` choice that
/// installs it, else `lnk module add`.
fn not_installed(group: &str, module: &str) -> Report {
    let (how, install) = match up_choice(module) {
        Some((choice, _)) => (
            phrase!("install it, with what goes with it"),
            lnk_command(["up", choice]),
        ),
        None => (phrase!("install it"), module_add(&[module])),
    };
    Report::error(sentence!(
        "{} is part of the {} module, which isn't installed.",
        lnk_command([group]),
        module_name(module)
    ))
    .next(install, how)
    .exit(Exit::NotInstalled)
}

/// `lnk <group>`'s module was built from other sources than this `lnk`
/// ([`link_core::source::Skew`]): never run, as programs of one version
/// built apart can disagree in ways neither says plainly.
fn skewed(group: &str, skew: &link_core::source::Skew) -> Report {
    let checkout = skew.checkout();
    Report::error(sentence!(
        "{} is part of the {} module, which was built from other sources than this lnk.",
        lnk_command([group]),
        module_name(&skew.module)
    ))
    .why(prose!(
        "{} was built from commit {}, and lnk from {}.",
        Shown::new(&skew.path),
        Value::from(skew.theirs.clone()),
        Value::from(skew.ours.clone())
    ))
    .next(
        cmd!(
            "cargo build --release --workspace --bins --manifest-path {}",
            format!("{checkout}/Cargo.toml")
        ),
        phrase!("build every program from your checkout together"),
    )
    .next(
        cmd!("lnk up --source {}", checkout),
        phrase!("or rebuild its modules beside lnk"),
    )
    .exit(Exit::NotInstalled)
}

/// Offers to install what `module` needs and isn't installed, in a
/// terminal, else installs it when typed ([`typed_unasked`]), else says
/// how. A module can need one it didn't when it was
/// installed: `lnk upgrade` updates only the modules there are, as the
/// lnk before it knew them.
fn missing_needs(
    out: &mut Out,
    module: &str,
    group: &str,
    args: &[OsString],
) -> Result<(), Report> {
    // What it needs, and what that needs.
    let descriptor = registry::REGISTRY.get(module);
    if descriptor.is_none() {
        return Ok(());
    }
    let words: Vec<String> = std::iter::once(group.to_string())
        .chain(args.iter().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    use link_component::requirements::{Operation, Values, operation_for_args};
    let (command, values) = match descriptor.map(|d| operation_for_args(d, &words)) {
        None | Some(Operation::Unmatched) => (None, Default::default()),
        Some(Operation::Matched {
            name,
            values: Values::Bound(values),
        }) => (Some(name), values),
        // Args its own parser refuses: it runs and says so, and nothing
        // is installed or offered for them.
        Some(Operation::Matched {
            values: Values::Refused { .. },
            ..
        }) => return Ok(()),
    };
    let required = link_component::requirements::required(
        &registry::REGISTRY,
        module,
        command,
        &values,
        &link_component::requirements::Environment::here(),
    )?;
    let missing: Vec<&str> = required
        .modules
        .into_iter()
        .filter(|p| *p != module && !link_core::module_present(p))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    if out.can_ask() {
        let yes = out.ask(Question::confirm(
            sentence!(
                "The {} module needs {}, which isn't installed. Install {}?",
                module_name(module),
                modules_value(&missing),
                it_or_them(missing.len())
            ),
            true,
        ))?;
        if yes {
            let done = wait(add(out, &missing))?;
            absorb(out, done);
            return Ok(());
        }
    } else if typed_unasked() {
        for need in missing {
            link_core::modules::add(need, &format!("The {module} module"))?;
        }
        return Ok(());
    }
    Err(Report::error(sentence!(
        "The {} module needs {}, which isn't installed.",
        module_name(module),
        modules_value(&missing)
    ))
    .next(module_add(&missing), phrase!("install what it needs")))
}

/// Whether the module providing `group` reads control, so core checks
/// its folders before running it: what its description declares, and yes
/// for a module Link doesn't know.
pub(super) fn reads_control(group: &str) -> bool {
    let module = GROUPS
        .iter()
        .find(|(g, _)| *g == group)
        .map_or(group, |(_, module)| *module);
    module_reads_control(module)
}

fn module_reads_control(module: &str) -> bool {
    registry::REGISTRY
        .get(module)
        .is_none_or(|resolved| resolved.component.reads_control)
}

/// The command running a module's `program`. One reading control is
/// handed what core holds for it: its secrets and its artifact cache. One
/// declaring it reads none is handed none of it, so the declaration that
/// spares it core's folders check can't also bring it control.
fn module_command(program: &Path, reads_control: bool) -> std::process::Command {
    if !reads_control {
        let mut child = link_secretive::process::command(program);
        child.env_remove(ARTIFACT_CACHE);
        return child;
    }
    let mut child = link_secretive::process::ours(program);
    if std::env::var_os(ARTIFACT_CACHE).is_none() {
        child.env(ARTIFACT_CACHE, link_files::layout::Hidden::Artifacts.path());
    }
    child
}

/// The variable a module finds core's artifact cache in.
const ARTIFACT_CACHE: &str = "LNK_ARTIFACT_CACHE";

/// Runs the module providing `group` with `args`, as if it were `lnk`:
/// The descriptor supplies the program; its argument words are preserved.
pub(super) fn run_module(out: &mut Out, group: &str, args: &[OsString]) -> Result<Done, Report> {
    let asking_help = args.iter().any(|a| a == "--help" || a == "-h");
    let module = match GROUPS.iter().find(|(g, _)| *g == group) {
        Some((_, module)) => *module,
        None if registry::valid_name(group) && link_core::find_module(group).is_some() => group,
        None => {
            let report = Report::error(sentence!(
                "There is no command {}.",
                format::Foreign::new(group)
            ))
            .exit(Exit::Usage);
            return Err(match meant(group) {
                Some(meant) => report.next(
                    lnk_command(meant.split_whitespace()),
                    phrase!("what you may have meant"),
                ),
                None => report.next(cmd!("lnk --help"), phrase!("list the commands")),
            });
        }
    };
    if asking_help && link_core::find_module(module).is_none() {
        let descriptor = registry::REGISTRY
            .get(module)
            .within("looking the module up in the registry")?;
        let mut path = group.to_string();
        for arg in args
            .iter()
            .filter_map(|s| s.to_str())
            .take_while(|s| !s.starts_with('-'))
        {
            path.push(' ');
            path.push_str(arg);
        }
        let command = descriptor
            .commands
            .get(&path)
            .or_else(|| descriptor.commands.get(group))
            .within("looking the command's help up")?;
        let mut text = format!("lnk {path}\n{}\n", command.help.trim());
        for (flag, about) in &command.flags {
            text += &format!("  {flag}  {about}\n");
        }
        for (name, entry) in &descriptor.commands {
            let hidden = descriptor.commands.iter().any(|(ancestor, command)| {
                command.hidden && (name == ancestor || name.starts_with(&format!("{ancestor} ")))
            });
            if name.starts_with(&format!("{path} ")) && !hidden {
                text += &format!(
                    "  {name}  {}\n",
                    entry.help.lines().next().unwrap_or_default()
                );
            }
        }
        out.data(text.as_bytes());
        return Ok(Done::shown());
    }
    let found = link_core::locate(module).map_err(|skew| skewed(group, &skew))?;
    let program = match found {
        Some(program) => program,
        None if offer(out, group, module)? => {
            link_core::find_module(module).within("finding the module that was just installed")?
        }
        None if typed_unasked() => link_core::modules::add(module, &format!("lnk {group}"))?,
        None => return Err(not_installed(group, module)),
    };
    if !asking_help {
        missing_needs(out, module, group, args)?;
    }
    let reads_control = module_reads_control(module);
    if !asking_help
        && reads_control
        && let Err(err) = offer_goes_with(out, module)
    {
        out.block(Report::warning(err.headline.clone()));
    }
    let mut child = module_command(&program, reads_control);
    // A module's program never reads its own namespace.
    child.args(args);
    if let Ok(exe) = std::env::current_exe() {
        child.env(link_core::LNK_EXE, exe);
    }
    // The module has the terminal now.
    let _terminal = out.hand_over();
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // It becomes this process, so it's handed the listing its
        // questions would otherwise start another lnk for.
        if reads_control && let Ok(modules) = modules_json() {
            let handed = link_core::modules::Handed {
                to: std::process::id(),
                modules,
            };
            if let Ok(handed) = serde_json::to_string(&handed) {
                child.env(link_core::modules::LNK_MODULES, handed);
            }
        }
        let err = child.exec();
        Err(err)
            .within(format!("running {}", program.display()))
            .map(|()| Done::shown())
            .map_err(Report::from)
    }
    #[cfg(not(unix))]
    {
        let status = child
            .status()
            .within(format!("running {}", program.display()))?;
        Ok(Done::passed(status.code().unwrap_or(1)))
    }
}

/// What `lnk <word>` most likely meant, when `word` is no group nor the
/// core's: the group of that plural or module (a plural or module id), the command it names (an unambiguous command name), or
/// the group or core command a typo or two away (a mistyped group).
fn meant(word: &str) -> Option<String> {
    if let Some((group, _)) = GROUPS
        .iter()
        .find(|(g, p)| word == format!("{g}s") || word == *p)
    {
        return Some(group.to_string());
    }
    let groups = || GROUPS.iter().map(|(g, _)| *g).chain(CORE.iter().copied());
    if !groups().any(|g| g == word) {
        let mut named = COMMANDS.iter().filter(|(_, c)| *c == word);
        if let (Some((group, command)), None) = (named.next(), named.next()) {
            return Some(format!("{group} {command}"));
        }
    }
    // Close enough to be a typo: a slip or two, never more than half the word.
    let most = (word.chars().count() / 2).clamp(1, 2);
    groups()
        .map(|g| (typos(word, g), g))
        .filter(|(n, _)| *n <= most)
        .min()
        .map(|(_, g)| g.to_string())
}

/// How many letters to add, remove, change or swap with the next to make
/// `a` into `b`.
fn typos(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    // d[i][j]: from a's first i letters to b's first j.
    let mut d: Vec<Vec<usize>> = (0..=a.len())
        .map(|i| (0..=b.len()).map(|j| if i == 0 { j } else { i }).collect())
        .collect();
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let change = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + change);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A module built from other sources than lnk is refused, naming
    /// where it is, both commits and the commands that build them
    /// together.
    #[test]
    fn a_module_built_apart_is_refused_by_name() {
        let skew = link_core::source::Skew {
            module: "sandbox".into(),
            path: PathBuf::from("/opt/link/lnk-sandbox"),
            ours: "0123456789abcdef0123456789abcdef01234567".into(),
            theirs: "fedcba9876543210fedcba9876543210fedcba98".into(),
        };
        let report = skewed("sandbox", &skew);
        let text = serde_json::to_string(&*report).unwrap();
        for part in [
            "other sources than this lnk",
            "/opt/link/lnk-sandbox",
            &skew.ours,
            &skew.theirs,
        ] {
            assert!(text.contains(part), "{text}");
        }
        let nexts: Vec<String> = report.next.0.iter().map(|n| n.command.text()).collect();
        assert!(
            nexts[0].starts_with("cargo build --release --workspace --bins --manifest-path"),
            "{nexts:?}"
        );
        assert!(nexts[1].starts_with("lnk up --source"), "{nexts:?}");
        assert_eq!(report.exit, Exit::NotInstalled);
    }

    /// A Link module built from this checkout says the commit it was built
    /// from only from a release build: a debug build, as these tests run,
    /// says none, so a commit never rebuilds `target/debug` or refuses a
    /// program in it. A release build's commit is `link-component-build`'s
    /// `a_release_build_records_its_checkouts_commit`.
    #[test]
    fn a_debug_built_module_says_no_sources() {
        let module = link_test_kit::programs::require(
            &link_test_kit::programs().join("lnk-manual"),
            link_test_kit::programs::BUILD,
        )
        .to_path_buf();
        if cfg!(debug_assertions) {
            assert_eq!(link_core::source::of(&module), None, "{}", module.display());
        }
    }

    /// A group whose module isn't installed names the `lnk up` choice
    /// that installs it, as Get Started does.
    #[test]
    fn a_missing_module_names_its_up_choice() {
        for (choice, _, selected, _) in USES.iter() {
            for module in *selected {
                assert_eq!(up_choice(module).map(|(c, _)| c), Some(*choice));
                let report = not_installed("example", module);
                assert_eq!(report.exit, Exit::NotInstalled);
                assert_eq!(report.next.0[0].command.text(), format!("lnk up {choice}"));
            }
        }
    }

    /// Args the module's own parser refuses (a boolean given a word)
    /// reach the module, to refuse with its usage code: nothing is
    /// installed or offered for them, and core fails nothing itself.
    #[test]
    fn args_the_module_refuses_reach_it() {
        let (mut out, _) = Out::capture(false, &["lnk", "archive"]);
        let args: Vec<OsString> = ["export", "--all=qa-invalid"].map(OsString::from).into();
        let words: Vec<String> = ["archive", "export", "--all=qa-invalid"]
            .map(String::from)
            .into();
        let archive = registry::REGISTRY
            .get("archive")
            .expect("archive is declared");
        assert!(matches!(
            link_component::requirements::operation_for_args(archive, &words),
            link_component::requirements::Operation::Matched {
                values: link_component::requirements::Values::Refused { .. },
                ..
            }
        ));
        missing_needs(&mut out, "archive", "archive", &args).unwrap();
    }

    #[test]
    fn a_pasted_comment_is_dropped_before_dashes_only() {
        let args = |line: &str| -> Vec<OsString> { line.split(' ').map(OsString::from).collect() };
        assert_eq!(
            without_comment(args("agent start # asks which model")),
            args("agent start")
        );
        assert_eq!(without_comment(args("up agents")), args("up agents"));
        assert_eq!(
            without_comment(args("agent run -- sh -c # x")),
            args("agent run -- sh -c # x")
        );
        assert_eq!(
            without_comment(args("tunnel open 3000 --auth a#b")),
            args("tunnel open 3000 --auth a#b")
        );
    }

    #[test]
    fn a_word_that_is_no_command_says_the_one_meant() {
        let meant = |word| meant(word).unwrap_or_default();
        assert_eq!(meant("agnet"), "agent");
        assert_eq!(meant("modle"), "model");
        assert_eq!(meant("measre"), "measure");
        assert_eq!(meant("bx"), "box");
        assert_eq!(meant("upgrdae"), "upgrade");
        assert_eq!(meant("agents"), "agent");
        // A command word only one group has names that group's command.
        assert_eq!(meant("approval"), "env approval");
        // In many groups: no one command meant.
        assert_eq!(meant("list"), "");
        assert_eq!(meant("xyz"), "");
        assert_eq!(meant("a"), "");
        assert_eq!(typos("agnet", "agent"), 1);
        assert_eq!(typos("", "box"), 3);
        assert_eq!(typos("kitten", "sitting"), 3);
    }

    /// Core skips its folders check for the offline manual alone: the one
    /// module declaring it reads no control. Any other, and one Link
    /// doesn't know, is checked.
    #[test]
    fn the_folders_check_is_skipped_for_the_manual_alone() {
        let unchecked: Vec<&str> = registry::REGISTRY
            .keys()
            .map(String::as_str)
            .filter(|module| !reads_control(module))
            .collect();
        assert_eq!(unchecked, ["manual"]);
        assert!(reads_control("no-such-module"));
    }

    /// A module declaring it reads no control is handed none by core: no
    /// secrets and no artifact cache, which one reading it is handed.
    #[test]
    fn a_module_reading_no_control_is_handed_none() {
        let handed = |reads_control| {
            let child = module_command(Path::new("lnk-example"), reads_control);
            child
                .get_envs()
                .filter_map(|(name, value)| value.map(|_| name.to_os_string()))
                .collect::<Vec<_>>()
        };
        let none = handed(false);
        assert!(!none.iter().any(|name| name == ARTIFACT_CACHE));
        assert!(
            !none
                .iter()
                .any(|name| name == link_secretive::process::HANDED_ENV)
        );
        assert!(handed(true).iter().any(|name| name == ARTIFACT_CACHE));
    }
}
