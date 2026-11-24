//! `lnk-bundle`: the bundle module's commands. `lnk` runs it for `lnk
//! bundle ...`.

use link_actions::value::Words;
use link_bundle::cli::{self, Command};
use link_format::{
    Done, Name, Out, Path as Where, Prose, Report, Sentence, Table, Version, cmd, phrase, sentence,
};

fn main() -> std::process::ExitCode {
    link_secretive::process::guard();
    link_core::describe_component!(link_bundle::cli::Bundle);
    link_settings::output::run(try_main)
}

/// A failure, headed by what didn't happen, with its cause's words.
fn failed(headline: Sentence, err: anyhow::Error) -> Report {
    Report::error(headline).why(Prose::from(Sentence::adopt(&format!("{err:#}"))))
}

fn try_main(out: &mut Out) -> Result<Done, Report> {
    let parsed = cli::parse();
    let fields = parsed.fields;
    let command = parsed.command.command;
    let here = std::env::current_dir()
        .map_err(|e| failed(sentence!("The current folder can't be read."), e.into()))?;
    match command {
        Command::BundleCreate(create) => {
            if create.name.is_some() && create.spec.is_some() {
                return Err(named_twice());
            }
            let spec = link_bundle::spec::merged(create, &fields, &here)
                .map_err(|e| failed(sentence!("The bundle's spec can't be read."), e))?;
            let notes = out.notes();
            let made = link_bundle::make::create(spec, &here, &|step| {
                notes.note(Sentence::adopt(&format!("{step}.")));
            })
            .map_err(|e| failed(sentence!("The bundle wasn't made."), e))?;
            Ok(made_done(&made))
        }
        Command::BundleList(list) => {
            let mut shelf = Vec::new();
            for (name, text) in link_bundle::spec::SHIPPED {
                let spec = link_bundle::spec::read(text)
                    .map_err(|e| failed(sentence!("A built-in bundle can't be read."), e))?;
                shelf.push(cli::Shipped {
                    name: name.to_string(),
                    build: spec.build.unwrap_or_default(),
                    signer: spec.signer,
                });
            }
            if list.json {
                out.reply(&cli::Shelf(shelf));
                return Ok(Done::shown());
            }
            let mut table = Table::new(
                ["Bundle", "Build", "Signer"],
                sentence!("It comes with no bundles."),
            );
            for shipped in shelf {
                table.row([
                    shipped.name,
                    shipped.build.word().to_string(),
                    shipped.signer.unwrap_or_default(),
                ]);
            }
            out.block(table);
            Ok(listed_done())
        }
        Command::SpecShow(show) => {
            let shown = link_spec::show::<link_bundle::cli::Bundle>(
                &show,
                &fields,
                Some(&|name: &str| link_bundle::spec::named(name)),
            )
            .map_err(|e| failed(sentence!("The bundle's spec can't be read."), e))?;
            match show.output.json {
                true => out.reply(&serde_json::from_str::<serde_json::Value>(&shown).map_err(
                    |e| failed(sentence!("The bundle's spec can't be read."), e.into()),
                )?),
                false => out.data(shown.as_bytes()),
            }
            Ok(Done::shown())
        }
        Command::SignerCreate(create) => {
            let made = link_bundle::seal::create(&create.name)
                .map_err(|e| failed(sentence!("The signer wasn't made."), e))?;
            signer(out, made, create.json, true)
        }
        Command::SignerShow(show) => {
            let kept = link_bundle::seal::kept(&show.name)
                .map_err(|e| failed(sentence!("The signer can't be shown."), e))?;
            signer(out, kept, show.json, false)
        }
        Command::BundleShow(show) => {
            let shown = link_bundle::show::show(
                std::path::Path::new(&show.folder),
                show.signer.as_deref(),
                &here,
            )
            .map_err(|e| failed(sentence!("The bundle doesn't check out."), e))?;
            if show.root {
                out.data(format!("{}\n", shown.root.display()).as_bytes());
                return Ok(Done::default());
            }
            if show.json {
                out.reply(&cli::Contents {
                    root: shown.root.clone(),
                    programs: shown
                        .programs
                        .iter()
                        .map(|(program, target)| cli::Built {
                            program: program.clone(),
                            target: *target,
                            architecture: target
                                .architecture()
                                .expect("an archive's target is a release target"),
                        })
                        .collect(),
                });
                return Ok(Done::shown());
            }
            let mut table = Table::new(
                ["Program", "Target", "Architecture"],
                sentence!("It holds no programs."),
            );
            for (program, target) in &shown.programs {
                let architecture = target.architecture().map_or("", |a| a.word());
                table.row([
                    program.clone(),
                    target.word().to_string(),
                    architecture.to_string(),
                ]);
            }
            out.block(table);
            out.block(link_format::Note(shown.verification()));
            let _ = shown.files;
            Ok(Done::default())
        }
    }
}

/// A signer of your own, made or shown: its fields, or its JSON with
/// `--json`, and what to do with it.
fn signer(
    out: &mut Out,
    kept: link_bundle::seal::Kept,
    json: bool,
    made: bool,
) -> Result<Done, Report> {
    let shown = cli::Signer {
        name: kept.name,
        folder: kept.folder,
        signers: kept.signers,
        line: kept.line,
    };
    if json {
        out.reply(&shown);
        return Ok(Done::shown());
    }
    out.block(
        link_format::Fields::new()
            .row("Signer", shown.name.clone())
            .row("Folder", shown.folder.display().to_string())
            .row("Signers", shown.signers.display().to_string())
            .row("Line", shown.line.clone()),
    );
    let mut done = match made {
        true => Done::success(sentence!(
            "Made signer {} in {}.",
            Name::new("signer", &shown.name),
            Where::new(&shown.folder)
        )),
        false => Done::default(),
    };
    done.next.push(
        cmd!(
            "lnk bundle create devbox --signer {}",
            Name::new("signer", &shown.name)
        ),
        phrase!("sign a bundle with it"),
    );
    Ok(done)
}

/// The refusal of a create given both a built-in bundle's name and a spec.
fn named_twice() -> Report {
    Report::error(sentence!("Couldn't tell which bundle to make."))
        .why(Prose::from(sentence!(
            "It takes a built-in bundle by name or a spec file, never both."
        )))
        .next(
            cmd!("lnk bundle create --help"),
            phrase!("see its arguments"),
        )
        .exit(link_format::Exit::Usage)
}

/// What a create says once it has made the bundle.
fn made_done(made: &link_bundle::make::Made) -> Done {
    let done = if made.signed {
        sentence!(
            "Made and signed bundle {} in {}.",
            Version::new(&made.version),
            Where::new(&made.out)
        )
    } else {
        sentence!(
            "Made unsigned bundle {} in {}.",
            Version::new(&made.version),
            Where::new(&made.out)
        )
    };
    let mut done = Done::success(done);
    done.next.push(
        cmd!("lnk bundle show {}", Where::new(&made.out)),
        phrase!("check what it holds"),
    );
    if !made.signed {
        done.next.push(
            cmd!("lnk bundle signer create <name>"),
            phrase!("make a signer of your own to sign the next"),
        );
    }
    done
}

/// What a list says after its table.
fn listed_done() -> Done {
    let mut done = Done::default();
    done.next.push(
        cmd!("lnk bundle create devbox"),
        phrase!("make a dev box's bundle"),
    );
    done.next.push(
        cmd!("lnk bundle spec show devbox"),
        phrase!("see what it builds"),
    );
    done
}

#[cfg(test)]
link_test_kit::program!(unit);

#[cfg(test)]
mod tests;
