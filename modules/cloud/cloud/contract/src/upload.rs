//! An object-scoped, write-once PUT. The signed URL is a credential.

use anyhow::ensure;
use link_secretive::Duplicate;
use serde::{Deserialize, Serialize};

/// The signed upload, generated from the contract's declarations.
pub use crate::declared::Upload;

impl Duplicate for Upload {
    fn duplicate(&self) -> Self {
        Self {
            url: self.url.duplicate(),
            headers: self.headers.clone(),
        }
    }
}

#[derive(link_secretive::Duplicate, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Results {
    #[duplicate]
    pub output: Upload,
    #[duplicate]
    pub record: Upload,
    /// Its profile's (`profile.jsonl`).
    #[duplicate]
    pub profile: Upload,
}

/// An object path, never a flag, URL or parent traversal.
pub fn check_object(object: &str) -> anyhow::Result<()> {
    ensure!(
        !object.is_empty()
            && object.len() <= 1024
            && object.split('/').all(|part| {
                !part.is_empty()
                    && part != "."
                    && part != ".."
                    && !part.starts_with('-')
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            }),
        "an object path is relative, of letters, digits, dots, dashes and underscores"
    );
    Ok(())
}

/// UTC in the signing protocol's fixed format, using the platform's clock.
pub fn timestamp(seconds: u64) -> anyhow::Result<String> {
    let seconds = libc::time_t::try_from(seconds)?;
    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    // SAFETY: both pointers refer to their declared types; gmtime_r initializes tm.
    let ptr = unsafe { libc::gmtime_r(&seconds, tm.as_mut_ptr()) };
    ensure!(
        !ptr.is_null(),
        "the signing date is outside the clock's range"
    );
    // SAFETY: gmtime_r returned a non-null pointer to the initialized tm.
    let tm = unsafe { tm.assume_init() };
    Ok(format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    ))
}
