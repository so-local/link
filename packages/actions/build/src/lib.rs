//! The Rust a module's build writes from the catalog, into `OUT_DIR`:
//! a type for each shape that refuses what its shape refuses, each spec
//! kind made a spec `link-spec` reads, and for a module with a command
//! line, each action's typed arguments, its parser, the handler trait and
//! the dispatch every other surface calls.
//!
//! Types need `link-actions`, `serde` and `serde_json` in the crate that
//! includes them, and a spec kind `link-spec`, `link-files` and
//! `schemars` too. The command line needs `clap` and `link-core`, whose
//! `uninstall` every module offers.

mod commands;
mod handler;
mod names;
mod specs;
mod tool_calls;
mod types;

use std::path::Path;

use anyhow::Context;
use link_actions::Catalog;

/// What a crate generates from its module's declarations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parts {
    /// The shapes and the spec kinds: what a contract crate shares.
    Types,
    /// The shapes every module shares, core's, for `link-types`: each
    /// with its JSON Schema, and checked as it's read, since hand-written
    /// specs hold them too.
    Shared,
    /// The shapes, the handler trait and dispatch, for a module other
    /// surfaces call and no command line reads.
    Handler,
    /// The shapes and each action's runtime arguments, with an `argv()`
    /// writing the command line that gives them: what a contract crate
    /// shares so the program parsing a command and a caller building one
    /// read one declaration.
    Args,
    /// Everything: the shapes, the command line with core's `uninstall`
    /// and `spec show`, the handler and dispatch.
    Program,
    /// The command line, the handler and dispatch, over the shapes another
    /// crate generated with [`Parts::Types`], named by its path
    /// (`::link_sandbox_contract`): a program whose contract owns its
    /// shapes, so each is one Rust type.
    ProgramOver(&'static str),
    /// [`Parts::ProgramOver`], over a crate that generated the shapes and
    /// the runtime arguments with [`Parts::Args`]: its argument groups
    /// and each action's arguments are that crate's types too, so a
    /// caller building a line and the program reading it share them.
    ProgramOverArgs(&'static str),
}

/// Writes `parts` for the module at `directory` to `output`, a file in
/// the build's `OUT_DIR`, and tells Cargo to run the build again when a
/// declaration changes.
pub fn write(directory: &Path, output: &Path, parts: Parts) -> anyhow::Result<()> {
    write_with(directory, &[], output, parts)
}

/// Another module whose shapes this one's declarations name: a contract
/// it implements, or the owner of a spec kind it binds. Its folder holds
/// its declarations, and `rust` is the crate its shapes are generated in
/// with [`Parts::Types`], as `::link_store_contract`.
#[derive(Clone, Copy, Debug)]
pub struct Owner<'a> {
    pub directory: &'a Path,
    pub rust: &'a str,
}

/// [`write`], with the other modules its declarations name.
pub fn write_with(
    directory: &Path,
    owners: &[Owner],
    output: &Path,
    parts: Parts,
) -> anyhow::Result<()> {
    let folders = std::iter::once(directory).chain(owners.iter().map(|o| o.directory));
    rerun_if_changed(&watched(folders));
    let code = generate_with(directory, owners, parts)?;
    std::fs::write(output, code).with_context(|| format!("writing {}", output.display()))
}

/// The declaration files in `folders` a build reads, which Cargo runs it
/// again for: each that exists of [`link_actions::file_names`] and each
/// spec kind's versions. A missing file is never named, since Cargo
/// counts a missing file as changed and would run the build, and
/// everything built on it, again on every command. So a declaration file
/// added beside existing ones is read once the build runs again for
/// another reason: a change to a declaration it already reads, or to its
/// `build.rs`.
pub fn watched<'a>(folders: impl IntoIterator<Item = &'a Path>) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    for folder in folders {
        let kinds = std::fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| link_actions::is_kind_schema(name));
        let named = link_actions::file_names().map(str::to_string);
        found.extend(
            named
                .into_iter()
                .chain(kinds)
                .map(|name| folder.join(name))
                .filter(|path| path.is_file()),
        );
    }
    found
}

