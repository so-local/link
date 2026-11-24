//! Install caller-selected source workspaces with Cargo, retaining Cargo's uninstall records.

use std::path::{Path, PathBuf};

use anyhow::{Context, ensure};
use serde::Deserialize;

use super::*;

#[derive(Deserialize)]
struct Config {
    cargo: String,
    binary: String,
    fixtures: String,
    bin_directory: String,
    /// The file Cargo marks its target folder with.
    target_tag: String,
    /// The file Cargo keeps in an installation's root.
    install_record: String,
    /// The target folder's subfolder of the profile named `debug_profile`,
    /// the one profile whose subfolder isn't its name.
    debug_directory: String,
    debug_profile: String,
    /// The profile `cargo install` builds with.
    install_profile: String,
    /// The file beside a module's `Cargo.toml` that declares it.
    declaration: String,
}

/// Where a source build's modules go: where `lnk` itself is, so it finds
/// them beside it and no second install mixes versions with it.
#[derive(Debug, PartialEq)]
enum Where {
    /// `<dir>/<profile's folder>/lnk`, a Cargo target folder: modules are
    /// built into it with `profile`, beside `lnk`.
    Target {
        dir: PathBuf,
        profile: String,
        bin: PathBuf,
    },
    /// `<root>/bin/lnk`, a Cargo installation: modules are installed into
    /// `root`.
    Installed { root: PathBuf },
}

impl Where {
    /// Where `lnk`, at `exe`, is: else why its modules have nowhere to go.
    fn of(config: &Config, exe: &Path) -> anyhow::Result<Where> {
        let exe = exe.canonicalize().unwrap_or_else(|_| exe.to_path_buf());
        let bin = exe.parent().context("finding lnk's folder")?;
        let above = bin.parent().context("finding lnk's folder")?;
        if above.join(&config.target_tag).is_file() {
            let folder = bin
                .file_name()
                .and_then(|n| n.to_str())
                .context("reading lnk's profile folder")?;
            let profile = match folder == config.debug_directory {
                true => config.debug_profile.clone(),
                false => folder.to_string(),
            };
            return Ok(Where::Target {
                dir: above.to_path_buf(),
                profile,
                bin: bin.to_path_buf(),
            });
        }
        if bin
            .file_name()
            .is_some_and(|n| n == config.bin_directory.as_str())
            && above.join(&config.install_record).is_file()
        {
            return Ok(Where::Installed {
                root: above.to_path_buf(),
            });
        }
        anyhow::bail!(
            "this lnk is at {}, neither in a Cargo target folder nor a Cargo installation, so its modules have nowhere to go beside it. Run the lnk your checkout builds (cargo build -p link-cli, then target/debug/lnk up --source .), or install it with sh core/cli/install.sh --source",
            exe.display()
        )
    }

    /// The folder its modules land in, beside `lnk`.
    fn bin(&self, config: &Config) -> PathBuf {
        match self {
            Where::Target { bin, .. } => bin.clone(),
            Where::Installed { root } => root.join(&config.bin_directory),
        }
    }
}

#[derive(Deserialize)]
struct Metadata {
    workspace_root: PathBuf,
    workspace_members: Vec<String>,
    target_directory: PathBuf,
    packages: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    manifest_path: PathBuf,
    targets: Vec<Target>,
    #[serde(default)]
    metadata: Option<CargoMetadata>,
}

#[derive(Default, Deserialize)]
struct CargoMetadata {
    #[serde(default)]
    link: LinkMetadata,
}

#[derive(Default, Deserialize)]
struct LinkMetadata {
    #[serde(default)]
    source: SourcePlatform,
}

/// Optional host restrictions owned by the package, not the installer.
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourcePlatform {
    #[serde(default)]
    os: Vec<String>,
    #[serde(default)]
    arch: Vec<String>,
}

impl SourcePlatform {
    fn supports(&self, os: &str, arch: &str) -> bool {
        (self.os.is_empty() || self.os.iter().any(|value| value == os))
            && (self.arch.is_empty() || self.arch.iter().any(|value| value == arch))
    }
}

struct Plan<'a> {
    programs: Vec<&'a Package>,
    skipped: Vec<&'a Package>,
}

#[derive(Deserialize)]
struct Target {
    name: String,
    kind: Vec<String>,
}

