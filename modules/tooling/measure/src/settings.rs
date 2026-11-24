//! The watcher's settings: how often it samples, how long it keeps what
//! it saw, and where else it sends it. Yours are
//! `~/.lnk/safe/measures/settings.toml`; what they leave out is Link's,
//! the file this module comes with (`defaults.toml`), never a constant.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::bail;
use link_format::WithinExt;
use link_secretive::Duplicate;
use serde::{Deserialize, Serialize};

/// Link's own settings, as this module comes with them.
pub const DEFAULTS: &str = include_str!("../defaults.toml");

/// The settings as written: yours, or Link's, each key optional.
#[derive(Duplicate, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Written {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub every: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_fine: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_minutes: Option<String>,
    /// Whether each agent's processes are kept, beside it and the
    /// machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processes: Option<bool>,
    /// How often each agent's folders are walked for their size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folders_every: Option<String>,
    /// The most entries a walk of an agent's folders counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folders_max: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[duplicate]
    pub export: Option<Export>,
}

/// Where the watcher also sends every sample, as OTLP.
#[derive(Duplicate, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Export {
    /// An OTLP/HTTP endpoint (`http://localhost:4318`): samples go to
    /// `<url>/v1/metrics`. `https://`, or `http://` to this machine.
    pub url: link_fetch::SendUrl,
    /// Sent with each request (an API key): why the file is only yours.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub headers: BTreeMap<String, link_secretive::Secret>,
}

/// The settings the watcher runs with: yours over Link's.
#[derive(Duplicate, Debug, PartialEq)]
pub struct Settings {
    pub every: Duration,
    pub keep_fine: Duration,
    pub keep_minutes: Duration,
    /// Each agent's processes kept too.
    pub processes: bool,
    pub folders_every: Duration,
    pub folders_max: u64,
    #[duplicate]
    pub export: Option<Export>,
    /// Which of them are yours, by name.
    pub yours: Vec<&'static str>,
}

/// The measure module's folder: `~/.lnk/safe/measures`.
pub fn dir() -> anyhow::Result<PathBuf> {
    link_files::layout::Hidden::Measures.try_path()
}

pub fn path() -> anyhow::Result<PathBuf> {
    Ok(dir()?.join("settings.toml"))
}

link_files::specification!(Written, include_str!("settings.schema.toml"));

/// Your measure settings' file, which an upgrade converts.
pub fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Written>()
}

/// Your settings, as written; none when there's no file.
pub fn yours() -> anyhow::Result<Written> {
    let path = path()?;
    match std::fs::read_to_string(&path) {
        Ok(text) => link_files::saved::parse(&text).within(format!("reading {}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Written::default()),
        Err(err) => Err(err).within(format!("reading {}", path.display())),
    }
}

/// Your settings but their endpoint, for `lnk measure export set`, which
/// sets it again: one refused as it's read (`http://` off this machine,
/// written before that was refused) is replaced, never in the way.
pub fn yours_but_export() -> anyhow::Result<Written> {
    let path = path()?;
    match std::fs::read_to_string(&path) {
        Ok(text) => but_export(&text).within(format!("reading {}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Written::default()),
        Err(err) => Err(err).within(format!("reading {}", path.display())),
    }
}

/// `text`, your settings, without their endpoint.
fn but_export(text: &str) -> anyhow::Result<Written> {
    let mut table: toml::Table = toml::from_str(&link_files::saved::advance::<Written>(text)?)?;
    table.remove("export");
    Ok(table.try_into()?)
}

/// Writes `written` as your settings, only yours to read.
pub fn save(written: &Written) -> anyhow::Result<()> {
    let path = path()?;
    link_files::write_private(&path, link_files::saved::print(written)?.as_bytes())
        .within(format!("writing {}", path.display()))
}

/// Link's own settings.
pub fn links() -> Written {
    toml::from_str(DEFAULTS).expect("defaults.toml is the module's own")
}

/// The settings the watcher runs with: `yours` over Link's.
pub fn merge(yours: &Written) -> anyhow::Result<Settings> {
    let links = links();
    let mut mine = Vec::new();
    let mut pick = |name: &'static str, yours: &Option<String>, links: &Option<String>| {
        if yours.is_some() {
            mine.push(name);
        }
        let value = yours.as_ref().or(links.as_ref()).within("no value")?;
        duration(value).within(format!("{name} = {value:?}"))
    };
    let every = pick("every", &yours.every, &links.every)?;
    let fine = pick("keep_fine", &yours.keep_fine, &links.keep_fine)?;
    let minutes = pick("keep_minutes", &yours.keep_minutes, &links.keep_minutes)?;
    let folders = pick("folders_every", &yours.folders_every, &links.folders_every)?;
    for (name, set) in [
        ("processes", yours.processes.is_some()),
        ("folders_max", yours.folders_max.is_some()),
    ] {
        if set {
            mine.push(name);
        }
    }
    let processes = yours
        .processes
        .or(links.processes)
        .within("no value for processes")?;
    let folders_max = yours
        .folders_max
        .or(links.folders_max)
        .within("no value for folders_max")?;
    if yours.export.is_some() {
        mine.push("export");
    }
    if every < Duration::from_secs(1) {
        bail!("every is at least 1s");
    }
    Ok(Settings {
        every,
        keep_fine: fine,
        keep_minutes: minutes,
        processes,
        folders_every: folders,
        folders_max,
        export: yours.export.duplicate(),
        yours: mine,
    })
}

/// The settings now.
pub fn load() -> anyhow::Result<Settings> {
    merge(&yours()?)
}

/// `90s`, `15m`, `12h`, `30d` (a number alone is seconds).
pub fn duration(text: &str) -> anyhow::Result<Duration> {
    let text = text.trim();
    let split = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: u64 = number.parse().within(format!(
        "{text:?} isn't a duration like 10s, 15m, 12h or 30d"
    ))?;
    let seconds = match unit {
        "" | "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        _ => bail!("{text:?} isn't a duration like 10s, 15m, 12h or 30d"),
    };
    Ok(Duration::from_secs(number.saturating_mul(seconds)))
}

/// `d` as [`duration`] reads it, in its largest whole unit.
pub fn said(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0 => "0s".into(),
        _ if s.is_multiple_of(86_400) => format!("{}d", s / 86_400),
        _ if s.is_multiple_of(3_600) => format!("{}h", s / 3_600),
        _ if s.is_multiple_of(60) => format!("{}m", s / 60),
        _ => format!("{s}s"),
    }
}

/// A header as `--header` takes it (`Name: value`), checked.
pub fn header(text: &str) -> anyhow::Result<(String, link_secretive::Secret)> {
    let (name, value) = text
        .split_once(':')
        .within("a header isn't one like x-api-key: <key>")?;
    let (name, value) = (name.trim(), value.trim());
    let token = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b));
    if !token || value.bytes().any(|b| b.is_ascii_control()) {
        bail!("a header isn't one like x-api-key: <key>");
    }
    Ok((name.to_string(), value.to_string().into()))
}

