//! The harness module's tests, one program: each file a module, so
//! `cargo test` links once, not once per file. One file: `cargo test
//! --test harness moving::`.

link_test_kit::program!(system);

mod common;
#[cfg(target_os = "linux")]
mod foreground_stop;
mod owner_cli;
mod questions;
mod state_export;

#[cfg(target_os = "linux")]
mod agents;
#[cfg(unix)]
mod answering;
#[cfg(unix)]
mod archives;
#[cfg(unix)]
mod connect;
#[cfg(target_os = "linux")]
mod connection_cli;
#[cfg(target_os = "linux")]
mod egress;
#[cfg(target_os = "linux")]
mod grants;
#[cfg(unix)]
mod moving;
#[cfg(unix)]
mod renamed;
#[cfg(unix)]
mod runtime;
#[cfg(unix)]
mod schedules;
#[cfg(unix)]
mod share;
#[cfg(target_os = "linux")]
mod skills;
#[cfg(target_os = "linux")]
mod stop;
mod storage;
#[cfg(target_os = "linux")]
mod team_definitions;
#[cfg(target_os = "linux")]
mod teams;
#[cfg(target_os = "linux")]
mod terminal;
mod up;
#[cfg(unix)]
mod webhooks;