impl Metadata {
    fn plan(&self, config: &Config, os: &str, arch: &str) -> anyhow::Result<Plan<'_>> {
        let (programs, skipped): (Vec<_>, Vec<_>) =
            self.programs(config)?.into_iter().partition(|package| {
                package
                    .metadata
                    .as_ref()
                    .is_none_or(|m| m.link.source.supports(os, arch))
            });
        Ok(Plan { programs, skipped })
    }

    fn programs(&self, config: &Config) -> anyhow::Result<Vec<&Package>> {
        let prefix = format!("{}-", config.binary);
        let mut programs: Vec<_> = self
            .packages
            .iter()
            .filter(|package| {
                self.workspace_members.contains(&package.id)
                    && package
                        .manifest_path
                        .strip_prefix(&self.workspace_root)
                        .is_ok_and(|p| {
                            !p.components()
                                .any(|c| c.as_os_str() == config.fixtures.as_str())
                        })
                    && package.targets.iter().any(|target| {
                        target.kind.iter().any(|kind| kind == "bin")
                            && target.name.starts_with(&prefix)
                            && !package.targets.iter().any(|t| t.name == config.binary)
                    })
            })
            .collect();
        ensure!(
            !programs.is_empty(),
            "this workspace contains no source module packages"
        );
        programs.sort_by_key(|p| p.name.as_str());
        Ok(programs)
    }
}

/// `lnk up`'s flags with the checkout a source-built `lnk` comes from
/// first among its workspaces, unless it's named already: that checkout
/// is always one of them, so `--source` names only the others.
pub(super) fn with_checkout(mut asked: cli::CoreUpArgs) -> cli::CoreUpArgs {
    let same = |a: &Path, b: &Path| a.canonicalize().ok() == b.canonicalize().ok();
    if let Some(root) = link_core::CHECKOUT.map(|checkout| checkout.root())
        && !asked.source.iter().any(|given| same(given, &root))
    {
        asked.source.insert(0, root);
    }
    asked
}

