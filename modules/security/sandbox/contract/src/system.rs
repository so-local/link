//! Baseline filesystem exposure used by security-sensitive consumers.

/// What Linux's sandbox shows a program of the machine outside its own
/// folders, read only, those that exist: the system's programs and
/// libraries, and selected public settings from `/etc`. Anything kept in them,
/// whatever its mode, a sandboxed program running as its owner can read;
/// `/run`, `/tmp` and the homes it gives fresh.
pub const LINUX_READS: &[&str] = &[
    "/usr",
    "/bin",
    "/sbin",
    "/lib",
    "/lib32",
    "/lib64",
    "/libx32",
    "/opt",
    "/nix",
    "/snap",
    "/sys",
    "/var/lib/ca-certificates",
];