fn rerun_if_changed(paths: &[std::path::PathBuf]) {
    use std::io::Write;
    for path in paths {
        let _ = writeln!(
            std::io::stdout(),
            "cargo::rerun-if-changed={}",
            path.display()
        );
    }
}

/// Writes the tool actions of the module at `directory` to `output`, a
/// file in the build's `OUT_DIR`, as the template its program fills at
/// runtime with [`link_actions::tools::offer`], and tells Cargo to run the
/// build again when a declaration changes.
pub fn write_tools(directory: &Path, output: &Path) -> anyhow::Result<()> {
    write_tools_with(directory, &[], output)
}

/// [`write_tools`], with the other modules its declarations name, as a
/// contract it implements.
pub fn write_tools_with(directory: &Path, owners: &[&Path], output: &Path) -> anyhow::Result<()> {
    rerun_if_changed(&watched([directory]));
    let text = tools_with(directory, owners)?;
    std::fs::write(output, text).with_context(|| format!("writing {}", output.display()))
}

/// The tools template of the module at `directory`, as JSON.
pub fn tools(directory: &Path) -> anyhow::Result<String> {
    tools_with(directory, &[])
}

/// [`tools`], with the other modules its declarations name.
pub fn tools_with(directory: &Path, owners: &[&Path]) -> anyhow::Result<String> {
    let folders: Vec<&Path> = std::iter::once(directory)
        .chain(owners.iter().copied())
        .collect();
    let catalog = Catalog::load(&folders)?;
    let wanted = directory
        .canonicalize()
        .with_context(|| format!("reading {}", directory.display()))?;
    let module = catalog
        .modules()
        .find(|m| m.directory.canonicalize().ok().as_ref() == Some(&wanted))
        .with_context(|| format!("no module at {}", directory.display()))?;
    let template = link_actions_surfaces::tools::template(&catalog, &module.identity.id)?;
    Ok(serde_json::to_string_pretty(&template)?)
}

/// The code for the module at `directory`.
pub fn generate(directory: &Path, parts: Parts) -> anyhow::Result<String> {
    generate_with(directory, &[], parts)
}

/// [`generate`], with the other modules its declarations name.
pub fn generate_with(directory: &Path, owners: &[Owner], parts: Parts) -> anyhow::Result<String> {
    let folders: Vec<&Path> = std::iter::once(directory)
        .chain(owners.iter().map(|o| o.directory))
        .collect();
    let catalog = Catalog::load(&folders)?;
    let wanted = directory
        .canonicalize()
        .with_context(|| format!("reading {}", directory.display()))?;
    let module = catalog
        .modules()
        .find(|m| m.directory.canonicalize().ok().as_ref() == Some(&wanted))
        .context("the module loaded")?;
    let mut types = types::Types::new(&catalog, module);
    types.shared = parts == Parts::Shared;
    for owner in owners {
        let wanted = owner.directory.canonicalize().ok();
        let id = catalog
            .modules()
            .find(|m| m.directory.canonicalize().ok() == wanted)
            .map(|m| m.identity.id.clone())
            .with_context(|| format!("the module at {}", owner.directory.display()))?;
        types.owners.insert(id, owner.rust.to_string());
    }
    let over = match parts {
        Parts::ProgramOver(owner) | Parts::ProgramOverArgs(owner) => Some(owner),
        _ => None,
    };
    types.shapes = over.map(str::to_string);
    types.all()?;
    // A spec kind is a spec where its type is written.
    let specs = match parts {
        _ if over.is_some() => String::new(),
        _ => specs::write(&catalog, module)?,
    };
    let mut commands = commands::Commands {
        types: &mut types,
        out: String::new(),
        cli: parts == Parts::Program || over.is_some(),
        defaults: Default::default(),
        argv: false,
        over: match parts {
            Parts::ProgramOverArgs(owner) => Some(owner.to_string()),
            _ => None,
        },
    };
    match parts {
        Parts::Types | Parts::Shared => {}
        Parts::Handler => commands.args_only()?,
        Parts::Args => {
            commands.argv = true;
            commands.args_only()?
        }
        Parts::Program | Parts::ProgramOver(_) | Parts::ProgramOverArgs(_) => commands.all()?,
    }
    let mut rest = commands.out;
    if parts == Parts::Handler || parts == Parts::Program || over.is_some() {
        rest += &handler::write(&mut types)?;
    }
    Ok(format!(
        "// Generated by link-actions-build from module {}'s declarations. Never edited.\n\n{}{specs}{rest}",
        module.identity.id, types.out
    ))
}

