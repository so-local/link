//! A record's issue: what `lnk issue create` turns into the bug form's fields,
//! made when the record is written, while its values are still typed, so
//! the first scrubbing pass is by type ([`crate::scrub`]). Scoped to the
//! report: its command, the names it holds, its trace and its tail.

use serde::{Deserialize, Serialize};

use crate::scrub::Scrub;
use crate::{Command, Report};

/// The bug form's fields for one failure, scrubbed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Issue {
    /// What happened: the command, its exit, the headline and why.
    pub happened: Vec<String>,
    /// Each step down to what failed, with its place in Link's code and
    /// the program it ran in when that was another.
    pub trace: Vec<Step>,
    /// Another program's last lines.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tail: Vec<String>,
    /// How many lines still looked like a secret, and were left out.
    #[serde(default)]
    pub left_out: usize,
    /// The names the report holds, each with what stands in for it: its
    /// scope. The names themselves stay on this machine.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<Scoped>,
    /// `lnk 0.14.0`.
    pub version: String,
    /// `linux x86_64`.
    pub platform: String,
    /// Each name's diagnosis, its module's `--json`, scrubbed: added by
    /// `lnk issue create`, which asks for them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state: Vec<State>,
}

/// One name's state: what stands in for it and its scrubbed diagnosis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub shown: String,
    pub diagnosis: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub what: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
}

/// A name in scope: its kind, the name, and what stands in for it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scoped {
    pub kind: String,
    pub name: String,
    pub shown: String,
}

/// The bug form, `.github/ISSUE_TEMPLATE/bug_report.yml`, whose fields an
/// issue fills by their ids.
pub const FORM: &str = "https://github.com/so-local/link/issues/new?template=bug_report.yml";

impl Issue {
    /// `report`'s issue, from `command` as typed, scrubbed with `scrub`.
    pub fn of(report: &Report, command: &Command, scrub: &Scrub) -> Self {
        scrub.meet(&report.headline.0);
        if let Some(why) = &report.why {
            scrub.meet(&why.0);
        }
        let mut happened = vec![format!(
            "{} failed (exit {}).",
            scrub.command(command),
            report.exit.code().max(1)
        )];
        happened.push(scrub.spans(&report.headline.0));
        happened.extend(report.why.as_ref().map(|w| scrub.spans(&w.0)));
        let mut left_out = 0;
        let mut text = |t: &str| {
            scrub.line(t).unwrap_or_else(|| {
                left_out += 1;
                "<left out>".into()
            })
        };
        let trace = report
            .trace
            .steps
            .iter()
            .map(|s| Step {
                what: text(&s.what),
                at: s.at.clone(),
                program: s.program.as_deref().map(&mut text),
            })
            .collect();
        let (tail, dropped) = report.tail.as_ref().map_or_else(Default::default, |t| {
            scrub.lines(t.lines.iter().map(|l| l.0.as_str()))
        });
        Issue {
            happened,
            trace,
            tail,
            left_out: left_out + dropped,
            names: scrub
                .names()
                .into_iter()
                .map(|(kind, name, shown)| Scoped { kind, name, shown })
                .collect(),
            version: crate::record::version(),
            platform: crate::record::platform(),
            state: Vec::new(),
        }
    }

    /// `diagnosis` (its module's `--json`) as `name`'s state, scrubbed
    /// with the names this issue numbered.
    pub fn diagnosed(&mut self, name: &Scoped, mut diagnosis: serde_json::Value, scrub: &Scrub) {
        for scoped in &self.names {
            scrub.know(&scoped.kind, &scoped.name);
        }
        self.left_out += scrub.json(&mut diagnosis);
        self.state.push(State {
            shown: name.shown.clone(),
            diagnosis,
        });
    }

