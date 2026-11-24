//! `lnk-document-converter`'s shapes, its runtime arguments and the
//! command line that gives them (`argv`), generated from the module's
//! declarations (`actions.toml` and `types.toml`): Link Memory builds
//! that line, and the program's own parser, generated from the same
//! files, reads it.

include!(concat!(env!("OUT_DIR"), "/args.rs"));
