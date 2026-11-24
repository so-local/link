//! The sandbox client's shapes, generated from its declarations
//! (`types.toml`): what `lnk sandbox run --with-fd` reads.

include!(concat!(env!("OUT_DIR"), "/declared.rs"));
