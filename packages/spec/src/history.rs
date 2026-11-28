//! A module's history, `history.toml`: the commands, runtime flags and
//! variables it retired (`[[gone]]`), each answered with its replacement
//! and never run, and, in the core's, the modules renamed (`[[renamed]]`)
//! and removed (`[[removed]]`).
//! Every entry names the release that brought it and the one that deletes
//! it, so history never accumulates.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
};

use anyhow::ensure;
use link_version::Version;
use serde::{Deserialize, Serialize};

mod wire;
pub use wire::{
    PROTOCOL, PROTOCOL_VARIABLE, Wire, WireGone, WireRemoved, WireRenamed, asked, asking,
};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct History {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gone: Vec<Gone>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub renamed: Vec<Renamed>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<Removed>,
}

/// A module a release no longer includes: `lnk upgrade` uninstalls its
/// program, and its name is answered with `why`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Removed {
    /// Its name: its program was `lnk-<module>`.
    pub module: String,
    /// Why it's gone, and what to use instead, as a sentence or more.
    pub why: String,
    pub since: Version,
    /// The release that deletes the entry, after which the name is just
    /// unknown.
    pub until: Version,
}

impl Removed {
    pub fn message(&self) -> String {
        format!(
            "The {} module is removed since {}. {}",
            self.module, self.since, self.why
        )
    }
}

/// A module renamed: `lnk upgrade` replaces its program with the new
/// name's, and its old name is answered as a retired one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Renamed {
    /// Its old name: its program was `lnk-<module>`.
    pub module: String,
    /// Its name since.
    pub now: String,
    pub since: Version,
    /// The release that deletes the entry, after which the old name is
    /// just unknown.
    pub until: Version,
}

impl Renamed {
    pub fn program(&self) -> String {
        format!("lnk-{}", self.module)
    }

    /// What's said of its old name, `now` its name today.
    pub fn message(&self, now: &str) -> String {
        format!(
            "The {} module is named {now} since {}.",
            self.module, self.since
        )
    }
}

/// A retired spelling: a command (`agent show --opinions` its words and
/// flag), a runtime flag or a variable (`LNK_AGENT_MODEL`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gone {
    pub was: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    pub since: Version,
    /// The release that deletes the entry, after which the old spelling
    /// is an unknown argument.
    pub until: Version,
}

impl Gone {
    pub fn variable(&self) -> bool {
        self.was.starts_with("LNK_")
    }

    pub fn message(&self) -> String {
        let mut text = format!("{} is gone since {}.", self.was, self.since);
        if let Some(now) = &self.now {
            text += &format!(" Use {now}.");
        }
        if let Some(why) = &self.why {
            text += &format!(" {why}");
        }
        text
    }
}

/// The flag a command's surface holds where the command takes the rest
/// of its line as values (`sandbox run <program> ...`): a word that
/// isn't a flag ends what's matched, so nothing after it is taken for a
/// retired flag.
pub const TRAILING: &str = "...";

/// A current command and whether each flag consumes a separate value.
pub type Surface = BTreeMap<String, BTreeMap<String, bool>>;

impl History {
    /// Whether it records nothing.
    pub fn is_empty(&self) -> bool {
        self.gone.is_empty() && self.renamed.is_empty() && self.removed.is_empty()
    }

    /// An entry for a command or flag not spelled as a person types it
    /// after `lnk`, `namespace` first: the program's entry puts its
    /// namespace back before it looks, so such an entry never matches.
    pub fn outside(&self, namespace: &str) -> Option<&Gone> {
        self.gone.iter().find(|gone| {
            !gone.variable()
                && gone.was != namespace
                && !gone
                    .was
                    .strip_prefix(namespace)
                    .is_some_and(|rest| rest.starts_with(' '))
        })
    }

