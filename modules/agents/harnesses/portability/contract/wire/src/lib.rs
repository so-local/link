//! The protocol-free vocabulary a harness and its agent's host share:
//! messages, prompts and their answers, and skills.

/// The wire's shapes, generated from its declarations.
pub mod declared {
    include!(concat!(env!("OUT_DIR"), "/declared.rs"));
}
pub mod evidence;
pub mod message;
pub mod runtime;
pub mod skill;

#[cfg(test)]
link_test_kit::program!(unit);
