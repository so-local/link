//! `lnk archive`: your things' state exported to one archive and
//! imported from one, by the archive package, each kind's owner asked
//! what only it knows. Files only: no network call, nothing started or
//! stopped, no program run but the owners' answers.

pub mod checks;
pub mod cli;
pub mod owners;

#[cfg(test)]
link_test_kit::program!(unit);
