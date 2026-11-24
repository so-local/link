//! What Link file offers of itself (`serve --no-find --no-diffs`), as
//! the agent's spec says (`[tool.file] find` and `diffs`): `glob` and
//! `grep`, and the diff a `write` or `edit` answers with. What it doesn't offer
//! isn't listed, said or answered.

use serde_json::Value;

use crate::{CONVERSATIONS, WHOLE, team_paths};

/// The calls `actions.toml` declares, as its contract renders them.
const TEMPLATE: &str = link_file_tool_contract::TOOLS;

/// What it offers beside `read`, `write` and `edit`, and where it
/// writes, which its instructions say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    /// `glob` and `grep`: files by name, lines by a pattern.
    pub find: bool,
    /// The change a `write` or `edit` made, as a diff.
    pub diffs: bool,
    /// The folders of Home it writes (`.` for all of it), as `apply`'s
    /// `--write` and `--whole` give them.
    pub write: Vec<String>,
    /// The folders of its teams' it writes and those it only reads, each
    /// `<team>/<folder>` (`<team>/.` for all of it), as `apply` takes
    /// them; and the teams whose folders a files bridge keeps, which
    /// reach as the bridge grants.
    pub team_write: Vec<String>,
    pub team_read: Vec<String>,
    pub team_bridge: Vec<String>,
    /// Every team the files bridge keeps the agent on, at `teams/<team>/`
    /// as each grants: a session wholly through the bridge (`bridge`),
    /// which names none of them.
    pub every_team: bool,
}

impl Default for Offer {
    fn default() -> Self {
        Self {
            find: true,
            diffs: true,
            write: Vec::new(),
            team_write: Vec::new(),
            team_read: Vec::new(),
            team_bridge: Vec::new(),
            every_team: false,
        }
    }
}

impl Offer {
    /// Every one, but `glob` and `grep` where `no_find` and the diffs where
    /// `no_diffs` (`serve --no-find --no-diffs`).
    pub fn leaving(no_find: bool, no_diffs: bool) -> Self {
        Self {
            find: !no_find,
            diffs: !no_diffs,
            ..Self::default()
        }
    }

    /// Whether it offers call `name`.
    pub fn offers(&self, name: &str) -> bool {
        !matches!(name, "glob" | "grep") || self.find
    }

    /// `answer`, a call's, without its diff when it offers none.
    pub fn answer(&self, mut answer: Value) -> Value {
        if !self.diffs
            && let Some(answer) = answer.as_object_mut()
        {
            for key in ["diff", "diff_cut", "sha256"] {
                answer.remove(key);
            }
        }
        answer
    }

    /// What it says of itself at `initialize`: how paths are given, the
    /// folders it writes and its teams', generated from its grants, so a
    /// model never guesses an absolute path or a team it isn't on.
    pub fn instructions(&self) -> String {
        let mut said = format!(
            "Read, write and change the files in your files folder. Paths are relative to it, \
             as 'work/notes.md': never absolute ('/...') and never through '..'. You may write \
             in {}; the rest you may only read. Your instructions/ and skills/ are read there \
             too, and of {CONVERSATIONS}/ only this conversation's own folder, and a stranger's \
             conversation none.",
            crate::writable_said(&self.write)
        );
        let teams = self.teams_said();
        if !teams.is_empty() {
            said.push_str(&format!(" Your teams' shared folders: {teams}."));
        } else if self.every_team {
            said.push_str(&format!(
                " Each team you're on has a shared folder at {}/<team>/, which you reach as \
                 the team grants.",
                team_paths()
            ));
        }
        said.push_str(" Read a file before you edit it");
        said.push_str(match self.diffs {
            true => "; 'write' and 'edit' answer with the change as a diff.",
            false => ".",
        });
        said
    }

