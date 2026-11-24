//! Link Harness's tests, one program: each file a module, so `cargo
//! test` links once, not once per file. One file: `cargo test --test
//! harness turns::`.

link_test_kit::program!(system);

mod common;

mod acp;

#[cfg(unix)]
mod blank;
mod busy;
mod commands;
mod compaction;
#[cfg(unix)]
mod conformance;
mod endpoint_key;
mod fallback;
mod features;
mod goals;
mod instructions;
mod interactions;
mod media;
mod memory_chats;
#[cfg(target_os = "linux")]
mod notes;
mod owners;
mod reads;
mod request;
#[cfg(target_os = "linux")]
mod run_file;
#[cfg(target_os = "linux")]
mod runtime;
#[cfg(target_os = "linux")]
mod sandbox;
mod shared;
#[cfg(target_os = "linux")]
mod shared_bridge;
#[cfg(target_os = "linux")]
mod shared_sandbox;
#[cfg(target_os = "linux")]
mod skills;
mod storage;
mod tools;
mod turns;
