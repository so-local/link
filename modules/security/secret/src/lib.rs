//! The secret module: `lnk secret`, the secrets a person keeps on this
//! machine, each given only where it's granted. The store is
//! `link_secret_contract`, which what gives a secret reads too; this is its command line, and the secret route's
//! bridge (`lnk secret bridge`, [`bridge`]), where a sandbox's call has
//! its secret added outside it.

pub mod bridge;
mod checks;
pub mod cli;
pub mod keeper;
pub mod settings;
pub mod socket;
mod tokens;

/// A failure whose cause is an error's: `headline`, and the error's own
/// words as its why.
pub fn failed(headline: link_format::Sentence, err: anyhow::Error) -> link_format::Report {
    use link_format::{Prose, Report, Sentence};
    Report::error(headline)
        .why(Prose::from(Sentence::adopt(&format!("{err:#}"))))
        .cause(err)
}

/// The machine's settings, unreadable.
pub fn unreadable(err: anyhow::Error) -> link_format::Report {
    failed(
        link_format::sentence!("This machine's settings can't be read."),
        err,
    )
}

#[cfg(test)]
link_test_kit::program!(system);
