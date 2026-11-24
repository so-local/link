//! The machine's calendar, which starts the runner once a minute for the
//! cron driver ([`crate::Cron`]): a line of the user's crontab, or on a
//! Mac a LaunchAgent whose calendar (`StartCalendarInterval`) is every
//! minute. Either is named by a label (the agent's), so installing it
//! again replaces what it started, and `remove` finds it. Its times are
//! never the calendar's: it only wakes the runner, which fires each job
//! by its own time, in the scheduler's zone.
//!
//! A Mac that slept is woken once for the minutes it missed (launchd
//! starts a calendar's job on waking); cron starts nothing for them, and
//! the runner fires the missed jobs at its next minute, as `late` says.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, bail};

/// The variable naming a program to stand in for `crontab` (tests): set,
/// the calendar is the crontab through it, on any machine.
pub const CRONTAB_ENV: &str = "LNK_CRONTAB";

/// Where the runner is started from once a minute.
#[derive(Clone, Debug, PartialEq)]
pub enum Calendar {
    /// The user's crontab, read and written through `program`
    /// (`crontab -l`, `crontab -`).
    Crontab { program: PathBuf },
    /// A LaunchAgent in `dir` (`~/Library/LaunchAgents`), loaded with
    /// `launchctl`; none: launchd is asked nothing (`LNK_SERVICES=off`).
    Launchd {
        dir: PathBuf,
        launchctl: Option<PathBuf>,
    },
}

impl Calendar {
    /// This machine's: launchd on a Mac, cron elsewhere; the crontab
    /// through [`CRONTAB_ENV`]'s program when it's set.
    pub fn of_machine() -> Self {
        if let Some(program) = std::env::var_os(CRONTAB_ENV) {
            return Self::Crontab {
                program: program.into(),
            };
        }
        match cfg!(target_os = "macos") {
            true => Self::Launchd {
                dir: link_files::home_dir().join("Library/LaunchAgents"),
                launchctl: link_service::managed().then(|| "launchctl".into()),
            },
            false => Self::Crontab {
                program: "crontab".into(),
            },
        }
    }

    /// Starts `argv` once a minute as `label`, in place of what `label`
    /// started, its output added to `log`.
    pub fn install(&self, label: &str, argv: &[String], log: &Path) -> anyhow::Result<()> {
        check_label(label)?;
        anyhow::ensure!(!argv.is_empty(), "nothing to start");
        match self {
            Self::Crontab { program } => {
                let mut lines = without(&read_crontab(program)?, label);
                lines.push(crontab_line(label, argv, log));
                write_crontab(program, &lines)
            }
            Self::Launchd { dir, launchctl } => {
                link_files::create_private_dir(dir)
                    .with_context(|| format!("making {}", dir.display()))?;
                let path = plist_path(dir, label);
                std::fs::write(&path, plist(label, argv, log))
                    .with_context(|| format!("writing {}", path.display()))?;
                if let Some(launchctl) = launchctl {
                    let _ = launchctl_run(launchctl, &["bootout", &target(label)]);
                    launchctl_run(
                        launchctl,
                        &[
                            "bootstrap",
                            &link_service::domain(),
                            &path.to_string_lossy(),
                        ],
                    )
                    .context("loading the calendar into launchd")?;
                }
                Ok(())
            }
        }
    }

    /// Stops starting what `label` started; nothing if it wasn't.
    pub fn remove(&self, label: &str) -> anyhow::Result<()> {
        check_label(label)?;
        match self {
            Self::Crontab { program } => {
                let crontab = read_crontab(program)?;
                let kept = without(&crontab, label);
                match kept.len() == crontab.lines().count() {
                    true => Ok(()),
                    false => write_crontab(program, &kept),
                }
            }
            Self::Launchd { dir, launchctl } => {
                if let Some(launchctl) = launchctl {
                    let _ = launchctl_run(launchctl, &["bootout", &target(label)]);
                }
                match std::fs::remove_file(plist_path(dir, label)) {
                    Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.into()),
                    _ => Ok(()),
                }
            }
        }
    }
}

/// A label is the agent's name or ID: lower-case letters, digits and
/// `-`, so it's one word in a crontab and a launchd label.
fn check_label(label: &str) -> anyhow::Result<()> {
    let plain = !label.is_empty()
        && label.len() <= 64
        && label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    anyhow::ensure!(
        plain,
        "{:?} isn't a label: lower-case letters, digits and -",
        link_format::clean(label)
    );
    Ok(())
}

