//! `lnk agent harness set <harness>`'s question: what the switch leaves behind
//! ([`link_harness_contract::harness::Contract::left_behind`]), said and
//! agreed before anything changes; then the harness it leaves exported
//! over ASP ([`exported`]) and kept in the agent's own folder ([`left`]),
//! and what went on elsewhere imported into the one it goes to
//! ([`carry_in`]).

use std::sync::atomic::Ordering;

use link_agent::adapter::{Adapter, Boxed};
use link_agent::settings::Settings;
use link_format::{
    Name, Note, Notes, Out, Question, Report, Sentence, Value, cmd, prose, sentence,
};
use link_harness_contract::asp;
use link_harness_contract::harness::features::Feature;

use crate::permissions::{YES, declined};

/// Says what agent `settings` leaves behind switching from harness
/// `from` to `to`: what `from` keeps that `to` won't read, and what its
/// spec turns on that `from` honors and `to` doesn't. Then asks; an
/// error, which changes nothing, unless it's agreed, in a terminal or by
/// `--yes`. Nothing left behind: nothing asked.
pub(crate) fn confirm_left_behind(
    out: &mut Out,
    settings: &Settings,
    from: (&str, &Adapter),
    to: (&str, &Adapter),
) -> Result<(), Report> {
    let agent = Name::new("agent", &settings.name);
    let ((from, leaving), (to, going)) = (from, to);
    let (from_name, to_name) = (Name::new("harness", from), Name::new("harness", to));
    let (leaving, going) = (leaving.contract()?, going.contract()?);
    let on = Feature::of_spec(&settings.spec()?.spec);
    let behind: Vec<String> = leaving
        .left_behind(&going)
        .into_iter()
        .chain(leaving.not_honored(&going, &on))
        .map(|b| b.said(from))
        .collect();
    if behind.is_empty() {
        return Ok(());
    }
    let left = Value::List(behind.iter().cloned().map(Value::from).collect());
    out.block(
        Report::warning(sentence!("{} won't carry {}.", &to_name, left.clone()))
            .why(prose!(
                "They stay with {}, and come back when you switch back.",
                &from_name
            ))
            .next(cmd!("lnk agent harness set {}", &from_name), "switch back"),
    );
    if YES.load(Ordering::Relaxed) {
        out.block(Note(sentence!("Agreed by --yes.")));
        return Ok(());
    }
    if !may_ask() {
        return Err(declined(
            sentence!(
                "Switching {} to {} leaves {} behind.",
                &agent,
                &to_name,
                left
            ),
            sentence!("Nothing changed."),
        )
        .next(out.typed().and("--yes"), "switch anyway"));
    }
    let anyway = Question::confirm(
        sentence!("Switch {} to {} anyway?", &agent, &to_name),
        false,
    )
    .why(prose!(
        "No keeps it on {}, and nothing changes.",
        &from_name
    ));
    if out.ask(anyway)? {
        return Ok(());
    }
    Err(declined(
        sentence!("Not switched: {} stays on {}.", &agent, &from_name),
        sentence!("Nothing changed."),
    ))
}

/// Whether a question can be asked here: a terminal in and out.
fn may_ask() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal() && link_format::streams::stderr_is_terminal()
}

/// What harness `from` holds of agent `agent` now, in ASP's portable
/// forms: exported by it over ASP while it runs, else, for a harness
/// whose contract says `identity`, what `lnk-<name> state export` reads
/// of its files, its conversations since it last left then staying with
/// it until a switch away from it while it runs. None: nothing read.
pub(crate) async fn exported(
    out: &mut Out,
    agent: &Settings,
    from: (&str, &Adapter),
) -> anyhow::Result<Option<asp::State>> {
    let (name, leaving) = from;
    let harness = Name::new("harness", name);
    if let Some(bridge) = running(agent) {
        match link_connection_contract::acp::exported(&bridge, asp::Kind::ALL.to_vec()).await {
            Ok((_, state)) => {
                for omission in &state.omissions {
                    out.block(
                        Report::warning(sentence!(
                            "{} retains state outside portable context.",
                            &harness
                        ))
                        .why(Sentence::adopt(&omission.reason)),
                    );
                }
                return Ok(Some(state));
            }
            Err(err) => anyhow::bail!(
                "{name} did not complete its state capture; the running source is retained: {err:#}"
            ),
        }
    }
    let contract = leaving.contract()?;
    if !contract.offline_state && !contract.state.export.is_empty() {
        out.block(Note(sentence!(
            "{} isn't answering: its conversations since it last left stay with it, and come \
             along at a switch away from it while it runs.",
            &harness
        )));
    }
    if !(contract.identity || contract.offline_state) {
        return Ok(None);
    }
    let boxed = Boxed::new(agent, name)?;
    // Its programs read its files: one never started here (a migrated
    // agent) has none installed yet, so they're installed first, as a
    // start would.
    leaving.install(&boxed, &leaving.needs(agent)?, YES.load(Ordering::Relaxed))?;
    let drivers = crate::conversations::store_drivers(agent)?;
    let state = leaving.exported(&boxed, &drivers)?;
    for omission in &state.omissions {
        out.block(
            Report::warning(sentence!(
                "{} retains state outside portable context.",
                &harness
            ))
            .why(Sentence::adopt(&omission.reason)),
        );
    }
    Ok(Some(state))
}

