//! `lnk uninstall`: lnk and its modules always go; what they kept (your
//! agents, your files, your keys) goes only when you pick it, one part at a
//! time, since some of it can't be made again.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

use link_format::{
    Command, Done, Exit, Foreign, List, Name, Node, Note, Out, Path as Shown, Product, Prose,
    Question, Report, Section, Sentence, Text, Tree, Value, WithinExt, cmd, phrase, prose,
    sentence,
};

use crate::{failed, registry::MODULES};

/// Something Link keeps, which `lnk uninstall` removes only when asked:
/// a part as the modules keeping it say (`lnk-<module> uninstall --parts
/// --json`), those of one name merged, or `rest`, what none claims.
struct Part {
    name: String,
    /// What removing it loses, for the question.
    about: String,
    paths: Vec<PathBuf>,
    /// Keychain accounts: exact, or a prefix ending in `/`.
    keys: Vec<String>,
    /// How to keep a copy first.
    first: Vec<String>,
}

impl Part {
    fn owns_key(&self, account: &str) -> bool {
        self.keys.iter().any(|k| match k.strip_suffix('/') {
            Some(prefix) => account.starts_with(&format!("{prefix}/")),
            None => account == k,
        })
    }
}

/// An installed module that answers `uninstall`, and what it keeps.
struct Keeper {
    program: PathBuf,
    kept: link_core::uninstall::Kept,
}

