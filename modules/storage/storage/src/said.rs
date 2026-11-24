//! What the library says to a person, and the reports its errors become.
//!
//! The library holds no `Out`: what it says along the way is said aside,
//! and an error a command must answer with its own words or Next is a
//! [`Said`] a command downcasts to its report ([`report`]).

use link_format::{Note, Prose, Report, Sentence, aside};

/// A report a library function returns as an error, for the command to
/// show as it is.
#[derive(Debug)]
pub struct Said(pub Report);

impl std::fmt::Display for Said {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.headline)
    }
}

impl std::error::Error for Said {}

/// `report` as an error.
pub fn said(report: Report) -> anyhow::Error {
    anyhow::Error::new(Said(report))
}

/// `err` as the report it carries, else a failure headed `headline`
/// with the error's own words as its why.
pub fn report(err: anyhow::Error, headline: Sentence) -> Report {
    match err.downcast::<Said>() {
        Ok(Said(report)) => report,
        Err(err) => failed(headline, err),
    }
}

/// A failure headed `headline`, the error's own words as its why.
pub fn failed(headline: Sentence, err: anyhow::Error) -> Report {
    Report::error(headline)
        .why(Prose::from(Sentence::adopt(&format!("{err:#}"))))
        .cause(err)
}

/// A warning the library found and goes on after: `headline`, and the
/// error's own words.
pub fn warn(headline: Sentence, err: &anyhow::Error) {
    aside(Report::warning(headline).why(Prose::from(Sentence::adopt(&format!("{err:#}")))));
}

/// A line along the way from code that holds no `Out`.
pub fn note(says: Sentence) {
    aside(Note(says));
}
