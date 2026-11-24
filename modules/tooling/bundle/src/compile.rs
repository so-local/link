//! Building the programs for each target, each workspace's in one Cargo
//! invocation, and packing each in its archive.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, bail, ensure};

use crate::cli::Build;
use crate::discover::Found;

/// What a build needs beyond the programs: what its `lnk` is and trusts.
pub struct How<'a> {
    pub build: Build,
    pub version: &'a str,
    /// A dev or prod build's own signer, the one allowed-signers line its
    /// `lnk` trusts.
    pub own_signer: Option<&'a str>,
    /// The component folders its `lnk` lists beyond its own workspace's,
    /// the spec's `components`, and no others.
    pub components: &'a [PathBuf],
    /// The workspace built at the bundle's version: another keeps its own.
    pub versioned: &'a Path,
}

/// Builds every one of `programs` for `target`, each workspace's in one
/// Cargo invocation, and returns each built file by its program. One
/// invocation unifies features as a workspace build does, so a crate the
/// programs share compiles once instead of once per program.
pub fn build(
    programs: &[String],
    found: &BTreeMap<String, Found>,
    target: &str,
    how: &How,
) -> anyhow::Result<BTreeMap<String, PathBuf>> {
    let mut built = BTreeMap::new();
    for group in groups(programs, found) {
        cargo(&group, found, target, how)?;
        for program in group {
            let f = &found[program];
            let file = f.target_dir.join(target).join("release").join(program);
            ensure!(file.is_file(), "cargo built no {}", file.display());
            entitle(program, f, target, &file)?;
            if link_core::release::target().ok() == Some(target) && f.source == how.versioned {
                check(program, &file, how)?;
            }
            built.insert(program.clone(), file);
        }
    }
    Ok(built)
}

/// `programs` by the workspace and target folder that build them, in the
/// order each workspace is first named.
fn groups<'a>(programs: &'a [String], found: &BTreeMap<String, Found>) -> Vec<Vec<&'a String>> {
    let mut groups: Vec<((&Path, &Path), Vec<&String>)> = Vec::new();
    for program in programs {
        let f = &found[program];
        let key = (f.source.as_path(), f.target_dir.as_path());
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, group)) => group.push(program),
            None => groups.push((key, vec![program])),
        }
    }
    groups.into_iter().map(|(_, group)| group).collect()
}

