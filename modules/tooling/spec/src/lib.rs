//! `lnk spec`: the spec kinds every installed module exports, listed,
//! shown and converted by the same code each module reads its specs with
//! (`link_spec`), and no code of the owning module's.

pub mod cli;
pub mod kinds;

#[cfg(test)]
link_test_kit::program!(unit);
