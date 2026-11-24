//! `lnk sandbox learn`: what a program runs, for `[commands]`.

use std::path::PathBuf;

use link_format::{
    Count, Done, Out, Path as Shown, Report, Text, Value, Verbatim, WithinExt, phrase, prose,
};
use link_sandbox::cli::{self};
use link_sandbox_client::Programs;
use link_sandbox_client::Wrapped;
use link_secretive::Duplicate;

use super::*;

pub(crate) fn learn_command(
    out: &mut Out,
    cli::SandboxLearnArgs {
        spec,
        upstream,
        yes,
        commands: command,
    }: cli::SandboxLearnArgs,
    fields: link_spec::FieldFlags,
) -> Result<Done, Report> {
    let opening = link_sandbox::opening::Opening::of(spec, fields, upstream, yes);
    flagless(&command)?;
    let name = format!("learn-{}", std::process::id());
    let opened = opening.opened(&name)?;
    if let Some(read) = &opened.read {
        asked(out, read, &opened, opening.yes)?;
    }
    Ok(Done::passed(learn(out, &opened, &command)?))
}

/// `lnk sandbox learn`: runs `command` in the sandbox with every program
/// open, shows the programs it started that its `[commands]` doesn't
/// list, and returns the command's exit status.
pub(crate) fn learn(out: &mut Out, opened: &Opened, command: &[String]) -> Result<i32, Report> {
    let backend = link_sandbox_confine::here()?;
    ready(out, true)?;
    let mut policy = opened.sandbox.duplicate();
    let listed = policy.commands.clone().listed();
    policy.set_programs(Programs::Any);
    let Wrapped { argv, env } = link_sandbox::wrap_learning(&policy, command)?;
    // Only what it keeps: none of this process's secrets.
    let env: Vec<(String, String)> = link_secretive::process::variables(|key| KEPT.contains(&key))
        .chain(env)
        .collect();
    // The program writes to this terminal itself.
    let _hand = out.hand_over();
    let ran = backend.learn(&argv, &env);
    clean_up(&policy);
    let (status, text) = ran?;
    print_learned(out, opened, listed.as_deref(), &text)?;
    Ok(exit_code(status))
}

/// What `learn` found, `text` its programs started (one line each, the
/// path given, a tab, the file): the entries to add to `[commands]
/// allow`, never one the sandbox may write.
pub(crate) fn print_learned(
    out: &mut Out,
    opened: &Opened,
    listed: Option<&[PathBuf]>,
    text: &str,
) -> Result<(), Report> {
    let path = link_files::absolute_path(&std::env::var_os("PATH").unwrap_or_default());
    let mut not_listed: Vec<PathBuf> = vec![
        std::env::current_exe()
            .and_then(|exe| exe.canonicalize())
            .within("finding this program")?,
    ];
    not_listed.extend(link_sandbox_confine::linux::landlock::loaders());
    let learned = link_sandbox_confine::programs::learned(
        text,
        listed,
        &not_listed,
        &opened.confinement.write,
        &path,
    );
    match (&listed, learned.add.is_empty()) {
        (None, _) => out.block(Text(prose!(
            "Its [commands] allow is \"*\": it runs any program already. It ran {}.",
            Count::new(learned.ran as u64, "program", "programs")
        ))),
        (Some(_), true) => out.block(Text(prose!("It ran only what [commands] allow lists."))),
        (Some(_), false) => {
            out.block(Text(prose!(
                "It ran these, which [commands] allow doesn't list."
            )));
            let mut lines = Verbatim::new().lead(phrase!("Add them"));
            for (entry, interpreter) in &learned.add {
                let line = format!("{},", toml::Value::String(entry.clone()));
                lines = match interpreter {
                    true => lines.commented(line, "an interpreter: it runs any code it's given"),
                    false => lines.line(line),
                };
            }
            out.block(lines);
        }
    }
    if !learned.writable.is_empty() {
        out.block(Text(prose!(
            "Not offered, since the sandbox may write them: {}.",
            Value::List(
                learned
                    .writable
                    .iter()
                    .map(|p| Shown::new(p).into())
                    .collect()
            )
        )));
        out.block(Text(prose!(
            "A script there runs through its interpreter instead: node ./x, not ./x."
        )));
    }
    Ok(())
}