    /// The bug form's fields by their ids, in its order: `what`,
    /// `version`, `os`, `more`.
    pub fn fields(&self) -> Vec<(&'static str, String)> {
        let mut what = self.happened.join("\n");
        if !self.trace.is_empty() {
            what += "\n\nTrace:\n```text\n";
            for s in &self.trace {
                what += &s.what;
                if let Some(at) = &s.at {
                    what += &format!("  {at}");
                }
                if let Some(program) = &s.program {
                    what += &format!("  ({program})");
                }
                what.push('\n');
            }
            what += "```";
        }
        let mut more = String::new();
        if !self.tail.is_empty() {
            more = format!("Its last lines:\n```text\n{}\n```", self.tail.join("\n"));
        }
        for state in &self.state {
            if !more.is_empty() {
                more.push_str("\n\n");
            }
            let json = serde_json::to_string_pretty(&state.diagnosis).unwrap_or_default();
            more += &format!("State of {}:\n```json\n{json}\n```", state.shown);
        }
        if self.left_out > 0 {
            if !more.is_empty() {
                more.push_str("\n\n");
            }
            more += &match self.left_out {
                1 => "1 line left out: it looked like a secret.".to_string(),
                n => format!("{n} lines left out: they looked like secrets."),
            };
        }
        vec![
            ("what", what),
            ("version", self.version.clone()),
            ("os", self.platform.clone()),
            ("more", more),
        ]
    }

    /// The fields as Markdown, each under its label, to paste.
    pub fn markdown(&self) -> String {
        let label = |id| match id {
            "what" => "What happened",
            "version" => "lnk version",
            "os" => "OS",
            _ => "Anything else",
        };
        self.fields()
            .into_iter()
            .filter(|(_, text)| !text.is_empty())
            .map(|(id, text)| format!("### {}\n\n{text}\n", label(id)))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The bug form's link with the fields filled in.
    pub fn link(&self) -> String {
        self.link_of(&["what", "version", "os", "more"])
    }

    /// The bug form's link with only the fields `ids` filled in.
    pub fn link_of(&self, ids: &[&str]) -> String {
        let mut link = FORM.to_string();
        for (id, text) in self.fields() {
            if !text.is_empty() && ids.contains(&id) {
                link += &format!("&{id}={}", encoded(&text));
            }
        }
        link
    }
}

/// `text` for a link's query: every byte but the unreserved percent-encoded.
fn encoded(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Name, Path, Tail, Trace, sentence};

    #[test]
    fn an_issue_holds_the_report_scrubbed() {
        let scrub = Scrub::new(Some("/home/ada".into()), Some("ada".into()), None);
        let otter = Name::new("agent", "my-otter");
        let mut report = Report::error(sentence!(
            "Couldn't install the harness of {} in {}.",
            &otter,
            Path::new("/home/ada/Link")
        ))
        .tail(Tail::last(
            "pip: no space left in /home/ada/.cache\nAPI_KEY=abc\n",
            5,
        ));
        report = report.under(Trace {
            steps: vec![crate::Step {
                what: "installing my-otter".into(),
                at: Some("modules/agents/agent/src/harness.rs:97".into()),
                program: Some("lnk agent start".into()),
            }],
        });
        let typed = Command::typed([
            "lnk".into(),
            "agent".into(),
            "start".into(),
            "my-otter".into(),
        ]);
        let issue = Issue::of(&report, &typed, &scrub);
        assert_eq!(
            issue.happened[0],
            "lnk agent start <agent 1> failed (exit 1)."
        );
        assert_eq!(
            issue.happened[1],
            "Couldn't install the harness of <agent 1> in ~/Link."
        );
        assert_eq!(issue.trace.last().unwrap().what, "installing <agent 1>");
        assert_eq!(issue.tail, ["pip: no space left in ~/.cache"]);
        assert_eq!(issue.left_out, 1);
        assert_eq!(issue.names[0].shown, "<agent 1>");
        let shown = issue.markdown();
        assert!(
            shown.starts_with("### What happened\n\nlnk agent start <agent 1>"),
            "{shown}"
        );
        assert!(
            !shown.contains("my-otter") && !shown.contains("/home/ada"),
            "{shown}"
        );
        assert!(
            shown.contains("### Anything else\n\nIts last lines:"),
            "{shown}"
        );
        let link = issue.link();
        assert!(link.starts_with(FORM) && link.contains("&os=") && !link.contains(' '));
    }
}
