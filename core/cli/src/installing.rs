//! `lnk up` and `lnk module add` and `remove`: which modules go with and
//! need which, choosing them, installing and removing them.

use link_format::{Exit, Note, Question, Refuser};

use super::*;
use crate::offers::it_or_them;

/// Required and recommended module edges come from their owning declarations.
#[cfg(test)]
fn needs_when(module: &str) -> Vec<&'static str> {
    registry::declared_modules(module)
}

pub(super) fn goes_with(module: &str) -> &'static [(&'static str, &'static str)] {
    registry::recommends(module)
}

/// `modules`, a use's, with what goes with each, without repeats: what
/// `lnk up` takes.
pub(super) fn with_goes_with<'a>(modules: &[&'a str]) -> Vec<&'a str> {
    // Recommendations are offered separately; they never select themselves.
    modules.to_vec()
}

/// `names`, each a module Link knows, with what they need (and what that
/// needs), each before the module needing it, without repeats.
fn with_needs<'a>(names: &[&'a str]) -> anyhow::Result<Vec<&'a str>> {
    link_component::required_programs(&registry::REGISTRY, names)
}

/// The names of `USES`, as a list value: "agents, models and ...".
pub(super) fn uses() -> Value {
    Value::List(
        USES.iter()
            .map(|(name, ..)| Value::from(Name::new("use", *name)))
            .collect(),
    )
}

/// Which of `USES` `words` name: numbers from the list, or names.
fn picks(words: &[&str]) -> Result<Vec<usize>, Report> {
    let mut picked = Vec::new();
    for word in words
        .iter()
        .flat_map(|w| w.split([' ', ',']))
        .filter(|w| !w.is_empty())
    {
        let index = match word.parse::<usize>() {
            Ok(n) if (1..=USES.len()).contains(&n) => n - 1,
            _ => USES
                .iter()
                .position(|(name, ..)| *name == word || name.trim_end_matches('s') == word)
                .ok_or_else(|| {
                    Report::error(sentence!(
                        "{} isn't one of the choices.",
                        format::Foreign::new(word)
                    ))
                    .why(prose!("The choices are {}.", uses()))
                    .next(cmd!("lnk up <use>"), phrase!("set one of them up"))
                    .exit(Exit::Usage)
                })?,
        };
        if !picked.contains(&index) {
            picked.push(index);
        }
    }
    Ok(picked)
}

/// The menu of `USES`, one a row, numbered.
fn menu() -> Table {
    let mut menu = Table::new(
        ["#", "Use", "About", "State"],
        sentence!("There are no uses."),
    )
    .right(0);
    for (i, (name, about, modules, _)) in USES.iter().enumerate() {
        let installed = modules
            .iter()
            .all(|p| link_core::installed_module(p).is_some());
        menu.row([
            Value::from(i + 1),
            Value::from(Name::new("use", *name)),
            Value::from(about.to_string()),
            Value::from(match installed {
                true => crate::Installed::Yes,
                false => crate::Installed::No,
            }),
        ]);
    }
    menu
}

/// `lnk up`: each word a use (`agents`), and with none, in a terminal,
/// the choice of uses, which `lnk module add` never offers: it needs a
/// name.
pub(super) async fn up(out: &mut Out, names: &[String]) -> Result<Done, Report> {
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let chosen = if !names.is_empty() {
        resolve(&names)?
    } else if out.can_ask() {
        out.now(Section::new(phrase!("How do you want to use Local Link")).with(menu()));
        let numbers: Vec<String> = (1..=USES.len()).map(|n| n.to_string()).collect();
        let answer = out.ask(
            Question::text(sentence!("Pick any ({}).", numbers.join(", "))).default("defaults"),
        )?;
        match answer.trim() {
            "defaults" => registry::default_choices(),
            answer => picks(&[answer])?,
        }
    } else {
        return Err(Report::error(sentence!("Say what to set up."))
            .why(prose!("The uses are {}.", uses()))
            .next(cmd!("lnk up <use>"), phrase!("set one of the uses up"))
            .next(cmd!("lnk module list"), phrase!("see the modules"))
            .exit(Exit::Usage));
    };
    let wanted: Vec<&str> = chosen
        .iter()
        .flat_map(|i| USES[*i].2.iter().copied())
        .collect();
    if !wanted.is_empty() {
        let done = add(out, &with_goes_with(&wanted)).await?;
        absorb(out, done);
    }
    let mut done = Done::shown();
    for i in &chosen {
        done = done.next(
            lnk_command(USES[*i].3.trim_start_matches("lnk ").split_whitespace()),
            phrase!("carry on with {}", Name::new("use", USES[*i].0)),
        );
    }
    Ok(done)
}

