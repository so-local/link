//! `lnk bundle create`, end to end: resolve the spec, build each program
//! for each target, add the outside programs, then seal and sign, or
//! seal parts built before (`--from`).

use std::path::{Path, PathBuf};

use anyhow::{Context, bail, ensure};

use crate::cli::Build;
use crate::compile::{self, How};
use crate::seal::{self, Seal, Signer};
use crate::spec::Merged;
use crate::{discover, outside, run};

/// What `create` made.
pub struct Made {
    /// What it's kept by in `~/.lnk/data/bundles`.
    pub name: String,
    /// The bundle's root, which `LNK_RELEASES_URL=file://` names.
    pub out: PathBuf,
    pub version: String,
    pub assets: Vec<String>,
    pub signed: bool,
    /// This computer's programs, ready to run, when it was a target.
    pub bin: Option<PathBuf>,
}

/// Makes the bundle `spec` describes, saying each step with `note`. A
/// bundle that fails part way is removed, so the next try starts clean.
pub fn create(spec: Merged, here: &Path, note: &dyn Fn(String)) -> anyhow::Result<Made> {
    let out = spec.fields.out.clone();
    let existed = out.as_ref().is_some_and(|o| o.exists());
    let made = make(spec, here, note);
    if made.is_err()
        && !existed
        && let Some(out) = out
    {
        let _ = std::fs::remove_dir_all(out);
    }
    made
}