/// A failure to read or write the settings, as a report: the file's own
/// words are its why.
pub fn unusable(err: anyhow::Error) -> link_format::Report {
    use link_format::{Prose, Report, Sentence, sentence};
    Report::error(sentence!("Your measure settings can't be used."))
        .why(Prose::from(Sentence::adopt(&format!("{err:#}"))))
        .cause(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An endpoint read back is typed: `http://` off this machine is
    /// refused, and `lnk measure export set` replaces it, keeping the rest.
    #[test]
    fn an_endpoint_off_this_machine_in_the_clear_is_refused_and_replaceable() {
        let text = |url: &str| format!("every = \"5s\"\n\n[export]\nurl = \"{url}\"\n");
        let read = toml::from_str::<Written>(&text("http://localhost:4318")).unwrap();
        assert_eq!(read.export.unwrap().url.as_str(), "http://localhost:4318");
        let err = toml::from_str::<Written>(&text("http://otel-collector:4318")).unwrap_err();
        assert!(err.to_string().contains("http:// to this machine"), "{err}");
        let kept = but_export(&text("http://otel-collector:4318")).unwrap();
        assert!(kept.export.is_none());
        assert_eq!(kept.every.as_deref(), Some("5s"));
    }

    #[test]
    fn links_settings_are_whole() {
        let settings = merge(&Written::default()).unwrap();
        assert!(settings.yours.is_empty());
        assert_eq!(said(settings.keep_fine), "1d");
        assert_eq!(said(settings.keep_minutes), "30d");
        assert!(!settings.processes);
        assert_eq!(said(settings.folders_every), "10m");
        assert_eq!(settings.folders_max, 1_000_000);
    }

    #[test]
    fn yours_win() {
        let yours = Written {
            keep_fine: Some("6h".into()),
            ..Written::default()
        };
        let settings = merge(&yours).unwrap();
        assert_eq!(settings.keep_fine, Duration::from_secs(6 * 3600));
        assert_eq!(settings.yours, ["keep_fine"]);
        assert!(
            merge(&Written {
                every: Some("0s".into()),
                ..Written::default()
            })
            .is_err()
        );
    }

    #[test]
    fn durations() {
        assert_eq!(duration("90").unwrap(), Duration::from_secs(90));
        assert_eq!(duration("15m").unwrap(), Duration::from_secs(900));
        assert!(duration("a week").is_err());
        assert!(duration("1w").is_err());
        assert_eq!(said(Duration::from_secs(90)), "90s");
    }

    #[test]
    fn headers() {
        assert_eq!(
            header("x-honeycomb-team: abc").unwrap(),
            ("x-honeycomb-team".into(), "abc".to_string().into())
        );
        assert!(header("no colon").is_err());
        assert!(header("bad name: x").is_err());
        assert!(header("x: a\r\nb").is_err());
    }
}