/// `lnk module add`: each word a module Link knows (`slack`), and each
/// `--url` a module from outside Link.
pub(super) async fn add_modules(
    out: &mut Out,
    names: &[String],
    urls: &[String],
    options: &outside::AddOptions,
) -> Result<Done, Report> {
    let mut modules = Vec::new();
    for word in names
        .iter()
        .flat_map(|w| w.split([' ', ',']))
        .filter(|w| !w.is_empty())
    {
        if registry::REGISTRY.contains_key(word) {
            if !modules.contains(&word) {
                modules.push(word);
            }
        } else if outside::is_url(word) {
            return Err(Report::error(sentence!(
                "{} is a module from outside Link: give it as --url.",
                format::Foreign::new(word)
            ))
            .next(
                cmd!("lnk module add --url <url>"),
                phrase!("add it by its URL"),
            )
            .exit(Exit::Usage));
        } else {
            return Err(Report::error(sentence!(
                "{} isn't a module Link knows.",
                format::Foreign::new(word)
            ))
            .next(cmd!("lnk module list"), phrase!("see the modules"))
            .exit(Exit::Usage));
        }
    }
    if let Some(url) = urls.iter().find(|u| !outside::is_url(u)) {
        return Err(Report::error(sentence!(
            "{} isn't a GitHub URL.",
            format::Foreign::new(url)
        ))
        .next(
            cmd!("lnk module add <module>"),
            phrase!("add a module of Link's by name"),
        )
        .exit(Exit::Usage));
    }
    if !modules.is_empty() {
        let done = add(out, &with_goes_with(&modules)).await?;
        absorb(out, done);
    }
    if !urls.is_empty() {
        let (_, dir) = install_dir()?;
        for url in urls {
            let (_, done) = outside::add(out, url, options, &dir).await?;
            absorb(out, done);
        }
    }
    Ok(Done::shown())
}

/// The uses `names` name: a use's name (`agents`), else its singular
/// (`box`) or its number.
fn resolve(names: &[&str]) -> Result<Vec<usize>, Report> {
    let mut chosen = Vec::new();
    for word in names
        .iter()
        .flat_map(|w| w.split([' ', ',']))
        .filter(|w| !w.is_empty())
    {
        if let Some(i) = USES.iter().position(|(name, ..)| *name == word) {
            chosen.push(i);
        } else {
            let i = picks(&[word]).map_err(|_| {
                Report::error(sentence!("{} isn't a use.", format::Foreign::new(word)))
                    .why(prose!("The uses are {}.", uses()))
                    .next(cmd!("lnk module add <module>"), phrase!("add a module"))
                    .exit(Exit::Usage)
            })?;
            chosen.extend(i);
        }
    }
    chosen.dedup();
    Ok(chosen)
}

/// Where lnk and its modules live: `lnk`'s own folder.
pub(super) fn install_dir() -> Result<(PathBuf, PathBuf), Report> {
    let exe = std::env::current_exe().within("finding the lnk binary")?;
    let exe = exe.canonicalize().unwrap_or(exe);
    if exe.components().any(|c| c.as_os_str() == "Cellar") {
        return Err(Report::refusal(
            Refuser::Policy("homebrew".into()),
            sentence!(
                "{} was installed with Homebrew, so brew changes it.",
                Product::new("lnk")
            ),
        )
        .next(
            cmd!("brew list lnk"),
            phrase!("see what Homebrew installed"),
        ));
    }
    let dir = exe.parent().within("finding lnk's folder")?.to_path_buf();
    Ok((exe, dir))
}

/// The options of `lnk module add`, which are for a module from outside
/// Link only.
pub(super) fn add_options(
    urls: &[String],
    yes: bool,
    signer: Option<String>,
) -> Result<outside::AddOptions, Report> {
    let options = outside::AddOptions { yes, signer };
    let urls = urls.len();
    if urls == 0 && (options.yes || options.signer.is_some()) {
        return Err(Report::error(sentence!(
            "The flags --yes and --signer are for a module from outside Link."
        ))
        .next(
            cmd!("lnk module add --url <url> --yes"),
            phrase!("add one, trusting its key"),
        )
        .exit(Exit::Usage));
    }
    if options.signer.is_some() && urls != 1 {
        return Err(Report::error(sentence!(
            "The flag --signer goes with one module from outside Link."
        ))
        .next(
            cmd!("lnk module add --url <url> --signer <key>"),
            phrase!("add one with its key"),
        )
        .exit(Exit::Usage));
    }
    Ok(options)
}

