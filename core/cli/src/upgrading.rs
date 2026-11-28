//! `lnk upgrade` and `lnk upgrade --check`: this `lnk`, how it was
//! installed, and the versions of it and its modules.

use link_files::upgrade::Change;
use link_format::{Count, Note};

use super::*;

mod migrating;

/// This program, its links resolved.
pub(super) fn this_lnk() -> anyhow::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    Ok(exe.canonicalize().unwrap_or(exe))
}

/// How the lnk at `exe` was installed, from where it lives.
pub(super) fn installed_how(exe: &Path) -> &'static str {
    let dir = exe.parent().unwrap_or(exe);
    if exe.components().any(|c| c.as_os_str() == "Cellar") {
        "Homebrew"
    } else if dir.ends_with(".cargo/bin") {
        "cargo install"
    } else if dir.ends_with("target/debug") || dir.ends_with("target/release") {
        "a build, not installed"
    } else {
        "installed"
    }
}

/// The other lnks on the PATH than `exe`, in PATH order.
pub(super) fn other_lnks(exe: &Path) -> Vec<PathBuf> {
    let name = format!("lnk{}", std::env::consts::EXE_SUFFIX);
    let mut found: Vec<PathBuf> = Vec::new();
    for dir in std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .filter(|d| d.is_absolute())
    {
        let path = dir.join(&name);
        let Ok(real) = path.canonicalize() else {
            continue;
        };
        if real.is_file() && real != exe && !found.contains(&real) {
            found.push(real);
        }
    }
    found
}

/// Replaces this lnk and its installed modules with a release, in place.
/// Nothing is installed unless the release's checksum list is signed by a
/// key built into this binary (`link_core::release`), so a tampered
/// release is refused. Nothing is downloaded when the release is the
/// version already installed, modules and all. Root's copies of module
/// programs follow ([`root_programs::follow`]), asked first in a terminal,
/// without asking with `yes`, and named as next steps otherwise.
pub(super) async fn upgrade(
    out: &mut Out,
    version: Option<link_version::Version>,
    yes: bool,
) -> Result<Done, Report> {
    let (exe, dir) = install_dir()?;
    let followed = release::Followed::here().map_err(|err| {
        failed(
            sentence!("The upgrade failed; the installed lnk was left unchanged."),
            err,
        )
    })?;
    if let Some(copy) = followed.filter(|f| f.lacks(version.as_ref())) {
        return Err(Report::error(sentence!(
            "{} {} follows this machine's copy of its release, in {}, which holds no other.",
            Product::new("lnk"),
            Version::new(env!("CARGO_PKG_VERSION")),
            link_format::clean(&copy.into_source().to_string())
        ))
        .next(
            cmd!("lnk box create <cloud> --bundle <bundle>"),
            phrase!("on your own machine, make the box again from a new bundle"),
        ));
    }
    let base = release::base(&release::Source::here()?, version.as_ref())?;
    out.block(format::Progress::new(phrase!(
        "Upgrading {} (currently {})",
        Shown::new(&exe),
        Version::new(env!("CARGO_PKG_VERSION"))
    )));
    let opening = out.activity(phrase!("Checking the release's signature"));
    let release = Release::open(&base, &release::trusted_signers()).await;
    drop(opening);
    let release = release.map_err(|err| {
        failed(
            sentence!("The upgrade failed; the installed lnk was left unchanged."),
            err,
        )
        .next(
            cmd!("lnk upgrade --check"),
            phrase!("see whether a newer release is out"),
        )
    })?;
    out.block(Note(sentence!("Signature verified.")));
    // The release's signed version, before any archive.
    let current = link_version::version();
    let said = release::check_installable(
        release
            .version()
            .await
            .within("reading the release's version")?,
        version.as_ref(),
        Some(&current),
    )?;
    let (done, put) = if said == current && current_here(&dir) {
        let done = Done::success(sentence!(
            "{} {} and its modules are this release already: nothing downloaded.",
            Product::new("lnk"),
            Version::new(current.to_string())
        ));
        // What an earlier upgrade left of its changes is finished: the
        // programs it renamed or removed, then the files.
        let history = link_cli::history::release();
        let incoming = link_cli::registry();
        let ids: Vec<&str> = incoming.keys().map(String::as_str).collect();
        let renamed: Vec<(String, String)> = history
            .renamed_in(&dir, &ids)
            .within("reading this release's renamed modules")?
            .into_iter()
            .map(|(program, now)| (program, now.to_string()))
            .collect();
        let removed: Vec<_> = history
            .removed_in(&dir, &ids)
            .into_iter()
            .map(|(program, removed)| (program, removed.clone()))
            .collect();
        retire(out, incoming, &dir, &renamed, &removed)
            .within("finishing an earlier upgrade's removals")?;
        match finish(out, &exe, &dir, yes)? {
            Some(plan) => (migrating::next(done, &plan), Vec::new()),
            None => return Ok(Done::success(sentence!("Nothing changed."))),
        }
    } else {
        match install(out, &release, &said, &exe, &dir, yes).await? {
            Some(installed) => installed,
            None => {
                return Ok(Done::success(sentence!(
                    "Nothing installed: {} {} and its modules are as they were.",
                    Product::new("lnk"),
                    Version::new(current.to_string())
                )));
            }
        }
    };
    // Those from outside Link, each from its own place, when no version
    // was asked for (which is Link's).
    if version.is_none() {
        outside::upgrade(out, &dir).await?;
    }
    outside::note_strays(out, &dir, &put)?;
    let left = root_programs::follow(out, &exe, &dir, yes)?;
    Ok(left.into_iter().fold(done, |done, command| {
        done.next(command, phrase!("bring root's copy to the new build"))
    }))
}