/// What ends the crontab line of `label`.
fn marker(label: &str) -> String {
    format!("# lnk-scheduler-tool {label}")
}

/// The lines of `crontab` but `label`'s.
fn without(crontab: &str, label: &str) -> Vec<String> {
    let marker = marker(label);
    crontab
        .lines()
        .filter(|line| {
            line.trim_end() != marker && !line.trim_end().ends_with(&format!(" {marker}"))
        })
        .map(str::to_string)
        .collect()
}

/// `argv` every minute, its output added to `log`: each word quoted for
/// the shell cron runs it with, and each `%` escaped, which cron would
/// otherwise take for a new line.
fn crontab_line(label: &str, argv: &[String], log: &Path) -> String {
    let quote = |word: &str| format!("'{}'", word.replace('\'', r"'\''")).replace('%', r"\%");
    let words: Vec<String> = argv.iter().map(|word| quote(word)).collect();
    format!(
        "* * * * * {} >> {} 2>&1 {}",
        words.join(" "),
        quote(&log.to_string_lossy()),
        marker(label)
    )
}

/// The user's crontab: empty when they have none.
fn read_crontab(program: &Path) -> anyhow::Result<String> {
    let output = link_secretive::process::command(program)
        .arg("-l")
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("running {} -l", program.display()))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    match output.status.success() {
        true => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
        // "no crontab for <user>"
        false if stderr.contains("no crontab") => Ok(String::new()),
        false => bail!(
            "{} -l failed: {}",
            program.display(),
            link_format::clean(stderr.trim())
        ),
    }
}

fn write_crontab(program: &Path, lines: &[String]) -> anyhow::Result<()> {
    let mut child = link_secretive::process::command(program)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("running {} -", program.display()))?;
    let mut text = lines.join("\n");
    text.push('\n');
    child
        .stdin
        .take()
        .context("no stdin")?
        .write_all(text.as_bytes())?;
    let output = child.wait_with_output()?;
    anyhow::ensure!(
        output.status.success(),
        "{} - failed: {}",
        program.display(),
        link_format::clean(String::from_utf8_lossy(&output.stderr).trim())
    );
    Ok(())
}

/// `label`'s launchd label.
fn launchd_label(label: &str) -> String {
    format!("link.scheduler.{label}")
}

fn target(label: &str) -> String {
    format!("{}/{}", link_service::domain(), launchd_label(label))
}

fn plist_path(dir: &Path, label: &str) -> PathBuf {
    dir.join(format!("{}.plist", launchd_label(label)))
}

fn launchctl_run(launchctl: &Path, args: &[&str]) -> anyhow::Result<()> {
    let output = link_secretive::process::command(launchctl)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("running {}", launchctl.display()))?;
    anyhow::ensure!(
        output.status.success(),
        "launchctl {} failed: {}",
        args.join(" "),
        link_format::clean(String::from_utf8_lossy(&output.stderr).trim())
    );
    Ok(())
}