/// `names` and what they need, each a module Link knows, that aren't
/// installed in `lnk`'s own folder: what `lnk module add` installs. A
/// program of the same name elsewhere on the PATH, such as a build from
/// source, isn't installed ([`link_core::installed_module`]).
pub(super) fn missing<'a>(names: &[&'a str]) -> anyhow::Result<Vec<&'a str>> {
    Ok(with_needs(names)?
        .into_iter()
        .filter(|p| link_core::installed_module(p).is_none())
        .collect())
}

/// `lnk module add`: the modules not installed yet, and what they need,
/// from this lnk's own (signed) release, into lnk's folder; a `lnk` built
/// from source installs them from its checkout, as `lnk up --source` does.
pub(super) async fn add(out: &mut Out, names: &[&str]) -> Result<Done, Report> {
    let wanted = missing(names)?;
    if wanted.is_empty() {
        return Ok(Done::success(sentence!(
            "Already installed: {}.",
            modules_value(&with_needs(names)?)
        )));
    }
    if let Some(checkout) = link_core::CHECKOUT {
        return source::install(out, &from_checkout(&checkout.root(), &wanted)?);
    }
    let (_, dir) = install_dir()?;
    let version = link_version::version();
    let base = release::base(&release::Source::here()?, Some(&version))?;
    let release = match Release::open(&base, &release::trusted_signers()).await {
        Ok(release) => release,
        Err(err) => {
            return Err(failed(
                sentence!(
                    "Nothing was installed: there is no signed lnk {} release to install {} from.",
                    Version::new(version.to_string()),
                    modules_value(&wanted)
                ),
                err,
            )
            .next(
                cmd!("lnk module list"),
                phrase!("see which modules are installed"),
            ));
        }
    };
    let work = dir.join(format!("lnk-install-{}", std::process::id()));
    std::fs::create_dir_all(&work).within("making a folder to install in")?;
    let result = async {
        // Any signed release could sit under this tag (someone with the
        // releases repo can move it), so check it is this lnk's version:
        // its signed VERSION file, and each program's own `--version`.
        release::check_installable(release.version().await?, Some(&version), None)?;
        let mut steps = format::Steps::new(wanted.len());
        for module in &wanted {
            out.step(
                &mut steps,
                phrase!("Installing the {} module", module_name(module)),
            );
            let archive = release.get(&release::program_asset(module)?).await?;
            release::unpack(&archive, &work)?;
            let name = format!("lnk-{module}");
            let found =
                version_of(&work.join(&name)).within(format!("checking lnk-{module}'s version"))?;
            ensure!(
                found == version,
                "the release under v{version} has lnk-{module} {found}; not installing it"
            );
            release::put(&work.join(&name), &dir.join(&name))?;
        }
        anyhow::Ok(())
    }
    .await;
    let _ = std::fs::remove_dir_all(&work);
    result.map_err(|err| {
        failed(sentence!("Installing modules failed."), err).next(
            cmd!("lnk module list"),
            phrase!("see which modules are installed"),
        )
    })?;
    Ok(Done::success(sentence!(
        "Installed {}, signature and checksum verified.",
        modules_value(&wanted)
    ))
    .changed(phrase!("Installed in"), Shown::new(&dir))
    .next(cmd!("lnk module list"), phrase!("see the modules")))
}

/// The `lnk up --source` that installs `wanted` from `checkout`, the one
/// this `lnk` was built from; if it's gone (moved or deleted), the
/// command, with where it was, for one run from where it is now.
fn from_checkout(checkout: &Path, wanted: &[&str]) -> Result<cli::CoreUpArgs, Report> {
    let module: Vec<String> = wanted.iter().map(|p| p.to_string()).collect();
    if !checkout.join("Cargo.toml").is_file() {
        return Err(Report::error(sentence!(
            "Nothing was installed: this lnk is built from source, and its checkout isn't at {} any more.",
            Shown::new(checkout)
        ))
        .next(
            cmd!(
                "lnk up --source {} --module {}",
                checkout.display().to_string(),
                module.join(",")
            ),
            phrase!("install {} from the checkout, wherever it is now", it_or_them(wanted.len())),
        )
        .exit(Exit::NotInstalled));
    }
    Ok(cli::CoreUpArgs {
        source: vec![checkout.to_path_buf()],
        module,
        ..cli::CoreUpArgs::default()
    })
}