#[cfg(test)]
mod tests {
    use link_test_kit::temp::TempDir;

    /// The code for a module whose `types.toml` is `types`.
    fn generated(types: &str) -> anyhow::Result<String> {
        let dir = TempDir::new("generated");
        std::fs::write(
            dir.path().join("module.toml"),
            "schema = 1\n[module]\nid = \"probe\"\nkind = \"module\"\nowner = \"probe\"\n",
        )?;
        std::fs::write(
            dir.path().join("types.toml"),
            format!("schema = 1\n{types}"),
        )?;
        super::generate(dir.path(), super::Parts::Types)
    }

    /// Cargo counts a missing file as changed, so a file the build would
    /// read but doesn't exist is never watched: naming one reran every
    /// declaration build, and recompiled about 90 packages, on every
    /// command (Nov 17 2026).
    #[test]
    fn only_the_declarations_that_exist_are_watched() {
        let dir = TempDir::new("watched");
        for name in [
            "module.toml",
            "types.toml",
            "profile-schema.toml",
            "notes.toml",
        ] {
            std::fs::write(dir.path().join(name), "schema = 1\n").unwrap();
        }
        let mut watched: Vec<String> = super::watched([dir.path()])
            .iter()
            .map(|path| path.strip_prefix(dir.path()).unwrap().display().to_string())
            .collect();
        watched.sort();
        assert_eq!(
            watched,
            ["module.toml", "profile-schema.toml", "types.toml"]
        );
    }

    #[test]
    fn an_optional_list_is_left_out_or_given_and_a_required_one_is_given() {
        let code = generated(
            "[probe]\ntype = \"object\"\nadditionalProperties = false\nrequired = [\"always\"]\n\
             properties.hosts = { type = \"array\", items = { type = \"string\" } }\n\
             properties.always = { type = \"array\", items = { type = \"string\" } }\n",
        )
        .unwrap();
        assert!(code.contains("pub hosts: Option<Vec<String>>"), "{code}");
        assert!(code.contains("pub always: Vec<String>"), "{code}");
    }

    #[test]
    fn a_shape_holding_its_own_kind_holds_it_boxed() {
        let code = generated(
            "[files]\ntype = \"object\"\nadditionalProperties = false\n\
             properties.write = { type = \"array\", items = { type = \"string\" } }\n\
             properties.macos = \"files\"\n",
        )
        .unwrap();
        assert!(code.contains("pub macos: Option<Box<Files>>"), "{code}");
    }

