//! Embed local declarations. Discovery never runs an installer or resolves Cargo.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, ensure};
use link_component::{Component, Page, Resolved, Source};

/// A directive to Cargo, which reads a build script's stdout: a protocol
/// with the build, not words for a person.
fn cargo(directive: &str, value: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stdout(), "cargo::{directive}={value}");
}

/// The variable that says a build is released (`link_core::Build`).
const BUILD_VARIABLE: &str = "LNK_BUILD";
use link_component::SOURCE_VARIABLE;

/// Which sources a release build from source is built from: the commit of
/// the Git checkout holding `directory`, handed to the package being
/// built, and its build script run again when the checkout's commit moves
/// (`HEAD`, the branch it names, packed refs). Nothing for a released
/// build, whose version is exact, outside a Git checkout, or for a debug
/// build: `cargo build --workspace --bins` keeps a `target/debug` folder's
/// programs together, and a commit there would rerun every component's
/// build script, and recompile what's built on it, at every commit, and
/// refuse each program a narrower build left at the last one.
fn source(directory: &Path) -> Option<String> {
    cargo("rerun-if-env-changed", BUILD_VARIABLE);
    let shipped = std::env::var_os(BUILD_VARIABLE).is_some();
    let release = std::env::var("PROFILE").as_deref() == Ok("release");
    recorded(directory, shipped, release)
}

/// [`source`], for a build released (`shipped`) or not and of the `release` profile
/// or not: a commit only for a release build from source.
fn recorded(directory: &Path, shipped: bool, release: bool) -> Option<String> {
    (!shipped && release).then(|| commit(directory)).flatten()
}

/// The commit of the Git checkout holding `directory`, handed to the
/// package being built, with its build script run again when the commit
/// moves.
#[expect(
    clippy::disallowed_methods,
    reason = "a build script asks Git for its checkout's commit as Cargo runs it: no Link program, secret or environment of Link's is there to keep"
)]
fn commit(directory: &Path) -> Option<String> {
    let git = |args: &[&str]| -> Option<String> {
        // Git sees only where to find itself and its user's configuration
        // (`safe.directory`), never the rest of the build's environment.
        let kept = ["PATH", "HOME"]
            .into_iter()
            .filter_map(|name| Some((name, std::env::var_os(name)?)));
        let output = std::process::Command::new("git")
            .env_clear()
            .envs(kept)
            .arg("-C")
            .arg(directory)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|output| output.status.success())?;
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    };
    let commit = git(&["rev-parse", "HEAD"])?;
    let mut watched = vec!["HEAD".to_string(), "packed-refs".to_string()];
    watched.extend(git(&["symbolic-ref", "-q", "HEAD"]));
    for name in watched {
        if let Some(path) = git(&["rev-parse", "--git-path", &name])
            .map(|path| directory.join(path))
            .filter(|path| path.exists())
        {
            cargo("rerun-if-changed", &path.display().to_string());
        }
    }
    cargo("rustc-env", &format!("{SOURCE_VARIABLE}={commit}"));
    Some(commit)
}

/// Reads a file the build depends on, and has Cargo run it again when the
/// file changes. A file is named only once it's read, so only while it
/// exists: Cargo counts a missing file as changed, and would run the
/// build, and everything built on it, again on every command.
fn read(path: &Path) -> anyhow::Result<String> {
    cargo("rerun-if-changed", &path.display().to_string());
    std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

fn manifest(path: &Path) -> anyhow::Result<toml::Value> {
    Ok(toml::from_str(&read(path)?)?)
}

fn workspace(directory: &Path) -> anyhow::Result<PathBuf> {
    directory
        .ancestors()
        .find(|p| {
            std::fs::read_to_string(p.join("Cargo.toml"))
                .ok()
                .and_then(|s| toml::from_str::<toml::Value>(&s).ok())
                .is_some_and(|v| v.get("workspace").is_some())
        })
        .map(Path::to_path_buf)
        .context("no owning Cargo workspace")
}

fn cargo_field(package: &toml::Value, ws: &toml::Value, name: &str) -> anyhow::Result<String> {
    let field = package
        .get(name)
        .context(format!("Cargo must supply {name}"))?;
    let field = if field.get("workspace").and_then(toml::Value::as_bool) == Some(true) {
        &ws["workspace"]["package"][name]
    } else {
        field
    };
    field
        .as_str()
        .map(String::from)
        .context(format!("Cargo {name} must be text"))
}

/// A translation, `<page>.<language>.md` beside its English `<page>.md`:
/// no page of its own here, which embeds the English.
fn translation(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| link_component::languages::translation(name).is_some())
}