/// `lnk module remove`: the modules named and what needs them (the
/// reverse of [`needs`]: it can't run without them), each told first
/// (`lnk-<module> uninstall`) so its services stop; what they keep stays.
pub(super) fn remove(out: &mut Out, names: &[&str]) -> Result<Done, Report> {
    let (ours, others): (Vec<&str>, Vec<&str>) = names
        .iter()
        .partition(|n| MODULES.iter().any(|(p, _)| p == *n));
    let (_, dir) = install_dir()?;
    let mut pinned = match others.is_empty() {
        true => outside::Pinned::default(),
        false => outside::Pinned::load()?,
    };
    // Any other: one added from outside Link, or one in lnk's folder.
    for name in &others {
        let valid = name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
        let there = pinned.modules.contains_key(*name) || dir.join(format!("lnk-{name}")).is_file();
        if !there && let Some(retired) = retired(name) {
            return Err(retired);
        }
        if !valid || !there {
            with_needs(&[name])?; // says which there are
        }
    }
    let mut removed: Vec<&str> = Vec::new();
    for name in &others {
        let path = dir.join(format!("lnk-{name}"));
        if path.is_file() {
            let kept = stop_outside(&path).map_err(|err| stopping(name, err))?.kept;
            std::fs::remove_file(&path).within(format!("removing {}", path.display()))?;
            removed.push(name);
            if !kept.is_empty() {
                out.block(Note(sentence!(
                    "Its {} stay: adding it again uses them, {} deletes them.",
                    Value::List(kept.iter().map(|k| Value::from(k.as_str())).collect()),
                    cmd!("lnk uninstall")
                )));
            }
        }
        if pinned.modules.remove(*name).is_some() {
            pinned.save()?;
            out.block(Note(sentence!(
                "{} no longer trusts {}'s key.",
                Product::new("lnk"),
                module_name(name)
            )));
        }
    }
    if !ours.is_empty() {
        for module in with_dependents(&ours) {
            let path = dir.join(format!("lnk-{module}"));
            match (path.is_file(), link_core::find_module(module)) {
                (true, _) => {
                    // A module that can't stop keeps its program, else its
                    // services would run on with nothing left to stop them.
                    // One that declares no removal keeps and runs nothing.
                    let kept = match crate::uninstall::answers_uninstall(module) {
                        true => {
                            stop_module(&path)
                                .map_err(|err| stopping(module, err))?
                                .kept
                        }
                        false => Vec::new(),
                    };
                    std::fs::remove_file(&path).within(format!("removing {}", path.display()))?;
                    removed.push(module);
                    if !kept.is_empty() {
                        out.block(Note(sentence!(
                            "Its {} stay: {} uses them again, {} deletes them.",
                            Value::List(kept.iter().map(|k| Value::from(k.as_str())).collect()),
                            module_add(&[module]),
                            cmd!("lnk uninstall")
                        )));
                    }
                }
                (false, Some(elsewhere)) => out.block(Note(sentence!(
                    "The {} module at {} isn't in lnk's folder; remove it yourself.",
                    module_name(module),
                    Shown::new(elsewhere)
                ))),
                (false, None) => {}
            }
        }
    }
    Ok(match removed.as_slice() {
        [] => {
            out.block(Note(sentence!(
                "Nothing was removed: none of those modules is installed here."
            )));
            Done::shown().next(cmd!("lnk module list"), phrase!("see which are installed"))
        }
        [one] => Done::success(sentence!("Removed the {} module.", module_name(one))),
        more => Done::success(sentence!("Removed the modules {}.", modules_value(more))),
    })
}