    /// Every entry the release `version` has reached the deletion of:
    /// none, or a test fails until it's deleted.
    pub fn check_retirement(&self, version: &Version) -> Vec<String> {
        let gone = self.gone.iter().map(|g| (&g.was, &g.until));
        let renamed = self.renamed.iter().map(|r| (&r.module, &r.until));
        let removed = self.removed.iter().map(|r| (&r.module, &r.until));
        gone.chain(renamed)
            .chain(removed)
            .filter(|(_, until)| *until <= version)
            .map(|(name, until)| {
                format!("the history of {name} is deleted at {until}, which {version} has reached: delete it")
            })
            .collect()
    }

    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let history: Self = toml::from_str(text)?;
        let mut names = BTreeSet::new();
        for (name, since, until) in history
            .gone
            .iter()
            .map(|g| (&g.was, &g.since, &g.until))
            .chain(
                history
                    .renamed
                    .iter()
                    .map(|r| (&r.module, &r.since, &r.until)),
            )
            .chain(
                history
                    .removed
                    .iter()
                    .map(|r| (&r.module, &r.since, &r.until)),
            )
        {
            ensure!(
                since < until,
                "the history of {name} is deleted at {until}, before or as it came at {since}"
            );
        }
        for gone in &history.gone {
            ensure!(
                names.insert(&gone.was),
                "duplicate history entry {}",
                gone.was
            );
            ensure!(
                gone.now.as_ref().is_some_and(|s| !s.trim().is_empty())
                    || gone.why.as_ref().is_some_and(|s| !s.trim().is_empty()),
                "{} needs now or why",
                gone.was
            );
            ensure!(
                !gone.was.is_empty()
                    && gone.was.split_whitespace().collect::<Vec<_>>().join(" ") == gone.was,
                "invalid history name {:?}",
                gone.was
            );
            if gone.variable() {
                ensure!(
                    gone.was
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'),
                    "invalid variable history"
                );
            } else {
                // Each word a command's, a flag, or a positional it names
                // in angle brackets (`<model>`).
                ensure!(
                    gone.was.split_whitespace().all(|s| {
                        let s = match placeholder(s) {
                            true => &s[1..s.len() - 1],
                            false => s,
                        };
                        s.bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                    }),
                    "invalid command history"
                );
            }
        }
        let mut modules = BTreeSet::new();
        for renamed in &history.renamed {
            for name in [&renamed.module, &renamed.now] {
                module_name(name)?;
            }
            ensure!(
                renamed.module != renamed.now,
                "{} is renamed to itself",
                renamed.module
            );
            ensure!(
                modules.insert(&renamed.module),
                "{} is renamed twice",
                renamed.module
            );
        }
        for renamed in &history.renamed {
            history.name(&renamed.module)?;
        }
        for removed in &history.removed {
            module_name(&removed.module)?;
            ensure!(
                !removed.why.trim().is_empty(),
                "{} is removed with no why",
                removed.module
            );
            // A module is renamed or removed, never both. A rename may
            // lead to one removed since, which removes its old name too.
            ensure!(
                modules.insert(&removed.module),
                "{} is removed twice, or both renamed and removed",
                removed.module
            );
        }
        Ok(history)
    }

    /// The name the module once named `module` has now, following every
    /// rename; a module never renamed is its own.
    pub fn name<'a>(&'a self, module: &'a str) -> anyhow::Result<&'a str> {
        name_in(self.renames(), module)
    }

    /// The modules renamed since, whose old programs are in `dir` and
    /// whose new names are among `modules`: each old program, and the
    /// module it now is.
    pub fn renamed_in<'a>(
        &'a self,
        dir: &std::path::Path,
        modules: &[&str],
    ) -> anyhow::Result<Vec<(String, &'a str)>> {
        renamed_in(self.renames(), dir, modules)
    }

    fn renames(&self) -> Vec<(&str, &str)> {
        self.renamed
            .iter()
            .map(|r| (r.module.as_str(), r.now.as_str()))
            .collect()
    }

    /// What `--link-history` writes: [`Wire`], in this release's
    /// [`PROTOCOL`].
    pub fn wire(&self) -> Wire {
        Wire {
            protocol: PROTOCOL.current,
            gone: self
                .gone
                .iter()
                .map(|g| WireGone {
                    command: g.was.clone(),
                    since: g.since.clone(),
                    now: g.now.clone(),
                    why: g.why.clone(),
                })
                .collect(),
            renamed: self
                .renamed
                .iter()
                .map(|r| WireRenamed {
                    module: r.module.clone(),
                    now: r.now.clone(),
                    since: r.since.clone(),
                })
                .collect(),
            removed: self
                .removed
                .iter()
                .map(|r| WireRemoved {
                    module: r.module.clone(),
                    why: r.why.clone(),
                    since: r.since.clone(),
                })
                .collect(),
            modules: BTreeMap::new(),
        }
    }

    /// [`History::wire`], with each module's own history: the release's,
    /// as the core answers it.
    pub fn release_wire<'a>(
        &self,
        modules: impl IntoIterator<Item = (&'a str, &'a History)>,
    ) -> Wire {
        let mut wire = self.wire();
        for (module, history) in modules {
            let gone = history.wire().gone;
            if !gone.is_empty() {
                wire.modules.insert(module.to_string(), gone);
            }
        }
        wire
    }

    /// The rename `args` name the old module of: as the command it was
    /// (`lnk memory`), or given to `lnk module add`. Given to `lnk module
    /// remove`, it's the core's to answer, which removes the old program
    /// an upgrade couldn't stop.
    pub fn find_renamed(&self, args: &[OsString], surface: &Surface) -> Option<&Renamed> {
        let module = named_module(args, surface, |m| {
            self.renamed.iter().any(|r| r.module == m) && self.removal(m).is_none()
        })?;
        self.renamed.iter().find(|r| r.module == module)
    }

    /// The removed module `args` name, as [`History::find_renamed`] finds
    /// a renamed one: by its name, or one renamed to it.
    pub fn find_removed(&self, args: &[OsString], surface: &Surface) -> Option<&Removed> {
        let module = named_module(args, surface, |m| self.removal(m).is_some())?;
        self.removal(module)
    }

    /// The removal of the module once named `module`, following its
    /// renames: none where it's a module today, or no module.
    pub fn removal(&self, module: &str) -> Option<&Removed> {
        let now = self.name(module).ok()?;
        self.removed.iter().find(|r| r.module == now)
    }

    /// Holds the history to the release: `surface` is its owner's
    /// commands, which no entry may still be, and `catalog` the release's
    /// whole surface, every module's and the core's, which a replacement
    /// may name, so a command moved to another module names its home.
    /// `version` is held as the release it's a build of, so a staging
    /// build of 0.0.2 holds entries since 0.0.2 as current.
    pub fn validate(
        &self,
        surface: &Surface,
        catalog: &Surface,
        modules: &[&str],
        version: &Version,
    ) -> anyhow::Result<()> {
        let version = &version.release();
        for removed in &self.removed {
            ensure!(
                &removed.since <= version,
                "{} has a future removal version",
                removed.module
            );
            ensure!(
                !modules.contains(&removed.module.as_str()) && !current(&removed.module, catalog),
                "{} is both a current module or command and removed",
                removed.module
            );
        }
        for renamed in &self.renamed {
            ensure!(
                &renamed.since <= version,
                "{} has a future rename version",
                renamed.module
            );
            ensure!(
                !modules.contains(&renamed.module.as_str()) && !current(&renamed.module, surface),
                "{} is both a current module or command and renamed",
                renamed.module
            );
            let now = self.name(&renamed.module)?;
            ensure!(
                modules.contains(&now) || self.removed.iter().any(|r| r.module == now),
                "{} is renamed to {now}, which isn't a module or a removed one",
                renamed.module
            );
        }
        for gone in &self.gone {
            ensure!(
                &gone.since <= version,
                "{} has a future retirement version",
                gone.was
            );
            if gone.variable() {
                continue;
            }
            // One naming a positional is gone with that word given, even
            // where its command words are a current command's.
            let named = gone.was.split_whitespace().any(placeholder);
            ensure!(
                named || !current(&gone.was, surface) || became_group(&gone.was, surface),
                "{} is both current and gone",
                gone.was
            );
            ensure!(
                !named || !command_words(&gone.was).is_empty(),
                "{} names a positional before any command word",
                gone.was
            );
            if let Some(now) = &gone.now {
                ensure!(
                    now.strip_prefix("lnk ")
                        .is_some_and(|p| current(p, catalog)),
                    "{} has no current replacement {}",
                    gone.was,
                    now
                );
            }
        }
        Ok(())
    }

    /// The retired variables set in this process, by their names alone:
    /// never their values.
    pub fn variables_set(&self) -> impl Iterator<Item = &Gone> {
        self.gone
            .iter()
            .filter(|gone| gone.variable() && std::env::var_os(&gone.was).is_some())
    }

    /// Match only syntax, never a flag's value or arguments following `--`.
    pub fn find<'a>(&'a self, args: &[OsString], surface: &Surface) -> Option<(&'a Gone, usize)> {
        self.gone
            .iter()
            .filter(|g| !g.variable())
            .filter_map(|gone| {
                let words: Vec<_> = gone.was.split_whitespace().collect();
                let flag = words.last().filter(|w| w.starts_with("--"));
                let prefix = if flag.is_some() {
                    &words[..words.len() - 1]
                } else {
                    &words[..]
                };
                if !prefix.iter().zip(args).all(|(a, b)| said(a, b)) || args.len() < prefix.len() {
                    return None;
                }
                // An entry naming a positional is its old self only with
                // that word given: `model uninstall <model>` never matches
                // `lnk model uninstall`, a current command.
                let named = prefix.iter().any(|w| placeholder(w));
                let path = command_words(&prefix.join(" "));
                let Some(flag) = flag else {
                    // A command that became a group (`job spec <id>`, now
                    // `job spec show <id>`) is its old self only before a
                    // word none of its subcommands is.
                    if !named && became_group(&path, surface) {
                        let next = args.get(prefix.len()).and_then(|a| a.to_str())?;
                        if next.starts_with('-') || surface.contains_key(&format!("{path} {next}"))
                        {
                            return None;
                        }
                    }
                    return Some((gone, prefix.len()));
                };
                let flags = surface
                    .iter()
                    .filter(|(p, _)| {
                        let p: Vec<_> = p.split_whitespace().collect();
                        p.len() <= args.len() && p.iter().zip(args).all(|(a, b)| *b == *a)
                    })
                    .max_by_key(|(p, _)| p.len())
                    .map(|(_, flags)| flags);
                // A prefix that became a group (`box spec`, now `box spec
                // show`) takes its old flag only before a word: after one,
                // the flag is that subcommand's, current.
                let group = format!("{path} ");
                let group = surface.keys().any(|p| p.starts_with(&group));
                let mut i = prefix.len();
                while let Some(arg) = args.get(i) {
                    if arg == "--" {
                        break;
                    }
                    let trailing = group || flags.is_some_and(|f| f.contains_key(TRAILING));
                    if trailing && arg.to_str().is_some_and(|a| !a.starts_with('-')) {
                        break;
                    }
                    if let Some(arg) = arg.to_str() {
                        let (name, inline) =
                            arg.split_once('=').map_or((arg, false), |(n, _)| (n, true));
                        if name == *flag {
                            return Some((gone, i));
                        }
                        if !inline && flags.and_then(|f| f.get(name)).copied().unwrap_or(false) {
                            i += 1;
                        }
                    }
                    i += 1;
                }
                None
            })
            .max_by_key(|(gone, _)| gone.was.len())
    }
}