/// Whether every Link module in `dir` is this lnk's version, under its
/// own name: then a release of this version changes nothing.
fn current_here(dir: &Path) -> bool {
    let current = link_version::version();
    registry::installed(&registry::REGISTRY, dir)
        .into_iter()
        .all(|module| {
            version_of(&dir.join(&registry::REGISTRY[module].component.program))
                .is_ok_and(|found| found == current)
        })
}

/// The changes the installed programs still have to make, made after
/// the person's yes: the plan, `None` when they said no.
fn finish(out: &mut Out, exe: &Path, dir: &Path, yes: bool) -> Result<Option<Vec<Change>>, Report> {
    let programs: Vec<PathBuf> = std::iter::once(exe.to_path_buf())
        .chain(
            registry::installed(&registry::REGISTRY, dir)
                .into_iter()
                .map(|module| dir.join(&registry::REGISTRY[module].component.program)),
        )
        .collect();
    let unfinished = |err| failed(sentence!("Couldn't finish the files' changes."), err);
    let (plan, spoken) = migrating::plan(&programs).map_err(unfinished)?;
    if plan.is_empty() {
        // A transfer an earlier run started, and the person agreed to, is
        // finished though nothing else is planned.
        let roots = link_files::upgrade::roots().map_err(unfinished)?;
        if link_files::upgrade::under_way(&roots).map_err(unfinished)? {
            migrating::apply(out, &programs, &spoken, &plan).map_err(unfinished)?;
        }
        return Ok(Some(plan));
    }
    if !migrating::agreed(out, None, &plan, yes)? {
        return Ok(None);
    }
    migrating::apply(out, &programs, &spoken, &plan).map_err(unfinished)?;
    Ok(Some(plan))
}

/// Installs `release`, which says it is `said`, once the person agrees
/// to it and to what its programs will change: its core in place of
/// `exe`, and the modules in `dir` as its own registry selects them.
/// `None` when they said no, and nothing's installed.
async fn install(
    out: &mut Out,
    release: &Release,
    said: &link_version::Version,
    exe: &Path,
    dir: &Path,
    yes: bool,
) -> Result<Option<(Done, Vec<String>)>, Report> {
    // The incoming core supplies the incoming release's registry and migrations.
    let work = dir.join(format!("lnk-upgrade-{}", std::process::id()));
    std::fs::create_dir_all(&work).within("making a folder to upgrade in")?;
    let result = install_from(out, release, said, exe, dir, &work, yes).await;
    let _ = std::fs::remove_dir_all(&work);
    let Some((installed, plan)) = result? else {
        return Ok(None);
    };
    let done = Done::success(sentence!(
        "Installed {} {}, and its modules: {}.",
        Product::new("lnk"),
        Version::new(said.to_string()),
        Value::List(
            installed
                .iter()
                .map(|p| Value::from(module_name(p)))
                .collect()
        )
    ));
    Ok(Some((migrating::next(done, &plan), installed)))
}

