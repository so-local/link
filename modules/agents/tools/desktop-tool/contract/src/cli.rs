//! `lnk-desktop-tool`'s runtime arguments, and the command line that gives
//! them (`argv`), generated from the tool's declarations (`actions.toml`
//! and `types.toml`): the agent module builds it, and the tool's own
//! parser, generated from the same files, reads it.

include!(concat!(env!("OUT_DIR"), "/args.rs"));
