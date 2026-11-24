use super::*;
mod states;
mod streams;
mod switches;
use super::archiving::{add, add_paths};
use super::indexes::index_from;
use super::landing::seed_files;
use super::receiving::{described_here, merge, put_connections, read_manifest, receive_in};
use super::restoring::{move_paths, remove_stale_restores};
use streams::signal;

/// A folder of the test's own, removed when it's dropped, the test
/// failed or not.
fn temp(name: &str) -> link_test_kit::TempDir {
    link_test_kit::TempDir::new(&format!("transfer-{name}"))
}