/// The LaunchAgent starting `argv` every minute: a calendar with no
/// field set is every minute.
fn plist(label: &str, argv: &[String], log: &Path) -> String {
    let string = |s: &str| format!("<string>{}</string>", xml(s));
    let args: String = argv
        .iter()
        .map(|a| format!("\n\t\t{}", string(a)))
        .collect();
    let log = string(&log.to_string_lossy());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<!-- Written by lnk-scheduler-tool; `lnk-scheduler-tool cron uninstall` removes it. -->
<plist version="1.0">
<dict>
	<key>Label</key>
	{label}
	<key>ProgramArguments</key>
	<array>{args}
	</array>
	<key>StartCalendarInterval</key>
	<dict/>
	<key>StandardOutPath</key>
	{log}
	<key>StandardErrorPath</key>
	{log}
	<key>ProcessType</key>
	<string>Background</string>
</dict>
</plist>
"#,
        label = string(&launchd_label(label)),
    )
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn dir(name: &str) -> link_test_kit::TempDir {
        link_test_kit::TempDir::new(&format!("calendar-{name}"))
    }

    /// A `crontab` keeping the user's crontab in `dir`, never the
    /// machine's.
    pub(crate) fn fake_crontab(dir: &Path) -> PathBuf {
        let program = dir.join("crontab");
        link_test_kit::write_program(
            &program,
            format!(
                "#!/bin/sh\ncase \"$1\" in\n\
                 -l) cat '{tab}' 2>/dev/null || {{ echo \"no crontab for $USER\" >&2; exit 1; }} ;;\n\
                 -) cat > '{tab}' ;;\n*) exit 2 ;;\nesac\n",
                tab = dir.join("tab").display()
            ),
        )
        .unwrap();
        program
    }

    /// The command of crontab line `line`, as cron hands it to the shell.
    fn command_of(line: &str) -> String {
        let command = line.splitn(6, ' ').nth(5).unwrap();
        command.replace(r"\%", "%")
    }

    /// Installed, the line runs the runner once a minute with its words
    /// as they were, quotes and `%` too; the user's other lines stay;
    /// installed again it's replaced, and removed it's gone.
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn a_crontab_line_starts_it_every_minute() {
        let dir = dir("crontab");
        let calendar = Calendar::Crontab {
            program: fake_crontab(&dir),
        };
        std::fs::write(dir.join("tab"), "0 6 * * * backup\n").unwrap();
        // A program printing its words, one a line.
        let echo = dir.join("words");
        link_test_kit::write_program(&echo, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n").unwrap();
        let argv: Vec<String> = [echo.to_str().unwrap(), "run", "it's 100%", "a b", "$HOME"]
            .map(String::from)
            .to_vec();
        let log = dir.join("cron log");
        calendar.install("main", &argv, &log).unwrap();
        calendar.install("main", &argv, &log).unwrap();
        calendar.install("work", &argv, &log).unwrap();
        let tab = std::fs::read_to_string(dir.join("tab")).unwrap();
        let lines: Vec<&str> = tab.lines().collect();
        assert_eq!(lines.len(), 3, "{tab}");
        assert_eq!(lines[0], "0 6 * * * backup");
        assert!(lines[1].starts_with("* * * * * "), "{tab}");
        assert!(lines[1].ends_with(" # lnk-scheduler-tool main"), "{tab}");
        let status = Command::new("sh")
            .args(["-c", &command_of(lines[1])])
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "run\nit's 100%\na b\n$HOME\n"
        );
        calendar.remove("main").unwrap();
        let tab = std::fs::read_to_string(dir.join("tab")).unwrap();
        assert_eq!(tab.lines().count(), 2, "{tab}");
        assert!(tab.contains("lnk-scheduler-tool work"), "{tab}");
        // A user with no crontab, and a label that isn't one.
        std::fs::remove_file(dir.join("tab")).unwrap();
        calendar.remove("main").unwrap();
        assert!(calendar.install("Main;rm", &argv, &log).is_err());
        assert!(calendar.remove("").is_err());
    }

    /// On a Mac: a LaunchAgent whose calendar is every minute, loaded in
    /// place of what it was, and unloaded and removed.
    #[test]
    fn a_launch_agent_starts_it_every_minute() {
        let dir = dir("launchd");
        let launchctl = dir.join("launchctl");
        let asked = dir.join("asked");
        link_test_kit::write_program(
            &launchctl,
            format!(
                "#!/bin/sh\necho \"$*\" >> '{}'\n[ \"$1\" = bootout ] && exit 3\nexit 0\n",
                asked.display()
            ),
        )
        .unwrap();
        let agents = dir.join("LaunchAgents");
        let calendar = Calendar::Launchd {
            dir: agents.clone(),
            launchctl: Some(launchctl),
        };
        let argv = vec!["/bin/lnk-scheduler-tool".to_string(), "<run>&".to_string()];
        calendar
            .install("main", &argv, &dir.join("cron.log"))
            .unwrap();
        let path = agents.join("link.scheduler.main.plist");
        let plist = std::fs::read_to_string(&path).unwrap();
        assert!(
            plist.contains("<string>link.scheduler.main</string>"),
            "{plist}"
        );
        assert!(
            plist.contains("<key>StartCalendarInterval</key>\n\t<dict/>"),
            "{plist}"
        );
        assert!(
            plist.contains("<string>&lt;run&gt;&amp;</string>"),
            "{plist}"
        );
        assert!(!plist.contains("KeepAlive") && !plist.contains("RunAtLoad"));
        let domain = link_service::domain();
        let said = std::fs::read_to_string(&asked).unwrap();
        assert_eq!(
            said,
            format!(
                "bootout {domain}/link.scheduler.main\nbootstrap {domain} {}\n",
                path.display()
            )
        );
        calendar.remove("main").unwrap();
        assert!(!path.exists());
        calendar.remove("main").unwrap();
    }
}
