//! One module per command; `main.rs` only dispatches.

pub(crate) mod apt;
pub(crate) mod asks;
pub(crate) mod check;
pub(crate) mod confine;
pub(crate) mod enter;
pub(crate) mod learn;
pub(crate) mod log;
pub(crate) mod opening;
pub(crate) mod permissions;
pub(crate) mod proxy;
pub(crate) mod ready;
pub(crate) mod run;
pub(crate) mod spec;
pub(crate) mod stop;
pub(crate) mod tool;
#[cfg(target_os = "linux")]
pub(crate) mod workers;

pub(crate) use opening::*;
pub(crate) use proxy::*;
pub(crate) use ready::*;
pub(crate) use run::*;
