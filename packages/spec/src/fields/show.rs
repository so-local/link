//! `lnk <group> spec show`, which every spec kind has: the effective spec
//! of a resource, a file or flags alone, printed and never applied.

use std::path::PathBuf;

use anyhow::{Context, bail};

use super::FieldFlags;
use super::given::{file_table, overlay_at};
use crate::Kind;

// A spec kind's own commands, under its group's `spec`. Not doc
// comments: clap would take them for help, which is each owner's, in
// its `actions.toml`.
#[derive(clap::Subcommand, Debug)]
pub enum SpecCommand {
    // The effective spec, every default written in: of a thing, a file or
    // flags alone, with field flags over it.
    Show(Show),
}

// What `spec show` prints the spec of.
#[derive(clap::Args, serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct Show {
    // An existing thing whose spec to print, as made again.
    #[serde(default)]
    pub thing: Option<String>,
    // The spec file to read.
    #[arg(long, value_name = "FILE")]
    #[serde(default)]
    pub spec: Option<PathBuf>,
    #[command(flatten)]
    #[serde(default, rename = "json")]
    pub output: crate::shared::Output,
}

/// The owner's way to the spec of an existing thing, by its name.
pub type Resource<'a, T> = &'a dyn Fn(&str) -> anyhow::Result<T>;

/// The spec `args` and `flags` name, printed in the current format with
/// its `schema` line, or as JSON: what the kind's reader takes back
/// unchanged. `resource` is the owner's, where its kind names things;
/// without one, a thing named is refused.
pub fn show<T: Kind>(
    args: &Show,
    flags: &FieldFlags,
    resource: Option<Resource<'_, T>>,
) -> anyhow::Result<String> {
    shown(args, flags, resource).map_err(link_format::refused)
}

fn shown<T: Kind>(
    args: &Show,
    flags: &FieldFlags,
    resource: Option<Resource<'_, T>>,
) -> anyhow::Result<String> {
    let format = T::format();
    let (base, file, root) = match (&args.thing, &args.spec) {
        (Some(_), Some(_)) => bail!("name a thing or give --spec FILE, not both"),
        (Some(thing), None) => {
            let resource = resource.with_context(|| {
                format!(
                    "a {} spec names no thing; give --spec FILE or flags",
                    format.kind
                )
            })?;
            let spec = resource(thing)?;
            match toml::Value::try_from(&spec)? {
                toml::Value::Table(table) => (table, None, None),
                other => bail!("a spec is a table, not {other}"),
            }
        }
        (None, Some(path)) => {
            let (table, read) = file_table::<T>(path, flags.is_empty())?;
            (table, Some(read.path), Some(read.dir))
        }
        (None, None) => (toml::Table::new(), None, None),
    };
    let root = root.map_or_else(std::env::current_dir, Ok)?;
    let spec = overlay_at::<T>(base, flags, file.as_deref(), Some(&root))?;
    match args.output.json {
        true => Ok(String::from_utf8(format.print_json(&spec)?)?),
        false => format.print(&spec),
    }
}
