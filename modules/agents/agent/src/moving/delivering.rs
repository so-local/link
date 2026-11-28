//! Messages a cloud held for an agent while it was paused there, given
//! to it where it went (`lnk agent deliver`).

use std::io::{BufRead, Write};
use std::process::Stdio;

use link_agent::bridge;
use link_agent::settings::Settings;
use link_format::WithinExt;
use link_format::{Foreign, Name, Note, Out, Path, Report, Sentence, cmd, prose, sentence};

use super::lnk_at;

/// Messages `held` given to agent `agent` on `to` (`lnk agent deliver`,
/// which keeps them there until they're given), one JSON line each.
/// Kept on this computer first ([`held_here`]), until `to` has them: said
/// when it doesn't.
pub(crate) fn deliver_at(
    out: &mut Out,
    to: &Option<String>,
    agent: &str,
    held: &[link_agent_place::Held],
) {
    if held.is_empty() {
        return;
    }
    let path = held_here(agent);
    let before = std::fs::read_to_string(&path).unwrap_or_default();
    let lines: Vec<u8> = held
        .iter()
        .filter_map(|message| bridge::HELD.print_json(message).ok())
        .flat_map(|line| line.into_iter().chain(*b"\n"))
        .collect();
    let kept = link_files::write_private(&path, &[before.as_bytes(), &lines].concat());
    let child = lnk_at(to, agent, &["deliver"])
        .stdin(Stdio::piped())
        .spawn();
    let handed = child.and_then(|mut child| {
        if let Some(mut input) = child.stdin.take() {
            input.write_all(&lines)?;
        }
        child.wait()
    });
    if handed.is_ok_and(|s| s.success()) {
        let _ = match before.is_empty() {
            true => std::fs::remove_file(&path),
            false => link_files::write_private(&path, before.as_bytes()),
        };
        return;
    }
    let agent = Name::new("agent", agent);
    out.block(match kept {
        Ok(()) => Report::warning(sentence!(
            "The {} message(s) that came while {} was paused couldn't be handed to it: kept in {}.",
            held.len(),
            &agent,
            Path::new(&path)
        ))
        .next(
            cmd!(
                "lnk agent deliver --agent {} < {}",
                &agent,
                Path::new(&path)
            ),
            "give them where it runs",
        ),
        Err(err) => Report::warning(sentence!(
            "The {} message(s) that came while {} was paused couldn't be handed to it nor kept.",
            held.len(),
            &agent
        ))
        .why(Sentence::adopt(&format!(
            "{err}: {}",
            held.iter()
                .map(|h| format!(
                    "{:?} from {}",
                    link_format::clean(&h.text),
                    link_format::clean(&h.from)
                ))
                .collect::<Vec<_>>()
                .join("; ")
        ))),
    });
}

/// Where this computer keeps the messages a cloud held for agent `agent`
/// until where it went has them: `~/.lnk/safe/agents/transfers/held/<agent>.jsonl`,
/// each line naming its schema ([`bridge::HELD`]).
pub(crate) fn held_here(agent: &str) -> std::path::PathBuf {
    link_files::layout::Hidden::Held
        .path()
        .join(format!("{agent}.jsonl"))
}

/// `lnk agent deliver`: each message on stdin, a JSON line as a cloud
/// that hosts agents held it while the agent was paused there
/// (`link_agent_place::Held`), kept with the agent's settings
/// ([`bridge::keep_held`]) before any is given; then each kept, given to
/// the agent here as its connection's message ([`bridge::give_held`]). One
/// it can't be given now waits for its bridge to give it as it reaches
/// its harness; one it's never given here is shown whole, and goes. One
/// held for another generation of it ([`bridge::for_generation`]: the
/// agent a move left, or a later one) is shown and never kept. Fails
/// only when they couldn't be kept.
pub async fn deliver(out: &mut Out, settings: &Settings) -> anyhow::Result<()> {
    let mut messages = Vec::new();
    let input = std::io::BufReader::new(std::io::Read::take(std::io::stdin().lock(), 256 << 20));
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        messages.push(
            bridge::HELD
                .read_json::<link_agent_place::Held>(line.as_bytes())
                .within("reading a held message")?
                .spec,
        );
    }
    let agent = Name::new("agent", &settings.name);
    let here = bridge::generation(&settings.name);
    let (messages, others): (Vec<_>, Vec<_>) = messages
        .into_iter()
        .partition(|held| bridge::for_generation(here.as_deref(), held));
    for held in &others {
        out.block(
            Report::warning(sentence!(
                "A message from {} on {} was held for another move of {}: not given here.",
                Foreign::new(&held.from),
                Foreign::new(&held.connection),
                &agent
            ))
            .why(prose!("It said {}.", Foreign::new(&held.text))),
        );
    }
    bridge::keep_held(&settings.name, &messages)?;
    let Ok(plan) = bridge::Plan::load(&settings.name) else {
        out.block(Note(sentence!(
            "{} message(s) wait for {}: given when it starts.",
            messages.len(),
            &agent
        )));
        return Ok(());
    };
    let given = bridge::give_held(settings, &plan, false).await?;
    for (held, why) in &given.refused {
        out.block(
            Report::warning(sentence!(
                "A message from {} on {} ({}) can't be given to {} here, {}.",
                Foreign::new(&held.from),
                Foreign::new(&held.connection),
                Foreign::new(&held.chat),
                &agent,
                Foreign::new(why)
            ))
            .why(prose!("It said {}.", Foreign::new(&held.text))),
        );
    }
    if given.given > 0 {
        out.block(Note(sentence!(
            "{} was given {} message(s).",
            &agent,
            given.given
        )));
    }
    if given.waiting > 0 {
        out.block(Note(sentence!(
            "{} message(s) wait for {}: given once it answers.",
            given.waiting,
            &agent
        )));
    }
    Ok(())
}
