//! The storage module: your files (`~/Link/Files`) and each owner's
//! folder, one another module says is its own, each backed up to its own
//! bucket in every cloud you connect. `lnk storage push` sends files to
//! every connected cloud and checks them; `lnk storage clean` deletes the
//! local copies only when every cloud has one whose checksum matches.
//! Nothing runs in the background, and your accounts' keys never leave
//! this machine: a box an owner's folder moves to gets a key reaching only
//! that owner's bucket ([`owners`]).
//!
//! The engine that talks to clouds is rclone ([`rclone`]), behind the
//! [`engine::Engine`] trait; everything above it ([`ops`]) is Link's. An
//! owned folder a cloud account keeps is there, behind that cloud's key:
//! its pushes and pulls go through the module that keeps its owner
//! instead ([`hosted`]).

mod checks;
pub mod cli;
pub mod clouds;
pub mod connect;
pub mod engine;
pub mod files;
pub mod hosted;
pub mod ids;
pub mod manifest;
pub mod ops;
pub mod owners;
pub mod rclone;
pub mod said;
pub mod tree;

#[cfg(test)]
link_test_kit::program!(unit);