fn pages(root: &Path) -> anyhow::Result<Vec<Page>> {
    fn walk(root: &Path, directory: &Path, found: &mut Vec<Page>) -> anyhow::Result<()> {
        cargo("rerun-if-changed", &directory.display().to_string());
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            ensure!(
                !path.is_symlink(),
                "documentation must stay in its owner: {}",
                path.display()
            );
            if path.is_dir() {
                walk(root, &path, found)?;
            } else if path.extension().is_some_and(|e| e == "md")
                && path != root.join("navigation.md")
                && path != root.join("website.md")
                && !translation(&path)
            {
                let relative = path
                    .strip_prefix(root)?
                    .to_string_lossy()
                    .replace('\\', "/");
                let topic = relative.strip_suffix(".md").unwrap();
                let topic = topic.strip_suffix("/README").unwrap_or(topic);
                found.push(Page {
                    path: if topic == "README" {
                        String::new()
                    } else {
                        topic.into()
                    },
                    markdown: read(&path)?,
                });
            }
        }
        Ok(())
    }
    let mut found = Vec::new();
    if root.is_dir() {
        walk(root, root, &mut found)?;
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(found)
}

fn owned_pages(owner: &Path) -> anyhow::Result<Vec<Page>> {
    fn collect(owner: &Path, directory: &Path, result: &mut Vec<Page>) -> anyhow::Result<()> {
        cargo("rerun-if-changed", &directory.display().to_string());
        result.extend(pages(&directory.join("docs"))?);
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            if !path.is_dir() || path.is_symlink() {
                continue;
            }
            if matches!(
                path.file_name().and_then(|n| n.to_str()),
                Some("docs" | "target" | "node_modules" | ".git")
            ) {
                continue;
            }
            // A nested module owns its own pages, even inside a grouping owner.
            if path != owner && path.join("module.toml").is_file() {
                continue;
            }
            collect(owner, &path, result)?;
        }
        Ok(())
    }
    let mut result = Vec::new();
    collect(owner, owner, &mut result)?;
    result.sort_by(|a, b| a.path.cmp(&b.path));
    ensure!(
        !result.windows(2).any(|pair| pair[0].path == pair[1].path),
        "duplicate documentation topic in {}",
        owner.display()
    );
    Ok(result)
}

fn resolve(directory: &Path, ws: &toml::Value, include_pages: bool) -> anyhow::Result<Resolved> {
    let cargo = manifest(&directory.join("Cargo.toml"))?;
    let mut source = Source::parse(&read(&directory.join("module.toml"))?)?;
    let actions = directory.join("actions.toml");
    if actions.is_file() {
        ensure!(
            source.commands.is_empty(),
            "{}: its commands are its actions.toml's",
            directory.display()
        );
        let description = cargo_field(&cargo["package"], ws, "description")?;
        source.commands = link_component::actions::commands(&link_component::actions::read_module(
            directory,
            Some(&description),
        )?);
    }
    let variables = directory.join("variables.toml");
    if variables.is_file() {
        link_component::actions::add_variables(&mut source.commands, &read(&variables)?)
            .with_context(|| format!("{}", variables.display()))?;
    }
    let package = &cargo["package"];
    let cargo_name = cargo_field(package, ws, "name")?;
    let id = source.module.id.clone();
    let program = format!("lnk-{id}");
    let bins: Vec<_> = cargo
        .get("bin")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| v.get("name").and_then(toml::Value::as_str))
        .collect();
    ensure!(
        bins == [program.as_str()],
        "{cargo_name} must own exactly {program}, got {bins:?}"
    );
    let component = Component {
        id,
        cargo_name,
        program,
        kind: source.module.kind.clone(),
        reads_control: source.module.reads_control,
        version: cargo_field(package, ws, "version")?,
        license: cargo_field(package, ws, "license")?,
        description: cargo_field(package, ws, "description")?,
    };
    let owner = if let Some(owner) = &source.module.owner {
        let owner = workspace(directory)?.join(owner);
        ensure!(
            directory.starts_with(&owner)
                && !owner
                    .components()
                    .any(|c| c == std::path::Component::ParentDir),
            "component owner must contain its member"
        );
        owner
    } else {
        directory.to_path_buf()
    };
    let mut own_pages = owned_pages(&owner)?;
    if !include_pages {
        for page in &mut own_pages {
            page.markdown.clear();
        }
    }
    let mut resolved = source.resolve(component, own_pages)?;
    let history = directory.join("history.toml");
    if history.is_file() {
        resolved.history = link_spec::history::History::parse(&read(&history)?)
            .with_context(|| format!("{}", history.display()))?;
        ensure!(
            resolved.history.renamed.is_empty(),
            "{}: a module can't record its own rename, which only the core's history answers",
            history.display()
        );
        if let Some(gone) = resolved.history.outside(&resolved.component.id) {
            anyhow::bail!(
                "{}: {:?} is spelled as typed after lnk, its module first: \"{} {}\"",
                history.display(),
                gone.was,
                resolved.component.id,
                gone.was
            );
        }
    }
    Ok(resolved)
}

