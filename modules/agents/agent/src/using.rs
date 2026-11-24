//! `lnk agent harness set <harness>`: the agent on another harness, what it may
//! do there agreed to, and its state brought along.

use link_agent::{adapter::Adapter, service, settings::Settings};
use link_format::{Done, Name, Note, Out, Report, Sentence, Value, cmd, sentence};

use crate::launching::up;
use crate::model::first_run;
use crate::permissions::{confirm_switch, is_declined};
use crate::switching;

pub(crate) async fn switch(
    out: &mut Out,
    agent: &mut Settings,
    name: String,
    foreground: bool,
) -> Result<Done, Report> {
    Adapter::find_or_add(&name)?;
    let spec = agent.spec()?;
    first_run(out, agent, None, Some(&spec)).await?;
    if let Some(note) = agent.check_files()? {
        out.block(Note(Sentence::adopt(&note)));
    }
    // Whether it answered before the switch: one failing halfway
    // starts it again where it was.
    let was_running = service::running_anywhere(&agent.name);
    let before = agent.harness().to_string();
    agent.harness = Some(name.clone());
    // Saved once what it may do there is agreed (below): a Ctrl-C
    // at the question leaves it on the harness it had.
    if before == name {
        agent.save()?;
    }
    if before != name {
        out.block(Note(sentence!(
            "Switching from {} to {}, and your settings and files come along.",
            Name::new("harness", &before),
            Name::new("harness", &name)
        )));
        let speaks = Adapter::find(&name)?.speaks(agent);
        let lost: Vec<_> = agent
            .connections()
            .iter()
            .filter(|c| !speaks.speaks(agent, c))
            .map(|c| agent.title(c))
            .collect();
        if !lost.is_empty() {
            out.block(Note(sentence!(
                "{} doesn't speak {}, so it stops answering there, and answers again when \
                 you switch back with {}.",
                Name::new("harness", &name),
                Value::List(lost.into_iter().map(Value::from).collect()),
                cmd!("lnk agent harness set {}", Name::new("harness", &before))
            )));
        }
    }
    // What it may do on the new harness, agreed to before anything
    // of it changes; then what the harness it leaves holds, read
    // while it still runs; then up.
    let switched: Result<(), Report> = match before != name {
        // What stays behind first, from the harness it leaves,
        // unless that harness is gone: then nothing it kept is read.
        true => match Adapter::find(&name)
            .map_err(Report::from)
            .and_then(|adapter| {
                if let Ok(leaving) = Adapter::find(&before) {
                    switching::confirm_left_behind(
                        out,
                        agent,
                        (&before, &leaving),
                        (&name, &adapter),
                    )?;
                }
                confirm_switch(out, agent, &before, &name, &adapter)
            }) {
            // What it holds there comes along, the switch agreed.
            Ok(()) => match Adapter::find(&before) {
                Ok(leaving) => match switching::exported(out, agent, (&before, &leaving)).await {
                    Ok(Some(state)) => {
                        let destination = Adapter::find(&name)?;
                        switching::confirm_content(
                            out,
                            agent,
                            &name,
                            &destination.contract()?,
                            &state,
                        )?;
                        switching::tools_left(out, &before, &name, &state);
                        switching::left(out, agent, &before, &state).map_err(Report::from)
                    }
                    Ok(None) => Ok(()),
                    Err(err) => Err(Report::from(err)),
                },
                Err(_) => Ok(()),
            },
            Err(err) => Err(err),
        },
        false => Ok(()),
    };
    // One running in a terminal, its state read, stops there as a
    // `stop` stops it: the new harness starts here instead.
    let switched = switched.and_then(|()| {
        if before != name {
            service::stop_foreground(&agent.name).map_err(Report::from)?;
        }
        Ok(())
    });
    // What went on elsewhere, into the new harness once it answers:
    // beside a start in this terminal, which runs until stopped.
    let notes = out.notes();
    // Marked as arriving before it starts, so no message reaches it first.
    let retrying = before == name;
    let arriving = if switched.is_ok()
        && (before != name
            || link_connection_contract::arriving(&agent.name)
                .try_exists()
                .map_err(anyhow::Error::from)?)
    {
        let adapter = Adapter::find(&name)?;
        switching::arriving(agent, (&name, &adapter))?
    } else {
        None
    };
    let carrying = arriving.map(|arriving| {
        let (agent, name) = (link_secretive::Duplicate::duplicate(&*agent), name.clone());
        let key = if retrying {
            None
        } else {
            switching::key_now(&agent)
        };
        std::thread::spawn(move || switching::carry_in(notes, &agent, &name, arriving, key))
    });
    let result: Result<Done, Report> = match switched {
        Ok(()) => match agent.save() {
            Ok(()) => up(out, agent, &name, foreground).await,
            Err(err) => Err(Report::from(err)),
        },
        Err(err) => Err(err),
    };
    if let Some(carrying) = carrying
        && result.is_ok()
    {
        let _ = carrying.join();
    }
    let done = match result {
        Ok(done) => done,
        Err(err) => {
            // Not switched: keep the harness it had, so the next start
            // isn't another try at this one, and nothing waits for what
            // was coming into the new one.
            if before != name {
                let _ = std::fs::remove_file(link_connection_contract::arriving(&agent.name));
                agent.harness = Some(before.clone());
                agent.save()?;
                // Stopped halfway: answering again where it was.
                if was_running && !service::running_anywhere(&agent.name) {
                    out.block(
                        Report::warning(sentence!(
                            "Switching to {} failed, so it starts again on {}.",
                            Name::new("harness", &name),
                            Name::new("harness", &before)
                        ))
                        .why(Sentence::adopt(&err.headline.to_string())),
                    );
                    if let Err(again) = up(out, agent, &before, foreground).await {
                        out.block(
                            Report::warning(sentence!("It didn't start again."))
                                .why(Sentence::adopt(&again.headline.to_string())),
                        );
                    }
                }
                let state = match service::running_anywhere(&agent.name) {
                    true => "still running",
                    false => "stopped",
                };
                out.block(Note(sentence!(
                    "Still on {}, and {}.",
                    Name::new("harness", &before),
                    state
                )));
                if !is_declined(&err) {
                    out.block(Note(sentence!(
                        "{} tries again later.",
                        cmd!("lnk agent harness set {}", Name::new("harness", &name))
                    )));
                }
                if !service::running_anywhere(&agent.name) {
                    out.block(Note(sentence!("{} starts it.", cmd!("lnk agent start"))));
                }
            }
            return Err(err);
        }
    };
    Ok(done)
}
