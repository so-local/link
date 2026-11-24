//! The public identity and access contract for agents. Names, IDs and
//! owners are every machine's ([`link_machine`]); where an agent's
//! secrets were copied, the secrets' ([`place`]); what its own secrets
//! are for, [`own`].

pub mod access;
mod constants;
pub mod own;
pub mod owner;

pub use link_machine::names;
pub use link_secret_contract::place;

#[cfg(test)]
link_test_kit::program!(unit);
