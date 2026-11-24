//! `lnk sandbox permission list`: what a spec can say.

use link_actions::value::Words;
use link_format::{Done, Fields, Out, Report, Text, cmd, phrase, prose};
use link_sandbox::cli::{self};

pub(crate) fn permissions(out: &mut Out) -> Result<Done, Report> {
    let mut fields = Fields::new();
    for (section, flags, about) in cli::sections() {
        fields = fields.row(
            phrase!("{}", format!("[{section}]")),
            format!("{flags}: {about}"),
        );
    }
    out.block(fields);
    // The toolchains `[commands] toolchains` takes, each naming its programs.
    let mut toolchains = Fields::new();
    for (toolchain, declared) in link_sandbox_contract::toolchains::all() {
        toolchains = toolchains.row(
            phrase!("{}", toolchain.word()),
            format!(
                "{}: {}",
                toolchain.meaning().unwrap_or_default(),
                declared.programs.join(", ")
            ),
        );
    }
    out.block(toolchains);
    out.block(Text(prose!(
        "Nothing is opened unless a spec's section or its flag asks for it."
    )));
    Ok(Done::shown()
        .next(
            cmd!(
                "lnk sandbox run --commands.toolchains {} --files.write . --internet.hosts {} -- bash",
                "shell",
                "*"
            ),
            "open a folder and the internet",
        )
        .next(
            cmd!(
                "lnk sandbox spec show --commands.toolchains {} --files.write .",
                "shell"
            ),
            "print the spec that makes",
        ))
}

#[cfg(test)]
mod tests {
    use link_actions::value::Words;

    /// Each next step names what it lets run, and every toolchain is
    /// listed with its programs.
    #[test]
    fn its_next_steps_name_what_they_run_and_it_lists_the_toolchains() {
        let (mut out, captured) =
            link_format::Out::capture(false, &["lnk-sandbox", "permission", "list"]);
        let done = super::permissions(&mut out).unwrap();
        assert!(!done.next.0.is_empty());
        for step in &done.next.0.clone() {
            let text = step.command.text();
            assert!(text.contains("--commands.toolchains shell"), "{text}");
            assert!(!text.contains("--commands.allow"), "{text}");
        }
        out.finish(Ok(done));
        let shown = format!("{}{}", captured.stdout(), captured.stderr());
        for toolchain in link_sandbox_contract::toolchains::all().keys() {
            let name = toolchain.word();
            assert!(shown.contains(name), "{name}: {shown}");
        }
    }
}
