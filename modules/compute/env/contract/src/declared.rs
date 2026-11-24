//! The environment module's shapes, generated from its declarations
//! (`types.toml`): the environment spec and its parts, and the outputs
//! `lnk env` answers.

include!(concat!(env!("OUT_DIR"), "/declared.rs"));