    /// Each team's folder, `teams/<team>/`, with what of it is written and
    /// read: none when it's on no team.
    fn teams_said(&self) -> String {
        let mut teams: std::collections::BTreeMap<&str, Vec<String>> = Default::default();
        for (grants, how) in [(&self.team_write, "written"), (&self.team_read, "read")] {
            for grant in grants {
                let Some((team, folder)) = grant.split_once('/') else {
                    continue;
                };
                let what = match folder {
                    WHOLE => format!("all of it {how}"),
                    folder => format!("{folder}/ {how}"),
                };
                teams.entry(team).or_default().push(what);
            }
        }
        for team in &self.team_bridge {
            teams
                .entry(team)
                .or_default()
                .push("as the team grants".into());
        }
        teams
            .iter()
            .map(|(team, what)| format!("{}/{team}/ ({})", team_paths(), what.join(", ")))
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Its calls, as `tools/list` gives them, from `actions.toml`: the
    /// diff sentence where it answers with diffs, `glob` and `grep` where
    /// it finds.
    pub fn offered(&self) -> anyhow::Result<Value> {
        use link_actions::tools::{Fill, Fills};
        let diff = match self.diffs {
            true => " Answers with the change as a diff.",
            false => "",
        };
        let fills = Fills {
            values: [("diff", Fill::Text(diff.into()))].into(),
            conditions: self.find.then_some("find").into_iter().collect(),
        };
        link_actions::tools::offer(TEMPLATE, &fills)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn what_it_doesnt_offer_isnt_listed_said_or_answered() {
        let all = Offer::default();
        let offered = all.offered().unwrap();
        let summary = |name: &str| {
            offered
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["name"] == name)
                .unwrap()["_meta"]["link.local/summary"]
                .clone()
        };
        assert_eq!(offered.as_array().unwrap().len(), 5);
        // Each call's title names the file it works on, or what it finds
        // and where, by the names the model calls them.
        for name in ["read", "write", "edit"] {
            assert_eq!(summary(name), json!(["file_path"]), "{name}");
        }
        assert_eq!(summary("glob"), json!(["pattern", "path"]));
        assert_eq!(summary("grep"), json!(["pattern", "path", "glob"]));
        assert!(all.instructions().contains("as a diff"));
        let none = Offer::leaving(true, true);
        let tools = none.offered().unwrap();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["edit", "read", "write"]);
        assert!(!tools.to_string().contains("diff"), "{tools}");
        assert!(!none.instructions().contains("diff"));
        assert!(!none.offers("glob") && !none.offers("grep") && none.offers("read"));
        let answer = json!({"path": "a.md", "diff": "+a", "diff_cut": true, "sha256": "00"});
        assert_eq!(none.answer(answer.clone()), json!({"path": "a.md"}));
        assert_eq!(all.answer(answer.clone()), answer);
    }

    /// Its instructions name the folders it writes and its teams', from
    /// its grants, and say paths are relative, so a model never guesses
    /// an absolute path or a team it isn't on.
    #[test]
    fn its_instructions_say_where_it_writes() {
        let offer = Offer {
            write: vec!["work".into(), "notes".into()],
            team_write: vec!["crew/drafts".into()],
            team_read: vec!["crew/.".into()],
            team_bridge: vec!["ops".into()],
            ..Offer::default()
        };
        let said = offer.instructions();
        for part in [
            "never absolute",
            "write in work/, notes/",
            "teams/crew/ (drafts/ written, all of it read)",
            "teams/ops/ (as the team grants)",
            "only this conversation's own folder",
        ] {
            assert!(said.contains(part), "{part}: {said}");
        }
        let bridged = Offer {
            every_team: true,
            ..Offer::default()
        };
        assert!(
            bridged
                .instructions()
                .contains("shared folder at teams/<team>/"),
            "{}",
            bridged.instructions()
        );
        let alone = Offer::default().instructions();
        assert!(alone.contains("none of your files folder"), "{alone}");
        assert!(!alone.contains("teams/"), "{alone}");
        let whole = Offer {
            write: vec![".".into()],
            ..Offer::default()
        };
        assert!(
            whole
                .instructions()
                .contains("your files folder but instructions/, skills/, chats/ and dot files")
        );
    }
}