/// The modules `--module` names and every module they need, by the
/// module dependencies each one's declaration requires here, followed
/// until none is new; nothing when `--module` names none, which means every module.
fn with_needs(
    config: &Config,
    plans: &[Plan<'_>],
    named: &[String],
) -> anyhow::Result<Vec<String>> {
    let prefix = format!("{}-", config.binary);
    let declared = |name: &str| -> anyhow::Result<Vec<String>> {
        let package = plans
            .iter()
            .flat_map(|plan| &plan.programs)
            .find(|package| {
                package
                    .targets
                    .iter()
                    .any(|t| t.name.strip_prefix(&prefix) == Some(name))
            });
        let Some(file) = package
            .and_then(|p| p.manifest_path.parent())
            .map(|dir| dir.join(&config.declaration))
            .filter(|file| file.is_file())
        else {
            return Ok(Vec::new());
        };
        let source = link_component::Source::parse(&std::fs::read_to_string(&file)?)
            .with_context(|| format!("reading {}", file.display()))?;
        Ok(required_here(&source))
    };
    let mut wanted: Vec<String> = named.to_vec();
    let mut index = 0;
    while let Some(name) = wanted.get(index).cloned() {
        for need in declared(&name)? {
            if !wanted.contains(&need) {
                wanted.push(need);
            }
        }
        index += 1;
    }
    Ok(wanted)
}

/// The modules a module's declaration requires on this machine, as
/// `lnk module add` follows them: what it may use but doesn't require,
/// such as the modules the sandbox runs for, is left out.
fn required_here(source: &link_component::Source) -> Vec<String> {
    let here = |when: &link_component::When| {
        (when.os.is_empty() || when.os.iter().any(|s| s == std::env::consts::OS))
            && (when.arch.is_empty() || when.arch.iter().any(|s| s == std::env::consts::ARCH))
            && when.options.is_empty()
    };
    source
        .requires
        .iter()
        .filter(|requirement| requirement.when.as_ref().is_none_or(here))
        .filter_map(
            |requirement| match source.dependencies.get(&requirement.dependency) {
                Some(link_component::Dependency::Module { id, .. }) => Some(id.clone()),
                _ => None,
            },
        )
        .collect()
}

/// The modules `--module` names: `--module a,b` names several, as
/// repeating it does.
pub(super) fn modules_named(given: &[String]) -> Vec<String> {
    given
        .iter()
        .flat_map(|names| names.split(','))
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

pub(super) fn install(out: &mut Out, asked: &cli::CoreUpArgs) -> Result<Done, Report> {
    run(out, asked).map_err(|err| failed(sentence!("Source installation did not complete."), err))
}

fn read_workspace(config: &Config, path: &Path) -> anyhow::Result<Metadata> {
    let path = path.canonicalize().context("finding the source checkout")?;
    let metadata = link_secretive::process::command(&config.cargo)
        .current_dir(&path)
        .args(["metadata", "--locked", "--no-deps", "--format-version", "1"])
        .arg("--manifest-path")
        .arg(path.join("Cargo.toml"))
        .output()
        .context("running Cargo; source installation requires Rust and Cargo")?;
    ensure!(
        metadata.status.success(),
        "Cargo could not read the checkout: {}",
        String::from_utf8_lossy(&metadata.stderr)
    );
    serde_json::from_slice(&metadata.stdout).context("reading Cargo workspace metadata")
}

fn run(out: &mut Out, asked: &cli::CoreUpArgs) -> anyhow::Result<Done> {
    let config: Config = toml::from_str(include_str!("../source.toml"))?;
    let place = Where::of(
        &config,
        &std::env::current_exe().context("finding this lnk")?,
    )?;
    let workspaces: Vec<_> = asked
        .source
        .iter()
        .map(|path| read_workspace(&config, path))
        .collect::<anyhow::Result<_>>()?;
    let mut roots = std::collections::HashSet::new();
    for workspace in &workspaces {
        ensure!(
            roots.insert(&workspace.workspace_root),
            "the same source workspace was selected more than once: {}",
            workspace.workspace_root.display()
        );
    }
    // Validate every workspace and binary name before changing any installation.
    let plans: Vec<_> = workspaces
        .iter()
        .map(|metadata| metadata.plan(&config, std::env::consts::OS, std::env::consts::ARCH))
        .collect::<anyhow::Result<_>>()?;
    let target = &workspaces
        .first()
        .context("no source workspace was selected")?
        .target_directory;
    let mut binaries = std::collections::BTreeMap::new();
    let mut programs = Vec::new();
    let prefix = format!("{}-", config.binary);
    let wanted = with_needs(&config, &plans, &asked.module)?;
    for (workspace, plan) in workspaces.iter().zip(&plans) {
        for package in plan.programs.iter().chain(&plan.skipped) {
            let supported = plan.programs.iter().any(|p| p.id == package.id);
            for binary in package
                .targets
                .iter()
                .filter(|t| t.kind.iter().any(|k| k == "bin") && t.name.starts_with(&prefix))
            {
                let name = binary.name.strip_prefix(&prefix).expect("module prefix");
                ensure!(
                    binaries.insert(name, supported).is_none(),
                    "more than one source package installs {}",
                    binary.name
                );
                if supported && (wanted.is_empty() || wanted.iter().any(|p| p == name)) {
                    programs.push(Program {
                        workspace,
                        package,
                        binary,
                    });
                }
            }
        }
    }
    for name in &wanted {
        match binaries.get(name.as_str()) {
            None => anyhow::bail!(
                "unknown source module {name}; available modules: {}",
                binaries.keys().copied().collect::<Vec<_>>().join(", ")
            ),
            Some(false) => anyhow::bail!("source module {name} does not support this host"),
            Some(true) => {}
        }
    }
    programs.sort_by_key(|program| program.binary.name.as_str());
    let host = format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH);
    for package in plans.iter().flat_map(|plan| &plan.skipped) {
        out.block(format::Note(sentence!(
            "Skipping {}: its source manifest does not support {}.",
            Product::new(&package.name),
            format::Foreign::new(&host)
        )));
    }
    let mut installed = 0_usize;
    let mut unchanged = 0_usize;
    let mut missing = 0_usize;
    let bin = place.bin(&config);
    // A source-built lnk rebuilds itself from its checkout first, so it and
    // every module it installs come from one build and never refuse each
    // other as built from other sources.
    if let Some(core) = core_program(&config, &workspaces) {
        out.block(format::Note(sentence!(
            "Updating {} from {}.",
            Product::new(&config.binary),
            Shown::new(&core.package.manifest_path)
        )));
        match &place {
            Where::Target { dir, profile, .. } => {
                core.build(&config, dir, profile)?;
            }
            Where::Installed { root } => core.install(&config, target, root)?,
        }
    }
    for program in &programs {
        let name = program
            .binary
            .name
            .strip_prefix(&prefix)
            .expect("module prefix");
        // Installed means its file is there, whatever build it's from:
        // a refresh is what brings a module of another build back in line.
        if asked.stale && !link_core::module_present(name) {
            missing += 1;
            out.block(format::Note(sentence!(
                "Skipping {}: it is not installed.",
                Product::new(name)
            )));
            continue;
        }
        let beside = bin.join(&program.binary.name);
        let current = |built: &Path| -> anyhow::Result<bool> {
            Ok(beside.is_file() && same_program(built, &beside)?)
        };
        match &place {
            Where::Target { dir, profile, .. } => {
                // Cargo rebuilds only what changed, in place beside lnk.
                let before = match beside.is_file() {
                    true => Some(std::fs::read(&beside).context("reading the module")?),
                    false => None,
                };
                out.block(format::Note(sentence!(
                    "Building {} from {}.",
                    Product::new(&program.binary.name),
                    Shown::new(&program.package.manifest_path)
                )));
                program.build(&config, dir, profile)?;
                let after = std::fs::read(&beside).ok();
                match asked.stale && before.is_some() && before == after {
                    true => unchanged += 1,
                    false => installed += 1,
                }
            }
            Where::Installed { root } => {
                if asked.stale {
                    out.block(format::Note(sentence!(
                        "Checking {} with Cargo.",
                        Product::new(name)
                    )));
                    let artifact = program.build(&config, target, &config.install_profile)?;
                    if current(&artifact)? {
                        unchanged += 1;
                        out.block(format::Note(sentence!(
                            "Skipping {}: its installed executable is current.",
                            Product::new(name)
                        )));
                        continue;
                    }
                }
                out.block(format::Note(sentence!(
                    "Installing {} from {}.",
                    Product::new(&program.binary.name),
                    Shown::new(&program.package.manifest_path)
                )));
                program.install(&config, target, root)?;
                installed += 1;
            }
        }
    }
    Ok(Done::success(sentence!(
        "Installed source modules: {}. Unchanged: {}. Not installed: {}.",
        installed,
        unchanged,
        missing
    ))
    .next(
        cmd!("lnk module list"),
        phrase!("see the installed modules"),
    ))
}

/// `lnk` itself, as a program of the checkout this `lnk` was built from,
/// when that checkout is one of the workspaces: what a source install
/// rebuilds first. Nothing for a release, or for workspaces without it.
fn core_program<'a>(config: &Config, workspaces: &'a [Metadata]) -> Option<Program<'a>> {
    let root = link_core::CHECKOUT?.root().canonicalize().ok()?;
    let workspace = workspaces
        .iter()
        .find(|w| w.workspace_root.canonicalize().ok().as_ref() == Some(&root))?;
    workspace
        .packages
        .iter()
        .filter(|package| workspace.workspace_members.contains(&package.id))
        .find_map(|package| {
            let binary = package
                .targets
                .iter()
                .find(|t| t.name == config.binary && t.kind.iter().any(|kind| kind == "bin"))?;
            Some(Program {
                workspace,
                package,
                binary,
            })
        })
}

