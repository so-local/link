//! The models module: `lnk model list` lists the model runtimes running here,
//! and `lnk model serve` serves one's
//! OpenAI-compatible API on a local port. `lnk model install` installs
//! Ollama and a model, and `lnk model runtime add` remembers a runtime at an
//! address of its own.
//!
//! Its bridge (`lnk model bridge`, [`bridge`]) serves the model routes a
//! sandbox's proxy hands it: a hosted model's key added outside the
//! sandbox, and this machine's runtimes for inference only.
//!
//! Model servers listen on this machine.

pub mod added;
pub mod bridge;
pub mod cli;
pub mod constants;
pub mod discover;
pub mod install;
pub mod serve;

pub use serve::{DEFAULT_UPSTREAM, LlmService, Target, choose, resolve, serve};

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

pub mod cli_api;

#[cfg(test)]
link_test_kit::program!(system);