/// One `cargo build` (or `zigbuild`) of `group`, programs of one
/// workspace, for `target`.
fn cargo(
    group: &[&String],
    found: &BTreeMap<String, Found>,
    target: &str,
    how: &How,
) -> anyhow::Result<()> {
    let first = &found[group[0]];
    let here = link_core::release::target().ok();
    // Cargo builds for this machine's target, and a Mac for either Mac's;
    // any other target needs zig's cross linker.
    let apple = |t: &str| t.ends_with("-apple-darwin");
    let native = here == Some(target) || (apple(target) && here.is_some_and(apple));
    let cargo_verb = if native { "build" } else { "zigbuild" };
    let mut command = link_secretive::process::command("cargo");
    command
        .arg(cargo_verb)
        .arg("--manifest-path")
        .arg(first.source.join("Cargo.toml"))
        .args(["--release", "--locked"]);
    let mut packages: Vec<&str> = Vec::new();
    for program in group {
        let package = found[*program].package.as_str();
        if !packages.contains(&package) {
            packages.push(package);
        }
    }
    for package in packages {
        command.args(["-p", package]);
    }
    for program in group {
        command.args(["--bin", program]);
    }
    command
        .args(["--target", target])
        .arg("--target-dir")
        .arg(&first.target_dir)
        .env("LNK_BUILD", how.build.word())
        .env_remove("LNK_DEV_SIGNER")
        .env_remove("LNK_PROD_SIGNER")
        .env_remove("LNK_COMPONENTS")
        .stdin(Stdio::null());
    if !how.components.is_empty() {
        command.env(
            "LNK_COMPONENTS",
            std::env::join_paths(how.components).context("a component folder's path")?,
        );
    }
    if let Some(signer) = how.own_signer {
        let variable = match how.build {
            Build::Prod => "LNK_PROD_SIGNER",
            _ => "LNK_DEV_SIGNER",
        };
        command.env(variable, signer);
    }
    let names = group
        .iter()
        .map(|p| p.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let status = command
        .status()
        .with_context(|| format!("running cargo {cargo_verb} for {names}"))?;
    if !status.success() {
        if cargo_verb == "zigbuild" && !zigbuild_installed() {
            bail!(
                "building {names} for {target} failed: building for another machine's target needs cargo zigbuild (pip install ziglang; cargo install cargo-zigbuild)"
            );
        }
        bail!("building {names} for {target} failed (above)");
    }
    Ok(())
}

/// A macOS program that needs an Apple entitlement keeps it beside its
/// manifest (`entitlements.plist`), and is signed with it here.
fn entitle(program: &str, found: &Found, target: &str, built: &Path) -> anyhow::Result<()> {
    if !target.ends_with("-apple-darwin") {
        return Ok(());
    }
    let Some(dir) = found.manifest.parent() else {
        return Ok(());
    };
    let entitlements = dir.join("entitlements.plist");
    if !entitlements.is_file() {
        return Ok(());
    }
    crate::run(
        "codesign",
        &[
            "--force",
            "--sign",
            "-",
            "--entitlements",
            &entitlements.to_string_lossy(),
            &built.to_string_lossy(),
        ],
        dir,
    )
    .with_context(|| format!("{program} needs codesign, so {target} is built on a Mac"))?;
    Ok(())
}

/// A program built for this machine runs here: `lnk` and each module say
/// the bundle's version, and `lnk` says it's the build it was made as.
fn check(program: &str, built: &Path, how: &How) -> anyhow::Result<()> {
    if program != "lnk" && !program.starts_with("lnk-") {
        return Ok(());
    }
    let said = crate::run(
        &built.to_string_lossy(),
        &["--version"],
        built.parent().unwrap_or(Path::new("/")),
    )?;
    let first = said.lines().next().unwrap_or_default();
    ensure!(
        first.split_whitespace().last() == Some(how.version),
        "its {program} says {first:?}, not version {}",
        how.version
    );
    if program == "lnk" {
        ensure!(
            says_its_build(how.build, &said),
            "its lnk isn't a {} build: it says {said:?}",
            how.build.word()
        );
    }
    Ok(())
}

/// Whether `said`, an `lnk --version`, names `build` on its second line,
/// indented as `lnk` prints it: each stage says what [`link_core::Build`]
/// says it trusts, and a build from source says none.
fn says_its_build(build: Build, said: &str) -> bool {
    let core = match build {
        Build::Dev => link_core::Build::Dev,
        Build::Staging => link_core::Build::Staging,
        Build::Prod => link_core::Build::Prod,
    };
    let line = said.lines().nth(1).unwrap_or_default();
    core.trusts().is_some_and(|trusts| line.trim() == trusts)
}

/// `built` packed alone in its archive in `dist`, under its own name.
pub fn pack(built: &Path, program: &str, target: &str, dist: &Path) -> anyhow::Result<String> {
    let asset = crate::asset(program, target);
    let dir = built.parent().context("a built program has a folder")?;
    let name = built
        .file_name()
        .context("a built program has a name")?
        .to_string_lossy();
    crate::run(
        "tar",
        &[
            // A Mac's tar would otherwise add its files' extended
            // attributes, which Linux's tar warns of on every install.
            "--no-xattrs",
            "-czf",
            &dist.join(&asset).to_string_lossy(),
            "-C",
            &dir.to_string_lossy(),
            &name,
        ],
        dist,
    )?;
    Ok(asset)
}

/// Whether `cargo zigbuild` runs here, so a cross build's failure is
/// blamed on it only when it's missing.
fn zigbuild_installed() -> bool {
    link_secretive::process::command("cargo")
        .args(["zigbuild", "--help"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_workspace_builds_its_programs_together_in_order() {
        let found = |source: &str, package: &str| Found {
            package: package.into(),
            manifest: PathBuf::from(format!("/{source}/{package}/Cargo.toml")),
            target_dir: PathBuf::from(format!("/{source}/target")),
            source: PathBuf::from(format!("/{source}")),
            module: true,
            released: true,
        };
        let found = BTreeMap::from([
            ("lnk".to_string(), found("public", "link-cli")),
            ("lnk-env".to_string(), found("public", "link-env")),
            ("lnk-extra".to_string(), found("private", "link-extra")),
            ("lnk-box".to_string(), found("public", "link-box")),
        ]);
        let programs: Vec<String> = ["lnk", "lnk-extra", "lnk-env", "lnk-box"]
            .map(String::from)
            .into();
        let groups: Vec<Vec<&str>> = groups(&programs, &found)
            .into_iter()
            .map(|g| g.into_iter().map(String::as_str).collect())
            .collect();
        assert_eq!(
            groups,
            [vec!["lnk", "lnk-env", "lnk-box"], vec!["lnk-extra"]]
        );
    }

    #[test]
    fn each_stage_says_its_own_and_source_says_none() {
        // As `lnk --version` prints it: each line after the first indented.
        let said = |line: &str| format!("lnk 0.14.0\n  {line}\n  /usr/local/bin/lnk (installed)\n");
        let prod = said("a prod build: it trusts only the signer it was built with");
        let dev = said("a dev build: it trusts the signer it was built with as well as Link's");
        let staging = said("a staging build: it trusts the staging signer as well as Link's");
        let source = "lnk 0.14.0\n  /home/me/link/target/debug/lnk (a build, not installed)\n";
        assert!(says_its_build(Build::Prod, &prod));
        assert!(says_its_build(Build::Dev, &dev));
        assert!(says_its_build(Build::Staging, &staging));
        for build in [Build::Prod, Build::Dev, Build::Staging] {
            assert!(!says_its_build(build, source), "{}", build.word());
        }
        assert!(!says_its_build(Build::Prod, &dev));
        assert!(!says_its_build(Build::Dev, &staging));
    }
}
