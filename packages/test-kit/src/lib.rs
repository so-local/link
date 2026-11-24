//! The testing kit: what keeps a test free of races and of the machine,
//! written once. A test uses these rather than rolling its own; the
//! kit's page says which raw call each one replaces.
//!
//! One module per concern. Each has a Unix and a Windows implementation,
//! or a compile error naming the one missing.

#[cfg(unix)]
pub mod deadline;
pub mod env;
pub mod fault;
pub mod gate;
pub mod home;
pub mod ids;
pub mod io;
pub mod keychain;
pub mod libtest;
pub mod net;
pub mod permit;
pub mod process;
pub mod program;
pub mod programs;
pub mod recording;
pub mod release;
pub mod temp;
pub mod users;
pub mod wait;

#[cfg(unix)]
pub use deadline::Deadline;
pub use gate::out_of_tmp;
#[cfg(unix)]
pub use net::HeldNumber;
pub use net::{Held, nodelay_of};
#[cfg(unix)]
pub use process::{ChildGroup, interrupt, kill_group};
#[cfg(target_os = "linux")]
pub use process::{end_under, left_under};
#[cfg(unix)]
pub use programs::write_core;
#[cfg(unix)]
pub use programs::write_program_with_mode;
pub use programs::{cargo, copy_program, link_program, programs, programs_in, write_program};
pub use temp::TempDir;
pub use wait::{
    eventually, eventually_async, eventually_await, within, within_async, within_await,
};

#[cfg(test)]
program!(system);
