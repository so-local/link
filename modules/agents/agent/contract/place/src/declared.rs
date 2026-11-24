//! The hosting commands' shapes, generated from their declarations
//! (`types.toml`): what each `agent-*` command reads and answers.

include!(concat!(env!("OUT_DIR"), "/declared.rs"));