/// Whether the member's `module.toml` declares a contract crate, a
/// protocol compiled into the modules implementing it, which has no
/// program and so no place in the registry.
fn is_package(member: &Path) -> anyhow::Result<bool> {
    let source = Source::parse(&read(&member.join("module.toml"))?)?;
    Ok(source.module.kind == link_component::Kind::Package)
}

fn members(root: &Path, ws: &toml::Value) -> anyhow::Result<Vec<PathBuf>> {
    let mut result = vec![];
    for member in ws["workspace"]["members"]
        .as_array()
        .context("workspace members")?
    {
        let name = member.as_str().context("member path")?;
        ensure!(
            !name.contains('*'),
            "expand Cargo member patterns in the structural registry: {name}"
        );
        result.push(root.join(name));
    }
    Ok(result)
}

/// Writes `descriptor` in `out` as `component.json`, and its history alone
/// beside it as `history.json`: what every command's parser reads, which
/// needn't read the description and its pages.
fn write_description(out: &Path, descriptor: &Resolved) -> anyhow::Result<()> {
    std::fs::write(out.join("component.json"), serde_json::to_vec(descriptor)?)?;
    std::fs::write(
        out.join("history.json"),
        serde_json::to_vec(&descriptor.history)?,
    )?;
    Ok(())
}

/// Called by a module's build script; all installed metadata is in the binary.
pub fn embed() {
    try_embed().expect("embedding owned component metadata");
}

fn try_embed() -> anyhow::Result<()> {
    let directory =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").context("CARGO_MANIFEST_DIR")?);
    let root = workspace(&directory)?;
    let ws = manifest(&root.join("Cargo.toml"))?;
    let mut descriptor = resolve(&directory, &ws, true)?;
    descriptor.source = source(&directory);
    let out = PathBuf::from(std::env::var_os("OUT_DIR").context("OUT_DIR")?);
    write_description(&out, &descriptor)?;
    let source = Source::parse(&read(&directory.join("module.toml"))?)?;
    let mut constants =
        String::from("// Generated from this component's authoritative external records.\n");
    for (alias, reference) in source.pins {
        let file = directory.join(reference.file);
        ensure!(
            file.canonicalize()?.starts_with(directory.canonicalize()?),
            "pin escapes its owner"
        );
        let data: serde_json::Value = serde_json::from_str(&read(&file)?)?;
        let pin = data
            .pointer(&reference.pointer)
            .context("missing authoritative pin")?;
        let name = alias.replace('-', "_").to_ascii_uppercase();
        ensure!(
            name.bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'),
            "invalid pin alias"
        );
        constants += &format!("pub const {name}_JSON: &str = {:?};\n", pin.to_string());
        for key in ["version", "commit", "url", "sha256"] {
            if let Some(value) = pin[key].as_str() {
                constants += &format!(
                    "pub const {name}_{}: &str = {value:?};\n",
                    key.to_ascii_uppercase()
                );
            }
        }
    }
    std::fs::write(out.join("external-pins.rs"), constants)?;
    Ok(())
}