/// [`install`], downloading into `work`: the modules it put and what
/// its programs planned.
async fn install_from(
    out: &mut Out,
    release: &Release,
    said: &link_version::Version,
    exe: &Path,
    dir: &Path,
    work: &Path,
    yes: bool,
) -> Result<Option<(Vec<String>, Vec<Change>)>, Report> {
    // A refusal naming a bridge release is answered by upgrading there
    // first: the same upgrade again would be refused again.
    let refused = |err: anyhow::Error| {
        let bridge = bridge(&err);
        let report = failed(
            sentence!("The upgrade failed; the downloaded work folder was removed."),
            err,
        );
        match bridge {
            Some(bridge) => report.next(
                lnk_command(["upgrade", &bridge]),
                phrase!("upgrade to the bridge release first"),
            ),
            None => report.next(cmd!("lnk upgrade"), phrase!("try the upgrade again")),
        }
    };
    let fetched = fetch(out, release, said, dir, work)
        .await
        .map_err(refused)?;
    let incoming = |module: &String| work.join(&fetched.incoming[module].component.program);
    let programs: Vec<PathBuf> = std::iter::once(work.join("lnk"))
        .chain(fetched.selected.iter().map(incoming))
        .collect();
    let (plan, spoken) = migrating::plan(&programs).map_err(refused)?;
    let installing = sentence!(
        "Installs {} {} and its modules: {}.",
        Product::new("lnk"),
        Version::new(said.to_string()),
        Value::List(
            fetched
                .selected
                .iter()
                .map(|p| Value::from(module_name(p)))
                .collect()
        )
    );
    // An older release reads no file a newer one wrote: its programs
    // name each such file in the plan, kept as it is, before the yes.
    if *said < link_version::version() {
        out.block(Note(sentence!(
            "{} {} came before this one: each file it can't read stays as it is, named below.",
            Product::new("lnk"),
            Version::new(said.to_string())
        )));
    }
    if !migrating::agreed(out, Some(installing), &plan, yes)? {
        return Ok(None);
    }
    // The new release shows the notes of every release since this one.
    link_core::update::upgrading_from(&link_version::version());
    put(out, &fetched, exe, dir, work).map_err(refused)?;
    let programs: Vec<PathBuf> = std::iter::once(exe.to_path_buf())
        .chain(
            fetched
                .selected
                .iter()
                .map(|module| dir.join(&fetched.incoming[module].component.program)),
        )
        .collect();
    migrating::apply(out, &programs, &spoken, &plan).map_err(|err| {
        failed(
            sentence!(
                "{} {} is installed, but its changes to your files didn't finish.",
                Product::new("lnk"),
                Version::new(said.to_string())
            ),
            err,
        )
        .next(cmd!("lnk upgrade"), phrase!("finish them"))
    })?;
    Ok(Some((fetched.selected, plan)))
}

/// A release downloaded into its work folder, not yet in place.
struct Fetched {
    /// Its registry.
    incoming: std::collections::BTreeMap<String, link_component::Resolved>,
    /// The modules it installs.
    selected: Vec<String>,
    /// Each module program renamed since, and its module's new ID.
    renamed: Vec<(String, String)>,
    /// Each installed program of a module the release no longer ships,
    /// and its removal.
    removed: Vec<(String, link_spec::history::WireRemoved)>,
}

