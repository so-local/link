//! `lnk-measure`: the measure module's commands. `lnk` runs it for `lnk
//! measure ...`.

use link_format::{
    Address, Count, Done, Elapsed, Exit, Out, Prose, Report, Sentence, Table, Text, WithinExt, cmd,
    phrase, prose, sentence,
};
use link_measure::cli::{self, Command};
use link_measure::settings::{self, Export, said};
use link_measure::{OLD_LABEL, OLD_UNIT, label_of, unit_of, view, watch};
use link_measure_contract::client as measure;
use link_service::{self as service, Job};

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::saved::own(&[link_measure::settings::saved]);
    link_core::describe_component!();
    link_settings::output::run(try_main)
}

/// A failure a person causes or a file gives: `headline`, and the
/// error's own words as its why.
fn failed(headline: Sentence, err: anyhow::Error) -> Report {
    Report::error(headline)
        .why(Prose::from(Sentence::adopt(&format!("{err:#}"))))
        .cause(err)
}

fn try_main(out: &mut Out) -> Result<Done, Report> {
    // `lnk uninstall` and `lnk module remove` stop the watcher, which
    // would outlive its program (`link_core::uninstall`).
    let command = cli::parse();
    if let Command::Uninstall(given) = &command {
        let args = given;
        use link_core::uninstall as u;
        // The export's headers (an API key) are in its settings, if any.
        let export = settings::yours().ok().and_then(|yours| yours.export);
        let credentials = export
            .iter()
            .flat_map(|export| {
                export.headers.keys().map(|name| u::Held {
                    what: link_core::uninstall::CredentialKind::ExportHeader.into(),
                    of: name.clone(),
                    kept: format!(
                        "{} (only yours)",
                        settings::path().unwrap_or_default().display()
                    ),
                    may: format!("Sent with each look to {}", export.url),
                })
            })
            .collect::<Vec<_>>();
        let kept = u::Kept {
            parts: vec![u::Part {
                name: "measures".into(),
                about: "what your agents and this machine used, as the watcher kept it, and its \
                        settings"
                    .into(),
                paths: vec![settings::dir().map_err(settings::unusable)?],
                keys: vec![],
                first: String::new(),
                // Numbers, but for an export's header (a key) in its settings.
                secrets: Some(match credentials.is_empty() {
                    true => vec![],
                    false => vec![settings::path().map_err(settings::unusable)?],
                }),
                credentials,
            }],
            stops: "Measuring (lnk measure) stops.".into(),
            stays: String::new(),
        };
        u::run(args, &kept, stop, |_| Ok(()))?;
        return Ok(Done::shown());
    }
    match command {
        Command::Uninstall(_) => unreachable!("uninstall handled above"),
        Command::MeasureShow(args) => link_measure::capacity::show(out, args.path.as_deref()),
        Command::MeasureList(args) if args.last => view::last(out),
        Command::MeasureList(args) if args.job.is_some() => {
            link_measure::job::show(out, args.job.as_deref().expect("job selected"))
        }
        Command::MeasureList(args) => view::show(
            out,
            args.agent
                .map(|agent| view::Pick {
                    agent,
                    pid: args.pid,
                })
                .as_ref(),
        ),
        Command::MeasureStart(args) => start(args.quiet),
        Command::MeasureStop(_) => {
            stop().map_err(|err| failed(sentence!("The watcher could not be stopped."), err))?;
            Ok(Done::success(sentence!("The watcher is stopped."))
                .next(cmd!("lnk measure start"), phrase!("start it again")))
        }
        Command::MeasureWatch(_) => watch::watch(out),
        Command::ExportSet(cli::ExportSetArgs { job: Some(id), .. }) => {
            link_measure::job::export(&id)
        }
        Command::ExportSet(cli::ExportSetArgs {
            job: None,
            url: Some(to),
            header_file,
            ..
        }) => export_to(to, header_file.as_deref()),
        Command::ExportSet(cli::ExportSetArgs { .. }) => export_off(),
        Command::MeasureSet(settings) => set(out, settings),
        Command::MeasureGet(_) => get(out),
    }
}

