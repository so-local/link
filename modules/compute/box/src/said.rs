//! A report a command wrote, carried up through `anyhow` from the code
//! that knows what went wrong to `main`, which shows it.

use link_format::{Prose, Report, Sentence};

/// A [`Report`] as an error: what `main` shows as it was written.
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

/// `err` as a report: the one written for it, with what was being done
/// when it came (`.within`) as its why unless it has one, else one made
/// from it, which is a bug.
pub fn report(err: anyhow::Error) -> Report {
    let Some(at) = err.chain().position(|c| c.downcast_ref::<Said>().is_some()) else {
        return Report::from(err);
    };
    let Some(Said(report)) = err.chain().nth(at).and_then(|c| c.downcast_ref::<Said>()) else {
        return Report::from(err);
    };
    let mut report = report.clone();
    let doing: Vec<String> = err.chain().take(at).map(|c| c.to_string()).collect();
    if report.why.is_none() && !doing.is_empty() {
        let mut text = doing.join(": ");
        if let Some(first) = text.get(..1).map(str::to_uppercase) {
            text.replace_range(..1, &first);
        }
        report.why = Some(Prose::from(Sentence::adopt(&text)));
    }
    report
}

/// A failure the person can read: `headline`, and the error's own words
/// as its cause.
pub fn failed(headline: Sentence, err: anyhow::Error) -> Report {
    match err.chain().find_map(|c| c.downcast_ref::<Said>()) {
        Some(Said(report)) => report.clone(),
        None => Report::error(headline).why(words(&err)).cause(err),
    }
}

/// An error's words, as prose: what a report says of why.
pub fn words(err: &anyhow::Error) -> Prose {
    Prose::from(Sentence::adopt(&format!("{err:#}")))
}

/// A refusal of what the person typed, as an error: `headline`, `why`,
/// and where to go from here.
pub fn usage(
    headline: Sentence,
    why: Prose,
    next: link_format::Command,
    phrase: &str,
) -> anyhow::Error {
    said(
        Report::error(headline)
            .why(why)
            .next(next, phrase)
            .exit(link_format::Exit::Usage),
    )
}

/// [`Out::ask`], its refusal to ask carried as an error.
pub trait Asks {
    fn asks<A: link_format::Answer>(
        &mut self,
        question: link_format::Question<A>,
    ) -> anyhow::Result<A>;
}

impl Asks for link_format::Out {
    fn asks<A: link_format::Answer>(
        &mut self,
        question: link_format::Question<A>,
    ) -> anyhow::Result<A> {
        self.ask(question).map_err(said)
    }
}
