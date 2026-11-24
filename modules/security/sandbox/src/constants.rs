//! Values the sandbox's commands use where TOML can't hold them.

/// How many of the latest calls `lnk sandbox log show` shows when `--lines`
/// is left out.
pub const LOG_LINES: usize = 40;

/// The variable a program on this machine's loopback reaches its proxy
/// by, whose address carries the run's proxy secret (`sandbox proxy
/// --pointed-by`).
pub const POINTED_BY: &str = "HTTPS_PROXY";