/// Compare contents rather than versions or timestamps, including a replaced installation.
fn same_program(built: &Path, installed: &Path) -> anyhow::Result<bool> {
    let hash = |path: &Path| -> anyhow::Result<String> {
        Ok(link_digest::sha256_of(
            std::fs::File::open(path).with_context(|| format!("reading {}", path.display()))?,
        )?)
    };
    Ok(hash(built)? == hash(installed)?)
}

struct Program<'a> {
    workspace: &'a Metadata,
    package: &'a Package,
    binary: &'a Target,
}

impl Program<'_> {
    fn command(&self, config: &Config, target: &Path, verb: &str) -> std::process::Command {
        let mut command = link_secretive::process::command(&config.cargo);
        command
            .current_dir(&self.workspace.workspace_root)
            .args([verb, "--locked", "--bin", &self.binary.name, "--target-dir"])
            .arg(target);
        command
    }

    fn build(&self, config: &Config, target: &Path, profile: &str) -> anyhow::Result<PathBuf> {
        let output = self
            .command(config, target, "build")
            .args([
                "--profile",
                profile,
                "--message-format=json-render-diagnostics",
                "--manifest-path",
            ])
            .arg(&self.package.manifest_path)
            .stderr(std::process::Stdio::inherit())
            .output()
            .context("running cargo build")?;
        ensure!(
            output.status.success(),
            "building {} failed; no installation was changed for this module",
            self.binary.name
        );
        for line in output
            .stdout
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            if let Artifact::CompilerArtifact {
                target,
                executable: Some(executable),
            } = serde_json::from_slice(line).context("reading Cargo build output")?
                && target.name == self.binary.name
                && target.kind.iter().any(|kind| kind == "bin")
            {
                return Ok(executable);
            }
        }
        anyhow::bail!(
            "Cargo did not report an executable for {}",
            self.binary.name
        )
    }

    fn install(&self, config: &Config, target: &Path, root: &Path) -> anyhow::Result<()> {
        let mut command = self.command(config, target, "install");
        command.args(["--force", "--path"]).arg(
            self.package
                .manifest_path
                .parent()
                .context("finding the package folder")?,
        );
        command.arg("--root").arg(root);
        let status = command.status().context("running cargo install")?;
        ensure!(
            status.success(),
            "installing {} failed; earlier modules remain installed. Fix the Cargo error and rerun the same source-install command",
            self.package.name
        );
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(tag = "reason", rename_all = "kebab-case")]
enum Artifact {
    CompilerArtifact {
        target: Target,
        executable: Option<PathBuf>,
    },
    #[serde(other)]
    Other,
}

#[cfg(test)]
mod tests;
