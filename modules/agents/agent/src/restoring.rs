//! `lnk agent restore`: an agent back from its bucket, or what it knew
//! from its backup, here or where it's hosted.

use link_agent::cli;
use link_agent::{moved, settings::Settings};
use link_format::{
    Done, Exit, Foreign, Name, Note, Out, Question, Report, Text, cmd, prose, sentence,
};

use crate::creating::start;
use crate::moving;

/// `lnk agent restore`, of the agent `named` or the one it names.
pub(crate) async fn restore(
    out: &mut Out,
    named: Option<&str>,
    restore: &cli::AgentRestoreArgs,
    fields: &link_spec::FieldFlags,
) -> Result<Done, Report> {
    let starting = &cli::Starting::restore(restore, fields);
    let cli::AgentRestoreArgs {
        name,
        id,
        yes,
        no_start,
        ..
    } = restore;
    let name = name.as_deref().or(named);
    if let Some(name) = name {
        link_machine::machine::check_name(name)?;
    }
    // A hosted agent: its host restores its last backup there.
    if let Some(name) = name
        && !Settings::path_of(name).exists()
        && let Some(note) = moved::of_name(name)?
        && note.to.cloud
    {
        spec_unread(starting)?;
        if id.is_some() {
            return Err(Report::error(sentence!(
                "{} picks an agent that isn't anywhere.",
                Foreign::new("--id")
            ))
            .exit(Exit::Usage));
        }
        out.block(Text(prose!(
            "This replaces {}'s settings, instructions, skills, notes and files on {} \
             with its last backup there: what changed since is lost. Its conversations \
             stay.",
            Name::new("agent", name),
            Foreign::new(&note.place())
        )));
        let restore = out.ask(Question::confirm(sentence!("Restore?"), false).yes(*yes))?;
        if !restore {
            return Ok(Done::success(sentence!("Nothing restored.")));
        }
        return Ok(link_agent::hosted::backup(&note, true)?);
    }
    // An agent here: back to its last backup.
    let Some(name) = name.filter(|n| !Settings::path_of(n).exists()) else {
        if id.is_some() {
            return Err(Report::error(sentence!(
                "{} picks an agent that isn't here.",
                Foreign::new("--id")
            ))
            .exit(Exit::Usage));
        }
        spec_unread(starting)?;
        let agent = Settings::resolve(name)?;
        if !agent.path().exists() {
            return Err(
                Report::error(sentence!("There is no agent here to restore."))
                    .exit(Exit::Usage)
                    .next(cmd!("lnk agent restore <name>"), "bring one back"),
            );
        }
        return moving::backups::restore_state(out, &agent, *yes, *no_start);
    };
    if *no_start {
        spec_unread(starting)?;
    }
    let (mut agent, restored) = moving::backups::restore(out, name, id.as_deref())?;
    if *no_start {
        return Ok(Done::success(restored));
    }
    out.block(Note(restored));
    let mut done = start(out, &mut agent, starting).await?;
    if agent.connections().is_empty() {
        done = done.next(
            cmd!(
                "lnk agent connect telegram --agent {}",
                Name::new("agent", name)
            ),
            "set its connection up again (or slack, discord, signal)",
        );
    }
    Ok(done)
}

/// A restore that starts nothing reads no spec: its backup's is the
/// agent's, so `--spec` and field flags, which only a start lays over,
/// are refused rather than dropped.
fn spec_unread(starting: &cli::Starting) -> Result<(), Report> {
    if starting.spec.is_none() && starting.fields.is_empty() {
        return Ok(());
    }
    Err(Report::error(sentence!(
        "This restore starts nothing, so it reads no spec: {} and field flags go with a start.",
        cmd!("--spec")
    ))
    .exit(Exit::Usage)
    .next(
        cmd!("lnk agent start --spec <file>"),
        "start it with them after",
    ))
}