/// The core's registry is generated from the resolved source declarations.
pub fn core() {
    try_core().expect("embedding local release registry");
}

fn try_core() -> anyhow::Result<()> {
    let directory =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").context("CARGO_MANIFEST_DIR")?);
    let root = workspace(&directory)?;
    // A prod build's registry holds the released components alone; a dev,
    // staging or source build keeps every one (link_core::Build reads the
    // same).
    source(&directory);
    let release = std::env::var_os(BUILD_VARIABLE).is_some_and(|build| build == "prod");
    cargo("rerun-if-env-changed", "LNK_COMPONENTS");
    let extras = std::env::var_os("LNK_COMPONENTS");
    let extras: Vec<_> = extras
        .iter()
        .flat_map(std::env::split_paths)
        .filter(|path| !path.as_os_str().is_empty())
        .collect();
    let registry = registry(&root, &extras, release)?;
    let out = PathBuf::from(std::env::var_os("OUT_DIR").context("OUT_DIR")?);
    std::fs::write(out.join("registry.json"), serde_json::to_vec(&registry)?)?;
    let mut documentation = pages(&directory.join("docs"))?;
    documentation.extend(pages(&root.join("core/framework/docs"))?);
    documentation.sort_by(|a, b| a.path.cmp(&b.path));
    std::fs::write(
        out.join("documentation.json"),
        serde_json::to_vec(&documentation)?,
    )?;
    Ok(())
}

/// Whether `member`'s `module.toml` is core's: `lnk`'s own declarations,
/// which no registry lists, since core is no module to install.
fn is_core(member: &Path) -> anyhow::Result<bool> {
    let text = read(&member.join("module.toml"))?;
    let table: toml::Table =
        toml::from_str(&text).with_context(|| format!("{}/module.toml", member.display()))?;
    Ok(table
        .get("module")
        .and_then(|m| m.get("id"))
        .and_then(toml::Value::as_str)
        == Some(link_actions::core().module.as_str()))
}

/// The core's registry for the workspace at `root`: every member with a
/// `module.toml`, then each of `extras`, a first-party component directory
/// a product build adds. A release's holds only the released ones
/// ([`link_component::release`]).
pub fn registry(
    root: &Path,
    extras: &[PathBuf],
    release: bool,
) -> anyhow::Result<BTreeMap<String, Resolved>> {
    let ws = manifest(&root.join("Cargo.toml"))?;
    let mut registry = BTreeMap::new();
    let mut held = std::collections::BTreeSet::new();
    let mut add = |member: &Path, owner: &toml::Value| -> anyhow::Result<Resolved> {
        let component = resolve(member, owner, false)?;
        if !link_component::release::manifest_released(&manifest(&member.join("Cargo.toml"))?)? {
            held.insert(component.component.id.clone());
        }
        Ok(component)
    };
    for member in members(root, &ws)? {
        // Only the files that exist, as `read` says. A new member comes
        // with a change to the workspace's `Cargo.toml`, read above.
        for file in ["module.toml", "actions.toml", "variables.toml"] {
            let path = member.join(file);
            if path.is_file() {
                cargo("rerun-if-changed", &path.display().to_string());
            }
        }
        if member.join("module.toml").is_file() && !is_package(&member)? && !is_core(&member)? {
            let component = add(&member, &ws)?;
            ensure!(
                registry
                    .insert(component.component.id.clone(), component)
                    .is_none(),
                "duplicate module identity"
            );
        }
    }
    // Product builds add local first-party declarations explicitly. An
    // account service never supplies this input or an executable URL.
    for path in extras {
        let member = root.join(path).canonicalize()?;
        let owner = workspace(&member)?;
        let component = add(&member, &manifest(&owner.join("Cargo.toml"))?)?;
        ensure!(
            component.component.version
                == ws["workspace"]["package"]["version"]
                    .as_str()
                    .context("workspace version")?,
            "extra component must have this core's version"
        );
        ensure!(
            registry
                .insert(component.component.id.clone(), component)
                .is_none(),
            "extra component would replace a registry identity"
        );
    }
    if release {
        return link_component::release::registry(registry, &held);
    }
    link_component::validate_registry(&registry)?;
    Ok(registry)
}

