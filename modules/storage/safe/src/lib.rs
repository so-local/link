//! The Safe: where the control files that steer Link live, a module's
//! settings among them, each owner's Safe named by its ID, `safe:<id>`.
//! Its actions, declared in this package's `actions.toml`, list, read and
//! write a Safe's files, and are control: no run is ever given one.

pub mod declared;

#[cfg(test)]
link_test_kit::program!(unit);