/// The endpoint of agent `agent`'s harness while it runs, as its bridge
/// reaches it.
fn running(agent: &Settings) -> Option<link_connection_contract::Bridge> {
    let plan = link_agent::bridge::Plan::load(&agent.name).ok()?;
    if plan.endpoint.is_empty() || !link_agent::service::running_anywhere(&agent.name) {
        return None;
    }
    Some(link_connection_contract::Bridge {
        agent: agent.name.clone(),
        identity: agent.identity(),
        endpoint: plan.endpoint,
        key: plan.key,
        proof: plan.proof,
        ..Default::default()
    })
}

/// A switch away from harness `from`, which holds `state` of agent
/// `agent`: kept in the agent's own folder, so a switch back finds it,
/// and who the agent became there brought into it
/// ([`link_agent::carried::left`]), said.
pub(crate) fn left(
    out: &mut Out,
    agent: &Settings,
    from: &str,
    state: &asp::State,
) -> anyhow::Result<()> {
    if let Some(schedules) = &state.schedules {
        crate::schedules_report(out, schedules);
        let adapter = Adapter::find(from)?;
        // Stop both native execution and its scheduler before clearing.
        link_agent::service::stop(&agent.name)?;
        link_agent::service::stop_foreground(&agent.name)?;
        let uid = link_service::user_id().unwrap_or_default();
        let native = adapter
            .traces()
            .into_iter()
            .find(|t| t.kind == link_harness_contract::harness::traces::Kind::Home)
            .ok_or_else(|| anyhow::anyhow!("adapter exports schedules but names no native home"))?
            .at(&agent.box_of(from), &uid);
        link_agent::schedules::import(agent, from, &native, schedules)?;
    }
    let spec = agent.spec()?;
    let own = agent.folder().agent;
    let taken = link_agent::carried::left(&spec.spec, &own, from, state)?;
    if !taken.is_empty() {
        out.block(Note(sentence!(
            "From {}: {}, into the own folder of {}.",
            Name::new("harness", from),
            Value::List(taken.into_iter().map(Value::from).collect()),
            Name::new("agent", &agent.name)
        )));
    }
    Ok(())
}

/// The key of agent `agent`'s harness's endpoint now, if it has one: a
/// start after makes another.
pub(crate) fn key_now(agent: &Settings) -> Option<link_secretive::Secret> {
    let plan = link_agent::bridge::Plan::load(&agent.name).ok()?;
    Some(plan.key)
}

/// Conversations from elsewhere on their way into a harness a switch
/// starts: marked ahead of its start until every import is acknowledged,
/// so each message waits for them
/// ([`link_connection_contract::arriving`]).
pub(crate) struct Arriving {
    mark: std::path::PathBuf,
    state: asp::State,
    count: usize,
    complete: bool,
}

impl Drop for Arriving {
    fn drop(&mut self) {
        if self.complete {
            let _ = std::fs::remove_file(&self.mark);
        }
    }
}

/// The conversations that went on elsewhere since harness `to` last held
/// them ([`link_agent::carried::elsewhere`]), marked as arriving:
/// none for a harness that takes none in, or when there are none.
pub(crate) fn arriving(agent: &Settings, to: (&str, &Adapter)) -> anyhow::Result<Option<Arriving>> {
    let (name, going) = to;
    if !going
        .contract()?
        .state
        .import
        .contains(&asp::Kind::Sessions)
    {
        return Ok(None);
    }
    let state = link_agent::carried::elsewhere(&agent.folder().agent, name)?;
    let count = state.sessions.as_ref().map_or(0, Vec::len);
    let mark = link_connection_contract::arriving(&agent.name);
    if count == 0 {
        match std::fs::remove_file(&mark) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        return Ok(None);
    }
    link_files::write_private(&mark, b"")?;
    Ok(Some(Arriving {
        mark,
        state,
        count,
        complete: false,
    }))
}

