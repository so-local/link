//! Modules from outside Link: `lnk module add --url <url>`.
//!
//! One comes from a GitHub repository's releases, laid out as Link's
//! (`lnk-<name>-<target>.tar.gz` and a signed `SHA256SUMS`), with the
//! public half of its own signing key as a release asset
//! ([`release::KEY_ASSET`]). The first `add` shows that key and asks; `lnk`
//! then pins the source and the key in this machine's settings
//! (`~/Link/Safe/settings.toml`), and every upgrade
//! must be signed by that key, or it's refused. The downloaded program must answer the generic component protocol.

use std::{collections::BTreeMap, path::Path};

use anyhow::{Context, bail, ensure};
use link_core::release::{self, Release};
use link_format::{
    Done, Exit, Fields, Foreign, Note, Out, Path as Shown, Product, Question, Refuser, Report,
    Text, Version, WithinExt, cmd, phrase, prose, sentence,
};

use crate::{failed, lnk_command};

/// Where the known modules from outside Link are listed, for messages.
pub const KNOWN_LIST: &str = link_core::docs!("more/modules/install");

/// The modules added from outside Link, pinned in this machine's
/// settings (`~/Link/Safe/settings.toml`), and the programs noted once in Hidden.
#[derive(Debug, Default)]
pub struct Pinned {
    /// Each module added from outside Link, by name.
    pub modules: BTreeMap<String, Source>,
    /// Programs in `lnk`'s folder that are neither Link's nor added from
    /// outside, which `lnk upgrade` has said once how to keep updated.
    pub noted: Vec<String>,
}

pub use link_settings::folders::ModuleSource as Source;

impl Pinned {
    pub fn load() -> anyhow::Result<Self> {
        let settings = link_settings::folders::Settings::load()?;
        Ok(Self {
            modules: settings.modules,
            noted: settings.noted,
        })
    }

    /// Writes them into this machine's settings, the rest kept.
    pub fn save(&self) -> anyhow::Result<()> {
        let mut settings = link_settings::folders::Settings::load()?;
        settings.modules = self.modules.clone();
        settings.noted = self.noted.clone();
        settings.save()
    }
}

/// Whether `word`, given to `lnk module add`, names a repository rather
/// than one of Link's modules.
pub fn is_url(word: &str) -> bool {
    word.contains("://") || word.starts_with("github.com/")
}

/// `url` as `https://github.com/<owner>/<repo>`: a GitHub repository,
/// with or without `https://`, a trailing `/` or `.git`.
pub fn repository(url: &str) -> Result<String, Report> {
    let not = || {
        Report::error(sentence!(
            "{} isn't a GitHub repository.",
            Foreign::new(url)
        ))
        .why(prose!(
            "A module's repository looks like https://github.com/<owner>/<repo>."
        ))
        .exit(Exit::Usage)
    };
    let rest = url
        .strip_prefix("https://")
        .unwrap_or(url)
        .strip_prefix("github.com/")
        .ok_or_else(not)?;
    let rest = rest.trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let parts: Vec<&str> = rest.split('/').collect();
    let plain = |part: &str| {
        !part.is_empty()
            && !part.starts_with('.')
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    };
    if parts.len() != 2 || !parts.iter().all(|p| plain(p)) {
        return Err(not());
    }
    Ok(format!("https://github.com/{}/{}", parts[0], parts[1]))
}

/// The variable naming where outside modules' releases are read in
/// GitHub's place (tests).
const GITHUB_VAR: &str = "LNK_GITHUB_URL";

/// Where `source` (`https://github.com/<owner>/<repo>`)'s latest release
/// is: its folder of GitHub's releases, or the same folder in the release
/// location `LNK_GITHUB_URL` names in GitHub's place (tests), read as
/// `LNK_RELEASES_URL` is ([`release::Source`]).
fn latest(source: &str) -> anyhow::Result<release::Source> {
    let named = std::env::var(GITHUB_VAR)
        .ok()
        .filter(|text| !text.is_empty());
    latest_in(source, named.as_deref())
}

/// [`latest`], with `named` the value of `LNK_GITHUB_URL`.
fn latest_in(source: &str, named: Option<&str>) -> anyhow::Result<release::Source> {
    let github: release::Source = match named {
        Some(text) => text
            .parse()
            .map_err(|e: String| anyhow::anyhow!("{GITHUB_VAR}: {e}"))?,
        None => "https://github.com"
            .parse()
            .map_err(|e: String| anyhow::anyhow!(e))?,
    };
    let repository = source
        .strip_prefix("https://github.com/")
        .with_context(|| format!("{} isn't a GitHub repository", link_format::clean(source)))?;
    github.join(&format!("{repository}/releases/latest/download"))
}

