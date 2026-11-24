//! `lnk sandbox question list` and `answer`: what a sandbox asks you.

use std::time::Duration;

use link_format::{
    Done, Elapsed, Foreign, Name, Out, Report, Table, Text, Value, cmd, prose, sentence,
};
use link_sandbox::bridges::asks;
use link_sandbox::cli::{self};
use link_sandbox_proxy::wire;

pub(crate) fn asks(
    out: &mut Out,
    cli::QuestionListArgs { json }: cli::QuestionListArgs,
) -> Result<Done, Report> {
    let waiting = asks::waiting(&wire::dir()?)?;
    if json {
        // A line each, as its readers parse it.
        for ask in &waiting {
            link_actions::answer::line::<link_sandbox_contract::spec::Question>(out, ask)?;
        }
        return Ok(Done::shown());
    }
    let mut table = Table::new(
        ["ID", "QUESTION", "LEFT"],
        sentence!("No questions are waiting."),
    );
    for ask in &waiting {
        let text = link_interactions::render::delivery(ask);
        let one_line = text.split("\n\n").collect::<Vec<_>>().join(" ");
        let left = ask
            .question
            .expires_at
            .saturating_sub(link_interactions::now());
        table.row([
            Value::from(Name::new("question", &ask.response_code)),
            Value::from(Foreign::new(&one_line)),
            Value::from(Elapsed(Duration::from_secs(left))),
        ]);
    }
    out.block(table);
    let mut done = Done::shown();
    if waiting.is_empty() {
        return Ok(done.next(cmd!("lnk sandbox log show"), "see the calls on the wire"));
    }
    out.block(Text(prose!(
        "Each one is no if not answered in the time it has left."
    )));
    for ask in &waiting {
        done = done.next(
            cmd!(
                "lnk sandbox answer {} yes",
                Name::new("question", &ask.response_code)
            ),
            "allow it once; no refuses it",
        );
    }
    Ok(done)
}

pub(crate) fn answer(
    cli::SandboxAnswerArgs { id, answer }: cli::SandboxAnswerArgs,
) -> Result<Done, Report> {
    let yes = answer == cli::SandboxAnswerArgsAnswer::Yes;
    let shown = asks::answer(&wire::dir()?, &id, yes).map_err(|err| {
        Report::from(err).next(cmd!("lnk sandbox question list"), "see what is waiting")
    })?;
    match shown {
        true if yes => Ok(Done::success(sentence!(
            "Allowed; awaiting source checks and application."
        ))
        .next(cmd!("lnk sandbox log show"), "see what it did")),
        true => Ok(Done::success(sentence!("Refused."))),
        false => Err(
            Report::error(sentence!("No question {} is waiting.", Foreign::new(&id)))
                .why(prose!("It was answered, or it was no in time."))
                .next(cmd!("lnk sandbox question list"), "see what is waiting"),
        ),
    }
}