/// Into harness `to`, once it runs with a key other than `before`'s:
/// the conversations [`arriving`] found, imported over ASP, its hub
/// keeping each as its thread so each chat follows it; what it took and
/// couldn't, said. Their mark goes only once every import is acknowledged.
#[expect(
    clippy::disallowed_methods,
    reason = "a harness that just started is asked again each second until it answers"
)]
pub(crate) fn carry_in(
    notes: Notes,
    agent: &Settings,
    name: &str,
    mut arriving: Arriving,
    before: Option<link_secretive::Secret>,
) {
    let (state, count) = (&arriving.state, arriving.count);
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            return notes.note(Sentence::adopt(&format!(
                "Conversations from elsewhere can't be taken in: {err:#}."
            )));
        }
    };
    let mut window = Window::default();
    let imported = loop {
        let bridge = running(agent).filter(|_| key_now(agent) != before);
        let answering = bridge.is_some();
        let tried = match bridge {
            // What it holds first: a version from elsewhere that parted
            // from its own goes beside it.
            Some(bridge) => runtime.block_on(async {
                let kinds = vec![asp::Kind::Sessions];
                let (_, held) = link_connection_contract::acp::exported(&bridge, kinds).await?;
                let mut coming = state.clone();
                let named = link_agent::carried::beside(&held, &mut coming);
                let expected:Vec<_> = coming.sessions.iter().flatten().map(|s|s.session_id.clone()).collect();
                let imported = link_connection_contract::acp::imported(&bridge, coming).await?;
                anyhow::ensure!(expected.iter().all(|id|imported.sessions.contains_key(id)),
                    "the destination did not acknowledge every retained conversation; messages remain blocked");
                anyhow::Ok((imported, named))
            }),
            None => Err(anyhow::anyhow!("{name} isn't answering yet")),
        };
        match tried {
            Ok(imported) => break Ok(imported),
            Err(err) => match window.open(
                &arriving.mark,
                answering,
                link_connection_contract::constants::ARRIVING,
            ) {
                // Its harness still installing or starting, or taking
                // them in: asked again in a second.
                Ok(true) => std::thread::sleep(std::time::Duration::from_secs(1)),
                Ok(false) => break Err(err),
                // The mark gone: the switch failed, and nothing waits.
                Err(gone) => break Err(gone.context(err)),
            },
        }
    };
    match imported {
        Ok((imported, named)) => {
            arriving.complete = true;
            for (thread, copy, from) in named {
                notes.note(sentence!(
                    "{} holds its own version of {}, which the conversation from {} doesn't go on \
                     from: that one is beside it, as {}.",
                    Name::new("harness", name),
                    thread.as_str(),
                    Name::new("harness", &from),
                    copy.as_str()
                ));
            }
            let took = imported.sessions.len();
            notes.note(sentence!(
                "{} took in {} of {} conversations from elsewhere.",
                Name::new("harness", name),
                took as u64,
                count as u64
            ));
            for not in imported.not_taken {
                notes.note(Sentence::adopt(&format!(
                    "{name} couldn't take its {:?}: {}.",
                    not.kind, not.reason
                )));
            }
        }
        Err(err) => notes.note(Sentence::adopt(&format!(
            "{count} conversations from elsewhere didn't reach {name} ({err:#}): they stay in the \
             folder of {}. Messages remain blocked until you retry the switch to {name}.",
            agent.name
        ))),
    }
}

/// How long [`carry_in`] goes on trying. A harness a switch starts may
/// install first, which takes Hermes minutes: that counts against no
/// deadline, its mark kept fresh meanwhile so a message waits for it
/// rather than finding it stale. From when it first answers, it has
/// `limit` to take the conversations in.
#[derive(Default)]
struct Window {
    answered: Option<std::time::Instant>,
}

impl Window {
    /// Whether to try again, after a try at a harness that `answering`
    /// or not. Refused once `mark` is gone: a switch that failed takes it
    /// away, and with it whatever waited.
    fn open(
        &mut self,
        mark: &std::path::Path,
        answering: bool,
        limit: std::time::Duration,
    ) -> anyhow::Result<bool> {
        if let Some(answered) = self.answered {
            return Ok(answered.elapsed() <= limit);
        }
        if answering {
            self.answered = Some(std::time::Instant::now());
        }
        std::fs::File::options()
            .write(true)
            .open(mark)?
            .set_modified(std::time::SystemTime::now())?;
        Ok(true)
    }
}

