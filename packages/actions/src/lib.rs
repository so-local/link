//! The catalog: every module's declarations, `module.toml`,
//! `actions.toml`, `types.toml`, `variables.toml` and `schema.toml`,
//! read strictly in the current format, each name resolved to the shape
//! it means, what the declarations imply filled in, and the rules
//! checked. Every generator reads the catalog and never a file, so a rule
//! checked once holds on every surface.

pub mod answer;
pub mod call;
mod catalog;
mod check;
pub mod cli;
mod config;
mod conforms;
mod crates;
mod effect;
mod iam;
mod json;
pub mod names;
mod read;
pub mod schema;
pub mod tools;
pub mod transport;
pub mod value;

pub use catalog::{Action, Catalog, Embedded, Module, Versions};
pub use config::{Core, HeldSpec, Implied};
pub use crates::Crates;
pub use effect::Effect;
pub use iam::On;
pub use names::{ActionId, ActionRef, TypeName};
pub use read::{
    actions::{
        Arg, Caller, Cli, File as ActionsFile, Group, Keeps, Lasting, Limits, Program, Role, Route,
        Surface, Tool, Tools, Verb, Written,
    },
    module::{Dependency, Identity},
    types::Declared,
    variables::{Access, Carries, Definition, Scalar, Use, Variables, When, family, is_variable},
};

/// The JSON Schema dialect every rendered shape follows: the version
/// OpenAPI 3.1 uses.
pub fn dialect() -> &'static str {
    &config::SETTINGS.format.dialect
}

/// The words of the commands core gives every module.
pub fn core() -> &'static Core {
    &config::SETTINGS.core
}

/// The program a namespace is dispatched to: `lnk-<namespace>`, the
/// module crate's name, or `lnk` itself for core's.
pub fn program(namespace: &str) -> String {
    let core = core();
    match namespace == core.module {
        true => core.program.clone(),
        false => format!("{}-{namespace}", core.program),
    }
}

/// The file holding a spec kind's versions: `schema.toml` for the kind
/// named as its module's namespace, `<kind>-schema.toml` for another.
pub fn schema_file(namespace: &str, kind: &str) -> String {
    let files = &config::SETTINGS.files;
    match namespace == kind {
        true => files.schema.clone(),
        false => files.kind_schema.replace("{kind}", kind),
    }
}

/// Whether `file` is a spec kind's versions other than `schema.toml`.
pub fn is_kind_schema(file: &str) -> bool {
    let pattern = &config::SETTINGS.files.kind_schema;
    pattern.split_once("{kind}").is_some_and(|(before, after)| {
        file.len() > before.len() + after.len() && file.starts_with(before) && file.ends_with(after)
    })
}

/// The words a module's program reads for a spec kind's `spec show`:
/// `spec show` for the kind named as its namespace, `<kind> spec show`
/// for another, so `lnk agent team spec show`.
pub fn spec_show(namespace: &str, kind: &str) -> Vec<String> {
    let core = core();
    let mut words = vec![core.spec.clone(), core.show.clone()];
    if namespace != kind {
        words.insert(0, kind.to_string());
    }
    words
}

/// The spec kind `name` is, when a package declares it: a package owns
/// no command, so each module binding it shows it with its own `spec
/// show`, as `link-actions-build` writes it.
pub fn package_kind<'a>(catalog: &'a Catalog, name: &TypeName) -> Option<&'a str> {
    let owner = catalog.module(name.module())?;
    if !owner.actions.is_empty() {
        return None;
    }
    owner.types.get(name.name())?.spec.as_deref()
}

/// A spec kind an action binds that is still read by a hand-written
/// type, until its module declares it (`held.toml`).
pub fn held_spec(kind: &str) -> Option<&'static HeldSpec> {
    config::HELD.specs.get(kind)
}

/// The folder of core's declarations, the shapes every module shares.
pub fn core_directory() -> &'static std::path::Path {
    std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/core"))
}

/// `actions.toml` alone, read and checked as far as the file alone can
/// be, its names qualified by the namespace its actions share.
pub fn read_actions(text: &str) -> anyhow::Result<ActionsFile> {
    let table: toml::Table = toml::from_str(text)?;
    let namespace = table
        .get("actions")
        .and_then(toml::Value::as_table)
        .and_then(|actions| actions.keys().next())
        .and_then(|id| id.split('.').next())
        .unwrap_or_default()
        .to_string();
    let file = read::actions::read(text, &namespace)?;
    anyhow::ensure!(
        file.actions.keys().all(|id| id.namespace() == namespace),
        "an actions.toml's actions share one namespace"
    );
    Ok(file)
}

/// `variables.toml` alone: the variables a crate defines and uses.
pub fn read_variables(text: &str) -> anyhow::Result<Variables> {
    read::variables::read(text)
}

/// `types.toml` alone, the shapes `module` declares, their names not yet
/// resolved.
pub fn read_types(
    text: &str,
    module: &str,
) -> anyhow::Result<std::collections::BTreeMap<String, Declared>> {
    read::types::read(text, module)
}

/// The declaration files a module's folder may hold, in the order a
/// build watches them.
pub fn file_names() -> [&'static str; 5] {
    let files = &config::SETTINGS.files;
    [
        &files.module,
        &files.actions,
        &files.types,
        &files.variables,
        &files.schema,
    ]
}

#[cfg(test)]
link_test_kit::program!(unit);