/// Whether `name` can name a module: lowercase words joined by hyphens.
fn module_name(name: &str) -> anyhow::Result<()> {
    ensure!(
        !name.is_empty()
            && !name.starts_with('-')
            && !name.ends_with('-')
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
        "invalid module name {name:?} in the history"
    );
    Ok(())
}

/// The module `args` name for which `known` holds: as the command it
/// was (`lnk memory`), or given to `lnk module add`, never a flag's value
/// or a word after `--`.
fn named_module<'a>(
    args: &'a [OsString],
    surface: &Surface,
    known: impl Fn(&str) -> bool,
) -> Option<&'a str> {
    let first = args.first()?.to_str()?;
    if known(first) {
        return Some(first);
    }
    let verb = args.get(1).and_then(|a| a.to_str())?;
    if first != "module" || verb != "add" {
        return None;
    }
    let flags = surface.get("module add");
    let mut rest = args[2..].iter();
    while let Some(arg) = rest.next() {
        let Some(arg) = arg.to_str() else {
            continue;
        };
        if arg == "--" {
            break;
        }
        if arg.starts_with('-') {
            if !arg.contains('=') && flags.and_then(|f| f.get(arg)).copied().unwrap_or(false) {
                rest.next();
            }
            continue;
        }
        if known(arg) {
            return Some(arg);
        }
    }
    None
}

