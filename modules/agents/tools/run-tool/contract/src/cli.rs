//! `lnk-run-tool`'s runtime arguments, and the command line that gives
//! them (`argv`), generated from the tool's declarations (`actions.toml`
//! and `types.toml`): the agent module builds `serve`'s and the
//! background commands', and the tool's own parser, generated from the
//! same files, reads them.

include!(concat!(env!("OUT_DIR"), "/args.rs"));