/// The refusal of a module's name the release renamed or removed, where
/// nothing of it is left in lnk's folder: what history says of it. One an
/// upgrade couldn't stop is still there, and `remove` takes it.
fn retired(name: &str) -> Option<Report> {
    let history = &link_cli::history::HISTORY;
    if let Some(renamed) = history.renamed.iter().find(|r| r.module == name) {
        let now = history.name(name).unwrap_or(&renamed.now);
        return Some(
            Report::error(format::Sentence::adopt(&link_format::clean(
                &renamed.message(now),
            )))
            .exit(Exit::Usage)
            .next(
                lnk_command(["module", "remove", now]),
                phrase!("by its name today"),
            ),
        );
    }
    let removed = history.removed.iter().find(|r| r.module == name)?;
    Some(
        Report::error(format::Sentence::adopt(&link_format::clean(
            &removed.message(),
        )))
        .exit(Exit::Usage),
    )
}

/// A module that couldn't stop what it runs, so it stays.
fn stopping(module: &str, err: anyhow::Error) -> Report {
    failed(
        sentence!(
            "The {} module couldn't stop what it runs, so it stays.",
            module_name(module)
        ),
        err,
    )
    .next(cmd!("lnk module list"), phrase!("see what is installed"))
}

/// `names` with every module needing one of them, the modules needing
/// them first, without repeats.
fn with_dependents<'a>(names: &[&'a str]) -> Vec<&'a str> {
    let mut all: Vec<&'a str> = Vec::new();
    for name in names {
        for (module, _) in MODULES.iter() {
            let needed = with_needs(&[module]).unwrap_or_default();
            if *module != *name && needed.contains(name) && !all.contains(module) {
                all.push(module);
            }
        }
    }
    for name in names {
        if !all.contains(name) {
            all.push(name);
        }
    }
    all
}

/// What a module told it's going ([`stop_module`]) said: the names of
/// the parts it keeps that are there, and what stopping it stopped, for
/// the user, as the module says it, empty when it says nothing.
pub(super) struct Stopped {
    pub kept: Vec<String>,
    pub stops: String,
}

