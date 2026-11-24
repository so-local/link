//! Where a call's path is: its parts, the team it's of, and the folder
//! opened on the way to it, never through a link.

use super::*;

/// The parts of `path`, as the model gives it: relative, without `..`.
pub(super) fn parts(path: &str) -> anyhow::Result<Vec<String>> {
    let relative = Path::new(path);
    ensure!(
        !path.is_empty()
            && relative
                .components()
                .all(|c| matches!(c, Component::Normal(_) | Component::CurDir)),
        "{path:?} isn't a path in your files folder: give it relative, as 'work/notes.md'"
    );
    Ok(relative
        .components()
        .filter_map(|c| match c {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect())
}

/// The team `path` is in (`teams/<team>/...`), and the rest of it there.
fn team_of<'a>(files: &'a Files, path: &str) -> anyhow::Result<Option<(&'a Team, Vec<String>)>> {
    let parts = parts(path)?;
    let team = match parts.as_slice() {
        [top, name, ..] if *top == team_paths() => files.teams.iter().find(|t| t.name == *name),
        _ => None,
    };
    Ok(team.map(|team| (team, parts[2..].to_vec())))
}

/// A path the model gave, where it is: `parts` under `root`, a folder
/// of Link's own (the files folder, the agent's own, a team's), shown to
/// the model after `shown` (a team's `teams/<team>/`). Opened a folder
/// at a time from the one before, never through a link, so a link
/// swapped in for a folder after a check is refused, not followed.
pub(super) struct Place {
    pub(super) root: PathBuf,
    pub(super) shown: String,
    pub(super) parts: Vec<String>,
}

impl Place {
    /// Its first `n` parts, as the model sees them.
    pub(super) fn shown(&self, n: usize) -> String {
        format!("{}{}", self.shown, self.parts[..n].join("/"))
    }

    /// The folder its first `n` parts are, each opened from the one
    /// before, a link refused; `make`: each missing one made.
    fn open(&self, n: usize, make: bool) -> anyhow::Result<Dir> {
        let mut dir = Dir::open(&self.root).with_context(|| {
            format!(
                "opening {}",
                match self.shown.is_empty() {
                    true => "your files folder".to_string(),
                    false => self.shown.clone(),
                }
            )
        })?;
        for (i, part) in self.parts[..n].iter().enumerate() {
            let name = OsStr::new(part);
            let next = match make {
                true => dir.create_folder(name, 0o777),
                false => dir.folder(name),
            };
            dir = next.map_err(|e| self.refused(&dir, i, e))?;
        }
        Ok(dir)
    }

    /// The folder it is.
    pub(super) fn folder(&self) -> anyhow::Result<Dir> {
        self.open(self.parts.len(), false)
    }

    /// The folder it's in, opened, and its last part, never a link;
    /// `make`: the folders on the way made.
    pub(super) fn parent(&self, make: bool) -> anyhow::Result<(Dir, &OsStr)> {
        let n = self.parts.len();
        let last = self
            .parts
            .last()
            .with_context(|| format!("{} isn't a file", self.shown(n)))?;
        let dir = self.open(n - 1, make)?;
        let name = OsStr::new(last);
        if let Ok(Kind::Link) = dir.kind(name) {
            bail!("{} is a link, which file doesn't follow", self.shown(n));
        }
        Ok((dir, name))
    }

    /// Why part `n` wasn't opened in `dir`: a link, which file never
    /// follows; missing; or `err`.
    fn refused(&self, dir: &Dir, n: usize, err: std::io::Error) -> anyhow::Error {
        let shown = self.shown(n + 1);
        match dir.kind(OsStr::new(&self.parts[n])) {
            Ok(Kind::Link) => anyhow!("{shown} is a link, which file doesn't follow"),
            Err(_) => anyhow!("no file {}", self.shown(self.parts.len())),
            Ok(_) => anyhow::Error::new(err).context(format!("opening {shown}")),
        }
    }
}

/// Path `path` in the files folder, or a team's, as the model gives it:
/// relative, without `..`, and with no link on the way, which it never
/// follows; in a team's, only what the agent's granted of it. Routed by
/// its parsed parts, so `./instructions/x.md` and `.//instructions/x.md`
/// are both the agent's own `instructions/`.
pub(super) fn inside(files: &Files, path: &str) -> anyhow::Result<Place> {
    if let Some((team, rest)) = team_of(files, path)? {
        ensure!(
            team.reaches(rest.first().map(String::as_str), false),
            "you may reach only {} of team {}'s folder: {path} isn't",
            team.granted(),
            team.name
        );
        return Ok(Place {
            root: team.root.clone(),
            shown: format!("{}/{}/", team_paths(), team.name),
            parts: rest,
        });
    }
    let parts = parts(path)?;
    Ok(Place {
        root: files
            .own_of(&parts)
            .unwrap_or(&files.folder.home)
            .to_path_buf(),
        shown: String::new(),
        parts,
    })
}

/// Path `path`, in a folder the agent may write: a team's as its grants
/// say, else Home's or its own folder's as [`home_writable`] says.
pub(super) fn writable(files: &Files, path: &str) -> anyhow::Result<Place> {
    let parts = parts(path)?;
    refuse_dot_files(&parts, path)?;
    if let Some((team, rest)) = team_of(files, path)? {
        let top = rest.first().map(String::as_str);
        ensure!(
            rest.len() > usize::from(!team.write.iter().any(|n| n == WHOLE))
                && team.reaches(top, true),
            "you may write only in {} of team {}'s folder: {path} isn't",
            team.granted(),
            team.name
        );
        return Ok(Place {
            root: team.root.clone(),
            shown: format!("{}/{}/", team_paths(), team.name),
            parts: rest,
        });
    }
    home_writable(&files.write, &parts, path)?;
    inside(files, path)
}

/// Never a name a person's tools run outside the sandbox (`.git`,
/// `.envrc`, `.vscode`, `.idea`), at any depth, as the sandbox's
/// contract lists them.
fn refuse_dot_files(parts: &[String], path: &str) -> anyhow::Result<()> {
    if let Some(part) = parts
        .iter()
        .find(|part| link_sandbox_contract::dot_files::named(part))
    {
        bail!(
            "file doesn't write {path}: a person's tools run {part} outside your sandbox (git, \
             direnv or an editor), so it's never written in Link's folders"
        );
    }
    Ok(())
}

/// Whether `parts` (of `path`, outside any team's folder) may be
/// written under the grants `write` (folder names, `.` for all of Home):
/// the one check both the local tool and the files bridge make. Never a
/// dot file a person's tools run, never the conversations, a file inside
/// a named folder (not the folder itself), and with `.` any file of Home
/// but the agent's own instructions and skills, which are written only
/// where named.
pub(super) fn home_writable(write: &[String], parts: &[String], path: &str) -> anyhow::Result<()> {
    refuse_dot_files(parts, path)?;
    let top = parts.first().map(String::as_str).unwrap_or_default();
    ensure!(
        top != CONVERSATIONS,
        "you may not write {CONVERSATIONS}/: your conversations are your harness's alone"
    );
    let own = link_agent_contract::folder::seen_collection(top).is_some();
    if write.iter().any(|w| w == WHOLE) && !own && !parts.is_empty() {
        return Ok(());
    }
    ensure!(
        parts.len() > 1 && write.iter().any(|w| w == top),
        "you may write only in {}: {path} isn't",
        writable_said(write)
    );
    Ok(())
}

/// The folders `write` grants, as the model reads them.
pub(super) fn writable_said(write: &[String]) -> String {
    let named: Vec<String> = write
        .iter()
        .filter(|w| *w != WHOLE)
        .map(|w| format!("{w}/"))
        .collect();
    match (write.iter().any(|w| w == WHOLE), named.is_empty()) {
        (true, true) => format!("your files folder but {}", whole_but()),
        (true, false) => format!(
            "your files folder but {}, and {}",
            whole_but(),
            named.join(", ")
        ),
        (false, true) => "none of your files folder".to_string(),
        (false, false) => named.join(", "),
    }
}

/// What a whole files folder's write leaves out, as [`home_writable`]
/// refuses it: the agent's own folders (instructions, skills and
/// conversations), unless named, and dot files a person's tools run.
fn whole_but() -> String {
    let own: Vec<String> = link_agent_contract::folder::SEEN
        .iter()
        .map(|collection| format!("{}/", collection.directory()))
        .collect();
    format!("{} and dot files (as .git/)", own.join(", "))
}

/// Who a call is made for, as its harness says (`_meta`'s
/// `link.local/thread` and `link.local/standing`): the conversation's
/// folder in `chats/`, and whether it's a stranger's.
#[derive(Clone, Debug, Default)]
pub struct Caller {
    /// Its conversation's folder (`chats/<folder>/`): none when the
    /// harness names no conversation.
    pub folder: Option<String>,
    /// Whether the message its turn answers is a stranger's (or its
    /// harness says no standing, which is never taken for more).
    pub stranger: bool,
}

impl Caller {
    /// The caller `meta` (a call's `_meta`) names.
    pub fn of(meta: Option<&Value>) -> Self {
        use link_connection_contract::access::{STANDING_META, Standing, THREAD_META};
        let thread = meta
            .and_then(|m| m.get(THREAD_META))
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty());
        let standing = meta
            .and_then(|m| m.get(STANDING_META))
            .and_then(|s| serde_json::from_value::<Standing>(s.clone()).ok());
        Self {
            folder: thread.map(link_store_contract::store::folder::folder_of),
            stranger: !matches!(standing, Some(Standing::Owner | Standing::Member)),
        }
    }

    /// Whether it may reach `parts` (a call's path or folder): anything
    /// but `chats/`, where only its own conversation's folder, and a
    /// stranger's call none.
    pub fn reaches(&self, parts: &[String]) -> anyhow::Result<()> {
        let [top, rest @ ..] = parts else {
            return Ok(());
        };
        if top != CONVERSATIONS {
            return Ok(());
        }
        ensure!(
            !self.stranger,
            "a stranger's conversation reaches no conversation in {CONVERSATIONS}/"
        );
        let own = self.folder.as_deref();
        ensure!(
            own.is_some() && rest.first().map(String::as_str) == own,
            "this conversation reaches only its own folder in {CONVERSATIONS}/{}",
            own.map(|o| format!(": {CONVERSATIONS}/{o}/"))
                .unwrap_or_default()
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder on the way swapped for a link after it was opened: what
    /// is read and written goes on in the folder opened, never through
    /// the link, and opened again, the link is refused.
    #[test]
    fn a_folder_swapped_for_a_link_is_never_followed() {
        let dir = link_test_kit::TempDir::new("file-swapped");
        let (home, elsewhere) = (dir.join("home"), dir.join("elsewhere"));
        std::fs::create_dir_all(home.join("work/deep")).unwrap();
        std::fs::create_dir_all(elsewhere.join("deep")).unwrap();
        std::fs::write(home.join("work/deep/notes.md"), "mine\n").unwrap();
        std::fs::write(elsewhere.join("deep/notes.md"), "not mine\n").unwrap();
        let files = Files {
            folder: link_agent_contract::folder::Folder {
                home: home.clone(),
                agent: dir.join("agent"),
            },
            write: vec!["work".into()],
            teams: vec![],
        };
        let place = writable(&files, "work/deep/notes.md").unwrap();
        let (opened, name) = place.parent(false).unwrap();
        // Swapped: `work` is now a link to a folder elsewhere.
        std::fs::rename(home.join("work"), home.join("moved")).unwrap();
        std::os::unix::fs::symlink(&elsewhere, home.join("work")).unwrap();
        assert_eq!(
            text_of(&opened, name, "work/deep/notes.md").unwrap(),
            "mine\n"
        );
        put(&opened, name, "written\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(home.join("moved/deep/notes.md")).unwrap(),
            "written\n"
        );
        assert_eq!(
            std::fs::read_to_string(elsewhere.join("deep/notes.md")).unwrap(),
            "not mine\n"
        );
        // Opened again, the link on the way is refused, for each call.
        for (call, arguments) in [
            ("read", json!({"file_path": "work/deep/notes.md"})),
            (
                "write",
                json!({"file_path": "work/deep/notes.md", "content": "x"}),
            ),
            (
                "write",
                json!({"file_path": "work/deep/new/x.md", "content": "x"}),
            ),
            (
                "edit",
                json!({"file_path": "work/deep/notes.md", "old_string": "not", "new_string": "x"}),
            ),
            ("grep", json!({"path": "work/deep", "pattern": "mine"})),
        ] {
            let why = apply_call(&files, call, &arguments)
                .unwrap_err()
                .to_string();
            assert!(
                why.contains("is a link") || why.contains("no such folder"),
                "{call}: {why}"
            );
        }
        // A link inside the folder walked is passed over, never followed.
        std::fs::remove_file(home.join("work")).unwrap();
        std::fs::rename(home.join("moved"), home.join("work")).unwrap();
        std::os::unix::fs::symlink(&elsewhere, home.join("work/out")).unwrap();
        let found = apply_call(&files, "grep", &json!({"pattern": "mine"})).unwrap();
        assert_eq!(
            found["found"],
            json!([]),
            "written over, and nothing found through the link"
        );
        assert_eq!(
            std::fs::read_to_string(elsewhere.join("deep/notes.md")).unwrap(),
            "not mine\n"
        );
    }

    /// A name a person's tools run outside the sandbox is never written,
    /// whatever its case or depth, and nothing is made on the way to it.
    #[test]
    fn what_runs_outside_the_sandbox_is_never_written() {
        let dir = link_test_kit::TempDir::new("file-dot-files");
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("work")).unwrap();
        std::fs::write(home.join("work/.gitignore"), "target\n").unwrap();
        let files = Files {
            folder: link_agent_contract::folder::Folder {
                home: home.clone(),
                agent: dir.join("agent"),
            },
            write: vec![".".into()],
            teams: vec![],
        };
        for path in [
            ".git/hooks/pre-commit",
            ".envrc",
            ".VSCode/tasks.json",
            "work/.idea/workspace.xml",
        ] {
            let why = apply_call(&files, "write", &json!({"file_path": path, "content": "x"}))
                .unwrap_err()
                .to_string();
            assert!(why.contains("outside your sandbox"), "{path}: {why}");
        }
        assert_eq!(
            std::fs::read_dir(&home).unwrap().count(),
            1,
            "nothing made but work/"
        );
        let why = apply_call(
            &files,
            "edit",
            &json!({"file_path": ".git/config", "old_string": "a", "new_string": "b"}),
        )
        .unwrap_err()
        .to_string();
        assert!(why.contains("outside your sandbox"), "{why}");
        // A name that only starts like one is written.
        apply_call(
            &files,
            "write",
            &json!({"file_path": "work/.gitignore", "content": "x"}),
        )
        .unwrap();
    }

    /// The agent's instructions, skills and conversations are in its own
    /// folder, named as they were beside Home's folders: read there,
    /// written only where named, its conversations never; `.` is Home's.
    #[test]
    fn its_own_folder_is_beside_home() {
        let dir = link_test_kit::TempDir::new("file-own");
        let (home, own) = (dir.join("home"), dir.join("agent"));
        for folder in [
            home.join("work"),
            own.join("instructions"),
            own.join("chats/a"),
        ] {
            std::fs::create_dir_all(folder).unwrap();
        }
        std::fs::write(own.join("instructions/instruction.md"), "Be brief.\n").unwrap();
        std::fs::write(own.join("chats/a/conversation.md"), "# a\n").unwrap();
        let files = |write: &[&str]| Files {
            folder: link_agent_contract::folder::Folder {
                home: home.clone(),
                agent: own.clone(),
            },
            write: write.iter().map(|w| w.to_string()).collect(),
            teams: vec![],
        };
        let call =
            |files: &Files, name: &str, arguments: Value| apply_call(files, name, &arguments);
        let mine = files(&["work", "instructions"]);
        let read = call(
            &mine,
            "read",
            json!({"file_path": "instructions/instruction.md"}),
        )
        .unwrap();
        assert!(
            read["text"].as_str().unwrap().contains("Be brief."),
            "{read}"
        );
        call(
            &mine,
            "write",
            json!({"file_path": "instructions/notes.md", "content": "x"}),
        )
        .unwrap();
        assert!(own.join("instructions/notes.md").is_file());
        assert!(!home.join("instructions").exists());
        call(
            &mine,
            "write",
            json!({"file_path": "work/a.md", "content": "a"}),
        )
        .unwrap();
        assert!(home.join("work/a.md").is_file());
        // Not named: not written. Its conversations: never.
        assert!(
            call(
                &mine,
                "write",
                json!({"file_path": "skills/a/SKILL.md", "content": "x"})
            )
            .is_err()
        );
        assert!(
            call(
                &mine,
                "write",
                json!({"file_path": "chats/a/x.md", "content": "x"})
            )
            .is_err()
        );
        let found = call(&mine, "grep", json!({"path": "chats", "pattern": "# a"})).unwrap();
        assert!(
            found.to_string().contains("chats/a/conversation.md"),
            "{found}"
        );
        // `.`: all of Home, none of its own folder unless named.
        let whole = files(&["."]);
        call(
            &whole,
            "write",
            json!({"file_path": "src/main.rs", "content": "fn main() {}"}),
        )
        .unwrap();
        assert!(
            call(
                &whole,
                "write",
                json!({"file_path": "instructions/x.md", "content": "x"})
            )
            .is_err()
        );
        // Routed by its parsed parts: however `./` is written, its own.
        let named = files(&["instructions"]);
        for path in ["./instructions/y.md", ".//instructions/z.md"] {
            call(&named, "write", json!({"file_path": path, "content": "x"})).unwrap();
        }
        assert!(own.join("instructions/y.md").is_file());
        assert!(own.join("instructions/z.md").is_file());
        assert!(
            !home.join("instructions").exists(),
            "nothing shadows it in Home"
        );
        let both = files(&[".", "instructions"]);
        call(
            &both,
            "write",
            json!({"file_path": "instructions/x.md", "content": "x"}),
        )
        .unwrap();
        assert!(own.join("instructions/x.md").is_file());
    }

    /// The one check of what in Home may be written, which the files
    /// bridge makes too: a file at the top with `.`, never without; never
    /// the conversations or a dot file a person's tools run; the agent's
    /// own folders only where named.
    #[test]
    fn one_check_says_what_home_writes() {
        let check = |write: &[&str], path: &str| {
            let write: Vec<String> = write.iter().map(|w| w.to_string()).collect();
            home_writable(&write, &parts(path).unwrap(), path)
        };
        assert!(check(&["."], "README.md").is_ok());
        assert!(check(&["."], "src/main.rs").is_ok());
        assert!(check(&["."], ".git/config").is_err());
        assert!(check(&["."], "chats/a/x.md").is_err());
        assert!(check(&["."], "instructions/x.md").is_err());
        assert!(check(&[".", "instructions"], "instructions/x.md").is_ok());
        assert!(check(&["work"], "README.md").is_err());
        assert!(check(&["work"], "work").is_err());
        assert!(check(&["work"], "work/a.md").is_ok());
        assert!(check(&["work"], "work/.idea/x").is_err());
        assert!(check(&[], "work/a.md").is_err());
    }

    /// The conversation a call is made in reaches only its own folder of
    /// `chats/`; a stranger's, or one its harness doesn't name, none.
    #[test]
    fn a_caller_reaches_only_its_own_conversation() {
        let caller = Caller::of(Some(
            &json!({"link.local/thread": "telegram:42", "link.local/standing": "member"}),
        ));
        let reaches = |caller: &Caller, path: &str| caller.reaches(&parts(path).unwrap());
        assert!(reaches(&caller, "chats/telegram-42/conversation.md").is_ok());
        assert!(reaches(&caller, "work/a.md").is_ok());
        assert!(reaches(&caller, "chats").is_err());
        assert!(reaches(&caller, "chats/telegram-7/conversation.md").is_err());
        let stranger = Caller::of(Some(
            &json!({"link.local/thread": "webhook:x", "link.local/standing": "stranger"}),
        ));
        assert!(reaches(&stranger, "chats/webhook-x/conversation.md").is_err());
        assert!(reaches(&Caller::of(None), "chats/telegram-42/conversation.md").is_err());
    }
}
