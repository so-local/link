//! The shared flags more than one module takes, one type per group of
//! `link-component`'s `shared.toml`, which a command flattens: `--json`
//! and `--yes` mean the same in every command because each is this one
//! field. Each type's clap group is `shared.<its group>`, which tells
//! the parser where a flag came from, and a field spelled like one of
//! these flags in a command's own type is refused.

use serde::{Deserialize, Serialize};

// `--json`: the answer as JSON for other programs. Not a doc comment:
// clap would take it for the help of every command that flattens it.
#[derive(clap::Args, Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[group(id = "shared.output", multiple = true)]
#[serde(transparent)]
pub struct Output {
    // Global, so a program taking it before its commands has
    // it after them too; a command with no subcommands is unchanged.
    #[arg(long, global = true)]
    pub json: bool,
}

// `--quiet`: say less, for another of Link's commands running it on the way.
#[derive(clap::Args, Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[group(id = "shared.quiet", multiple = true)]
#[serde(transparent)]
pub struct Quiet {
    #[arg(long)]
    pub quiet: bool,
}

// `--yes`: say yes to what it would ask.
#[derive(clap::Args, Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[group(id = "shared.confirm", multiple = true)]
#[serde(transparent)]
pub struct Confirm {
    #[arg(long)]
    pub yes: bool,
}

// `--request`: a typed request read whole, from a file or standard input.
#[derive(clap::Args, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[group(id = "shared.request", multiple = true)]
#[serde(transparent)]
pub struct Request {
    #[arg(long, value_name = "FILE")]
    pub request: Option<std::path::PathBuf>,
}

/// The clap group a shared type's flags are in, `shared.<group>`.
pub const GROUP_PREFIX: &str = "shared.";
