//! `lnk-archive`: the archive module's commands. `lnk` runs it for `lnk
//! archive ...`.

use link_archive::{Roots, Selection, names::Ask};
use link_archive_module::cli::{
    self, ArchiveExportArgs as Export, ArchiveImportArgs as Import, Command,
};
use link_archive_module::owners::Installed;
use link_format::{
    Command as Cmd, Done, Exit, Out, Path as Where, Prose, Question, Report, Sentence, Size, Table,
    cmd, phrase, sentence,
};

fn main() -> std::process::ExitCode {
    link_secretive::process::guard();
    link_core::describe_component!();
    link_settings::output::run(try_main)
}

/// A failure, headed by what didn't happen, with its cause's words.
fn failed(headline: Sentence, err: anyhow::Error) -> Report {
    Report::error(headline).why(Prose::from(Sentence::adopt(&format!("{err:#}"))))
}

fn try_main(out: &mut Out) -> Result<Done, Report> {
    let command = cli::parse();
    let roots =
        Roots::here().map_err(|e| failed(sentence!("Link's folders can't be found."), e))?;
    let owners = Installed::here();
    match command {
        Command::ArchiveExport(export) => export_things(out, &roots, &owners, export),
        Command::ArchiveImport(import) => import_archive(out, &roots, &owners, import),
    }
}

/// A command another program wrote, word by word.
fn command_of(text: &str) -> Cmd {
    Cmd::typed(text.split_whitespace().map(str::to_string))
}

fn today() -> String {
    link_format::time::date(std::time::SystemTime::now())
}

fn export_things(
    out: &mut Out,
    roots: &Roots,
    owners: &Installed,
    export: Export,
) -> Result<Done, Report> {
    if let Some(folder) = &export.from {
        return export_folder(roots, folder, export.to.as_deref());
    }
    let things: Vec<Selection> = match &export.select {
        Some(names) => {
            let picks = link_archive::parse_picks(names)
                .map_err(|e| failed(sentence!("Nothing was exported."), e))?;
            if let Some(pick) = picks.iter().find(|p| p.to.is_some()) {
                return Err(Report::error(sentence!(
                    "An export keeps each thing's name: {} names another.",
                    pick.thing.to_string()
                ))
                .exit(Exit::Usage)
                .next(
                    cmd!("lnk archive import <path> --select <kind.name>=<kind.new-name>"),
                    phrase!("rename it as it's imported"),
                ));
            }
            picks.into_iter().map(|p| p.thing).collect()
        }
        None => link_archive::everything(roots, owners)
            .map_err(|e| failed(sentence!("Nothing was exported."), e))?,
    };
    if things.is_empty() {
        return Err(Report::error(sentence!("Nothing here exports.")).next(
            cmd!("lnk archive export --help"),
            phrase!("see what exports"),
        ));
    }
    let to = export
        .to
        .clone()
        .unwrap_or_else(|| link_archive::default_name(roots, &things, &today()));
    let description = things
        .iter()
        .map(Selection::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let exported = link_archive::export(roots, &things, owners, &to, &description, None)
        .map_err(|e| failed(sentence!("Nothing was exported."), e))?;
    let mut table = Table::new(["Thing", "Size"], sentence!("It holds nothing."));
    for (thing, bytes) in &exported.things {
        table.row([
            link_format::Value::from(thing.to_string()),
            Size(*bytes).into(),
        ]);
    }
    out.block(table);
    let mut done = Done::success(sentence!(
        "Exported to {}. It holds everything of each, its files included.",
        Where::new(&exported.path)
    ));
    done.next.push(
        cmd!("lnk archive import {}", Where::new(&exported.path)),
        phrase!("import it on another machine"),
    );
    Ok(done)
}

/// `--from`: an archive kept as a folder, written as one archive by the
/// same writer, to `to` or `~/Link/Archive/<folder's name>.tar.gz`.
fn export_folder(
    roots: &Roots,
    folder: &std::path::Path,
    to: Option<&std::path::Path>,
) -> Result<Done, Report> {
    let to = match to {
        Some(to) => to.to_path_buf(),
        None => {
            let name = std::path::absolute(folder)
                .ok()
                .and_then(|f| f.file_name().and_then(|n| n.to_str()).map(str::to_string))
                .ok_or_else(|| {
                    Report::error(sentence!(
                        "{} has no name to give its archive.",
                        Where::new(folder)
                    ))
                    .exit(Exit::Usage)
                    .next(
                        cmd!("lnk archive export --from <folder> --to <file>"),
                        phrase!("name the archive"),
                    )
                })?;
            roots.archive().join(format!("{name}.tar.gz"))
        }
    };
    link_archive::pack(folder, &to).map_err(|e| failed(sentence!("Nothing was exported."), e))?;
    let mut done = Done::success(sentence!(
        "Exported {} to {}.",
        Where::new(folder),
        Where::new(&to)
    ));
    done.next.push(
        cmd!("lnk archive import {}", Where::new(&to)),
        phrase!("import it"),
    );
    Ok(done)
}

/// Import's questions, in this terminal.
struct Terminal<'a>(&'a mut Out);

impl Ask for Terminal<'_> {
    fn confirm(&mut self, question: &str) -> anyhow::Result<bool> {
        self.0
            .ask(Question::confirm(Sentence::adopt(question), false))
            .map_err(|_| anyhow::anyhow!("no answer was given, so nothing was imported"))
    }

    fn line(&mut self, prompt: &str) -> anyhow::Result<String> {
        self.0
            .ask(Question::text(Sentence::adopt(
                prompt.trim_end_matches(' '),
            )))
            .map_err(|_| anyhow::anyhow!("no answer was given, so nothing was imported"))
    }

    fn say(&mut self, text: &str) {
        self.0.block(link_format::Note(Sentence::adopt(text)));
    }
}

fn import_archive(
    out: &mut Out,
    roots: &Roots,
    owners: &Installed,
    import: Import,
) -> Result<Done, Report> {
    let picks = import
        .select
        .as_deref()
        .map(link_archive::parse_picks)
        .transpose()
        .map_err(|e| failed(sentence!("Nothing was imported."), e))?;
    let can_ask = out.can_ask();
    let imported = {
        let mut terminal = Terminal(out);
        let ask: Option<&mut dyn Ask> = match can_ask {
            true => Some(&mut terminal),
            false => None,
        };
        link_archive::import(&import.path, roots, picks.as_deref(), owners, ask)
    }
    .map_err(|e| failed(sentence!("Nothing was imported."), e))?;
    let mut done = Done::success(Sentence::adopt(&format!(
        "Imported {}",
        imported
            .things
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    )));
    for thing in &imported.things {
        for note in &thing.notes {
            out.block(link_format::Note(Sentence::adopt(note)));
        }
        if !thing.missing.is_empty() {
            let mut needs = Table::new(
                ["It needs", "Set up with"],
                sentence!("It needs nothing more."),
            );
            for need in &thing.missing {
                needs.row([need.what.clone(), need.setup.clone()]);
            }
            out.block(needs);
        }
        done.next.push(command_of(&thing.next), phrase!("start it"));
    }
    Ok(done)
}
