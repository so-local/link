//! How apt (Ubuntu's package manager) fetches on a machine Link sets up:
//! its own retries and per-connection timeout, as `apt.toml` says, so a
//! slow mirror that ends a download midway is fetched again inside the
//! one run rather than failing it. One source for both ways apt reads
//! them: on its command line ([`apt_get`]), as Link's own installs give
//! them, and in a file of apt's configuration ([`Apt::file`],
//! [`Apt::file_lines`]), as a box keeps them for every apt run on it.

use std::sync::LazyLock;

use anyhow::{Context, ensure};
use serde::Deserialize;

/// apt's settings, as this package comes with them.
const APT: &str = include_str!("apt.toml");

/// apt's settings ([`APT`]).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Apt {
    /// How many times apt fetches a failed download again.
    retries: u32,
    /// How long a connection may send nothing before apt drops it.
    timeout_seconds: u32,
    /// Where a box keeps them: a file of apt's configuration, which
    /// every apt run on it reads.
    file: String,
}

impl Apt {
    fn read(text: &str) -> anyhow::Result<Self> {
        let apt: Self = toml::from_str(text).context("apt.toml")?;
        ensure!(apt.retries > 0, "apt.toml: retries is at least 1");
        ensure!(
            apt.timeout_seconds > 0,
            "apt.toml: timeout_seconds is at least 1"
        );
        ensure!(
            apt.file.starts_with("/etc/apt/apt.conf.d/"),
            "apt.toml: file is in /etc/apt/apt.conf.d"
        );
        Ok(apt)
    }

    /// Each setting's name and value: its retries, and the timeout for
    /// each of its two transports.
    fn settings(&self) -> [(&'static str, u32); 3] {
        [
            ("Acquire::Retries", self.retries),
            ("Acquire::http::Timeout", self.timeout_seconds),
            ("Acquire::https::Timeout", self.timeout_seconds),
        ]
    }

    /// apt's `-o` options, one pair a setting.
    fn options(&self) -> Vec<String> {
        self.settings()
            .into_iter()
            .flat_map(|(name, value)| ["-o".to_string(), format!("{name}={value}")])
            .collect()
    }

    /// Where a box keeps the settings.
    pub fn file(&self) -> &str {
        &self.file
    }

    /// The settings as that file's lines, in apt's configuration syntax.
    pub fn file_lines(&self) -> Vec<String> {
        self.settings()
            .into_iter()
            .map(|(name, value)| format!("{name} \"{value}\";"))
            .collect()
    }
}

/// [`APT`], read once.
static SETTINGS: LazyLock<anyhow::Result<Apt>> = LazyLock::new(|| Apt::read(APT));

/// apt's settings, as this package comes with them.
pub fn apt() -> anyhow::Result<&'static Apt> {
    SETTINGS.as_ref().map_err(|e| anyhow::anyhow!("{e:#}"))
}

/// `apt-get` with its settings as `-o` options, then `args`: the words
/// of a command run as root (with `sudo` or without).
pub fn apt_get(args: &[&str]) -> anyhow::Result<Vec<String>> {
    Ok(apt_get_with(apt()?, args))
}

fn apt_get_with(apt: &Apt, args: &[&str]) -> Vec<String> {
    let mut words = vec!["apt-get".to_string()];
    words.extend(apt.options());
    words.extend(args.iter().map(|a| a.to_string()));
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST: &str = "retries = 4\ntimeout_seconds = 30\nfile = \"/etc/apt/apt.conf.d/80-lnk\"\n";

    /// A command gives apt its retries and a timeout for each transport,
    /// before its own words.
    #[test]
    fn a_command_gives_apt_its_retries_and_timeouts() {
        let apt = Apt::read(TEST).unwrap();
        assert_eq!(
            apt_get_with(&apt, &["install", "-y", "-q", "bubblewrap"]),
            [
                "apt-get",
                "-o",
                "Acquire::Retries=4",
                "-o",
                "Acquire::http::Timeout=30",
                "-o",
                "Acquire::https::Timeout=30",
                "install",
                "-y",
                "-q",
                "bubblewrap",
            ]
        );
    }

    /// The file holds the same settings, in apt's configuration syntax.
    #[test]
    fn the_file_holds_the_same_settings() {
        let apt = Apt::read(TEST).unwrap();
        assert_eq!(apt.file(), "/etc/apt/apt.conf.d/80-lnk");
        assert_eq!(
            apt.file_lines(),
            [
                "Acquire::Retries \"4\";",
                "Acquire::http::Timeout \"30\";",
                "Acquire::https::Timeout \"30\";",
            ]
        );
    }

    /// The package's own `apt.toml` reads.
    #[test]
    fn the_packages_apt_settings_read() {
        let apt = apt().unwrap();
        assert!(apt_get(&["update"]).unwrap().len() > 2);
        assert!(apt.file_lines()[0].starts_with("Acquire::Retries \""));
    }

    /// No retries, a zero timeout, a file outside apt's configuration or
    /// an unknown key is refused.
    #[test]
    fn a_bad_apt_toml_is_refused() {
        let file = "file = \"/etc/apt/apt.conf.d/80-lnk\"\n";
        for text in [
            format!("retries = 0\ntimeout_seconds = 60\n{file}"),
            format!("retries = 3\ntimeout_seconds = 0\n{file}"),
            "retries = 3\ntimeout_seconds = 60\nfile = \"/etc/lnk/apt\"\n".to_string(),
            format!("retries = 3\ntimeout_seconds = 60\n{file}retry = 1\n"),
        ] {
            assert!(Apt::read(&text).is_err(), "{text}");
        }
    }
}