/// The one module a release holds for this machine: `name` of
/// `lnk-<name>-<target>.tar.gz`.
fn module_in(release: &Release) -> anyhow::Result<String> {
    let suffix = format!("-{}.tar.gz", release::target()?);
    let names: Vec<&str> = release
        .assets()
        .filter_map(|a| a.strip_prefix("lnk-")?.strip_suffix(suffix.as_str()))
        .collect();
    match names.as_slice() {
        [one] => Ok(one.to_string()),
        [] => bail!("its release holds no lnk module for this machine ({suffix})"),
        more => bail!(
            "its release holds more than one module ({}); lnk adds one at a time",
            link_format::clean(&more.join(", "))
        ),
    }
}

/// Fails unless `name` can be a module of its own: a plain name, none of
/// Link's.
fn check_name(name: &str) -> anyhow::Result<()> {
    ensure!(
        !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
        "{} isn't a module name",
        link_format::clean(name)
    );
    ensure!(
        name != "core" && !crate::MODULES.iter().any(|(p, _)| *p == name),
        "it would replace Link's own {name} module; not adding it"
    );
    Ok(())
}

/// Validate the generic component answer and its binary identity.
fn check_component(program: &Path, name: &str) -> anyhow::Result<()> {
    let mut asking = link_secretive::process::command(program);
    link_component::asking(&mut asking);
    let output = asking
        .arg("--link-describe")
        .stdin(std::process::Stdio::null())
        .output()
        .within(format!("running the downloaded lnk-{name}"))?;
    ensure!(
        output.status.success(),
        "lnk-{name} did not answer --link-describe: {}",
        link_format::clean(String::from_utf8_lossy(&output.stderr).trim())
    );
    let descriptor = link_component::Resolved::parse(std::str::from_utf8(&output.stdout)?)?;
    ensure!(
        descriptor.component.id == name && descriptor.component.program == format!("lnk-{name}"),
        "the downloaded lnk-{name} described another program"
    );
    ensure!(
        descriptor.component.kind != link_component::Kind::Program,
        "a standalone program is not an installable module"
    );
    Ok(())
}

/// What `lnk module add --url <url>` is told besides the URL.
#[derive(Default)]
pub struct AddOptions {
    /// `--yes`: don't ask before trusting its key.
    pub yes: bool,
    /// `--signer`: the key to trust, as `lnk module list --json` shows it,
    /// in place of the one its release offers (a box adding what this
    /// computer added).
    pub signer: Option<String>,
}

