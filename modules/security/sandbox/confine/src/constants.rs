//! The confinement's values: read here, used by their one name.

/// The variable naming the file of certificate authorities a program
/// trusts: OpenSSL's, and so curl's, Python's, uv's and rustls's.
pub const CA_FILE: &str = "SSL_CERT_FILE";

/// `fchmodat2`'s system call number, 452 on every Linux architecture.
/// The libc crate names it on some targets and not others (aarch64), so
/// the seccomp filter names it here, once.
#[cfg(target_os = "linux")]
pub const SYS_FCHMODAT2: libc::c_long = 452;
