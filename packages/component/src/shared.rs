//! Flags more than one module takes, each declared once, with its help,
//! by the shared type that owns it (`shared.toml`): `output` owns `--json`,
//! `quiet` owns `--quiet`, `workplace` the slots saying where work goes. A
//! module's `actions.toml` names a flag's type as its argument's `shared`,
//! and the parser types the flag as the declaration says. An action may
//! add what is its own to say after the shared help (what `--json`
//! prints), never replace it. A composed type may also declare a
//! positional (`workplace`'s cloud, short for `--cloud <cloud>`), which
//! every command composing the type takes after its own.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::Deserialize;

use crate::{OptionDeclaration, ValueType};

/// A shared type: what it's for, whether a command takes its flags only
/// by composing it, and its flags by their long names.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Type {
    pub help: String,
    /// Its flags are its only where a command flattens the type, which
    /// the parser shows as a group named the type: `--on` written out by a
    /// command for its own meaning stays that command's.
    #[serde(default)]
    pub composed: bool,
    /// Its flags are taken by a command's own field, once, required or
    /// repeated, which one Rust type can't say: the type holds their one
    /// meaning, and each declaration is typed as it says.
    #[serde(default)]
    pub counted: bool,
    pub flags: BTreeMap<String, Flag>,
    /// Its positionals by their names, which only a composed type has:
    /// each follows the command's own positionals.
    #[serde(default)]
    pub positionals: BTreeMap<String, Flag>,
}

/// A shared flag or positional: its shape and the help every command
/// that takes it shows.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Flag {
    pub r#type: ValueType,
    #[serde(default)]
    pub short: Option<String>,
    #[serde(default)]
    pub environment: Option<String>,
    pub help: String,
    /// The words every command's own help for a counted flag holds.
    #[serde(default)]
    pub says: Option<String>,
}

/// The help of a composed flag a command refuses begins this way: it says
/// why, in place of what the flag means.
pub const NOT_TAKEN: &str = "Not taken";

static TYPES: LazyLock<BTreeMap<String, Type>> = LazyLock::new(|| {
    toml::from_str(include_str!("../shared.toml")).expect("shared.toml declares the shared types")
});

/// Every shared type, by its name.
pub fn types() -> &'static BTreeMap<String, Type> {
    &TYPES
}

/// The flag `--<long>` of the shared type `name`.
pub fn flag(name: &str, long: &str) -> Option<&'static Flag> {
    TYPES.get(name)?.flags.get(long)
}

/// The positional `name` of the shared type `ty`.
pub fn positional(ty: &str, name: &str) -> Option<&'static Flag> {
    TYPES.get(ty)?.positionals.get(name)
}

/// What the shared type `ty` declares of the argument `name`, a flag by
/// its long name, else a positional by its own.
pub fn declared(ty: &str, name: &str, long: Option<&str>) -> Option<&'static Flag> {
    match long {
        Some(long) => flag(ty, long),
        None => positional(ty, name),
    }
}

/// The type a command takes `--<long>` from when it writes the flag out
/// itself: a shared type that isn't composed, whose flag it is.
pub fn owner(long: &str) -> Option<(&'static str, &'static Flag)> {
    TYPES
        .iter()
        .filter(|(_, ty)| !ty.composed)
        .find_map(|(name, ty)| ty.flags.get(long).map(|flag| (name.as_str(), flag)))
}

impl Flag {
    /// Whether `arg`, the flag `--<long>`, is this flag as the parser
    /// types it.
    pub fn is(&self, arg: &OptionDeclaration) -> bool {
        arg.r#type == self.r#type && arg.short == self.short && arg.environment == self.environment
    }

    /// The help a command shows: the shared help, then its own `detail`;
    /// or the detail alone where the command refuses the flag.
    pub fn help(&self, detail: Option<&str>) -> String {
        match detail.map(str::trim) {
            Some(detail) if detail.starts_with(NOT_TAKEN) => detail.to_string(),
            Some(detail) => format!("{}: {detail}", self.help),
            None => self.help.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_flag_has_one_type_and_only_a_composed_type_has_positionals() {
        let mut seen = BTreeMap::new();
        for (name, ty) in types() {
            assert!(!ty.flags.is_empty(), "{name} has no flags");
            for long in ty.flags.keys() {
                // A composed type's flags are its only where a command
                // flattens it, so one may also be a counted type's.
                assert!(
                    ty.composed || seen.insert(long, name).is_none(),
                    "--{long} is in two types"
                );
            }
            assert!(
                ty.composed || ty.positionals.is_empty(),
                "{name} has positionals, but no command composes it"
            );
        }
        assert_eq!(owner("json").map(|(name, _)| name), Some("output"));
        assert!(
            owner("on").is_none(),
            "--on is workplace's only where it's composed"
        );
        assert!(flag("workplace", "on").is_some());
        assert!(declared("workplace", "place", None).is_some());
        assert!(declared("workplace", "place", Some("place")).is_none());
    }

    #[test]
    fn a_refused_flag_says_why_alone() {
        let on = flag("workplace", "on").unwrap();
        assert_eq!(on.help(Some("a box of yours")), "A machine: a box of yours");
        assert_eq!(
            on.help(Some("Not taken: it acts on a box")),
            "Not taken: it acts on a box"
        );
        assert_eq!(on.help(None), "A machine");
    }
}
