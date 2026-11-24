//! `lnk-spec`: the spec module's commands. `lnk` runs it for `lnk spec
//! ...`.

use link_format::{
    Done, Name, Note, Out, Path as Where, Prose, Report, Sentence, Table, cmd, phrase, sentence,
};
use link_spec_module::cli::{self, Command};
use link_spec_module::kinds::{self, Owned};

#[cfg(test)]
link_test_kit::program!(unit);

fn main() -> std::process::ExitCode {
    link_secretive::process::guard();
    link_core::describe_component!();
    link_settings::output::run(try_main)
}

/// A failure, headed by what didn't happen, with its cause's words.
fn failed(headline: Sentence, err: anyhow::Error) -> Report {
    Report::error(headline).why(Prose::from(Sentence::adopt(&format!("{err:#}"))))
}

/// Every kind the installed modules export. A module that doesn't answer
/// is said so and passed over.
fn installed(out: &mut Out) -> Result<Vec<Owned>, Report> {
    let mut descriptions = Vec::new();
    for (program, described) in link_core::descriptor::installed() {
        match described {
            Ok(description) => descriptions.push(description),
            Err(e) => out.block(Note(Sentence::adopt(&format!(
                "{program} didn't describe itself, so its kinds aren't here: {e:#}."
            )))),
        }
    }
    kinds::owned(&descriptions)
        .map_err(|e| failed(sentence!("The installed modules' spec kinds disagree."), e))
}

/// What a schema step changed, as `spec show` lists it.
fn step_said(kind: &str, step: &link_spec::schema::Step) -> String {
    let mut changes: Vec<String> = Vec::new();
    changes.extend(
        step.rename
            .iter()
            .map(|(old, new)| format!("{old} became {new}")),
    );
    changes.extend(
        step.remove
            .iter()
            .map(|(old, why)| format!("{old} went ({why})")),
    );
    changes.extend(
        step.refuse
            .iter()
            .map(|(old, why)| format!("{old} is refused ({why})")),
    );
    let until = match &step.until {
        Some(until) => format!(" until {until}"),
        None => String::new(),
    };
    format!(
        "{kind} schema {}, since {}{until}: {}.",
        step.from + 1,
        step.since,
        changes.join("; ")
    )
}

fn try_main(out: &mut Out) -> Result<Done, Report> {
    match cli::parse() {
        Command::SpecList(_) => {
            let owned = installed(out)?;
            out.reply(
                &owned
                    .iter()
                    .map(|o| {
                        serde_json::json!({
                            "kind": o.kind.kind,
                            "module": o.module,
                            "schema": o.kind.schema,
                        })
                    })
                    .collect::<Vec<_>>(),
            );
            let mut table = Table::new(
                ["Kind", "Module", "Schema"],
                sentence!("No installed module exports a spec kind."),
            );
            for o in &owned {
                table.row([
                    o.kind.kind.clone(),
                    o.module.clone(),
                    o.kind.schema.to_string(),
                ]);
            }
            out.block(table);
            let mut done = Done::default();
            done.next
                .push(cmd!("lnk spec show <kind>"), phrase!("see a kind's fields"));
            Ok(done)
        }
        Command::SpecShow(show) => {
            let owned = installed(out)?;
            let Some(kind) = owned.iter().find(|o| o.kind.kind == show.kind) else {
                return Err(Report::error(sentence!(
                    "No installed module exports the {} spec kind.",
                    Name::new("kind", &show.kind)
                ))
                .next(cmd!("lnk spec list"), phrase!("list the kinds there are")));
            };
            let format = kind.kind.format();
            out.reply(&format.json_schema(kind.kind.shape.clone()));
            let mut table = Table::new(
                ["Flag", "Value", "Meaning"],
                sentence!("The {} spec has no fields.", kind.kind.kind.clone()),
            );
            for field in link_spec::fields::fields(&kind.kind.shape) {
                table.row([
                    format!("--{}", field.flag),
                    field.hint,
                    field.summary.unwrap_or_default(),
                ]);
            }
            out.block(table);
            out.block(Note(Sentence::adopt(&format!(
                "The {} module owns it, at schema {}. {}.",
                kind.module,
                kind.kind.schema,
                match kind.consumers.as_slice() {
                    [] => "No action consumes it".to_string(),
                    consumers => format!("Consumed by {}", consumers.join(", ")),
                }
            ))));
            for step in &kind.kind.steps {
                out.block(Note(Sentence::adopt(&step_said(&kind.kind.kind, step))));
            }
            for gone in &kind.history {
                out.block(Note(Sentence::adopt(&gone.message())));
            }
            Ok(Done::default())
        }
        Command::SpecConvert(convert) => {
            let owned = installed(out)?;
            // Read bounded and trusted, as every spec is, before anything
            // looks inside it.
            let text = link_spec::read::<toml::Table>(&convert.file)
                .map_err(|e| failed(sentence!("{} can't be read.", Where::new(&convert.file)), e))?
                .text;
            let cannot = |e| {
                failed(
                    sentence!("{} can't be converted.", Where::new(&convert.file)),
                    e,
                )
            };
            let kind = match kinds::kind_of(&text).map_err(cannot)? {
                Some(name) => {
                    let Some(kind) = owned.iter().find(|o| o.kind.kind == name) else {
                        return Err(Report::error(sentence!(
                            "No installed module exports the {} spec kind.",
                            Name::new("kind", &name)
                        ))
                        .next(cmd!("lnk spec list"), phrase!("list the kinds there are")));
                    };
                    kind
                }
                None => {
                    return Err(failed(
                        sentence!("{} can't be converted.", Where::new(&convert.file)),
                        anyhow::anyhow!(kinds::no_kind(&owned)),
                    )
                    .next(cmd!("lnk spec list"), phrase!("list the kinds there are")));
                }
            };
            let name = kind.kind.kind.clone();
            let converted =
                link_spec::convert(&kind.kind.format(), &kind.kind.shape, &convert.file).map_err(
                    |e| {
                        failed(
                            sentence!("{} wasn't converted.", Where::new(&convert.file)),
                            e,
                        )
                    },
                )?;
            Ok(Done::success(match converted.changed {
                true if converted.from == kind.kind.schema => sentence!(
                    "Wrote {}'s kind and schema at its top.",
                    Where::new(&convert.file)
                ),
                true => sentence!(
                    "Rewrote {} from {} schema {} to schema {}.",
                    Where::new(&convert.file),
                    name.clone(),
                    u64::from(converted.from),
                    u64::from(kind.kind.schema)
                ),
                false => sentence!(
                    "{} is in the current {} schema already.",
                    Where::new(&convert.file),
                    name.clone()
                ),
            }))
        }
    }
}
