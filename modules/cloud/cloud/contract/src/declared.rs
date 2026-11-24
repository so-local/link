//! The cloud contract's shapes, generated from its declarations
//! (`types.toml`): the request and reply every cloud adapter speaks, the
//! account, the machine a box asks for, and what a reply finds.

include!(concat!(env!("OUT_DIR"), "/declared.rs"));