/// `lnk module add --url <url>`: the module in `url`'s latest release, into
/// `dir` (lnk's folder), pinned to its key. Its name, and what to show of
/// it.
pub async fn add(
    out: &mut Out,
    url: &str,
    options: &AddOptions,
    dir: &Path,
) -> Result<(String, Done), Report> {
    let source = repository(url)?;
    let base = latest(&source)?;
    let mut pinned = Pinned::load()?;
    let was = pinned
        .modules
        .values()
        .find(|p| p.source == source)
        .cloned();
    let key = match (&options.signer, &was) {
        (Some(key), _) => release::one_key(key).within("reading --signer as an ssh-ed25519 key")?,
        (None, Some(was)) => was.signer.clone(),
        (None, None) => {
            let text = Release::unsigned(&base, release::KEY_ASSET)
                .await
                .map_err(|err| {
                    failed(
                        sentence!(
                            "{} has no release with its key, {}: it may not be a module for lnk.",
                            Shown::new(&source),
                            Product::new(release::KEY_ASSET)
                        ),
                        err,
                    )
                })?;
            release::one_key(&String::from_utf8_lossy(&text))
                .within(format!("reading {} of {source}", release::KEY_ASSET))?
        }
    };
    if let Some(was) = &was
        && was.signer != key
    {
        return Err(Report::refusal(
            Refuser::Policy("trusted keys".into()),
            sentence!(
                "{} was added with another key, {}.",
                Product::new(source.as_str()),
                release::fingerprint(&was.signer)?
            ),
        )
        .why(prose!(
            "To trust {} instead, remove it and add it again.",
            release::fingerprint(&key)?
        ))
        .next(
            lnk_command(["module", "remove", "<module>"]),
            phrase!("remove it first"),
        ));
    }
    let release = Release::open(&base, &key).await.map_err(|err| {
        failed(
            sentence!(
                "Nothing was installed from {}.",
                Product::new(source.as_str())
            ),
            err,
        )
    })?;
    let name = module_in(&release).map_err(|err| {
        failed(
            sentence!(
                "Nothing was installed from {}.",
                Product::new(source.as_str())
            ),
            err,
        )
    })?;
    check_name(&name)?;
    if let Some(other) = pinned.modules.get(&name)
        && other.source != source
    {
        return Err(Report::error(sentence!(
            "{} was added from {} already.",
            Product::new(format!("lnk-{name}")),
            Product::new(other.source.as_str())
        ))
        .next(
            lnk_command(["module", "remove", name.as_str()]),
            phrase!("remove it first"),
        ));
    }
    if was.is_none() && !options.yes {
        out.now(
            Fields::new()
                .row("Module", Product::new(format!("lnk-{name}")))
                .row("From", Product::new(source.as_str()))
                .row("Signed by the key", release::fingerprint(&key)?),
        );
        out.now(Text(prose!(
            "It isn't Link's: it runs as you, like any program you install. lnk will install its updates only when signed by this key."
        )));
        let trusted = out
            .ask(Question::confirm(sentence!("Trust this key and add it?"), false).flag("--yes"))?;
        if !trusted {
            return Err(Report::refusal(
                Refuser::Person,
                sentence!("Not added, as you chose."),
            ));
        }
    }
    let installed = dir.join(format!("lnk-{name}"));
    let work = dir.join(format!("lnk-add-{}", std::process::id()));
    std::fs::create_dir_all(&work).within("making a folder to install in")?;
    let result = async {
        if not_newer(&release, &installed).await? {
            return anyhow::Ok(false);
        }
        let wait = out.activity(phrase!(
            "Installing {} from {}",
            Product::new(format!("lnk-{name}")),
            Product::new(source.as_str())
        ));
        let archive = release.get(&release::program_asset(&name)?).await?;
        drop(wait);
        release::unpack(&archive, &work)?;
        let program = work.join(format!("lnk-{name}"));
        ensure!(
            program.symlink_metadata().is_ok_and(|m| m.is_file()),
            "its archive holds no lnk-{name}"
        );
        check_component(&program, &name)?;
        // Never back: a release before the one here, signed by the
        // same key, could only take back its fixes.
        let Some(version) = replaces(&program, &installed, &name)? else {
            return anyhow::Ok(false);
        };
        release::put(&program, &installed)?;
        out.block(Note(sentence!(
            "{} {}.",
            Product::new(format!("lnk-{name}")),
            Version::new(version.to_string())
        )));
        anyhow::Ok(true)
    }
    .await;
    let _ = std::fs::remove_dir_all(&work);
    let installed = result.map_err(|err| {
        failed(
            sentence!("Installing {} failed.", Product::new(format!("lnk-{name}"))),
            err,
        )
    })?;
    pinned.modules.insert(
        name.clone(),
        Source {
            source: source.clone(),
            signer: key,
        },
    );
    pinned.noted.retain(|n| *n != name);
    pinned.save()?;
    let done = match installed {
        true => Done::success(sentence!(
            "Installed {}, signature and checksum verified.",
            Product::new(format!("lnk-{name}"))
        )),
        false => Done::success(sentence!(
            "{} is installed, as new as its latest release.",
            Product::new(format!("lnk-{name}"))
        )),
    }
    .changed(
        phrase!("Its source and key are kept in"),
        Shown::new(link_files::layout::Home::Settings.try_path()?),
    );
    Ok((name, done))
}

/// Whether `release` says, in its signed `VERSION`, that it is no newer
/// than `installed`: then it is neither downloaded nor run. False when
/// there's nothing installed, or a version on either side is missing or
/// can't be compared: [`replaces`] decides then, after running it.
async fn not_newer(release: &Release, installed: &Path) -> anyhow::Result<bool> {
    if !installed.exists() {
        return Ok(false);
    }
    let Some(version) = release.version().await? else {
        return Ok(false);
    };
    let Ok(old) = crate::version_of(installed) else {
        return Ok(false);
    };
    Ok(version <= old)
}

/// The version of `new`, a downloaded `lnk-<name>`, when it should
/// replace `installed`: there's none, or it's newer. None when it isn't
/// newer. An error when either doesn't say a version `lnk` can compare,
/// rather than taking a release it can't tell isn't older.
fn replaces(
    new: &Path,
    installed: &Path,
    name: &str,
) -> anyhow::Result<Option<link_version::Version>> {
    let version = crate::version_of(new).with_context(|| {
        format!("its lnk-{name} says no version lnk can compare: not installing it")
    })?;
    if !installed.exists() {
        return Ok(Some(version));
    }
    let old = crate::version_of(installed).ok().with_context(|| {
        format!(
            "the installed lnk-{name} says no version lnk can compare, so lnk can't tell \
             whether {version} is newer: lnk module remove {name}, then add it again"
        )
    })?;
    Ok((version > old).then_some(version))
}

