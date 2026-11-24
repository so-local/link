//! What an upgrade changes in the files Link writes, by the release's
//! own programs (`link_files::upgrade`): each is asked what it will
//! change before anything is installed, trying each conversion on what
//! the release's transfers leave, the list is shown and agreed to once,
//! and once the release is in place each runs its transfers, then its
//! conversions, making only what was agreed. History is never rewritten,
//! and a spec you wrote is offered for `lnk spec convert`, never
//! rewritten here.

use std::io::Write;
use std::path::{Path, PathBuf};

use link_files::upgrade::{Answer, Change, Request, Stage};
use link_format::{Note, Question, Table};

use super::*;

/// Ask `program` to do `stage`, given `request`, none for the first plan
/// a core asks: its answer, in the version it speaks to this core, which
/// each later request to it is written in.
fn ask(program: &Path, stage: Stage, request: Option<Request>) -> anyhow::Result<Answer> {
    let name = program
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let asking = || format!("asking {name} what it changes");
    let mut command = link_secretive::process::command(program);
    link_files::upgrade::asking(&mut command);
    let mut child = command
        .args([link_spec::entry::UPGRADE, stage.word()])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .within(asking())?;
    let request = request
        .map(|request| serde_json::to_vec(&request))
        .transpose()?;
    let written = child
        .stdin
        .take()
        .map(|mut stdin| stdin.write_all(request.as_deref().unwrap_or_default()))
        .transpose();
    let answer = child.wait_with_output().within(asking())?;
    ensure!(
        answer.status.success(),
        "{name} couldn't {} its files: {}",
        stage.word(),
        link_format::clean(String::from_utf8_lossy(&answer.stderr).trim())
    );
    // A program that answered without reading all of it has said its say.
    if let Err(err) = written
        && err.kind() != std::io::ErrorKind::BrokenPipe
    {
        return Err(err).within(asking());
    }
    Answer::parse(&name, &answer.stdout)
}

/// The protocol version each program of a plan answered in, in the
/// order they were asked, which each later request to it is written in.
pub(super) struct Spoken(Vec<u32>);

/// What `programs` will change, changing nothing, and the version each
/// speaks: asked twice, first for the transfers they'll run, then for
/// every change, each conversion tried on what those transfers leave.
pub(super) fn plan(programs: &[PathBuf]) -> anyhow::Result<(Vec<Change>, Spoken)> {
    let mut transfers = Vec::new();
    let mut spoken = Vec::new();
    for program in programs {
        let answer = ask(program, Stage::Plan, None)?;
        spoken.push(answer.protocol);
        transfers.extend(
            answer
                .changes
                .into_iter()
                .filter(|change| matches!(change, Change::Transfer { .. })),
        );
    }
    let mut changes = Vec::new();
    for (program, &version) in programs.iter().zip(&spoken) {
        let request = Request::stating(transfers.clone(), version);
        changes.extend(ask(program, Stage::Plan, Some(request))?.changes);
    }
    Ok((changes, Spoken(spoken)))
}

/// Whether the person agrees to `installing` (what's installed, if
/// anything) and `plan`, asked once: `yes` agrees ahead, and with no one
/// to ask and no `yes` it's refused, naming the flag.
pub(super) fn agreed(
    out: &mut Out,
    installing: Option<format::Sentence>,
    plan: &[Change],
    yes: bool,
) -> Result<bool, Report> {
    let mut table = Table::new(
        ["Change", "File", "To"],
        sentence!("No file Link writes changes."),
    );
    for change in plan {
        match change {
            Change::Convert {
                file,
                kind,
                from,
                to,
            } => table.row([
                Value::from(format!("Converted from {kind} schema {from}")),
                Value::from(Shown::new(file)),
                Value::from(format!("schema {to}")),
            ]),
            Change::Transfer {
                source,
                from,
                into,
                kind,
                paths,
            } => table.row([
                Value::from(format!("Moves {} from {from} to {kind}", paths.join(", "))),
                Value::from(Shown::new(source)),
                Value::from(Shown::new(into)),
            ]),
            Change::Kept {
                file,
                kind,
                from,
                why,
                ..
            } => table.row([
                Value::from(format!("Kept at {kind} schema {from}: {why}")),
                Value::from(Shown::new(file)),
                Value::from(format!("schema {from}")),
            ]),
            Change::Offer { .. } => {}
        }
    }
    if let Some(installing) = installing {
        out.block(Note(installing));
    }
    out.block(table);
    for (file, kind, from, to) in offers(plan) {
        out.block(Note(sentence!(
            "Your {} spec {} is in schema {}: {} converts it to schema {}, keeping its comments.",
            kind.clone(),
            Shown::new(file),
            from.to_string(),
            lnk_command(["spec", "convert", &file.display().to_string()]),
            to.to_string()
        )));
    }
    out.ask(
        Question::confirm(sentence!("Upgrade, changing these?"), true)
            .why(prose!(
                "History (conversations, logs and ledgers) is never rewritten, and your specs \
                 are left as you wrote them."
            ))
            .yes(yes),
    )
}

/// Each spec of yours a plan offers to convert: its file, kind and
/// schemas.
pub(super) fn offers(plan: &[Change]) -> impl Iterator<Item = (&PathBuf, &String, &u32, &u32)> {
    plan.iter().filter_map(|change| match change {
        Change::Offer {
            file,
            kind,
            from,
            to,
        } => Some((file, kind, from, to)),
        _ => None,
    })
}

/// Have `programs`, in place, make the changes of `agreed`, the plan
/// the person agreed to: every transfer, then every conversion, so a
/// record is taken in the schema it was written in. Each is asked in the
/// version it spoke to the plan (`spoken`, in the same order). A file a
/// step refuses is kept and named, and the rest go on. What they made.
pub(super) fn apply(
    out: &mut Out,
    programs: &[PathBuf],
    spoken: &Spoken,
    agreed: &[Change],
) -> anyhow::Result<Vec<Change>> {
    ensure!(
        programs.len() == spoken.0.len(),
        "the programs to change files aren't the ones that planned it"
    );
    let mut made = Vec::new();
    for stage in [Stage::Transfer, Stage::Convert] {
        for (program, &version) in programs.iter().zip(&spoken.0) {
            let request = Request::stating(agreed.to_vec(), version);
            made.extend(ask(program, stage, Some(request))?.changes);
        }
    }
    let changed = made
        .iter()
        .filter(|change| matches!(change, Change::Convert { .. } | Change::Transfer { .. }))
        .count();
    if changed > 0 {
        out.block(Note(sentence!(
            "Converted the files Link writes to this release's shapes ({} changes).",
            changed.to_string()
        )));
    }
    for change in &made {
        if let Change::Kept { file, why, .. } = change {
            out.block(Note(sentence!(
                "Kept {} as it was: {}.",
                Shown::new(file),
                link_format::clean(why)
            )));
        }
    }
    Ok(made)
}

/// The next steps a plan leaves: converting each spec of yours.
pub(super) fn next(done: Done, plan: &[Change]) -> Done {
    offers(plan).fold(done, |done, (file, ..)| {
        done.next(
            lnk_command(["spec", "convert", &file.display().to_string()]),
            phrase!("convert your spec to this release's schema"),
        )
    })
}