/// Downloads `release` into `work`, its core and the modules it
/// installs, putting nothing in place.
async fn fetch(
    out: &mut Out,
    release: &Release,
    said: &link_version::Version,
    dir: &Path,
    work: &Path,
) -> anyhow::Result<Fetched> {
    let wait = out.activity(phrase!("Downloading the new lnk"));
    let archive = release.get(&release::program_asset("core")?).await?;
    drop(wait);
    release::unpack(&archive, work)?;
    let new = work.join("lnk");
    let new_version = version_of(&new)?;
    ensure!(
        new_version == *said,
        "the release is lnk {said} but holds lnk {new_version}"
    );
    let mut asking = link_secretive::process::command(&new);
    link_component::asking(&mut asking);
    let answer = link_core::answers::Said::shipped().output(
        asking
            .arg(link_cli::REGISTRY_FLAG)
            .stdin(std::process::Stdio::null()),
    )?;
    ensure!(
        answer.status.success(),
        "the downloaded core did not describe its release registry: {}",
        link_format::clean(String::from_utf8_lossy(&answer.stderr).trim())
    );
    let incoming = link_component::parse_registry(&answer.stdout)?;
    let history = incoming_history(&new)?;
    let mut installed = registry::installed(&incoming, dir);
    // A module renamed since: its new name installed in its place.
    let ids: Vec<&str> = incoming.keys().map(String::as_str).collect();
    let renamed = history.renamed_in(dir, &ids)?;
    for (_, now) in &renamed {
        if !installed.contains(now) {
            installed.push(incoming[*now].component.id.as_str());
        }
    }
    let selected = link_component::upgrade_programs(&incoming, &installed)?;
    let mut steps = format::Steps::new(selected.len());
    for module in &selected {
        out.step(
            &mut steps,
            phrase!("Downloading the {} module", module_name(module)),
        );
        let archive = release.get(&release::program_asset(module)?).await?;
        release::unpack(&archive, work)?;
        let program = &incoming[*module].component.program;
        ensure!(
            work.join(program).is_file(),
            "the release archive has no {program}"
        );
    }
    let selected = selected.iter().map(|s| s.to_string()).collect();
    let renamed = renamed
        .iter()
        .map(|(program, now)| (program.to_string(), now.to_string()))
        .collect();
    let removed = history
        .removed_in(dir, &ids)
        .into_iter()
        .map(|(program, removed)| (program, removed.clone()))
        .collect();
    Ok(Fetched {
        incoming,
        selected,
        renamed,
        removed,
    })
}

/// Puts a fetched release's programs in place, from `work`.
fn put(
    out: &mut Out,
    fetched: &Fetched,
    exe: &Path,
    dir: &Path,
    work: &Path,
) -> anyhow::Result<()> {
    let incoming = &fetched.incoming;
    for module in &fetched.selected {
        let program = &incoming[module].component.program;
        release::put(&work.join(program), &dir.join(program))?;
    }
    release::put(&work.join("lnk"), exe)?;
    retire(out, incoming, dir, &fetched.renamed, &fetched.removed)
}

/// Removes from `dir` each program `renamed` names, once its new name,
/// by `incoming`, is in place, and each program `removed` names, saying
/// why. Each is stopped first, through `lnk module remove`'s own path, so
/// nothing it runs is left with no program to stop it: one that can't
/// stop is kept, and named, for `lnk module remove` or a rerun to take.
/// One already gone is passed over, so a rerun finishes what an upgrade
/// stopped partway left.
fn retire(
    out: &mut Out,
    incoming: &std::collections::BTreeMap<String, link_component::Resolved>,
    dir: &Path,
    renamed: &[(String, String)],
    removed: &[(String, link_spec::history::WireRemoved)],
) -> anyhow::Result<()> {
    // What it said stopping it, once it's stopped and gone; none where it
    // was gone already or couldn't stop, which is said. One whose module
    // declares no removal keeps and runs nothing, so it isn't asked.
    let gone = |out: &mut Out,
                program: &str,
                why: &str,
                declared: bool|
     -> anyhow::Result<Option<crate::installing::Stopped>> {
        let path = dir.join(program);
        if !path.is_file() {
            return Ok(None);
        }
        let stopped = match declared {
            true => crate::installing::stop_module(&path),
            false => Ok(crate::installing::Stopped {
                kept: Vec::new(),
                stops: String::new(),
            }),
        };
        let stopped = match stopped {
            Ok(stopped) => stopped,
            Err(err) => {
                let module = program.strip_prefix("lnk-").unwrap_or(program);
                out.block(
                    Report::warning(sentence!(
                        "Kept {}: it couldn't stop what it runs.",
                        Product::new(program.to_string())
                    ))
                    .why(format::Prose::from(format::Sentence::adopt(
                        &link_format::clean(&format!("{err:#}")),
                    )))
                    .next(
                        lnk_command(["module", "remove", module]),
                        phrase!("remove it once it can stop"),
                    ),
                );
                return Ok(None);
            }
        };
        match std::fs::remove_file(&path) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            removed => removed
                .within(format!("removing {program}, {why}"))
                .map(|()| Some(stopped)),
        }
    };
    for (program, now) in renamed {
        let Some(resolved) = incoming.get(now) else {
            continue;
        };
        let new = &resolved.component.program;
        if !dir.join(new).is_file() {
            continue;
        }
        let declared = resolved.removal.is_some();
        let Some(stopped) = gone(out, program, &format!("now lnk-{now}"), declared)? else {
            continue;
        };
        out.block(Note(sentence!(
            "Replaced {} with {}, its new name.",
            Product::new(program.clone()),
            Product::new(new.clone())
        )));
        // What the old one ran stopped with it, and runs again once
        // started under the new name.
        if !stopped.stops.is_empty() {
            out.block(Note(format::Sentence::adopt(&stopped.stops)));
            out.block(Note(sentence!(
                "Start them again as the {} module: {} says how.",
                module_name(now),
                lnk_command([now.as_str(), "--help"])
            )));
        }
    }
    // A module the release no longer ships: uninstalled, saying why.
    for (program, removed) in removed {
        // The release no longer knows it, so it's asked to stop.
        let Some(stopped) = gone(out, program, "a module removed", true)? else {
            continue;
        };
        let kept = stopped.kept;
        out.block(Note(sentence!(
            "Removed {}: the {} module is removed since {}.",
            Product::new(program.clone()),
            link_format::clean(&removed.module),
            Version::new(removed.since.to_string())
        )));
        out.block(Note(format::Sentence::adopt(&link_format::clean(
            &removed.why,
        ))));
        if !kept.is_empty() {
            out.block(Note(sentence!(
                "Its {} stay: {} deletes them.",
                Value::List(kept.iter().map(|k| Value::from(k.as_str())).collect()),
                cmd!("lnk uninstall")
            )));
        }
    }
    Ok(())
}