/// What `program` keeps (`uninstall --parts --json`); none for a module
/// that doesn't answer it: it keeps nothing and runs nothing the core
/// knows of.
fn ask_module(program: &Path) -> Option<Keeper> {
    let output = link_secretive::process::command(program)
        .args(["uninstall", "--parts", "--json"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let kept = serde_json::from_slice(&output.stdout).ok()?;
    Some(Keeper {
        program: program.to_path_buf(),
        kept,
    })
}

/// Whether Link's module `module` answers `uninstall`, as its
/// `module.toml` declares with `[removal]`: only those are asked, so
/// listing what Link keeps starts none that keeps nothing. One naming no
/// part is asked too: its uninstall stops what it runs.
pub(crate) fn answers_uninstall(module: &str) -> bool {
    crate::registry::REGISTRY
        .get(module)
        .is_some_and(|declared| declared.removal.is_some())
}

/// Whether `path` is inside one of `roots`, not one of them, as written:
/// absolute, and without a `..` (which `starts_with` doesn't
/// resolve, so `~/.lnk/../../x` would pass).
fn written_inside(path: &Path, roots: &[&Path]) -> bool {
    use std::path::Component;
    path.components()
        .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
        && roots.iter().any(|r| path.starts_with(r) && path != *r)
}

/// Whether `path` is inside one of `roots` on disk: its folder, links
/// followed, is in one of theirs, so a link in the middle of the path
/// can't send a removal elsewhere. `path` itself is never followed.
fn really_inside(path: &Path, roots: &[&Path]) -> bool {
    let Some(parent) = path.parent().and_then(|p| p.canonicalize().ok()) else {
        return false;
    };
    roots
        .iter()
        .filter_map(|r| r.canonicalize().ok())
        .any(|r| parent.starts_with(r))
}

/// The parts `keepers` keep, merged by name in the order they come, then
/// `rest`. A path outside Link's two folders is left out: `rest` is only
/// ever what's in them.
fn parts_of(keepers: &[Keeper], roots: &[&Path]) -> Vec<Part> {
    let mut parts: Vec<Part> = Vec::new();
    for part in keepers.iter().flat_map(|k| &k.kept.parts) {
        let paths = part
            .paths
            .iter()
            .filter(|p| written_inside(p, roots))
            .cloned();
        match parts.iter_mut().find(|p| p.name == part.name) {
            Some(merged) => {
                merged.paths.extend(paths);
                merged.keys.extend(part.keys.iter().cloned());
                merged
                    .first
                    .extend(Some(part.first.clone()).filter(|f| !f.is_empty()));
            }
            None => parts.push(Part {
                name: part.name.clone(),
                about: part.about.clone(),
                paths: paths.collect(),
                keys: part.keys.clone(),
                first: Some(part.first.clone())
                    .into_iter()
                    .filter(|f| !f.is_empty())
                    .collect(),
            }),
        }
    }
    parts.retain(|p| p.name != "rest" && p.name != "all");
    parts.push(Part {
        name: "rest".into(),
        about: "everything else Link keeps: unclaimed settings \
                and files, and what a module no longer here kept"
            .into(),
        paths: Vec::new(),
        keys: Vec::new(),
        first: Vec::new(),
    });
    parts
}

/// Where Link keeps things, and what of each part is there.
struct Kept {
    parts: Vec<Part>,
    settings: PathBuf,
    files: PathBuf,
    /// Link's Keychain items, or why they couldn't be listed.
    keys: Result<Vec<String>, String>,
}

impl Kept {
    fn rest(&self) -> usize {
        self.parts.len() - 1
    }

    /// The paths of `part` that exist, each inside Link's folders on disk.
    fn paths(&self, part: usize) -> Vec<PathBuf> {
        if part == self.rest() {
            let claimed: Vec<&PathBuf> = self.parts.iter().flat_map(|p| &p.paths).collect();
            return [&self.settings, &self.files]
                .into_iter()
                .flat_map(|root| std::fs::read_dir(root).into_iter().flatten().flatten())
                .map(|entry| entry.path())
                .filter(|entry| {
                    !claimed
                        .iter()
                        .any(|c| c.starts_with(entry) || entry.starts_with(c))
                })
                .collect();
        }
        self.parts[part]
            .paths
            .iter()
            .filter(|p| p.symlink_metadata().is_ok())
            .filter(|p| really_inside(p, &[&self.settings, &self.files]))
            .cloned()
            .collect()
    }

    /// The Keychain accounts of `part`.
    fn keys(&self, part: usize) -> Vec<String> {
        let Ok(keys) = &self.keys else {
            return Vec::new();
        };
        let rest = self.rest();
        let owner = |account: &str| self.parts[..rest].iter().position(|p| p.owns_key(account));
        keys.iter()
            .filter(|k| owner(k).unwrap_or(rest) == part)
            .cloned()
            .collect()
    }

    fn exists(&self, part: usize) -> bool {
        !self.paths(part).is_empty() || !self.keys(part).is_empty()
    }
}

/// The parts Link's modules keep, by name: one whose module is gone is
/// passed over, any other word refused.
fn known_parts() -> std::collections::BTreeSet<&'static str> {
    std::iter::once("rest")
        .chain(
            crate::registry::REGISTRY
                .values()
                .filter_map(|d| d.removal.as_ref())
                .flat_map(|r| r.parts.iter().map(String::as_str)),
        )
        .collect()
}

/// The parts `words` name (`agents`, `personal`, ...), comma- or
/// space-separated, of `parts`.
fn named(out: &mut Out, parts: &[Part], words: &[String]) -> Result<Vec<usize>, Report> {
    let mut chosen = Vec::new();
    for word in words.iter().flat_map(|w| w.split([',', ' '])) {
        let word = word.trim();
        if word.is_empty() {
            continue;
        }
        let found = parts.iter().position(|p| p.name == word).or_else(|| {
            word.parse::<usize>()
                .ok()
                .filter(|n| (1..=parts.len()).contains(n))
                .map(|n| n - 1)
        });
        let names: Vec<Value> = parts
            .iter()
            .map(|p| Value::from(Name::new("part", p.name.as_str())))
            .collect();
        // A word that's no part at all, a typo: refused before anything
        // goes, rather than uninstall and keep what was meant.
        if found.is_none() && !known_parts().contains(word) {
            return Err(
                Report::error(sentence!("There is no part {}.", Foreign::new(word)))
                    .why(prose!("The parts are {}, or --all.", Value::List(names)))
                    .next(
                        cmd!("lnk uninstall --delete <part>"),
                        phrase!("delete one of them"),
                    )
                    .exit(Exit::Usage),
            );
        }
        // One of Link's parts no installed module keeps (its module's
        // gone, what it kept then in `rest`): nothing to delete by that
        // name.
        let Some(i) = found else {
            out.block(
                Report::warning(sentence!(
                    "No installed module keeps {}, so nothing is deleted by that name.",
                    Foreign::new(word)
                ))
                .why(prose!("The parts are {}, or --all.", Value::List(names))),
            );
            continue;
        };
        if !chosen.contains(&i) {
            chosen.push(i);
        }
    }
    Ok(chosen)
}

/// The modules in lnk's folder `dir`: `lnk-<name>`, Link's and any other.
/// Only Link's own (in `MODULES`) are run before the user says yes.
fn modules_in(dir: &Path) -> Vec<String> {
    let mut modules: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let name = name
                .strip_suffix(std::env::consts::EXE_SUFFIX)
                .unwrap_or(&name);
            name.strip_prefix("lnk-").map(str::to_string)
        })
        .filter(|p| {
            !p.is_empty()
                && p.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
        .collect();
    // Link's own in `MODULES`' order (the harness, which stops the agents,
    // first), then any other.
    modules.sort_by_key(|p| {
        (
            MODULES
                .iter()
                .position(|(known, _)| known == p)
                .unwrap_or(usize::MAX),
            p.clone(),
        )
    });
    modules
}

/// `lnk uninstall --parts --json`: what each of Link's installed modules
/// keeps, by part, as each says (`lnk-<module> uninstall --parts
/// --json`), one after another, unmerged. Only Link's own are asked, as
/// `lnk uninstall` asks them before its question: any other `lnk-*` is
/// someone else's program. Read by callers that need the installed data inventory.
pub fn parts() -> anyhow::Result<link_core::uninstall::Kept> {
    let exe = std::env::current_exe().within("finding the lnk binary")?;
    let exe = exe.canonicalize().unwrap_or(exe);
    let dir = exe.parent().within("finding lnk's folder")?;
    let program = |p: &str| dir.join(format!("lnk-{p}{}", std::env::consts::EXE_SUFFIX));
    let parts = modules_in(dir)
        .iter()
        .filter(|p| MODULES.iter().any(|(known, _)| known == p))
        .filter(|p| answers_uninstall(p))
        .filter_map(|p| ask_module(&program(p)))
        .flat_map(|keeper| keeper.kept.parts)
        .collect();
    Ok(link_core::uninstall::Kept {
        parts,
        ..Default::default()
    })
}

/// `lnk uninstall`: each module stops its services (and deletes what of it
/// was picked), what no module claims goes when `rest` is picked, then
/// the programs.
pub fn uninstall(out: &mut Out, yes: bool, delete: &[String], all: bool) -> Result<Done, Report> {
    // Without HOME the machine and Link folders would be relative ones,
    // and whatever `./.lnk` or `./Link` is here would be deleted.
    let home = |err: anyhow::Error| {
        failed(
            sentence!(
                "Nothing was removed: HOME isn't set, so Link can't tell where its files are."
            ),
            err,
        )
    };
    let settings = link_files::try_installation_dir().map_err(home)?;
    let files = link_settings::try_link_dir().map_err(home)?;
    let exe = std::env::current_exe().within("finding the lnk binary")?;
    let exe = exe.canonicalize().unwrap_or(exe);
    let dir = exe.parent().within("finding lnk's folder")?.to_path_buf();
    let modules = modules_in(&dir);
    let cargo = dir.ends_with(".cargo/bin");
    let brew = exe.components().any(|c| c.as_os_str() == "Cellar");
    let program = |p: &str| dir.join(format!("lnk-{p}{}", std::env::consts::EXE_SUFFIX));
    // Only Link's own are asked what they keep before the question: any
    // other `lnk-*` here is someone else's program, run only once the
    // user has seen its name and said yes.
    let (ours, others): (Vec<&String>, Vec<&String>) = modules
        .iter()
        .partition(|p| MODULES.iter().any(|(known, _)| known == p));
    let keepers: Vec<Keeper> = ours
        .iter()
        .filter(|p| answers_uninstall(p))
        .filter_map(|p| ask_module(&program(p)))
        .collect();
    let kept = Kept {
        parts: parts_of(&keepers, &[&settings, &files]),
        settings,
        files,
        keys: link_keychain::accounts().map_err(|err| format!("{err:#}")),
    };
    let mut chosen = match all {
        true => (0..kept.parts.len()).collect(),
        false => named(out, &kept.parts, delete)?,
    };
    let programs = std::iter::once("lnk".to_string())
        .chain(ours.iter().map(|p| format!("lnk-{p}")))
        .fold(
            List::new(sentence!("No programs are installed.")),
            |list, p| list.value(Product::new(p)),
        );

    let mut intro = Section::new(phrase!("This uninstall"));
    intro = intro.with(Text(if brew {
        prose!("This removes Link's background services; then Homebrew removes lnk.")
    } else if cargo {
        prose!(
            "This removes Link's background services, and lnk and its modules in {}: what cargo installed with {}, which it prints, the rest itself.",
            Shown::new(&dir),
            cmd!("cargo uninstall")
        )
    } else {
        prose!(
            "This removes lnk and its modules in {}.",
            Shown::new(&dir)
        )
    }));
    if !brew {
        intro = intro.with(programs);
    }
    if !brew && !others.is_empty() {
        let programs = others.iter().fold(
            List::new(sentence!("No other programs are installed.")),
            |list, p| list.value(Product::new(format!("lnk-{p}"))),
        );
        intro = intro
            .with(Text(prose!(
                "These aren't Link's; each is told to uninstall first."
            )))
            .with(programs);
    }
    if let Some(stops) = notices(&keepers, |kept| &kept.stops) {
        intro = intro.with(stops);
    }
    out.now(intro);
    // What's here to delete.
    let here: Vec<usize> = (0..kept.parts.len()).filter(|&i| kept.exists(i)).collect();
    if chosen.is_empty() && !yes && out.can_ask() && !here.is_empty() {
        out.now(Text(prose!(
            "What else should go? Each part stays unless you pick it."
        )));
        for &i in &here {
            let part = &kept.parts[i];
            let yes = out.ask(
                Question::confirm(
                    sentence!("Delete {} for good?", Name::new("part", part.name.as_str())),
                    false,
                )
                .why(Prose::from(Sentence::adopt(&part.about))),
            )?;
            if yes {
                chosen.push(i);
            }
        }
    }
    chosen.retain(|i| here.contains(i));
    chosen.sort();
    chosen.dedup();
    let picked = |name: &str| chosen.iter().any(|&i| kept.parts[i].name == name);

    let mut goes = Section::new(phrase!("What goes"));
    if chosen.is_empty() {
        goes = goes.with(Note(sentence!(
            "Nothing else goes: reinstalling lnk picks up everything it kept."
        )));
    } else {
        let mut tree = Tree::new(sentence!("Nothing is deleted."));
        for &i in &chosen {
            let part = &kept.parts[i];
            // Each path, `rest`'s too (whatever else is in ~/Link and
            // ~/.lnk, which may be the user's own): named, so
            // nothing goes unseen.
            let mut node = Node::new([
                Value::from(Name::new("part", part.name.as_str())),
                Value::from(part.about.as_str()),
            ]);
            for path in kept.paths(i) {
                node = node.child(Node::new([Value::from(Shown::new(path))]));
            }
            tree = tree.root(node);
        }
        goes = goes
            .with(Text(prose!("This also deletes these, for good.")))
            .with(tree);
        for &i in &chosen {
            for first in &kept.parts[i].first {
                goes = goes.with(Note(Sentence::adopt(first)));
            }
        }
    }
    let kept_parts: Vec<Value> = here
        .iter()
        .filter(|i| !chosen.contains(i))
        .map(|&i| Value::from(Name::new("part", kept.parts[i].name.as_str())))
        .collect();
    if !kept_parts.is_empty() {
        goes = goes.with(Note(sentence!("Kept: {}.", Value::List(kept_parts))));
    }
    out.now(goes);
    if !out.ask(Question::confirm(sentence!("Uninstall?"), false).yes(yes))? {
        out.block(Note(sentence!("Nothing was removed.")));
        return Ok(Done::shown());
    }

    // Each module, while it's still here: its services stop, since its
    // program is going, and what of it was picked goes (a harness with
    // everything of it outside its home). Another's `lnk-*` that answers
    // is only told to stop: none of its parts were offered.
    let foreign: Vec<Keeper> = if brew {
        Vec::new()
    } else {
        others
            .iter()
            .filter_map(|p| ask_module(&program(p)))
            .collect()
    };
    for (keeper, ours) in keepers
        .iter()
        .map(|k| (k, true))
        .chain(foreign.iter().map(|k| (k, false)))
    {
        let mut args = vec!["uninstall".to_string()];
        for part in keeper.kept.parts.iter().filter(|_| ours) {
            if picked(&part.name) && !args.contains(&part.name) {
                args.extend(["--delete".to_string(), part.name.clone()]);
            }
        }
        let output = link_secretive::process::command(&keeper.program)
            .args(&args)
            .stdin(Stdio::null())
            .output();
        let failed = match output {
            Ok(output) if output.status.success() => None,
            Ok(output) => Some(String::from_utf8_lossy(&output.stderr).trim().to_string()),
            Err(err) => Some(err.to_string()),
        };
        if let Some(why) = failed {
            let command = Command::typed(
                std::iter::once(keeper.program.display().to_string()).chain(args.iter().cloned()),
            );
            out.block(
                Report::warning(sentence!("{} failed; carrying on.", command))
                    .why(Prose::from(Sentence::adopt(&why))),
            );
        }
    }
    // What the modules didn't take: what's left of a picked part, and
    // `rest`, which no module claims.
    for &i in &chosen {
        for path in kept.paths(i) {
            remove(out, &path);
        }
        for account in kept.keys(i) {
            if let Err(err) = link_keychain::delete(&account) {
                out.block(
                    Report::warning(sentence!(
                        "Couldn't remove the Keychain item {}.",
                        Product::new(format!("link:{account}"))
                    ))
                    .why(Prose::from(Sentence::adopt(&format!("{err:#}"))))
                    .cause(err),
                );
            }
        }
        out.block(Note(sentence!(
            "Deleted {}.",
            Name::new("part", kept.parts[i].name.as_str())
        )));
    }
    if let Err(err) = &kept.keys
        && !chosen.is_empty()
    {
        out.block(
            Report::warning(sentence!(
                "Couldn't list Link's Keychain items, so they stay."
            ))
            .why(Prose::from(Sentence::adopt(&format!(
                "{err}. Keychain Access shows them as link:…"
            )))),
        );
    }
    // Folders left empty go too: ~/Link/Archive once its parts went.
    let archive = link_files::layout::Home::Archive.at(&kept.files);
    for root in [&archive, &kept.settings, &kept.files] {
        let _ = std::fs::remove_dir(root);
    }

    let mut done = Done::shown();
    if cargo {
        // Cargo's own names for what it installed; what it doesn't track
        // (copied in by hand) goes here and now.
        let programs: Vec<String> = std::iter::once("lnk".to_string())
            .chain(modules.iter().map(|p| format!("lnk-{p}")))
            .collect();
        let listed = link_secretive::process::command("cargo")
            .args(["install", "--list"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();
        let (crates, untracked) = cargo_crates(&listed, &programs);
        for program in &untracked {
            remove(out, &dir.join(program));
        }
        if crates.is_empty() {
            done = Done::success(sentence!("{} is uninstalled.", Product::new("lnk")));
        } else {
            out.block(Note(sentence!(
                "{} was installed with cargo.",
                Product::new("lnk")
            )));
            done = done.next(
                Command::typed(
                    ["cargo".to_string(), "uninstall".to_string()]
                        .into_iter()
                        .chain(crates),
                ),
                phrase!("finish removing lnk"),
            );
        }
    } else if brew {
        out.block(Note(sentence!(
            "{} was installed with Homebrew.",
            Product::new("lnk")
        )));
        done = done.next(cmd!("brew uninstall lnk"), phrase!("finish removing lnk"));
    } else {
        for module in &modules {
            remove(out, &program(module));
        }
        remove(out, &exe);
        done = Done::success(sentence!("{} is uninstalled.", Product::new("lnk")));
    }
    if let Some(stays) = notices(&keepers, |kept| &kept.stays) {
        out.now(stays);
    }
    Ok(done)
}

/// Each distinct module notice once, in discovery order, as a list.
fn notices(
    keepers: &[Keeper],
    notice: impl Fn(&link_core::uninstall::Kept) -> &str,
) -> Option<List> {
    let mut seen = std::collections::BTreeSet::new();
    let mut list = List::new(sentence!("No additional effects."));
    for keeper in keepers {
        let line = link_format::clean(notice(&keeper.kept));
        let line = line.trim();
        if !line.is_empty() && seen.insert(line.to_owned()) {
            list = list.item(Prose::from(Sentence::adopt(line)));
        }
    }
    (!list.items.is_empty()).then_some(list)
}

#[cfg(test)]
#[test]
fn repeated_channel_notices_are_shown_once() {
    let keepers: Vec<_> = [
        "The bridge stops.",
        "The bridge stops.",
        "Other services stop.",
        "",
    ]
    .into_iter()
    .map(|stops| Keeper {
        program: PathBuf::new(),
        kept: link_core::uninstall::Kept {
            stops: stops.into(),
            ..Default::default()
        },
    })
    .collect();
    let list = notices(&keepers, |kept| &kept.stops).unwrap();
    assert_eq!(
        link_format::render::block(&list.into(), &link_format::render::Ctx::plain()),
        "  - The bridge stops.\n  - Other services stop.\n"
    );
    assert!(notices(&keepers, |kept| &kept.stays).is_none());
}

/// Removes `path`, a warning when it can't be.
fn remove(out: &mut Out, path: &Path) {
    if let Err(err) = link_core::uninstall::remove(path) {
        out.block(
            Report::warning(sentence!("Couldn't remove {}.", Shown::new(path)))
                .why(Prose::from(Sentence::adopt(&format!("{err}"))))
                .cause(anyhow::anyhow!("{err}")),
        );
    }
}

/// Of `programs`, the crates `cargo install --list` (`listed`) installed
/// them from, and the programs it doesn't track.
fn cargo_crates(listed: &str, programs: &[String]) -> (Vec<String>, Vec<String>) {
    let mut crates = Vec::new();
    let mut tracked = Vec::new();
    let mut current: Option<&str> = None;
    for line in listed.lines() {
        if line.starts_with(char::is_whitespace) {
            let program = line.trim();
            if let Some(name) = current
                && programs.iter().any(|p| p == program)
            {
                tracked.push(program.to_string());
                if !crates.iter().any(|c| c == name) {
                    crates.push(name.to_string());
                }
            }
        } else {
            current = line.split_whitespace().next();
        }
    }
    let untracked = programs
        .iter()
        .filter(|p| !tracked.contains(p))
        .cloned()
        .collect();
    (crates, untracked)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parts as three modules say them: two clouds merged into one part.
    fn keepers() -> Vec<Keeper> {
        let kept = |json: serde_json::Value| Keeper {
            program: PathBuf::from("/x"),
            kept: serde_json::from_value(json).unwrap(),
        };
        vec![
            kept(
                serde_json::json!({"parts": [{"name": "agents", "about": "your agents",
                "paths": ["/s/agents", "/f/Agents"], "keys": ["agent/"],
                "first": "back up first"}], "stops": "Your agents stop: main."}),
            ),
            kept(serde_json::json!({"parts": [
                {"name": "personal", "about": "your files", "paths": ["/f/Personal"]},
                {"name": "buckets", "about": "buckets", "paths": ["/s/bucket"], "keys": ["bucket/"]}]})),
            kept(
                serde_json::json!({"parts": [{"name": "clouds", "about": "clouds",
                "paths": ["/s/cloud", "/etc/passwd", "/s", "/s/../etc", "s/y"],
                "keys": ["cloud/"]}]}),
            ),
            kept(
                serde_json::json!({"parts": [{"name": "clouds", "about": "again",
                "paths": ["/s/aws"]}], "stays": "a bot stays"}),
            ),
        ]
    }

    #[test]
    fn parts_by_name_or_number() {
        let (mut out, _) = Out::capture(false, &["lnk", "uninstall"]);
        let parts = parts_of(&keepers(), &[Path::new("/s"), Path::new("/f")]);
        let names: Vec<&str> = parts.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["agents", "personal", "buckets", "clouds", "rest"]);
        // Merged, and never a path outside Link's folders, nor one of them,
        // nor one with `..`, nor a relative one.
        assert_eq!(parts[3].paths, [PathBuf::from("/s/cloud"), "/s/aws".into()]);
        assert_eq!(parts[0].first, ["back up first"]);
        let words = |w: &[&str]| w.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            named(&mut out, &parts, &words(&["agents", "buckets"])).unwrap(),
            [0, 2]
        );
        assert_eq!(
            named(&mut out, &parts, &words(&["agents,personal"])).unwrap(),
            [0, 1]
        );
        assert!(named(&mut out, &parts, &words(&["all"])).is_err());
        // A typo is refused; a part of Link's no module keeps here, passed
        // over.
        assert!(named(&mut out, &parts, &words(&["everything"])).is_err());
        assert!(named(&mut out, &parts, &words(&["agent"])).is_err());
        assert!(
            named(&mut out, &parts, &words(&["measures"]))
                .unwrap()
                .is_empty()
        );
        assert!(named(&mut out, &parts, &[]).unwrap().is_empty());
    }

    #[test]
    fn rest_is_what_no_module_claims() {
        let dir = link_test_kit::TempDir::new("uninstall-rest");
        let (settings, files) = (dir.join("s"), dir.join("f"));
        for path in [
            "s/agents",
            "s/installation.toml",
            "s/sandbox",
            "f/Agents",
            "f/Personal",
            "f/Mine",
        ] {
            std::fs::create_dir_all(dir.join(path)).unwrap();
        }
        let json = serde_json::json!({"parts": [{"name": "agents", "about": "a",
            "paths": [settings.join("agents"), files.join("Agents")], "keys": ["agent/"]}]});
        let keepers = vec![Keeper {
            program: PathBuf::from("/x"),
            kept: serde_json::from_value(json).unwrap(),
        }];
        let kept = Kept {
            parts: parts_of(&keepers, &[&settings, &files]),
            settings: settings.clone(),
            files: files.clone(),
            keys: Ok(["agent/main/model-key", "relay-token"]
                .map(String::from)
                .to_vec()),
        };
        assert_eq!(kept.paths(0).len(), 2);
        let mut rest = kept.paths(kept.rest());
        rest.sort();
        assert_eq!(
            rest,
            [
                files.join("Mine"),
                files.join("Personal"),
                settings.join("installation.toml"),
                settings.join("sandbox")
            ]
        );
        assert_eq!(kept.keys(0), ["agent/main/model-key"]);
        assert_eq!(kept.keys(kept.rest()), ["relay-token"]);
    }

    /// A part's path through a link in its middle, out of Link's folders,
    /// is neither listed nor removed.
    #[test]
    fn a_link_in_a_path_leads_nowhere_else() {
        let dir = link_test_kit::TempDir::new("uninstall-link");
        let (settings, files, outside) = (dir.join("s"), dir.join("f"), dir.join("outside"));
        for path in [&settings, &files, &outside.join("victim")] {
            std::fs::create_dir_all(path).unwrap();
        }
        std::fs::create_dir_all(files.join("Agents")).unwrap();
        std::os::unix::fs::symlink(&outside, files.join("away")).unwrap();
        let json = serde_json::json!({"parts": [{"name": "agents", "about": "a",
            "paths": [files.join("away/victim"), files.join("Agents")]}]});
        let keepers = vec![Keeper {
            program: PathBuf::from("/x"),
            kept: serde_json::from_value(json).unwrap(),
        }];
        let kept = Kept {
            parts: parts_of(&keepers, &[&settings, &files]),
            settings,
            files: files.clone(),
            keys: Ok(Vec::new()),
        };
        assert_eq!(kept.parts[0].paths.len(), 2);
        assert_eq!(kept.paths(0), [files.join("Agents")]);
    }

    /// Each of Link's built modules answers `uninstall --parts --json`
    /// exactly when its `module.toml` declares `[removal]`, with parts it
    /// declares: what lets `lnk uninstall --parts` ask only those.
    #[test]
    fn modules_answer_uninstall_as_they_declare() {
        let home = link_test_kit::TempDir::new("uninstall-declared");
        let programs = link_test_kit::programs();
        let answered: Vec<(&str, Option<link_core::uninstall::Kept>)> = std::thread::scope(|s| {
            let asking: Vec<_> = MODULES
                .iter()
                .map(|(module, _)| {
                    let program = programs.join(format!("lnk-{module}"));
                    link_test_kit::programs::require(&program, link_test_kit::programs::BUILD);
                    let home = &home;
                    s.spawn(move || {
                        let output = link_test_kit::env::command(&program, home)
                            .args(["uninstall", "--parts", "--json"])
                            .stdin(Stdio::null())
                            .output()
                            .unwrap();
                        let kept = output
                            .status
                            .success()
                            .then(|| serde_json::from_slice(&output.stdout).unwrap());
                        (*module, kept)
                    })
                })
                .collect();
            asking.into_iter().map(|a| a.join().unwrap()).collect()
        });
        for (module, kept) in answered {
            let declared = crate::registry::REGISTRY[module].removal.as_ref();
            match (declared, kept) {
                (Some(declared), Some(kept)) => {
                    for part in kept.parts {
                        assert!(
                            declared.parts.contains(&part.name),
                            "lnk-{module} keeps {}, which its [removal] doesn't list",
                            part.name
                        );
                    }
                }
                (None, None) => {}
                (Some(_), None) => panic!("lnk-{module} declares [removal] but doesn't answer"),
                (None, Some(_)) => panic!("lnk-{module} answers uninstall without [removal]"),
            }
        }
    }

    #[test]
    fn cargo_names_what_it_installed() {
        let listed = "link-cli v0.11.0 (/src/local/cli):\n    lnk\n\
                      link-example v0.11.0 (/src/local/modules/harness):\n    lnk-example\n\
                      ripgrep v14.1.0:\n    rg\n";
        let programs = ["lnk", "lnk-example", "lnk-widget"].map(String::from);
        let (crates, untracked) = cargo_crates(listed, &programs);
        assert_eq!(crates, ["link-cli", "link-example"]);
        assert_eq!(untracked, ["lnk-widget"]);
        // Nothing tracked: everything goes by hand.
        let (crates, untracked) = cargo_crates("", &programs);
        assert!(crates.is_empty());
        assert_eq!(untracked, programs);
    }
}