/// Shared guides and documentation-only subjects belong to the optional manual.
pub fn manual() {
    try_manual().expect("embedding shared manual pages");
}

fn try_manual() -> anyhow::Result<()> {
    let directory =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").context("CARGO_MANIFEST_DIR")?);
    let root = workspace(&directory)?;
    let index_path = root.join("scripts/documentation-index.json");
    cargo("rerun-if-changed", &index_path.display().to_string());
    let index: serde_json::Value = serde_json::from_str(&read(&index_path)?)?;
    ensure!(index["schema"] == 1, "unsupported documentation index");
    let site = index["sites"]
        .as_array()
        .context("documentation sites")?
        .iter()
        .find(|site| site["roots"] == true)
        .context("documentation site publishing the shared roots")?;
    let homepage = root.join(
        site["homepage"]
            .as_str()
            .context("documentation homepage")?,
    );
    ensure!(
        homepage.starts_with(&root) && !homepage.is_symlink(),
        "documentation homepage must stay in its workspace"
    );
    cargo("rerun-if-changed", &homepage.display().to_string());
    let mut shared = vec![Page {
        path: String::new(),
        markdown: read(&homepage)?,
    }];
    for owner in index["roots"].as_array().context("documentation roots")? {
        let path = root.join(owner["directory"].as_str().context("documentation owner")?);
        ensure!(
            path.starts_with(&root),
            "documentation owner escapes workspace"
        );
        if path.starts_with(root.join("web"))
            || path.starts_with(root.join("core"))
            || path
                .ancestors()
                .take_while(|p| *p != root)
                .any(|p| p.join("module.toml").is_file())
        {
            continue;
        }
        shared.extend(pages(&path.join("docs"))?);
    }
    shared.sort_by(|a, b| a.path.cmp(&b.path));
    let out = PathBuf::from(std::env::var_os("OUT_DIR").context("OUT_DIR")?);
    std::fs::write(out.join("manual.json"), serde_json::to_vec(&shared)?)?;
    Ok(())
}

#[cfg(test)]
mod navigation_tests {
    use super::pages;
    use std::path::PathBuf;

    struct Directory(PathBuf);

    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn metadata_is_not_embedded_but_a_navigation_guide_is() {
        let directory = Directory(
            std::env::temp_dir().join(format!("link-component-navigation-{}", std::process::id())),
        );
        std::fs::create_dir_all(directory.0.join("guides")).unwrap();
        std::fs::write(directory.0.join("navigation.md"), "# Site\n\n## Pages\n").unwrap();
        std::fs::write(directory.0.join("website.md"), "# Site metadata\n").unwrap();
        std::fs::write(
            directory.0.join("guides/navigation.md"),
            "# Navigation Guide\n",
        )
        .unwrap();
        let embedded = pages(&directory.0).unwrap();
        assert_eq!(embedded.len(), 1);
        assert_eq!(embedded[0].path, "guides/navigation");
        assert!(embedded[0].markdown.contains("Navigation Guide"));
    }
}

#[cfg(test)]
mod release_tests {
    use super::*;