/// The bridge release a protocol refusal in `err` names, the one to
/// upgrade to first, if it's one.
fn bridge(err: &anyhow::Error) -> Option<String> {
    use link_version::protocol::Older;
    err.chain().find_map(|cause| {
        let older = cause.downcast_ref::<Older>().or_else(|| {
            match cause.downcast_ref::<link_version::Unread>() {
                Some(link_version::Unread::Older(older)) => Some(older),
                _ => None,
            }
        });
        older.and_then(|older| older.bridge.clone())
    })
}

/// The incoming core's command history, for the modules it renames and
/// removes.
fn incoming_history(new: &Path) -> anyhow::Result<link_cli::history::Wire> {
    let mut asking = link_secretive::process::command(new);
    link_spec::history::asking(&mut asking);
    let answer = link_core::answers::Said::shipped().output(
        asking
            .arg(link_cli::HISTORY_FLAG)
            .stdin(std::process::Stdio::null()),
    )?;
    ensure!(
        answer.status.success(),
        "the downloaded core did not describe its command history: {}",
        link_format::clean(String::from_utf8_lossy(&answer.stderr).trim())
    );
    let history = link_cli::history::Wire::parse("the downloaded lnk", &answer.stdout)?;
    for renamed in &history.renamed {
        history.name(&renamed.module)?;
    }
    Ok(history)
}

/// `lnk upgrade --check`: whether a newer `lnk` is out ([`check_core`]),
/// and a newer release of each module added from outside Link
/// ([`outside::check`]). Installs nothing. Each is checked whatever the
/// other found, and the check fails when any of them couldn't be made,
/// `lnk`'s own refusal first.
pub(super) async fn check(out: &mut Out) -> Result<Done, Report> {
    let core = check_core().await;
    let modules = match install_dir() {
        Ok((_, dir)) => outside::check(out, &dir).await,
        Err(err) => Err(err),
    };
    checked(core, modules)
}

/// What `lnk upgrade --check` ends with, from `lnk`'s own check and how
/// many modules couldn't be checked: `lnk`'s refusal first, then why the
/// modules couldn't be listed, else one naming how many weren't checked,
/// else what `lnk`'s check found.
fn checked(core: Result<Done, Report>, modules: Result<usize, Report>) -> Result<Done, Report> {
    let done = core?;
    let unchecked = modules?;
    if unchecked > 0 {
        return Err(Report::error(sentence!(
            "Couldn't check {} for a newer release.",
            Count::new(unchecked as u64, "module", "modules")
        ))
        .next(cmd!("lnk upgrade --check"), phrase!("try the check again")));
    }
    Ok(done)
}

