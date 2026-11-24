//! How a turn's tool calls show above its answer: a line each, its mark,
//! the call, then its separator and what it works on.

/// The most of a turn's calls shown at once, its latest.
const CALLS_SHOWN: usize = 8;

/// What an answer being written shows: the tools it called so far, a
/// line each (`› file read: notes/plan.md`), its latest [`CALLS_SHOWN`],
/// then its answer so far. The whole answer, once done, shows the answer
/// alone, or below every call where they're kept ([`kept`]).
pub(super) fn activity(
    called: &[crate::connection::acp::client::Call],
    answer: &str,
    look: &crate::connection::acp::client::Look,
) -> String {
    above(
        &called[called.len().saturating_sub(CALLS_SHOWN)..],
        answer,
        look,
    )
}

/// A finished answer whose calls are kept: every one, a line each, above it.
pub(super) fn kept(
    called: &[crate::connection::acp::client::Call],
    answer: &str,
    look: &crate::connection::acp::client::Look,
) -> String {
    above(called, answer, look)
}

/// `called`, a line each, above `answer`.
fn above(
    called: &[crate::connection::acp::client::Call],
    answer: &str,
    look: &crate::connection::acp::client::Look,
) -> String {
    if called.is_empty() {
        return answer.to_string();
    }
    let lines: Vec<String> = called.iter().map(|call| line(call, look)).collect();
    match answer.is_empty() {
        true => lines.join("\n"),
        false => format!("{}\n\n{answer}", lines.join("\n")),
    }
}

/// A call's line: its mark, the call, then the separator and what it
/// works on where it said, on one line, at most
/// [`crate::constants::CALL_LINE_MAX`] characters after its mark, cut
/// with `…`.
fn line(
    call: &crate::connection::acp::client::Call,
    look: &crate::connection::acp::client::Look,
) -> String {
    let flat = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    let said = match &call.detail {
        Some(detail) => format!("{}{}{}", flat(&call.call), look.separator, flat(detail)),
        None => flat(&call.call),
    };
    let said = link_format::clean(&said);
    let most = crate::constants::CALL_LINE_MAX;
    let said = match said.chars().count() > most {
        true => format!("{}…", said.chars().take(most - 1).collect::<String>()),
        false => said,
    };
    match look.mark.is_empty() {
        true => said,
        false => format!("{} {said}", look.mark),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::acp::client::{Call, Look};

    /// Link's look: `›` leads a line, `: ` stands before its detail.
    fn links() -> Look {
        Look {
            mark: "›".into(),
            separator: ": ".into(),
        }
    }

    fn call(call: &str, detail: Option<&str>) -> Call {
        Call {
            call: call.into(),
            detail: detail.map(str::to_string),
        }
    }

    /// An answer being written shows its latest calls above it, a line
    /// each with what the call works on; a finished answer that keeps
    /// them shows every one.
    #[test]
    fn calls_show_above_the_answer() {
        let paths: Vec<String> = (0..10).map(|n| format!("notes/{n}.md")).collect();
        let called: Vec<Call> = paths.iter().map(|p| call("file read", Some(p))).collect();
        let shown = activity(&called, "Done.", &links());
        assert!(shown.starts_with("› file read: notes/2.md\n"), "{shown}");
        assert!(
            shown.ends_with("› file read: notes/9.md\n\nDone."),
            "{shown}"
        );
        assert_eq!(shown.matches("› ").count(), CALLS_SHOWN);
        let kept = kept(&called, "Done.", &links());
        assert!(kept.starts_with("› file read: notes/0.md\n"), "{kept}");
        assert_eq!(kept.matches("› ").count(), called.len());
        assert_eq!(activity(&[], "Done.", &links()), "Done.");
        assert_eq!(
            activity(&called[..1], "", &links()),
            "› file read: notes/0.md"
        );
    }

    /// Link's line: the call, then its detail after `: `; a call with no
    /// detail shows no separator. Another mark and separator are shown as
    /// a spec says, and none leads it where the spec says nothing.
    #[test]
    fn a_calls_line_looks_as_its_spec_says() {
        let commit = call("run command", Some("git commit -m 'a fix'"));
        assert_eq!(
            line(&commit, &links()),
            "› run command: git commit -m 'a fix'"
        );
        assert_eq!(line(&call("web search", None), &links()), "› web search");
        let own = Look {
            mark: "🔧".into(),
            separator: " → ".into(),
        };
        assert_eq!(
            line(&commit, &own),
            "🔧 run command → git commit -m 'a fix'"
        );
        assert_eq!(
            line(&commit, &Look::of(None)),
            "run command git commit -m 'a fix'"
        );
    }

    /// A call's line is one line, cut to its most, and a name with
    /// underscores in it, as `__init__.py`, is shown as it is.
    #[test]
    fn a_calls_line_is_one_line_cut_to_its_most() {
        assert_eq!(
            line(&call("run command", Some("cargo\n  test")), &links()),
            "› run command: cargo test"
        );
        assert_eq!(
            line(&call("file read", Some("__init__.py")), &links()),
            "› file read: __init__.py"
        );
        let long = line(
            &call("a", Some(&"a".repeat(crate::constants::CALL_LINE_MAX * 2))),
            &links(),
        );
        assert_eq!(
            long.chars().count(),
            "› ".chars().count() + crate::constants::CALL_LINE_MAX
        );
        assert!(long.ends_with('…'));
    }

    /// A call is split as Link Harness titles it: its name's words, then
    /// what it works on; any other title is the call itself.
    #[test]
    fn a_call_is_its_names_words_then_its_detail() {
        assert_eq!(
            Call::of(Some("run__command"), "run command git status"),
            call("run command", Some("git status"))
        );
        assert_eq!(
            Call::of(Some("web__search"), "web search"),
            call("web search", None)
        );
        assert_eq!(
            Call::of(Some("file__read"), "Read src/main.rs"),
            call("Read src/main.rs", None)
        );
        assert_eq!(
            Call::of(Some("run__command"), "run commander"),
            call("run commander", None)
        );
        assert_eq!(Call::of(None, "Edit a.rs"), call("Edit a.rs", None));
    }
}