    /// This workspace's registries: a release's holds every component but
    /// those marked `released = false`, and builds, so no released one
    /// requires a held one; any other build's holds them all.
    #[test]
    fn the_release_registry_leaves_out_the_held_components_alone() {
        let root = workspace(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        let every = registry(&root, &[], false).unwrap();
        let release = registry(&root, &[], true).unwrap();
        let ws = manifest(&root.join("Cargo.toml")).unwrap();
        let mut held = vec![];
        for member in members(&root, &ws).unwrap() {
            if member.join("module.toml").is_file()
                && !is_package(&member).unwrap()
                && !is_core(&member).unwrap()
                && !link_component::release::manifest_released(
                    &manifest(&member.join("Cargo.toml")).unwrap(),
                )
                .unwrap()
            {
                held.push(resolve(&member, &ws, false).unwrap().component.id);
            }
        }
        let kept: Vec<_> = every.keys().filter(|id| !held.contains(id)).collect();
        assert_eq!(release.keys().collect::<Vec<_>>(), kept);
    }

    /// A component named outside the workspace (`LNK_COMPONENTS`, a
    /// bundle's `components`) is in a release's registry when it's
    /// released, and left out like any other when it's held.
    #[test]
    fn a_release_registry_lists_a_released_extra_component() {
        let root = workspace(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        let ws = manifest(&root.join("Cargo.toml")).unwrap();
        let version = ws["workspace"]["package"]["version"].as_str().unwrap();
        let extra = link_test_kit::TempDir::new("extra-components");
        std::fs::write(
            extra.join("Cargo.toml"),
            format!(
                "[workspace]\nmembers = [\"shown\", \"kept\"]\n\n[workspace.package]\nversion = \"{version}\"\n"
            ),
        )
        .unwrap();
        for (id, metadata) in [
            ("shown", ""),
            ("kept", "\n[package.metadata.link]\nreleased = false\n"),
        ] {
            let member = extra.join(id);
            std::fs::create_dir_all(&member).unwrap();
            std::fs::write(
                member.join("Cargo.toml"),
                format!(
                    "[package]\nname = \"link-{id}\"\nversion.workspace = true\ndescription = \"{id}\"\nlicense = \"MIT\"\n\n[[bin]]\nname = \"lnk-{id}\"\npath = \"main.rs\"\n{metadata}"
                ),
            )
            .unwrap();
            std::fs::write(
                member.join("module.toml"),
                format!("schema = 1\n[module]\nid = \"{id}\"\nkind = \"module\"\n"),
            )
            .unwrap();
        }
        let extras = [extra.join("shown"), extra.join("kept")];
        let release = registry(&root, &extras, true).unwrap();
        assert!(release.contains_key("shown"));
        assert!(!release.contains_key("kept"));
        assert!(
            registry(&root, &extras, false)
                .unwrap()
                .contains_key("kept")
        );
    }
}

#[cfg(test)]
mod source_tests {
    use super::recorded;

    /// Only a release build from source records its checkout's commit: a
    /// debug build records none, so a commit rebuilds nothing in
    /// `target/debug`, and a released build's version is exact.
    #[test]
    fn a_release_build_records_its_checkouts_commit() {
        let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let Some(commit) = recorded(here, false, true) else {
            // A source tree outside a Git checkout records none either.
            assert!(!here.ancestors().any(|dir| dir.join(".git").exists()));
            return;
        };
        assert!(
            commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()),
            "{commit}"
        );
        assert_eq!(recorded(here, false, false), None, "a debug build");
        assert_eq!(recorded(here, true, true), None, "a released build");
    }
}

#[cfg(test)]
mod package_tests {
    use super::is_package;

    /// A contract crate's `module.toml`, of kind package, is no component:
    /// the registry skips it, since it has no program.
    #[test]
    fn a_contract_crate_has_no_place_in_the_registry() {
        let dir = link_test_kit::TempDir::new("package-kind");
        let module =
            |kind: &str| format!("schema = 1\n\n[module]\nid = \"store\"\nkind = \"{kind}\"\n");
        std::fs::write(dir.join("module.toml"), module("package")).unwrap();
        assert!(is_package(&dir).unwrap());
        std::fs::write(dir.join("module.toml"), module("module")).unwrap();
        assert!(!is_package(&dir).unwrap());
    }
}

#[cfg(test)]
mod description_tests {
    use super::*;

    /// A program's history goes beside its description, whole, so its
    /// parser reads that alone, never the description and its pages.
    #[test]
    fn the_history_is_written_whole_beside_the_description() {
        let root = workspace(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        let ws = manifest(&root.join("Cargo.toml")).unwrap();
        let mut described = resolve(&root.join("modules/security/secret"), &ws, true).unwrap();
        described.history = link_spec::history::History::parse(
            "[[gone]]\nwas = \"secret old\"\nnow = \"lnk secret new\"\nsince = \"1.0.0\"\nuntil = \"2.0.0\"\n",
        )
        .unwrap();
        let out = link_test_kit::TempDir::new("description");
        write_description(&out, &described).unwrap();
        let history = std::fs::read(out.join("history.json")).unwrap();
        let history: link_spec::history::History = serde_json::from_slice(&history).unwrap();
        assert_eq!(history, described.history);
        let whole = std::fs::read_to_string(out.join("component.json")).unwrap();
        assert_eq!(Resolved::parse(&whole).unwrap().history, described.history);
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