/// Names each MCP server harness `from` was given by its owner, not by
/// Link (`state`'s tools): a switch carries none, so each stays with it.
pub(crate) fn tools_left(out: &mut Out, from: &str, to: &str, state: &asp::State) {
    let names: Vec<Value> = state
        .tools
        .iter()
        .flatten()
        .filter_map(|server| serde_json::to_value(server).ok())
        .filter_map(|wire| {
            wire["name"]
                .as_str()
                .map(|name| Value::from(name.to_string()))
        })
        .collect();
    if names.is_empty() {
        return;
    }
    out.block(
        Report::warning(sentence!(
            "{} won't use the MCP servers {} added: {}.",
            Name::new("harness", to),
            Name::new("harness", from),
            Value::List(names)
        ))
        .why(prose!(
            "They stay with {}, and work again when you switch back.",
            Name::new("harness", from)
        )),
    );
}

/// Names content the destination cannot use before a switch changes the
/// running harness. Exact updates remain in the source snapshot.
pub(crate) fn confirm_content(
    out: &mut Out,
    agent: &Settings,
    to: &str,
    destination: &link_harness_contract::harness::Contract,
    state: &asp::State,
) -> Result<(), Report> {
    if !destination.state.import.contains(&asp::Kind::Sessions) {
        // The conversation refusal is already named by left_behind.
        return Ok(());
    }
    let omissions: Vec<String> = destination
        .session_omissions(state)
        .into_iter()
        .map(|kind| kind.words().to_string())
        .collect();
    if omissions.is_empty() {
        return Ok(());
    }
    let who = Name::new("agent", &agent.name);
    let harness = Name::new("harness", to);
    let content = Value::List(omissions.iter().cloned().map(Value::from).collect());
    out.block(
        Report::warning(sentence!(
            "{} cannot use {} in the continued conversation.",
            &harness,
            content.clone()
        ))
        .why(prose!(
            "The original session updates stay in the harness snapshots of {}.",
            &who
        )),
    );
    if YES.load(Ordering::Relaxed) {
        return Ok(());
    }
    if !may_ask() {
        return Err(declined(
            sentence!("Switching to {} omits {}.", &harness, content),
            sentence!("Nothing changed."),
        )
        .next(out.typed().and("--yes"), "switch anyway"));
    }
    let with = Question::confirm(
        sentence!("Switch {} to {} with these omissions?", &who, &harness),
        false,
    );
    if out.ask(with)? {
        return Ok(());
    }
    Err(declined(
        sentence!("Not switched: {} stays on its current harness.", &who),
        sentence!("Nothing changed."),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    /// A harness that installs for longer than the window isn't given up
    /// on: its mark is kept fresh until it answers, and only then does
    /// the window to take the conversations in start.
    #[test]
    fn the_window_opens_when_the_harness_first_answers() {
        let dir = link_test_kit::TempDir::new("arriving");
        let mark = dir.path().join("arriving");
        std::fs::write(&mark, b"").unwrap();
        // Made before an install as long as the window itself.
        let then = SystemTime::now() - link_connection_contract::constants::ARRIVING;
        std::fs::File::options()
            .write(true)
            .open(&mark)
            .unwrap()
            .set_modified(then)
            .unwrap();
        let mut window = Window::default();
        assert!(window.open(&mark, false, Duration::ZERO).unwrap());
        let age = std::fs::metadata(&mark)
            .unwrap()
            .modified()
            .unwrap()
            .elapsed()
            .unwrap_or_default();
        assert!(age < Duration::from_secs(60), "{age:?}");
        // Answering: the window starts now, so one of none is shut.
        assert!(window.open(&mark, true, Duration::ZERO).unwrap());
        assert!(!window.open(&mark, true, Duration::ZERO).unwrap());
    }

    /// A switch that failed took the mark away: nothing waits, so
    /// nothing is carried in.
    #[test]
    fn a_mark_taken_away_ends_the_window() {
        let dir = link_test_kit::TempDir::new("arriving-gone");
        let mut window = Window::default();
        assert!(
            window
                .open(&dir.path().join("arriving"), false, Duration::ZERO)
                .is_err()
        );
    }
}