    #[test]
    fn a_number_or_a_word_is_told_by_its_value() {
        let code = generated(
            "[port]\noneOf = [{ type = \"integer\", minimum = 0, maximum = 65535 }, { const = \"*\" }]\n",
        )
        .unwrap();
        assert!(code.contains("#[serde(untagged)]\npub enum Port"), "{code}");
        assert!(
            code.contains("Number(u16)") && code.contains("Word(String)"),
            "{code}"
        );
        assert!(code.contains(r#"one_of(at, word, &["*"])"#), "{code}");
        let refused = format!(
            "{:#}",
            generated("[port]\noneOf = [{ type = \"integer\" }, { type = \"integer\" }]\n")
                .unwrap_err()
        );
        assert!(refused.contains("at most one number"), "{refused}");
    }

    #[test]
    fn a_program_over_its_contract_names_the_contracts_shapes() {
        let dir = TempDir::new("over");
        std::fs::write(
            dir.path().join("module.toml"),
            "schema = 1\n[module]\nid = \"probe\"\nkind = \"module\"\nowner = \"probe\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("types.toml"),
            "schema = 1\n[shown]\ntype = \"object\"\nadditionalProperties = false\n\
             properties.name = { type = \"string\" }\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("actions.toml"),
            "schema = 1\n[verbs]\nlook = { description = \"Look\", effect = \"find\" }\n[actions.\"probe.probe.look\"]\n\
             description = \"Look\"\noutput = \"shown\"\n",
        )
        .unwrap();
        let code =
            super::generate(dir.path(), super::Parts::ProgramOver("::probe_contract")).unwrap();
        assert!(!code.contains("pub struct Shown"), "{code}");
        assert!(code.contains("::probe_contract::Shown"), "{code}");
    }

    /// A module binding a kind a package owns shows it with its own
    /// `spec show`, the package owning no command; one binding another
    /// module's kind leaves it to its owner.
    #[test]
    fn a_module_binding_a_packages_kind_shows_it() {
        let root = TempDir::new("bound");
        let package = root.path().join("rules");
        let module = root.path().join("guard");
        for dir in [&package, &module] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(
            package.join("module.toml"),
            "schema = 1\n[module]\nid = \"rules\"\nkind = \"package\"\nowner = \"rules\"\n",
        )
        .unwrap();
        std::fs::write(
            package.join("types.toml"),
            "schema = 1\n[rules]\ntype = \"object\"\nadditionalProperties = false\n\
             x-link-spec = { kind = \"rules\" }\nproperties.allow = { type = \"string\" }\n",
        )
        .unwrap();
        std::fs::write(
            module.join("module.toml"),
            "schema = 1\n[module]\nid = \"guard\"\nkind = \"module\"\nowner = \"guard\"\n\
             [dependencies.rules]\nkind = \"package\"\nid = \"rules\"\n",
        )
        .unwrap();
        std::fs::write(
            module.join("types.toml"),
            "schema = 1\n[checked]\ntype = \"object\"\nadditionalProperties = false\n\
             properties.allowed = { type = \"boolean\" }\n",
        )
        .unwrap();
        std::fs::write(
            module.join("actions.toml"),
            "schema = 1\n[actions.\"guard.guard.check\"]\n\
             description = \"Check\"\nspec = \"rules.rules\"\noutput = \"checked\"\n",
        )
        .unwrap();
        let code = super::generate_with(
            &module,
            &[super::Owner {
                directory: &package,
                rust: "::rules",
            }],
            super::Parts::Program,
        )
        .unwrap();
        assert!(code.contains("RulesSpecShow(::link_spec::Show)"), "{code}");
        assert!(
            code.contains(r#"["rules", "spec", "show"] => Some("#),
            "{code}"
        );
    }

    #[test]
    fn two_shapes_never_share_one_rust_name() {
        let inline = "[probe]\ntype = \"object\"\nadditionalProperties = false\n\
             properties.stage = { type = \"object\", additionalProperties = false, properties.run = { type = \"string\" } }\n\
             properties.other = \"probe-stage\"\n\
             [probe-stage]\ntype = \"object\"\nadditionalProperties = false\nproperties.port = { type = \"integer\" }\n";
        let refused = format!("{:#}", generated(inline).unwrap_err());
        assert!(
            refused.contains("would both be the Rust type ProbeStage"),
            "{refused}"
        );
        let fields = "[probe]\ntype = \"object\"\nadditionalProperties = false\n\
             properties.foo-bar = { type = \"string\" }\nproperties.foo_bar = { type = \"string\" }\n";
        let refused = format!("{:#}", generated(fields).unwrap_err());
        assert!(refused.contains("both the Rust field foo_bar"), "{refused}");
        let fine = "[probe]\ntype = \"object\"\nadditionalProperties = false\nproperties.port = { type = \"integer\" }\n";
        assert!(generated(fine).unwrap().contains("pub struct Probe"));
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