fn make(spec: Merged, here: &Path, note: &dyn Fn(String)) -> anyhow::Result<Made> {
    let sources: Vec<PathBuf> = if spec.fields.source.as_deref().unwrap_or_default().is_empty() {
        vec![here.to_path_buf()]
    } else {
        spec.fields.source.clone().unwrap_or_default()
    };
    let sources = sources
        .iter()
        .map(|s| {
            s.canonicalize()
                .with_context(|| format!("no source {}", s.display()))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let first = sources[0].clone();
    let public = sources
        .iter()
        .find(|s| s.join("core/cli/install.sh").is_file())
        .cloned();
    if !spec.fields.from.as_deref().unwrap_or_default().is_empty() {
        return parts(&spec, &first, public.as_deref(), note);
    }
    let build = spec.fields.build.unwrap_or_default();
    let workspace = crate::workspace_version(&first)?;
    let version = crate::version(
        build,
        spec.fields.version.as_deref(),
        &workspace,
        &crate::Commit::head(&first)?,
    )?;
    let signer = match spec.fields.signer.as_deref().filter(|n| *n != "none") {
        Some(named) => {
            let public = public.as_deref().context(
                "signing needs Link's public source among --source: its install.sh and signers",
            )?;
            let signer = Signer::read(named, public)?;
            seal::check_signer(build, &signer, public)?;
            if signer.made {
                note(format!(
                    "Made signer {named}, kept in {}. A machine trusts it with this line: {}",
                    seal::signer_dir(named)?.display(),
                    signer.lines.trim_end()
                ));
            }
            let kept = signer.secret.as_ref().is_some_and(|s| s.is_file());
            // Link's own signers' public lines are committed, so a runner
            // without their secret builds a part compiled to trust them,
            // unsigned, which the job holding the secret seals (`--from`).
            ensure!(
                spec.signing_secret.is_some() || kept || signer.links(),
                "signer {named} has no secret here: put it in {}, or give --signing-secret",
                signer
                    .secret
                    .as_ref()
                    .map_or_else(|| "a file".to_string(), |s| s.display().to_string())
            );
            Some(signer)
        }
        None => None,
    };
    ensure!(
        !matches!(build, Build::Dev | Build::Prod) || signer.is_some(),
        "a {} build's lnk trusts the bundle's own signer: give --signer",
        build.word()
    );
    let root = run("git", &["rev-parse", "--show-toplevel"], &first)?
        .trim()
        .to_string();
    let root = PathBuf::from(root);
    ensure!(
        committed(&root)?,
        "a bundle is built from a commit: commit or stash your changes first"
    );
    let commit = run("git", &["rev-parse", "HEAD"], &root)?
        .trim()
        .to_string();
    let out = match spec.fields.out.clone() {
        Some(out) => out,
        None => default_out(&spec.name.clone(), &version)?,
    };
    let dist = out.join("download").join(format!("v{version}"));
    ensure!(
        !dist.exists() || std::fs::read_dir(&dist)?.next().is_none(),
        "{} already holds a bundle",
        dist.display()
    );
    std::fs::create_dir_all(&dist)?;

    let found = discover::programs(&sources)?;
    let programs = if spec
        .fields
        .program
        .as_deref()
        .unwrap_or_default()
        .is_empty()
    {
        discover::default_programs(&found, &first, build == Build::Prod)
    } else {
        spec.fields.program.clone().unwrap_or_default()
    };
    for program in &programs {
        ensure!(
            found.contains_key(program),
            "no source builds {program}: add the workspace that does with --source"
        );
    }
    use link_actions::value::Words;
    let host = crate::cli::Target::here()?.word().to_string();
    // `host` is this computer's target.
    let mut targets: Vec<String> = Vec::new();
    for target in if spec.fields.target.as_deref().unwrap_or_default().is_empty() {
        vec![crate::cli::Target::Host]
    } else {
        spec.fields.target.clone().unwrap_or_default()
    } {
        let target = target.resolved()?.word().to_string();
        if !targets.contains(&target) {
            targets.push(target);
        }
    }
    let outside = spec
        .fields
        .outside
        .iter()
        .flatten()
        .map(|f| outside::read(f))
        .collect::<anyhow::Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    // A staging or dev build says its own version, so it's built from a
    // worktree of the commit with that version set, never the checkout:
    // the same worktree each time, so Cargo rebuilds only what changed.
    let worktree = if version == workspace {
        None
    } else {
        let target_dir = found
            .values()
            .find(|f| f.source == first)
            .or_else(|| found.values().next())
            .map(|f| f.target_dir.clone())
            .context("no source builds a program")?;
        Some(Worktree::new(
            &root,
            &commit,
            &target_dir,
            &sources,
            &version,
            public.as_deref(),
        )?)
    };
    let built_found = match &worktree {
        Some(w) => {
            let mut inside = discover::programs(&w.sources)?;
            for (name, f) in inside.iter_mut() {
                if let Some(original) = found.get(name) {
                    f.target_dir = original.target_dir.clone();
                }
            }
            inside
        }
        None => found.clone(),
    };
    let dev_line = match (&signer, build) {
        (Some(s), Build::Dev | Build::Prod) => Some(s.own_line()?.to_string()),
        _ => None,
    };
    let versioned = match &worktree {
        Some(w) => w.sources[0].clone(),
        None => first.clone(),
    };
    // Each component as the build sees it: in the worktree, when the build
    // is made from one.
    let components = spec
        .fields
        .components
        .iter()
        .flatten()
        .map(|c| match &worktree {
            Some(w) => w.inside(c),
            None => Ok(c.clone()),
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let how = How {
        build,
        version: &version,
        own_signer: dev_line.as_deref(),
        components: &components,
        versioned: &versioned,
    };
    let mut assets = Vec::new();
    for target in &targets {
        note(format!(
            "Building {} {version} for {target}",
            programs.join(", ")
        ));
        let built = compile::build(&programs, &built_found, target, &how)?;
        for program in &programs {
            assets.push(compile::pack(&built[program], program, target, &dist)?);
        }
        if !outside.is_empty() {
            let work = out.join("outside");
            for program in &outside {
                note(format!(
                    "Fetching {} {} for {target}",
                    program.name, program.version
                ));
                let file = program.fetch(target, &work)?;
                assets.push(compile::pack(&file, &program.name, target, &dist)?);
            }
            std::fs::remove_dir_all(&work).ok();
        }
    }
    drop(worktree);

    let secret = spec.signing_secret.clone().or_else(|| {
        signer
            .as_ref()
            .and_then(|s| s.secret.clone())
            .filter(|s| s.is_file())
    });
    let signed = match (&signer, &secret) {
        (Some(signer), Some(key)) => {
            note(format!("Sealing and signing as {}", signer.name));
            seal::seal(
                &dist,
                &Seal {
                    version: &version,
                    commit: &commit,
                    public: public
                        .as_deref()
                        .context("signing needs Link's public source")?,
                    signer,
                    secret: key,
                    files: spec.fields.file.as_deref().unwrap_or_default(),
                },
            )?;
            true
        }
        _ => {
            ensure!(
                spec.fields.file.as_deref().unwrap_or_default().is_empty(),
                "--file is sealed with the bundle: give --signer"
            );
            std::fs::write(dist.join("VERSION"), format!("{version}\n"))?;
            false
        }
    };
    latest(&out, &dist)?;
    // Built for this computer, its programs are ready to run from
    // `<out>/bin`: a dev bundle's `lnk` there trusts the bundle's signer,
    // so it makes boxes from the bundle.
    let bin = targets
        .contains(&host)
        .then(|| unpack(&dist, &host, &out.join("bin")))
        .transpose()?;
    Ok(Made {
        name: spec.name.clone(),
        out,
        version,
        assets,
        signed,
        bin,
    })
}

/// Whether every tracked file at `root` is as its commit has it. A file
/// git doesn't track (a log a run is writing, a build's output) is never
/// built, so it doesn't count.
fn committed(root: &Path) -> anyhow::Result<bool> {
    Ok(run(
        "git",
        &["status", "--porcelain", "--untracked-files=no"],
        root,
    )?
    .trim()
    .is_empty())
}

/// Where a bundle goes when `--out` names nowhere, the layout's:
/// `~/.lnk/data/bundles/<name>/<version>`.
pub fn default_out(name: &str, version: &str) -> anyhow::Result<PathBuf> {
    Ok(link_files::layout::Hidden::Bundles
        .try_path()?
        .join(name)
        .join(version))
}

/// A kept bundle, by its name (the newest of it) or its version.
pub fn kept(named: &str) -> anyhow::Result<Option<PathBuf>> {
    let root = link_files::layout::Hidden::Bundles.try_path()?;
    let newest = |dir: &Path| -> Option<PathBuf> {
        std::fs::read_dir(dir)
            .ok()?
            .filter_map(Result::ok)
            .filter(|e| e.path().is_dir())
            .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
            .map(|e| e.path())
    };
    if root.join(named).is_dir() {
        return Ok(newest(&root.join(named)));
    }
    for entry in std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
    {
        let candidate = entry.path().join(named);
        if candidate.is_dir() {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

/// Every archive for `target` in `dist`, unpacked into `bin`.
fn unpack(dist: &Path, target: &str, bin: &Path) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(bin)?;
    for name in crate::files(dist)? {
        if name.ends_with(&format!("-{target}.tar.gz")) {
            run(
                "tar",
                &[
                    "-xzf",
                    &dist.join(&name).to_string_lossy(),
                    "-C",
                    &bin.to_string_lossy(),
                ],
                dist,
            )?;
        }
    }
    Ok(bin.to_path_buf())
}

/// Seals programs built before, by other runners, into one bundle.
fn parts(
    spec: &Merged,
    first: &Path,
    public: Option<&Path>,
    note: &dyn Fn(String),
) -> anyhow::Result<Made> {
    let public = public.context("sealing needs Link's public source among --source")?;
    let named = spec
        .fields
        .signer
        .as_deref()
        .context("--from seals parts: give --signer")?;
    let given = spec.signing_secret.clone();
    let mut version = None::<String>;
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    for from in spec.fields.from.as_deref().unwrap_or_default() {
        let dir = crate::show::version_dir(from)?;
        let said = std::fs::read_to_string(dir.join("VERSION"))
            .with_context(|| format!("{} has no VERSION: it isn't a bundle's part", dir.display()))?
            .trim()
            .to_string();
        match &version {
            Some(v) => ensure!(*v == said, "the parts are of versions {v} and {said}"),
            None => version = Some(said),
        }
        for name in crate::files(&dir)? {
            if name.ends_with(".tar.gz") {
                ensure!(
                    !found.iter().any(|(n, _)| *n == name),
                    "two parts hold {name}"
                );
                found.push((name.clone(), dir.join(name)));
            } else {
                ensure!(
                    name == "VERSION",
                    "{} holds {name}: a part is programs alone",
                    dir.display()
                );
            }
        }
    }
    let version = version.context("--from names no parts")?;
    same_programs(found.iter().map(|(n, _)| n.as_str()))?;
    let build = if version.contains("-staging.") {
        Build::Staging
    } else if version.contains("-dev.") {
        Build::Dev
    } else {
        Build::Prod
    };
    let signer = Signer::read(named, public)?;
    seal::check_signer(build, &signer, public)?;
    let key = given
        .or_else(|| signer.secret.clone().filter(|s| s.is_file()))
        .context("--from seals with a signer: name one kept here, or give --signing-secret")?;
    let key = key.as_path();
    let out = match spec.fields.out.clone() {
        Some(out) => out,
        None => default_out(&spec.name.clone(), &version)?,
    };
    let dist = out.join("download").join(format!("v{version}"));
    ensure!(!dist.exists(), "{} already holds a bundle", dist.display());
    std::fs::create_dir_all(&dist)?;
    for (name, path) in &found {
        std::fs::copy(path, dist.join(name))?;
    }
    let commit = run("git", &["rev-parse", "HEAD"], first)?
        .trim()
        .to_string();
    note(format!(
        "Sealing {} parts as {}",
        spec.fields.from.as_deref().unwrap_or_default().len(),
        signer.name
    ));
    seal::seal(
        &dist,
        &Seal {
            version: &version,
            commit: &commit,
            public,
            signer: &signer,
            secret: key,
            files: spec.fields.file.as_deref().unwrap_or_default(),
        },
    )?;
    latest(&out, &dist)?;
    Ok(Made {
        name: spec.name.clone(),
        out,
        version,
        assets: found.into_iter().map(|(n, _)| n).collect(),
        signed: true,
        bin: None,
    })
}

/// Parts sealed together hold the same programs for every target, so a
/// part missing a program, or built from another spec, is refused before
/// it's signed.
fn same_programs<'a>(names: impl Iterator<Item = &'a str>) -> anyhow::Result<()> {
    let mut by_target = std::collections::BTreeMap::<String, Vec<String>>::new();
    for name in names {
        let target = crate::show::target_of(name)
            .map(|t| link_actions::value::Words::word(t).to_string())
            .with_context(|| format!("{name} names no target a bundle builds"))?;
        let program = name
            .strip_suffix(&format!("-{target}.tar.gz"))
            .unwrap_or(name)
            .to_string();
        by_target.entry(target).or_default().push(program);
    }
    let mut targets = by_target.iter_mut();
    let Some((first, want)) = targets.next() else {
        bail!("the parts hold no programs");
    };
    want.sort();
    for (target, got) in targets {
        got.sort();
        ensure!(
            got == want,
            "the parts for {target} hold {}, and for {first} {}",
            got.join(", "),
            want.join(", ")
        );
    }
    Ok(())
}

/// The bundle again at `latest/download`, where `lnk` looks when no
/// version is asked for.
fn latest(out: &Path, dist: &Path) -> anyhow::Result<()> {
    let latest = out.join("latest/download");
    if latest.exists() {
        std::fs::remove_dir_all(&latest)?;
    }
    std::fs::create_dir_all(&latest)?;
    for name in crate::files(dist)? {
        std::fs::copy(dist.join(&name), latest.join(&name))?;
    }
    Ok(())
}

/// The worktree a staging or dev build compiles, one per checkout, kept
/// in the first source's target folder at `bundle/source` so each build
/// compiles the same paths: moved from commit to commit, a file the
/// commits share keeps its time, and Cargo rebuilds only what changed.
/// While a build holds it, `bundle/source.lock` refuses another.
struct Worktree {
    sources: Vec<PathBuf>,
    /// The checkout's root, and the worktree's in its place.
    root: PathBuf,
    path: PathBuf,
    /// Held for the build: closing it lets the next build in.
    _lock: std::fs::File,
}

/// Where the checkout whose builds use `target_dir` keeps its worktree,
/// and the lock a build holds while using it.
fn worktree_place(target_dir: &Path) -> (PathBuf, PathBuf) {
    let dir = target_dir.join("bundle");
    (dir.join("source"), dir.join("source.lock"))
}

impl Worktree {
    fn new(
        root: &Path,
        commit: &str,
        target_dir: &Path,
        sources: &[PathBuf],
        version: &str,
        public: Option<&Path>,
    ) -> anyhow::Result<Self> {
        let (path, lock) = checkout(root, commit, target_dir)?;
        let mut tree = Worktree {
            sources: Vec::new(),
            root: root.to_path_buf(),
            path: path.clone(),
            _lock: lock,
        };
        for source in sources {
            let inside = source
                .strip_prefix(root)
                .with_context(|| format!("{} isn't in {}", source.display(), root.display()))?;
            let mapped = path.join(inside);
            run("cargo", &["fetch", "--locked", "--quiet"], &mapped)?;
            tree.sources.push(mapped);
        }
        let public =
            public.context("a staging or dev build sets its version with Link's public source")?;
        let set = path
            .join(public.strip_prefix(root)?)
            .join("scripts/release/set-version.sh");
        let mut args = vec![set.to_string_lossy().into_owned()];
        args.push(tree.sources[0].to_string_lossy().into_owned());
        args.push(version.to_string());
        args.extend(
            tree.sources[1..]
                .iter()
                .map(|s| s.to_string_lossy().into_owned()),
        );
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        run("bash", &args, &tree.sources[0])?;
        Ok(tree)
    }
}

impl Worktree {
    /// `path`, a folder of the checkout, at its place in the worktree.
    fn inside(&self, path: &Path) -> anyhow::Result<PathBuf> {
        let inside = path
            .strip_prefix(&self.root)
            .with_context(|| format!("{} isn't in {}", path.display(), self.root.display()))?;
        Ok(self.path.join(inside))
    }
}

/// Takes the lock on the worktree for `target_dir`, then puts `commit`
/// in it exactly: made when there's none, or moved there, with every
/// change and file the commit doesn't track removed. Returns its path
/// and the held lock.
fn checkout(
    root: &Path,
    commit: &str,
    target_dir: &Path,
) -> anyhow::Result<(PathBuf, std::fs::File)> {
    let (path, lock_path) = worktree_place(target_dir);
    std::fs::create_dir_all(lock_path.parent().unwrap_or(target_dir))?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("opening {}", lock_path.display()))?;
    ensure!(
        link_files::fs::try_lock(&lock)?,
        "another bundle build is using {}: wait for it to finish",
        path.display()
    );
    let common = |dir: &Path| -> anyhow::Result<PathBuf> {
        let said = run(
            "git",
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            dir,
        )?;
        Ok(PathBuf::from(said.trim()).canonicalize()?)
    };
    let ours = path.join(".git").exists()
        && common(&path).ok().as_deref() == Some(common(root)?.as_path());
    if ours {
        let at = |args: &[&str]| run("git", args, &path);
        at(&["checkout", "--quiet", "--detach", "--force", commit])?;
        at(&["clean", "-ffdxq"])?;
    } else {
        if path.exists() {
            std::fs::remove_dir_all(&path)
                .with_context(|| format!("removing {}", path.display()))?;
        }
        run("git", &["worktree", "prune"], root)?;
        run(
            "git",
            &[
                "worktree",
                "add",
                "--quiet",
                "--detach",
                &path.to_string_lossy(),
                commit,
            ],
            root,
        )?;
    }
    Ok((path, lock))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_hold_one_set_of_programs() {
        let linux = "x86_64-unknown-linux-musl";
        let mac = "aarch64-apple-darwin";
        let names = |programs: &[(&str, &str)]| {
            programs
                .iter()
                .map(|(p, t)| crate::asset(p, t))
                .collect::<Vec<_>>()
        };
        let whole = names(&[
            ("lnk", linux),
            ("lnk-box", linux),
            ("lnk-box", mac),
            ("lnk", mac),
        ]);
        assert!(same_programs(whole.iter().map(String::as_str)).is_ok());
        let short = names(&[("lnk", linux), ("lnk-box", linux), ("lnk", mac)]);
        let refused = same_programs(short.iter().map(String::as_str)).unwrap_err();
        assert!(refused.to_string().contains("lnk-box"), "{refused}");
        assert!(same_programs(["lnk-core-sparc.tar.gz"].into_iter()).is_err());
        assert!(same_programs(std::iter::empty()).is_err());
    }

    #[test]
    fn only_a_tracked_change_stops_a_build() {
        let dir = link_test_kit::temp::TempDir::new("bundle-committed");
        let root = dir.path();
        let git = |args: &[&str]| run("git", args, root).unwrap();
        git(&["init", "-q"]);
        std::fs::write(root.join("built.rs"), "one").unwrap();
        git(&["add", "built.rs"]);
        git(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "commit",
            "-qm",
            "one",
        ]);
        std::fs::write(root.join("run.log"), "a run's log").unwrap();
        assert!(committed(root).unwrap(), "an untracked file is never built");
        std::fs::write(root.join("built.rs"), "two").unwrap();
        assert!(!committed(root).unwrap(), "a tracked change is");
    }

    #[test]
    fn one_worktree_follows_the_commits() {
        let dir = link_test_kit::temp::TempDir::new("bundle-worktree");
        let root = dir.path().join("repo");
        let target = dir.path().join("target");
        std::fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| run("git", args, &root).unwrap();
        let commit = |message: &str| {
            git(&["add", "-A"]);
            git(&[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.com",
                "commit",
                "-qm",
                message,
            ]);
            git(&["rev-parse", "HEAD"]).trim().to_string()
        };
        git(&["init", "-q"]);
        std::fs::write(root.join("kept.rs"), "kept").unwrap();
        std::fs::write(root.join("changed.rs"), "one").unwrap();
        let one = commit("one");
        std::fs::write(root.join("changed.rs"), "two").unwrap();
        let two = commit("two");

        let (path, lock) = checkout(&root, &one, &target).unwrap();
        assert_eq!(path, worktree_place(&target).0);
        let refused = checkout(&root, &two, &target).unwrap_err();
        assert!(
            refused.to_string().contains("another bundle build"),
            "{refused}"
        );
        // A build stamps its version and leaves files behind.
        std::fs::write(path.join("changed.rs"), "stamped").unwrap();
        std::fs::write(path.join("stray.rs"), "left").unwrap();
        let kept_time = |p: &Path| std::fs::metadata(p).unwrap().modified().unwrap();
        let before = kept_time(&path.join("kept.rs"));
        drop(lock);

        let (again, _lock) = checkout(&root, &two, &target).unwrap();
        assert_eq!(again, path, "every build uses one place");
        let read = |name: &str| std::fs::read_to_string(path.join(name)).unwrap();
        assert_eq!(read("changed.rs"), "two");
        assert_eq!(read("kept.rs"), "kept");
        assert!(
            !path.join("stray.rs").exists(),
            "a file the commit lacks is removed"
        );
        assert_eq!(
            kept_time(&path.join("kept.rs")),
            before,
            "a file the commits share keeps its time"
        );
        let at = run("git", &["rev-parse", "HEAD"], &path).unwrap();
        assert_eq!(at.trim(), two);
    }
}
