//! The repository's checks, one program: each file a module, so `cargo
//! test` links once. One check: `cargo test -p link-repository-check
//! dependencies::`.

link_test_kit::program!(system);

mod architecture;
mod commands;
mod credentials;
mod dependencies;
mod flags;
mod flakes;
mod legacy;
mod licenses;
mod lints;
mod manifest_order;
mod placed;
mod program_names;
mod ready_mades;
mod release;
mod released;
mod saved;
mod shapes;
mod state_roots;
mod test_kit;
mod tool_names;
mod versions;