/// Whether a newer `lnk` is out, by the latest release's signed
/// `VERSION`, written down for the notice `lnk up` and other
/// participating commands give (`link_core::update`). An `lnk`
/// following this machine's copy of its own release, as on a box made
/// from your own build, has nothing later to check for, and says so.
async fn check_core() -> Result<Done, Report> {
    let current = link_version::version();
    let followed = release::Followed::here()
        .map_err(|err| failed(sentence!("Couldn't check for a newer lnk."), err))?;
    if let Some(release::Followed::Copy(copy)) = followed {
        link_core::update::record_release(None);
        return Ok(Done::success(sentence!(
            "{} {} follows this machine's copy of its release, in {}, which holds no later one.",
            Product::new("lnk"),
            Version::new(current.to_string()),
            link_format::clean(&copy.to_string())
        )));
    }
    let latest = async {
        let base = release::base(&release::Source::here()?, None)?;
        let release = Release::open(&base, &release::trusted_signers()).await?;
        let version = release.version().await?;
        let advisories = release.advisories().await?.affecting(&current);
        anyhow::Ok(version.map(|version| (version, advisories)))
    };
    let latest = tokio::time::timeout(std::time::Duration::from_secs(30), latest).await;
    let latest = match latest {
        Ok(latest) => latest,
        Err(_) => Err(anyhow::anyhow!(
            "the release host didn't answer in 30 seconds"
        )),
    };
    link_core::update::record_release(latest.as_ref().ok().cloned().flatten());
    let latest = latest.map_err(|err| {
        link_core::update::notice(true);
        failed(sentence!("Couldn't check for a newer lnk."), err)
            .next(cmd!("lnk upgrade --check"), phrase!("try the check again"))
    })?;
    if let Some((_, advisories)) = &latest {
        for advisory in advisories {
            link_format::aside(Note(sentence!(
                "{} advisory {}: {} Fixed in lnk {}. Run {}.",
                advisory.level.to_string(),
                link_format::clean(&advisory.id),
                link_format::clean(&advisory.summary),
                Version::new(advisory.fixed.to_string()),
                cmd!("lnk upgrade")
            )));
        }
    }
    Ok(match latest.map(|(version, _)| version) {
        Some(latest) if latest > current => Done::success(sentence!(
            "{} {} is out (you have {}).",
            Product::new("lnk"),
            Version::new(latest.to_string()),
            Version::new(current.to_string())
        ))
        .next(cmd!("lnk upgrade"), phrase!("install it")),
        Some(_) => Done::success(sentence!(
            "{} {} is the latest.",
            Product::new("lnk"),
            Version::new(current.to_string())
        )),
        None => Done::success(sentence!("The latest release doesn't say its version."))
            .next(cmd!("lnk upgrade"), phrase!("fetch it")),
    })
}

/// `0.0.1` from a downloaded program's `--version` line: `lnk 0.0.1` (lnk
/// and the modules, whose commands are named `lnk`) or `lnk-<name> 0.0.1`.
pub(super) fn version_of(binary: &std::path::Path) -> anyhow::Result<link_version::Version> {
    let output = link_secretive::process::command(binary)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .within(format!("running the downloaded {}", binary.display()))?;
    let text = String::from_utf8_lossy(&output.stdout);
    parse_version_line(&text)
        .filter(|_| output.status.success())
        .and_then(|v| v.parse().ok())
        .with_context(|| {
            format!(
                "the downloaded {} reported {:?}",
                binary.display(),
                link_format::clean(text.trim())
            )
        })
}

