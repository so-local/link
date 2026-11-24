//! A box's first boot: the cloud-config `lnk box create` writes, which an
//! adapter adds to. Its files and commands are list items two spaces in,
//! under `write_files:` and `runcmd:`, as the box module writes its own:
//! a line out of step makes the whole file one cloud-init can't read, and
//! it then sets up none of it. Whoever adds to it or takes from it does so
//! here.

/// The guest's deadline: a unit that powers the machine off, and its timer.
pub const DEADLINE_SERVICE: &str = "/etc/systemd/system/lnk-deadline.service";
pub const DEADLINE_TIMER: &str = "/etc/systemd/system/lnk-deadline.timer";

/// What starts the deadline's timer, last in `runcmd:`.
const DEADLINE_COMMANDS: &str = "  - [systemctl, daemon-reload]\n  \
                                 - [systemctl, enable, --now, lnk-deadline.timer]\n";

/// A file under `write_files:`, root's, `0644`, holding `lines`.
pub fn file(path: &str, lines: &[&str]) -> String {
    let content: String = lines.iter().map(|l| format!("      {l}\n")).collect();
    format!(
        "  - path: {path}\n    owner: root:root\n    permissions: '0644'\n    content: |\n{content}"
    )
}

/// `init` with `files` at the end of its `write_files:`, and `commands`
/// at the end of its `runcmd:`, which is last.
pub fn with(init: &str, files: &str, commands: &str) -> String {
    let mut init = init.replacen("\nruncmd:\n", &format!("\n{files}runcmd:\n"), 1);
    init.push_str(commands);
    init
}

/// The files and commands of a root-owned calendar timer that powers the
/// machine off at Unix time `until`: it survives a reboot and a missed
/// deadline.
pub fn deadline(until: u64) -> (String, String) {
    let files: String = deadline_units(until)
        .iter()
        .map(|(path, lines)| file(path, &lines.iter().map(String::as_str).collect::<Vec<_>>()))
        .collect();
    (files, DEADLINE_COMMANDS.to_string())
}

/// [`deadline`]'s timer as shell lines run on the box as its user, with
/// sudo: what puts it back on a running box whose first boot is over.
pub fn deadline_script(until: u64) -> String {
    let mut script = String::new();
    for (path, lines) in deadline_units(until) {
        let quoted: Vec<String> = lines.iter().map(|l| format!("'{l}'")).collect();
        script.push_str(&format!(
            "printf '%s\\n' {} | sudo -n tee {path} >/dev/null\n",
            quoted.join(" ")
        ));
    }
    script.push_str(&format!(
        "sudo -n chmod 0644 {DEADLINE_SERVICE} {DEADLINE_TIMER}\n\
         sudo -n systemctl daemon-reload\n\
         sudo -n systemctl enable --now lnk-deadline.timer\n"
    ));
    script
}

/// The deadline's two units, each its path and lines, ending at Unix
/// time `until`.
fn deadline_units(until: u64) -> [(&'static str, Vec<String>); 2] {
    let lines = |lines: &[&str]| lines.iter().map(|l| l.to_string()).collect::<Vec<_>>();
    [
        (
            DEADLINE_SERVICE,
            lines(&[
                "[Unit]",
                "Description=End this box at its deadline",
                "[Service]",
                "Type=oneshot",
                "ExecStart=/sbin/poweroff",
            ]),
        ),
        (
            DEADLINE_TIMER,
            lines(&[
                "[Timer]",
                &format!("OnCalendar=@{until}"),
                "Persistent=true",
                "AccuracySec=1s",
                "[Install]",
                "WantedBy=timers.target",
            ]),
        ),
    ]
}

/// `init` without [`deadline`]'s files and commands, whatever its time:
/// for a cloud whose own deadline a guest poweroff would clear.
pub fn without_deadline(init: &str) -> String {
    let init = without_file(&without_file(init, DEADLINE_SERVICE), DEADLINE_TIMER);
    init.replace(DEADLINE_COMMANDS, "")
}

/// `init` without the file at `path`: its item's line, and the lines
/// further in that follow it.
fn without_file(init: &str, path: &str) -> String {
    let item = format!("  - path: {path}");
    let mut out = String::with_capacity(init.len());
    let mut inside = false;
    for line in init.split_inclusive('\n') {
        if line.trim_end() == item {
            inside = true;
        } else if inside && line.starts_with("    ") {
        } else {
            inside = false;
            out.push_str(line);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOOT: &str = "#cloud-config\nwrite_files:\n  - path: /etc/lnk/kind.toml\n    \
                        owner: root:root\n    permissions: '0644'\n    content: |\n      \
                        kind = \"box\"\nruncmd:\n  - [chmod, '0755', /etc/lnk]\n";

    #[test]
    fn a_deadline_is_two_spaces_in_and_comes_out_whole() {
        let (files, commands) = deadline(14500);
        let init = with(BOOT, &files, &commands);
        let items: Vec<_> = init
            .lines()
            .filter(|l| l.trim_start().starts_with("- "))
            .collect();
        assert_eq!(items.len(), 6, "{init}");
        assert!(items.iter().all(|l| l.starts_with("  - ")), "{init}");
        assert!(init.contains("      OnCalendar=@14500\n"));
        assert!(init.find(DEADLINE_TIMER).unwrap() < init.find("runcmd:").unwrap());
        assert_eq!(without_deadline(&init), BOOT);
    }

    #[test]
    fn a_deadline_put_back_is_the_first_boots_timer() {
        let script = deadline_script(14500);
        assert!(
            script.contains("'OnCalendar=@14500'")
                && script.contains(&format!("sudo -n tee {DEADLINE_TIMER} >"))
                && script.contains(&format!("sudo -n tee {DEADLINE_SERVICE} >"))
                && script.ends_with("sudo -n systemctl enable --now lnk-deadline.timer\n"),
            "{script}"
        );
        // The first boot's lines, the same ones.
        let (files, _) = deadline(14500);
        for (_, lines) in deadline_units(14500) {
            for line in lines {
                assert!(files.contains(&format!("      {line}\n")), "{files}");
                assert!(script.contains(&format!("'{line}'")), "{script}");
            }
        }
    }

    #[test]
    fn a_file_is_added_beside_the_first_boots_own() {
        let init = with(BOOT, &file("/etc/lnk/end.json", &["{}"]), "");
        assert!(
            init.contains("      kind = \"box\"\n  - path: /etc/lnk/end.json\n"),
            "{init}"
        );
        assert!(
            init.ends_with("      {}\nruncmd:\n  - [chmod, '0755', /etc/lnk]\n"),
            "{init}"
        );
    }
}