/// `lnk measure start`: the watcher as a background service, started
/// unless it runs as this lnk would start it.
fn start(quiet: bool) -> Result<Done, Report> {
    if !service::managed() {
        if quiet {
            return Ok(Done::shown());
        }
        return Err(Report::error(sentence!(
            "Background services are off here, so the watcher can't run in one."
        ))
        .why(prose!("LNK_SERVICES=off is set."))
        .next(cmd!("lnk measure watch"), phrase!("watch in this terminal")));
    }
    let job = job()?;
    stop_old().map_err(|err| failed(sentence!("The watcher could not be started."), err))?;
    let (label, unit) = names();
    let installed = std::fs::read_to_string(if cfg!(target_os = "macos") {
        service::plist_path_of(&label)
    } else {
        service::unit_path_of(&unit)
    })
    .is_ok_and(|text| {
        text == match cfg!(target_os = "macos") {
            true => service::plist(&label, &job),
            false => service::unit(&job),
        }
    });
    if !(installed && service::running(&label, &unit)) {
        service::start(&label, &unit, &job)
            .within("starting the watcher")
            .map_err(|err| failed(sentence!("The watcher could not be started."), err))?;
    }
    if quiet {
        return Ok(Done::shown());
    }
    let every = settings::load().map_err(settings::unusable)?.every;
    Ok(Done::success(sentence!(
        "The watcher runs in the background, a look every {}.",
        Elapsed(every)
    ))
    .next(cmd!("lnk measure"), phrase!("see what it keeps")))
}

fn stop() -> anyhow::Result<()> {
    stop_old()?;
    let (label, unit) = names();
    service::stop(&label, &unit)
}

/// This home's watcher's launchd label and systemd unit.
fn names() -> (String, String) {
    let home = link_files::home_dir();
    (label_of(&home), unit_of(&home))
}

/// Stops the service an earlier lnk started for this home under the old
/// name, known by its file here: another home's old service is left alone.
fn stop_old() -> anyhow::Result<()> {
    let file = if cfg!(target_os = "macos") {
        service::plist_path_of(OLD_LABEL)
    } else {
        service::unit_path_of(OLD_UNIT)
    };
    if file.exists() {
        service::stop(OLD_LABEL, OLD_UNIT)?;
    }
    Ok(())
}

/// What the watcher's service runs: `lnk measure watch`.
fn job() -> Result<Job, Report> {
    let lnk = link_core::lnk().get_program().to_owned();
    if !std::path::Path::new(&lnk).is_file() {
        return Err(
            Report::error(sentence!("Link can't find lnk to run the watcher."))
                .next(cmd!("lnk upgrade"), phrase!("reinstall lnk")),
        );
    }
    let home = link_files::home_dir();
    Ok(Job {
        argv: vec![
            lnk.to_string_lossy().into(),
            "measure".into(),
            "watch".into(),
        ],
        env: std::collections::BTreeMap::from([
            ("HOME".to_string(), home.display().to_string()),
            ("PATH".into(), "/usr/bin:/bin:/usr/sbin:/sbin".into()),
        ]),
        workdir: home,
        log: settings::dir()
            .map_err(settings::unusable)?
            .join("watch.log"),
    })
}

/// `lnk measure export set --off`: looks stay on this machine.
fn export_off() -> Result<Done, Report> {
    let mut yours = settings::yours_but_export().map_err(settings::unusable)?;
    yours.export = None;
    settings::save(&yours).map_err(settings::unusable)?;
    Ok(
        Done::success(sentence!("Measures stay on this machine now.")).next(
            cmd!("lnk measure export set --url <url>"),
            phrase!("send them on again"),
        ),
    )
}

/// `lnk measure export set --url <url> [--header-file <file>]`.
fn export_to(to: String, header_file: Option<&std::path::Path>) -> Result<Done, Report> {
    // The endpoint is set again: one written before that's refused now
    // is never in the way.
    let mut yours = settings::yours_but_export().map_err(settings::unusable)?;
    let to: link_fetch::SendUrl = to.parse().map_err(|err: String| {
        let report = Report::error(sentence!("That endpoint can't be sent to."))
            .why(Prose::from(Sentence::adopt(&err)))
            .exit(Exit::Usage);
        match err.contains("login") {
            true => report.next(
                cmd!("lnk measure export set --url <url> --header-file <file>"),
                phrase!(
                    "put an endpoint's credentials in a header, in a file of name: secret lines"
                ),
            ),
            false => report.next(cmd!("lnk measure get"), phrase!("see where it goes now")),
        }
    })?;
    let headers = match header_file {
        Some(file) => link_secretive::read_secret(file).map_err(|err| {
            failed(sentence!("The header file could not be read."), err).exit(Exit::Usage)
        })?,
        None => link_secretive::Secret::default(),
    };
    let headers = headers
        .expose()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(settings::header)
        .collect::<anyhow::Result<_>>()
        .map_err(|err| {
            failed(sentence!("A header isn't valid."), err)
                .exit(Exit::Usage)
                .next(
                    cmd!("lnk measure export set --url <url> --header-file <file>"),
                    phrase!("give each header as name: secret"),
                )
        })?;
    let url = measure::metrics_url(to.as_str());
    yours.export = Some(Export { url: to, headers });
    settings::save(&yours).map_err(settings::unusable)?;
    Ok(Done::success(sentence!(
        "The watcher sends every sample to {}, as OTLP.",
        Address::new(url)
    ))
    .next(
        cmd!("lnk measure export set --off"),
        phrase!("stop sending them"),
    ))
}

