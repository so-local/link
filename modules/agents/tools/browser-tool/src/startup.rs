//! A startup failure keeps the browser's diagnostic and an owner action:
//! the line naming its cause, then its final lines.

pub(super) fn failure(err: &anyhow::Error, stderr: &str) -> String {
    let lines: Vec<_> = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let tail = lines.len().saturating_sub(5);
    let mut said: Vec<&str> = Vec::new();
    if let Some(cause) = lines[..tail].iter().find(|line| {
        crate::settings::get()
            .causes
            .iter()
            .any(|cause| line.contains(cause.as_str()))
    }) {
        said.extend([*cause, "…"]);
    }
    said.extend(&lines[tail..]);
    let details = match said.is_empty() {
        true => String::new(),
        false => format!(": {}", said.join(" / ")),
    };
    format!(
        "The browser didn't start ({err:#}){details}. No page was opened. Next: run lnk agent restart to reload its browser tool, then try again. If startup still fails, run lnk agent log show for the tool's failure; on macOS also include the crash report's Termination Reason."
    )
}

#[cfg(test)]
mod tests {
    use super::failure;

    /// A crash as Chromium reports it: the cause, then a stack and
    /// registers that fill the last five lines.
    #[test]
    fn a_crash_keeps_its_cause_before_its_registers() {
        let stderr = "[1:1:FATAL:zygote_host_impl_linux.cc(127)] No usable sandbox!\n\
                      Received signal 6\n#0 0x55 base::debug::StackTrace\n#1 0x56 abort\n\n\
                      di: 0000000000000007  si: 0000000000000007\n\
                      dx: 0000000000000006  ax: 0000000000000000\n\
                      ip: 00007fde3869ec0c efl: 0000000000000246\n\
                      trp: 0000000000000000 msk: 0000000000000000\n[end of stack trace]\n";
        let said = failure(&anyhow::anyhow!("it closed its pipe"), stderr);
        assert!(
            said.contains(
                ": [1:1:FATAL:zygote_host_impl_linux.cc(127)] No usable sandbox! / … / di:"
            ),
            "{said}"
        );
        assert!(said.contains("[end of stack trace]. No page"), "{said}");
    }

    /// A short failure is said whole, its cause already among its last lines.
    #[test]
    fn a_short_failure_is_said_whole() {
        let said = failure(
            &anyhow::anyhow!("it closed its pipe"),
            "ERROR: cannot open display\n",
        );
        assert!(
            said.contains("(it closed its pipe): ERROR: cannot open display. No page"),
            "{said}"
        );
    }
}
