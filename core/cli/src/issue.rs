//! `lnk issue create`: the last failure's record as the bug form's fields, scoped
//! and scrubbed when the record was written ([`format::issue`]), to paste,
//! or with `--open` in the form itself. Link sends nothing: the person
//! reads it all first.

use link_format::{self as format, Done, Out, Report, WithinExt, cmd, phrase, sentence};

/// A link longer than this may not open: the fields that don't fit are
/// left to paste.
const LINK_LIMIT: usize = 8000;

pub fn run(out: &mut Out, open: bool) -> Result<Done, Report> {
    let Some(mut issue) = last()? else {
        return Err(
            Report::error(sentence!("No failure is recorded on this machine.")).next(
                cmd!("lnk --help"),
                phrase!("see what's here, or open the bug form with nothing filled in"),
            ),
        );
    };
    let scrub = format::scrub::Scrub::here();
    for name in issue.names.clone() {
        if let Some(diagnosis) = diagnose(&name.kind, &name.name) {
            issue.diagnosed(&name, diagnosis, &scrub);
        }
    }
    out.data(issue.markdown().as_bytes());
    if !open {
        return Ok(Done::shown());
    }
    let mut link = issue.link();
    if link.len() > LINK_LIMIT {
        link = format::issue::Issue::link_of(&issue, &["version", "os"]);
    }
    if !open_in_browser(&link) {
        out.data(format!("\n{link}\n").as_bytes());
        return Ok(Done::success(sentence!(
            "The bug form's link is above, with what fits filled in: paste the rest."
        )));
    }
    Ok(Done::success(sentence!(
        "Opened the bug form with what fits filled in: paste the rest from above."
    )))
}

/// The newest record's issue, if any record has one.
fn last() -> Result<Option<format::issue::Issue>, Report> {
    let Some(dir) = link_settings::output::records() else {
        return Ok(None);
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(None);
    };
    let mut records: Vec<_> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    // Their names are their times, so the newest sorts last.
    records.sort();
    for path in records.iter().rev() {
        let bytes = std::fs::read(path).within("reading the last record")?;
        let doc = format::record::parse(&bytes).within("reading the last record")?;
        if let Some(issue) = doc
            .get("issue")
            .and_then(|i| serde_json::from_value(i.clone()).ok())
        {
            return Ok(Some(issue));
        }
    }
    Ok(None)
}

/// `name`'s state, from the module whose command diagnoses its `kind`:
/// `lnk <command> <name> --json`, typed blocks. None where no installed
/// module diagnoses that kind yet, or it couldn't say.
fn diagnose(kind: &str, name: &str) -> Option<serde_json::Value> {
    let own = link_component::actions::parse(crate::cli::HELP.toml)
        .ok()?
        .into_iter()
        .find(|(_, a)| a.diagnoses.as_deref() == Some(kind))
        .map(|(path, _)| path);
    let path = own.or_else(|| {
        crate::registry::REGISTRY.values().find_map(|component| {
            component
                .commands
                .iter()
                .find(|(_, c)| c.diagnoses.as_deref() == Some(kind))
                .map(|(path, _)| path.clone())
        })
    })?;
    let out = link_core::lnk()
        .args(path.split(' '))
        .arg(name)
        .arg("--json")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let answer: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    answer.get("blocks").is_some().then_some(answer)
}

/// Whether the browser was started, best effort: the fields are printed
/// either way.
fn open_in_browser(link: &str) -> bool {
    if std::env::var_os("LNK_NO_BROWSER").is_some() || !link.starts_with("https://") {
        return false;
    }
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    link_secretive::process::command(opener)
        .arg(link)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}