/// The name `module` has now, through `renames` (each old name and the
/// next).
fn name_in<'a>(renames: Vec<(&'a str, &'a str)>, mut module: &'a str) -> anyhow::Result<&'a str> {
    let mut seen = BTreeSet::new();
    while let Some((_, now)) = renames.iter().find(|(old, _)| *old == module) {
        ensure!(seen.insert(module), "{module} is renamed in a circle");
        module = now;
    }
    Ok(module)
}

fn renamed_in<'a>(
    renames: Vec<(&'a str, &'a str)>,
    dir: &std::path::Path,
    modules: &[&str],
) -> anyhow::Result<Vec<(String, &'a str)>> {
    let mut found = Vec::new();
    for (old, _) in &renames {
        let now = name_in(renames.clone(), old)?;
        let program = format!("lnk-{old}");
        if dir.join(&program).is_file() && modules.contains(&now) && !modules.contains(old) {
            found.push((program, now));
        }
    }
    Ok(found)
}

/// Whether the command `path`, no flag, is now a group of commands.
/// Whether `word`, a word of a retired spelling, stands for a positional
/// argument, as `<model>` does: any one word that isn't a flag.
fn placeholder(word: &str) -> bool {
    word.len() > 2 && word.starts_with('<') && word.ends_with('>')
}

/// Whether `arg`, a word of the command line, is `word` of a retired
/// spelling: the same word, or one a placeholder stands for.
fn said(word: &str, arg: &OsString) -> bool {
    match placeholder(word) {
        true => arg.to_str().is_some_and(|a| !a.starts_with('-')),
        false => *arg == *word,
    }
}

/// `was`'s command words, up to the first positional it names.
fn command_words(was: &str) -> String {
    was.split_whitespace()
        .take_while(|w| !placeholder(w))
        .collect::<Vec<_>>()
        .join(" ")
}

fn became_group(path: &str, surface: &Surface) -> bool {
    !path.split_whitespace().any(|w| w.starts_with('-'))
        && surface.keys().any(|p| p.starts_with(&format!("{path} ")))
}

fn current(name: &str, surface: &Surface) -> bool {
    if let Some((path, flag)) = name.rsplit_once(' ')
        && flag.starts_with("--")
    {
        return surface.get(path).is_some_and(|f| f.contains_key(flag));
    }
    surface.contains_key(name)
}

#[cfg(test)]
mod tests;