/// The version in a `lnk 0.0.1` or `lnk-<name> 0.0.1` line.
fn parse_version_line(text: &str) -> Option<String> {
    // The first line: lnk's own then says where it runs from.
    let (name, version) = text.trim().lines().next()?.split_once(' ')?;
    (name == "lnk" || name.starts_with("lnk-")).then(|| version.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The check fails when any part of it couldn't be made: `lnk`'s own
    /// refusal is what it ends with, then the modules'; with every part
    /// made, it's what `lnk`'s check found.
    #[test]
    fn a_check_that_could_not_be_made_fails_the_whole() {
        let refused = || Err(Report::error(sentence!("Couldn't check for a newer lnk.")));
        let latest = || Ok(Done::success(sentence!("Nothing newer is out.")));
        let headline = |result: Result<Done, Report>| result.unwrap_err().headline.to_string();
        let unlisted = || Err(Report::error(sentence!("Couldn't read the modules.")));
        assert_eq!(
            headline(checked(refused(), Ok(0))),
            "Couldn't check for a newer lnk."
        );
        assert_eq!(
            headline(checked(refused(), Ok(2))),
            "Couldn't check for a newer lnk."
        );
        assert_eq!(
            headline(checked(refused(), unlisted())),
            "Couldn't check for a newer lnk."
        );
        assert_eq!(
            headline(checked(latest(), unlisted())),
            "Couldn't read the modules."
        );
        assert_eq!(
            headline(checked(latest(), Ok(2))),
            "Couldn't check 2 modules for a newer release."
        );
        assert!(checked(latest(), Ok(0)).is_ok());
    }

    /// A module the release removes is stopped through `lnk module
    /// remove`'s own path before its program goes, so nothing it runs is
    /// left with no program to stop it; one that can't stop is kept, and
    /// named with the command that removes it later.
    #[cfg(unix)]
    #[test]
    fn a_retired_module_is_stopped_before_its_program_goes() {
        let dir = link_test_kit::TempDir::new("upgrade-retire");
        let stopped = dir.join("stopped");
        let module = |name: &str, stop: &str| {
            let script = format!(
                "#!/bin/sh\ncase \"$*\" in\n\
                 'uninstall --parts --json') echo '{{\"parts\": []}}' ;;\n\
                 uninstall) {stop} ;;\nesac\n"
            );
            link_test_kit::write_program(&dir.join(name), script).unwrap();
        };
        module(
            "lnk-hooks",
            &format!("echo hooks >> '{}'", stopped.display()),
        );
        module("lnk-stuck", "echo 'its service is busy' >&2; exit 1");
        // One that can't list what it keeps can't be told to stop.
        link_test_kit::write_program(
            &dir.join("lnk-mute"),
            format!(
                "#!/bin/sh
case \"$*\" in\nuninstall) echo mute >> '{}' ;;\n*) exit 1 ;;\nesac\n",
                stopped.display()
            ),
        )
        .unwrap();
        let removal = |module: &str| link_spec::history::WireRemoved {
            module: module.into(),
            why: "Use lnk webhook-connection.".into(),
            since: link_version::Version::new(1, 1, 0),
        };
        let removed = [
            ("lnk-hooks".to_string(), removal("hooks")),
            ("lnk-stuck".to_string(), removal("stuck")),
            ("lnk-mute".to_string(), removal("mute")),
        ];
        let (mut out, captured) = Out::capture(false, &["lnk", "upgrade"]);
        retire(
            &mut out,
            &std::collections::BTreeMap::new(),
            &dir,
            &[],
            &removed,
        )
        .unwrap();
        drop(out);
        assert_eq!(std::fs::read_to_string(&stopped).unwrap(), "hooks\n");
        assert!(!dir.join("lnk-hooks").exists());
        assert!(dir.join("lnk-stuck").is_file());
        let said = format!("{}{}", captured.stdout(), captured.stderr());
        assert!(said.contains("Kept lnk-stuck"), "{said}");
        assert!(said.contains("its service is busy"), "{said}");
        assert!(said.contains("lnk module remove stuck"), "{said}");
        assert!(dir.join("lnk-mute").is_file());
        assert!(said.contains("lnk module remove mute"), "{said}");
    }

    /// A refusal naming a bridge release names the upgrade to it, not the
    /// same upgrade again; another names none.
    #[test]
    fn a_bridge_refusal_names_the_bridge_release() {
        let protocol = link_version::Protocol::parse(
            "name = \"upgrade\"\ncurrent = 3\noldest = 2\nupgrade = \"lnk upgrade\"\n\
             bridge = \"0.0.20\"\n",
        )
        .unwrap();
        let refused = anyhow::Error::new(protocol.read("the downloaded lnk", 1).unwrap_err())
            .context("asking lnk-hooks what it changes");
        assert_eq!(bridge(&refused).as_deref(), Some("0.0.20"));
        let older = protocol.accept("the downloaded lnk", 1).unwrap_err();
        assert_eq!(
            bridge(&anyhow::Error::new(older)).as_deref(),
            Some("0.0.20")
        );
        assert_eq!(bridge(&anyhow::anyhow!("the network went away")), None);
    }

    #[test]
    fn version_lines() {
        assert_eq!(parse_version_line("lnk 0.0.2\n").as_deref(), Some("0.0.2"));
        assert_eq!(
            parse_version_line("lnk 0.0.2\n  /usr/local/bin/lnk (installed)\n").as_deref(),
            Some("0.0.2")
        );
        assert_eq!(
            parse_version_line("lnk-otter 0.0.2").as_deref(),
            Some("0.0.2")
        );
        assert_eq!(parse_version_line("other 0.0.2"), None);
        assert_eq!(parse_version_line("lnk"), None);
        assert_eq!(parse_version_line(""), None);
    }
}