/// What module `program` keeps (`uninstall --parts --json`), none when it
/// doesn't answer.
fn parts(program: &std::path::Path) -> Option<link_core::uninstall::Kept> {
    link_secretive::process::ours(program)
        .args(["uninstall", "--parts", "--json"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<link_core::uninstall::Kept>(&o.stdout).ok())
}

/// Tells module `program` it's going (`uninstall`: it stops its
/// services): a module that answers `uninstall`, as one declaring its
/// removal does. One that can't list its parts can't stop, so it's an
/// error, as is one that answers but fails to stop, with what it said.
pub(super) fn stop_module(program: &std::path::Path) -> anyhow::Result<Stopped> {
    let kept = parts(program).with_context(|| {
        format!(
            "{} didn't list what it keeps (uninstall --parts), so it can't be told to stop",
            program.display()
        )
    })?;
    stop(program, kept)
}

/// [`stop_module`] for a module from outside Link, which may not answer
/// `uninstall`: one that doesn't keeps nothing and runs nothing the core
/// knows of.
pub(super) fn stop_outside(program: &std::path::Path) -> anyhow::Result<Stopped> {
    match parts(program) {
        Some(kept) => stop(program, kept),
        None => Ok(Stopped {
            kept: Vec::new(),
            stops: String::new(),
        }),
    }
}

fn stop(program: &std::path::Path, kept: link_core::uninstall::Kept) -> anyhow::Result<Stopped> {
    let output = link_core::answers::Said::shipped()
        .output(
            link_secretive::process::ours(program)
                .arg("uninstall")
                .stdin(std::process::Stdio::null()),
        )
        .within(format!("running {}", program.display()))?;
    if !output.status.success() {
        bail!(
            "{} uninstall failed: {}",
            program.display(),
            link_format::clean(String::from_utf8_lossy(&output.stderr).trim())
        );
    }
    Ok(Stopped {
        kept: kept
            .parts
            .into_iter()
            .filter(|p| p.paths.iter().any(|p| p.symlink_metadata().is_ok()))
            .map(|p| link_format::clean(&p.name))
            .collect(),
        stops: link_format::clean(&kept.stops),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_use_takes_what_goes_with_its_modules() {
        for (_, _, selected, _) in USES.iter() {
            assert_eq!(with_goes_with(selected), *selected);
            with_needs(selected).unwrap();
        }
    }

    /// A `lnk` built from source installs from the checkout it was built
    /// from, through `lnk up --source`'s own install, the modules asked.
    #[test]
    fn a_source_build_installs_from_its_checkout() {
        let checkout = link_test_kit::TempDir::new("source-checkout");
        std::fs::write(checkout.join("Cargo.toml"), "[workspace]\n").unwrap();
        let up = from_checkout(&checkout, &["storage", "sandbox"]).unwrap();
        assert_eq!(up.source, [checkout.to_path_buf()]);
        assert_eq!(up.module, ["storage", "sandbox"]);
        assert!(!up.stale && up.uses.is_empty());
    }

    /// Its checkout moved or deleted: nothing is installed, and the
    /// source command is named with where the checkout was.
    #[test]
    fn a_source_build_without_its_checkout_names_the_command() {
        let home = link_test_kit::TempDir::new("source-checkout");
        let gone = home.join("link-public");
        let Err(report) = from_checkout(&gone, &["storage", "sandbox"]) else {
            panic!("a missing checkout installs nothing");
        };
        let text = serde_json::to_string(&*report).unwrap();
        let command = format!(
            "lnk up --source {} --module storage,sandbox",
            gone.display()
        );
        assert!(
            report.next.0.iter().any(|n| n.command.text() == command),
            "{text}"
        );
        assert!(text.contains(&gone.display().to_string()), "{text}");
        assert_eq!(report.exit, Exit::NotInstalled);
    }

    /// A module of the same name elsewhere on the PATH, such as a build
    /// from source in a checkout's `target/debug`, isn't installed: `lnk
    /// module add` still installs its own, beside `lnk`.
    #[cfg(unix)]
    #[test]
    fn a_module_elsewhere_on_the_path_is_not_installed() {
        let home = link_test_kit::TempDir::new("module-add-path");
        let (own, elsewhere) = (home.join("own"), home.join("elsewhere"));
        for dir in [&own, &elsewhere] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let lnk = own.join("lnk");
        link_test_kit::link_program(&link_test_kit::programs().join("lnk"), &lnk).unwrap();
        link_test_kit::write_program(&elsewhere.join("lnk-measure"), "#!/bin/sh\nexit 0\n")
            .unwrap();
        let path = std::env::join_paths(std::iter::once(elsewhere.to_path_buf()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
        ))
        .unwrap();
        let mut dry = link_test_kit::env::command(&lnk, &home);
        let output = dry
            .args(["module", "add", "--dry", "measure"])
            .env("PATH", path)
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "{said}{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(said.lines().any(|line| line == "measure"), "{said}");
    }

    /// The groups of each `lnk <group> ...` in `code` whose group is a
    /// string: `lnk().args(["box", ...])`, `lnk().arg("bucket")`, or a
    /// command made by `lnk()` and given its group in a later line.
    fn groups_run(code: &str) -> Vec<String> {
        // Tests run what they like.
        let code = code.split("#[cfg(test)]").next().unwrap_or_default();
        let group_at = |rest: &str| -> Option<String> {
            let rest = rest.trim_start();
            let rest = rest
                .strip_prefix(".args(")
                .or_else(|| rest.strip_prefix(".arg("))?
                .trim_start();
            let rest = rest.strip_prefix('[').unwrap_or(rest).trim_start();
            let word: String = rest
                .strip_prefix('"')?
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || *c == '-')
                .collect();
            (!word.is_empty()).then_some(word)
        };
        // What link_core runs for a module that calls it.
        let mut found: Vec<String> = code
            .matches("cloud::account(")
            .map(|_| "cloud".to_string())
            .collect();
        for (at, _) in code.match_indices("::lnk()") {
            let after = &code[at + "::lnk()".len()..];
            if let Some(group) = group_at(after) {
                found.push(group);
                continue;
            }
            // `let mut start = link_core::lnk();`, then `start.args([...])`.
            let line = code[..at].rsplit('\n').next().unwrap_or_default();
            let Some(name) = line
                .trim_start()
                .strip_prefix("let ")
                .map(|l| l.trim_start_matches("mut "))
                .and_then(|l| l.split([' ', '=', ':']).next())
                .filter(|n| !n.is_empty())
            else {
                continue;
            };
            let window: String = after.chars().take(1500).collect();
            if let Some(group) = window
                .match_indices(name)
                .find_map(|(i, _)| group_at(&window[i + name.len()..]))
            {
                found.push(group);
            }
        }
        found
    }

    /// Every `lnk <group>` a module runs is its own, one it needs, or one
    /// a feature of it needs ([`super::needs_when`]): none it runs
    /// unsaid, so none is a second command to find. And each feature's
    /// need is still run.
    #[test]
    fn every_group_a_module_runs_is_declared() {
        let own = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut directories = vec![own.clone()];
        if let Some(paths) = std::env::var_os("LNK_COMPONENTS") {
            directories.extend(
                std::env::split_paths(&paths)
                    .filter(|path| !path.as_os_str().is_empty())
                    .map(|path| own.join("../..").join(path)),
            );
        }
        let mut packages = Vec::new();
        for directory in directories {
            let output = link_test_kit::cargo()
                .args([
                    "metadata",
                    "--offline",
                    "--no-deps",
                    "--format-version",
                    "1",
                ])
                .current_dir(directory)
                .output()
                .unwrap();
            assert!(output.status.success());
            let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            packages.extend(metadata["packages"].as_array().unwrap().iter().cloned());
        }
        let mut checked = 0;
        for (module, _) in super::MODULES.iter() {
            let package = packages
                .iter()
                .find(|p| {
                    p["targets"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|t| t["name"] == format!("lnk-{module}"))
                })
                .unwrap();
            let dir = std::path::Path::new(package["manifest_path"].as_str().unwrap())
                .parent()
                .unwrap()
                .join("src");
            if !dir.is_dir() {
                continue;
            }
            let mut run = Vec::new();
            let mut files = vec![dir];
            while let Some(path) = files.pop() {
                if path.is_dir() {
                    files.extend(std::fs::read_dir(&path).unwrap().map(|e| e.unwrap().path()));
                } else if path.extension().is_some_and(|e| e == "rs") {
                    run.extend(groups_run(&std::fs::read_to_string(&path).unwrap()));
                }
            }
            let declared = super::with_needs(&[module]).unwrap();
            let mut used = Vec::new();
            for group in run {
                let Some((_, of)) = super::GROUPS.iter().find(|(g, _)| *g == group) else {
                    // The core's own: `lnk module`, `lnk upgrade`.
                    continue;
                };
                used.push(*of);
                assert!(
                    of == module || declared.contains(of) || super::needs_when(module).contains(of),
                    "the {module} module runs `lnk {group}`, the {of} module's, which neither \
                     needs() nor needs_when() declares"
                );
            }
            checked += 1;
        }
        assert!(checked > 15, "only {checked} modules read");
    }

    #[test]
    fn a_group_is_read_from_each_way_a_command_is_made() {
        let code = r#"
            link_core::lnk().args(["box", "list"]);
            crate::lnk()
                .arg("bucket")
                .args(args);
            let mut start = link_core::lnk();
            start.args(["sandbox", "run"]);
            link_core::lnk().args(words);
            #[cfg(test)]
            link_core::lnk().arg("storage");
        "#;
        assert_eq!(groups_run(code), ["box", "bucket", "sandbox"]);
    }

    /// Removal takes required consumers, never optional recommendations.
    #[test]
    fn removing_a_module_takes_what_needs_it() {
        for (module, _) in MODULES.iter() {
            let removed = with_dependents(&[module]);
            assert_eq!(removed.last(), Some(module));
            for (consumer, _) in MODULES.iter() {
                assert_eq!(
                    removed.contains(consumer),
                    with_needs(&[consumer]).unwrap().contains(module),
                    "removing {module} must take required dependents only: {consumer}"
                );
            }
        }
    }

    #[test]
    fn choices_by_number_or_name() {
        for (i, (name, _, _, _)) in USES.iter().enumerate() {
            assert_eq!(picks(&[name]).unwrap(), vec![i]);
            assert_eq!(picks(&[&(i + 1).to_string()]).unwrap(), vec![i]);
        }
        assert_eq!(picks(&["1,1"]).unwrap(), vec![0]);
        assert!(picks(&["0"]).is_err());
        assert!(picks(&[&(USES.len() + 1).to_string()]).is_err());
        assert!(picks(&["__unknown_choice__"]).is_err());
    }

    #[test]
    fn required_components_precede_their_owner() {
        for (module, _) in MODULES.iter() {
            let closure = with_needs(&[module]).unwrap();
            assert_eq!(closure.last(), Some(module));
            let required = link_component::requirements::required(
                &registry::REGISTRY,
                module,
                None,
                &std::collections::BTreeMap::new(),
                &link_component::requirements::Environment::here(),
            )
            .unwrap();
            for required in required.modules.iter().filter(|p| *p != module) {
                assert!(closure.iter().position(|p| p == required).unwrap() < closure.len() - 1);
            }
        }
        assert!(with_needs(&["__unknown_component__"]).is_err());
    }
}