/// `lnk upgrade` for the modules added from outside Link, in `dir`: each
/// from its own repository's latest release, signed by its pinned key.
/// A module that can't be upgraded is said, and left as it is.
pub async fn upgrade(out: &mut Out, dir: &Path) -> Result<(), Report> {
    let pinned = Pinned::load()?;
    for (name, from) in &pinned.modules {
        let path = dir.join(format!("lnk-{name}"));
        // One that's Link's own now comes with Link's release.
        if !path.is_file() || check_name(name).is_err() {
            continue;
        }
        match upgrade_one(name, from, &path, dir).await {
            Ok(Some(version)) => out.block(Note(sentence!(
                "{} {}, from {}.",
                Product::new(format!("lnk-{name}")),
                Version::new(version.to_string()),
                Product::new(from.source.as_str())
            ))),
            Ok(None) => {}
            Err(err) => {
                let untrusted = format!("{err:#}").contains("doesn't trust");
                let mut report = Report::warning(sentence!(
                    "{} was not upgraded.",
                    Product::new(format!("lnk-{name}"))
                ))
                .why(link_format::Prose::from(link_format::Sentence::adopt(
                    &format!("{err:#}"),
                )));
                if untrusted {
                    report = report
                        .next(
                            lnk_command(["module", "remove", name.as_str()]),
                            phrase!("remove it, if its maker changed keys"),
                        )
                        .next(
                            lnk_command(["module", "add", "--url", from.source.as_str()]),
                            phrase!("add it again, trusting the new key"),
                        );
                }
                out.block(report.cause(err));
            }
        }
    }
    Ok(())
}

/// `lnk upgrade --check` for the modules added from outside Link, in
/// `dir`: each one's latest release, signed by its pinned key, said when
/// it's newer than the one installed. Installs nothing. One that can't be
/// checked is said with why; how many couldn't be.
pub async fn check(out: &mut Out, dir: &Path) -> Result<usize, Report> {
    let pinned = Pinned::load()?;
    let mut unchecked = 0;
    for (name, from) in &pinned.modules {
        let path = dir.join(format!("lnk-{name}"));
        // One that's Link's own now comes with Link's release.
        if !path.is_file() || check_name(name).is_err() {
            continue;
        }
        let program = Product::new(format!("lnk-{name}"));
        match newer(name, from, &path).await {
            Ok((latest, installed)) if latest > installed => out.block(Note(sentence!(
                "{} {} is out (you have {}): {} installs it.",
                program,
                Version::new(latest.to_string()),
                Version::new(installed.to_string()),
                cmd!("lnk upgrade")
            ))),
            Ok((_, installed)) => out.block(Note(sentence!(
                "{} {} is the latest.",
                program,
                Version::new(installed.to_string())
            ))),
            Err(err) => {
                unchecked += 1;
                out.block(
                    Report::warning(sentence!("Couldn't check {} for a newer release.", program))
                        .why(link_format::Prose::from(link_format::Sentence::adopt(
                            &format!("{err:#}"),
                        )))
                        .cause(err),
                );
            }
        }
    }
    Ok(unchecked)
}

/// The version of `lnk-<name>`'s latest release, signed by its pinned
/// key, and of the one installed at `path`.
async fn newer(
    name: &str,
    from: &Source,
    path: &Path,
) -> anyhow::Result<(link_version::Version, link_version::Version)> {
    let release = Release::open(&latest(&from.source)?, &from.signer).await?;
    let found = module_in(&release)?;
    ensure!(
        found == name,
        "its latest release holds lnk-{found}, not lnk-{name}"
    );
    let latest = release
        .version()
        .await?
        .context("its latest release doesn't say its version")?;
    let installed = crate::version_of(path)
        .with_context(|| format!("the installed lnk-{name} says no version lnk can compare"))?;
    Ok((latest, installed))
}