/// `lnk measure set [--every] [--keep-fine] [--keep-minutes] ...`: sets
/// those given, then shows them all, yours and Link's.
fn set(out: &mut Out, given: cli::MeasureSetArgs) -> Result<Done, Report> {
    let cli::MeasureSetArgs {
        every,
        keep_fine,
        keep_minutes,
        processes,
        no_processes,
        folders_every,
        folders_max,
        ..
    } = given;
    // Declared at least 0, so it fits.
    let mut yours = settings::yours().map_err(settings::unusable)?;
    let usage = |headline: Sentence| {
        Report::error(headline)
            .exit(Exit::Usage)
            .next(cmd!("lnk measure get"), phrase!("see the settings"))
    };
    if folders_max == Some(0) {
        return Err(usage(sentence!("The flag --folders-max is at least 1.")));
    }
    yours.folders_max = folders_max.or(yours.folders_max);
    if processes || no_processes {
        yours.processes = Some(processes);
    }
    for (value, slot) in [
        (every, &mut yours.every),
        (keep_fine, &mut yours.keep_fine),
        (keep_minutes, &mut yours.keep_minutes),
        (folders_every, &mut yours.folders_every),
    ] {
        if let Some(value) = value {
            let duration = settings::duration(&value).map_err(|err| {
                failed(sentence!("That isn't a duration."), err)
                    .exit(Exit::Usage)
                    .next(cmd!("lnk measure get"), phrase!("see the settings"))
            })?;
            *slot = Some(said(duration));
        }
    }
    let merged = settings::merge(&yours).map_err(|err| {
        failed(sentence!("Those settings can't be used."), err)
            .exit(Exit::Usage)
            .next(cmd!("lnk measure get"), phrase!("see the settings"))
    })?;
    settings::save(&yours).map_err(settings::unusable)?;
    shown(out, &merged);
    Ok(Done::success(sentence!("Your settings are saved.")).next(
        cmd!("lnk measure get"),
        phrase!("see them, and where looks go"),
    ))
}

/// `lnk measure get`: every setting, yours and Link's, and where else
/// the watcher sends its looks.
fn get(out: &mut Out) -> Result<Done, Report> {
    let yours = settings::yours().map_err(settings::unusable)?;
    let merged = settings::merge(&yours).map_err(|err| {
        failed(sentence!("Your settings can't be used."), err).next(
            cmd!("lnk measure set --help"),
            phrase!("see how to set them"),
        )
    })?;
    shown(out, &merged);
    Ok(match &yours.export {
        Some(e) => {
            out.block(Text(prose!(
                "Every sample also goes to {} ({}).",
                Address::new(measure::metrics_url(e.url.as_str())),
                Count::new(e.headers.len() as u64, "header", "headers")
            )));
            Done::shown().next(
                cmd!("lnk measure export set --off"),
                phrase!("stop sending them"),
            )
        }
        None => {
            out.block(Text(prose!("Measures stay on this machine.")));
            Done::shown().next(
                cmd!("lnk measure export set --url <url>"),
                phrase!("send them to an endpoint"),
            )
        }
    }
    .next(
        cmd!("lnk measure set --every <duration>"),
        phrase!("change how often the watcher looks"),
    ))
}

/// The settings table: each one's value, and whether it's yours or Link's.
fn shown(out: &mut Out, merged: &settings::Settings) {
    let whose = |name: &str| match merged.yours.contains(&name) {
        true => "yours",
        false => "Link's",
    };
    let rows = [
        ("every", said(merged.every), "how often the watcher looks"),
        (
            "keep_fine",
            said(merged.keep_fine),
            "how long every look is kept",
        ),
        (
            "keep_minutes",
            said(merged.keep_minutes),
            "how long each minute's summary is kept",
        ),
        (
            "processes",
            match merged.processes {
                true => "on".into(),
                false => "off".into(),
            },
            "each agent's processes kept too",
        ),
        (
            "folders_every",
            said(merged.folders_every),
            "how often each agent's folders are walked",
        ),
        (
            "folders_max",
            merged.folders_max.to_string(),
            "the most entries a walk counts",
        ),
    ];
    let mut table = Table::new(
        ["Setting", "Value", "Whose", "What"],
        sentence!("There are no settings."),
    );
    for (name, value, what) in rows {
        table.row([name.to_string(), value, whose(name).into(), what.into()]);
    }
    out.block(table);
}