/// Upgrades `lnk-<name>` at `path`; the new version, if it was older.
async fn upgrade_one(
    name: &str,
    from: &Source,
    path: &Path,
    dir: &Path,
) -> anyhow::Result<Option<link_version::Version>> {
    let release = Release::open(&latest(&from.source)?, &from.signer).await?;
    let found = module_in(&release)?;
    ensure!(
        found == name,
        "its latest release holds lnk-{found}, not lnk-{name}"
    );
    if not_newer(&release, path).await? {
        return Ok(None);
    }
    let work = dir.join(format!("lnk-upgrade-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&work)?;
    let result = async {
        let archive = release.get(&release::program_asset(name)?).await?;
        release::unpack(&archive, &work)?;
        let new = work.join(format!("lnk-{name}"));
        ensure!(
            new.symlink_metadata().is_ok_and(|m| m.is_file()),
            "its archive holds no lnk-{name}"
        );
        check_component(&new, name)?;
        let Some(new_version) = replaces(&new, path, name)? else {
            return Ok(None);
        };
        release::put(&new, path)?;
        Ok(Some(new_version))
    }
    .await;
    let _ = std::fs::remove_dir_all(&work);
    result
}

/// Programs in `dir` (lnk's folder) named like modules that are neither
/// Link's nor added from outside: programs no longer in its release registry,
/// which they no longer do. `lnk upgrade` leaves them as they are, and
/// says once, for each, how to keep it updated.
pub fn note_strays(out: &mut Out, dir: &Path, link: &[String]) -> Result<(), Report> {
    let mut pinned = Pinned::load()?;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    let mut strays: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            e.file_name()
                .to_str()?
                .strip_prefix("lnk-")
                .map(str::to_string)
        })
        .filter(|name| check_name(name).is_ok())
        .filter(|name| !pinned.modules.contains_key(name) && !pinned.noted.contains(name))
        // Link's own, as the release just installed names them.
        .filter(|name| !link.contains(name))
        .collect();
    strays.sort();
    if strays.is_empty() {
        return Ok(());
    }
    for name in &strays {
        out.block(
            Report::warning(sentence!(
                "{} isn't one of Link's modules, so lnk upgrade leaves it as it is.",
                Product::new(format!("lnk-{name}"))
            ))
            .why(prose!(
                "If it's a module from outside Link, add it from its own repository to keep getting its updates. The known ones are listed in {}.",
                Product::new(KNOWN_LIST)
            ))
            .next(
                cmd!("lnk module add --url <url>"),
                phrase!("add it from its own repository"),
            ),
        );
    }
    pinned.noted.extend(strays);
    Ok(pinned.save()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A repository's latest release is in GitHub's releases, or in the
    /// same folder of the release location `LNK_GITHUB_URL` names, read
    /// as `LNK_RELEASES_URL` is: a folder here or a bucket in every build,
    /// and plain `http://` to this machine in a debug build alone.
    #[test]
    fn a_latest_release_is_in_github_or_where_the_test_variable_says() {
        let latest = |named| {
            latest_in("https://github.com/some_one/link-otter", named).map(|s| s.to_string())
        };
        assert_eq!(
            latest(None).unwrap(),
            "https://github.com/some_one/link-otter/releases/latest/download"
        );
        for (named, at) in [
            ("https://builds.example/gh", "https://builds.example/gh"),
            ("file:///home/me/releases", "file:///home/me/releases"),
            ("s3://my-builds/gh", "s3://my-builds/gh"),
        ] {
            assert_eq!(
                latest(Some(named)).unwrap(),
                format!("{at}/some_one/link-otter/releases/latest/download"),
                "{named}"
            );
        }
        let here = latest(Some("http://127.0.0.1:8080"));
        assert_eq!(here.is_ok(), cfg!(debug_assertions), "{here:?}");
        for bad in ["http://builds.example", "ftp://builds.example", "builds"] {
            let err = latest(Some(bad)).unwrap_err().to_string();
            assert!(err.starts_with("LNK_GITHUB_URL: "), "{bad}: {err}");
        }
    }

    #[test]
    fn repositories_are_github_urls() {
        for url in [
            "https://github.com/someone/link-otter",
            "https://github.com/someone/link-otter/",
            "https://github.com/someone/link-otter.git",
            "github.com/someone/link-otter",
        ] {
            assert_eq!(
                repository(url).unwrap(),
                "https://github.com/someone/link-otter",
                "{url}"
            );
        }
        for url in [
            "http://github.com/someone/link-otter",
            "https://gitlab.com/someone/link-otter",
            "https://github.com/someone",
            "https://github.com/someone/link-otter/releases",
            "https://github.com/../link-otter",
            "https://github.com/someone/link otter",
            "https://github.com.evil.example/someone/link-otter",
        ] {
            assert!(repository(url).is_err(), "{url}");
        }
        assert!(is_url("https://github.com/someone/link-otter"));
        assert!(is_url("github.com/someone/link-otter"));
        assert!(!is_url("tunnel"));
    }

    #[test]
    fn an_outside_module_takes_none_of_links_names() {
        check_name("otter").unwrap();
        check_name("my-harness2").unwrap();
        for name in ["agent", "measure", "core", "", "Otter", "../x", "a b"] {
            assert!(check_name(name).is_err(), "{name}");
        }
    }
}
