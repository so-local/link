//! Shared harness vocabulary, independent of host orchestration.
use link_secretive::Secret;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
/// Where an endpoint answers Link's challenge (`GET`), at its address's
/// host and port: [`CHALLENGE_HEADER`] in, [`PROOF_HEADER`] out.
pub const PROOF_PATH: &str = "/_link/proof";

/// The challenge a connection sends an endpoint: random hex, 32 to 128
/// characters ([`is_challenge`]).
pub const CHALLENGE_HEADER: &str = "Link-Challenge";

/// An endpoint's answer to the challenge: [`endpoint_proof`] of it.
pub const PROOF_HEADER: &str = "Link-Proof";

/// Whether `challenge` is one an endpoint answers: 32 to 128 hex
/// characters.
pub fn is_challenge(challenge: &str) -> bool {
    (32..=128).contains(&challenge.len()) && challenge.bytes().all(|b| b.is_ascii_hexdigit())
}

/// What only the holder of the endpoint's key can answer to `challenge`:
/// HMAC-SHA256 of it with the key, in hex. Never the key itself, and of
/// no use for anything but this challenge.
pub fn endpoint_proof(key: &Secret, challenge: &str) -> String {
    link_secretive::hmac::hex(&link_secretive::hmac::sign(
        key.expose().as_bytes(),
        &proof_message(challenge),
    ))
}

/// Whether `proof` is [`endpoint_proof`] of `challenge` with `key`,
/// compared in constant time.
pub fn proof_matches(key: &Secret, challenge: &str, proof: &str) -> bool {
    let Some(proof) = link_secretive::hmac::unhex(proof) else {
        return false;
    };
    link_secretive::hmac::verify(key.expose().as_bytes(), &proof_message(challenge), &proof)
}

/// What's signed: the challenge, after a label of its own, so a proof is
/// never a signature of anything else made with the key.
fn proof_message(challenge: &str) -> Vec<u8> {
    [b"link endpoint proof\0".as_slice(), challenge.as_bytes()].concat()
}

/// The programs an agent's `run` tool may start (the sandbox file's
/// `[commands]`): names found on the system's PATH, absolute paths, and
/// the sandbox's toolchains, each standing for programs it names. Empty:
/// none, and no `run`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Commands {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub toolchains: Vec<link_sandbox_contract::toolchains::Toolchain>,
}

impl Commands {
    /// Whether `allow` is `["*"]`: any program on the PATH.
    pub fn any(&self) -> bool {
        use link_sandbox_contract::spec::Item;
        matches!(self.allow.as_slice(), [only] if only.any())
    }

    /// An error for an entry that isn't a program's name or an absolute
    /// path without `..`, and for `*` beside other entries (it is the
    /// whole list). A toolchain the sandbox doesn't declare is refused as
    /// they're read.
    pub fn check(&self) -> anyhow::Result<()> {
        use link_sandbox_contract::spec::Item;
        if self.any() {
            return Ok(());
        }
        anyhow::ensure!(
            !self.allow.iter().any(Item::any),
            "[commands] allow's \"*\" is any program, so it stands alone: drop the others"
        );
        for entry in &self.allow {
            let name_ok = (1..=64).contains(&entry.len())
                && entry
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.+".contains(&b))
                && !entry.starts_with('.');
            let path_ok = entry.len() <= 1024
                && entry.starts_with('/')
                && !entry.ends_with('/')
                && !entry.split('/').any(|part| part == "..")
                && !entry.chars().any(char::is_control);
            anyhow::ensure!(
                name_ok || path_ok,
                "[commands] {:?} isn't a program: a name on PATH (\"python3\") or an absolute \
                 path (\"/opt/tools/bin/tool\")",
                link_format::clean(entry)
            );
        }
        Ok(())
    }

    /// Of them, those this machine has, looked up on `path` (a PATH's
    /// folders): each entry as written, then each of its toolchains'
    /// programs by name, each once; `["*"]` alone when it allows any.
    pub fn present(&self, path: &str) -> Vec<String> {
        if self.any() {
            return self.allow.clone();
        }
        let toolchains = self
            .toolchains
            .iter()
            .flat_map(|t| link_sandbox_contract::toolchains::named(*t).programs.iter());
        let mut present: Vec<String> = Vec::new();
        for entry in self.allow.iter().chain(toolchains) {
            if !present.contains(entry) && find_program(entry, path).is_some() {
                present.push(entry.clone());
            }
        }
        present
    }
}

/// Program `entry` (a name or an absolute path) as the file it is: a
/// name looked up in `path`'s folders. None when no such program is
/// here to run.
pub fn find_program(entry: &str, path: &str) -> Option<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let runnable = |file: &std::path::Path| {
        std::fs::metadata(file).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    if entry.starts_with('/') {
        let file = std::path::PathBuf::from(entry);
        return runnable(&file).then_some(file);
    }
    path.split(':')
        .filter(|dir| dir.starts_with('/'))
        .map(|dir| std::path::Path::new(dir).join(entry))
        .find(|file| runnable(file))
}

/// The folders of an agent's folder ([`crate::folder`]) by name,
/// lowercase: what it's told to be, its skills and its conversations in
/// its own folder, as it sees them beside Home's own
/// ([`crate::folder::SEEN`]), and the rest of its work in Home. Side by
/// side, never nested: a sandbox has no read-only folder inside a
/// writable one.
pub mod layout {
    /// `instructions/`: its standing instructions, its notes, a model's.
    pub const INSTRUCTIONS: &str = "instructions";
    /// Its standing instructions, in [`INSTRUCTIONS`].
    pub const STANDING: &str = "instructions/instruction.md";
    /// Its notes, in [`INSTRUCTIONS`]: what it noted for itself, written
    /// by its memory's `remember`.
    pub const NOTES: &str = "memory/notes.md";
    /// `skills/`: a folder per skill.
    pub const SKILLS: &str = "skills";
    /// `chats/`: a folder per conversation, written by its
    /// harness only.
    pub const CONVERSATIONS: &str = "chats";
    /// `harnesses/`: each harness's own state as a switch away from it
    /// left it, in ASP's portable forms, written by `lnk agent harness set` only.
    pub const HARNESSES: &str = "harnesses";
    /// Home's `work/`: anything else it makes.
    pub const WORK: &str = "work";
    /// Home's `memory/`: what its memory indexes, beside its
    /// conversations: the documents you give it to search (a text file,
    /// a PDF, a Word document). The rest of Home is read, never indexed.
    pub const MEMORY: &str = "memory";
    /// Home's own folders, made at each start beside what's there.
    pub const IN_HOME: [&str; 2] = [WORK, MEMORY];
}

/// The folders an agent may write of what it sees of its folder
/// ([`crate::folder::Folder::seen`]), and those it may only read (the
/// sandbox file's `[files]`), each by name: one of Home's (`work`), or
/// its `instructions`, `skills` or `chats`, kept in its own
/// folder; or `.` alone for the whole of Home (a repository it works
/// in). The rest it reads; its harness writes `chats/` whatever
/// this says.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Folders {
    #[serde(default)]
    pub write: Vec<String>,
    #[serde(default)]
    pub read: Vec<String>,
    /// Of each of its teams' shared folders, by the team's name, what it
    /// may write and what it may only read (`[teams.<team>]`). A
    /// team not here, it can't reach at all.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub teams: BTreeMap<String, TeamFolders>,
}

pub use link_sandbox_contract::spec::{TeamFolders, WHOLE, team_paths};

/// A team's shared folder as IAM names it, the bucket `team-<id>`:
/// `bucket:team-<id>/` for what's at its top, `bucket:team-<id>/<folder>/`
/// for a folder of it, so a grant reaches either by a pattern
/// (`bucket:team-<id>/drafts/*`, `bucket:team-<id>/*`).
pub fn team_resource(id: &str, folder: Option<&str>) -> String {
    match folder {
        Some(folder) => format!("bucket:team-{id}/{folder}/"),
        None => format!("bucket:team-{id}/"),
    }
}

impl Default for Folders {
    /// Link's: its work, its instructions and its skills written, so
    /// asked to change how it works it changes them; its conversations
    /// read only.
    fn default() -> Self {
        Self {
            write: vec![
                layout::WORK.into(),
                layout::INSTRUCTIONS.into(),
                layout::SKILLS.into(),
            ],
            read: vec![layout::CONVERSATIONS.into()],
            teams: BTreeMap::new(),
        }
    }
}

impl Folders {
    /// An error for a name that isn't a folder's (lowercase, one level,
    /// as the layout's are), one named twice or in both lists,
    /// `chats` among those written, or `.` beside a folder of
    /// Home's, which it holds.
    pub fn check(&self) -> anyhow::Result<()> {
        if self.whole() {
            anyhow::ensure!(
                self.write
                    .iter()
                    .chain(&self.read)
                    .all(|n| n == WHOLE || seen_collection(n)),
                "[files] writes \".\", the whole of Home: beside it, name only instructions, \
                 skills or chats, which are kept in the agent's own folder"
            );
        }
        let mut seen = std::collections::BTreeSet::new();
        for name in self.write.iter().chain(&self.read) {
            anyhow::ensure!(
                TeamFolders::is_folder_name(name) || (name == WHOLE && self.whole()),
                "[files] {:?} isn't a folder's name: a folder in the agent's Home, lowercase, \
                 one level (\"work\", \"instructions\"), never a path like \"~/work\"",
                link_format::clean(name)
            );
            anyhow::ensure!(
                seen.insert(name.as_str()),
                "[files] names {name} twice: a folder is written or read, not both"
            );
        }
        anyhow::ensure!(
            !self.write.iter().any(|n| n == layout::CONVERSATIONS),
            "[files] write can't hold chats: its harness alone writes them, so no \
             tool rewrites what was said"
        );
        for (team, folders) in &self.teams {
            anyhow::Context::with_context(crate::machine::check_name(team), || {
                format!("[teams.{team}]")
            })?;
            folders.check(&format!("[teams.{team}]"))?;
        }
        Ok(())
    }

    /// Whether the agent may write folder `name`: one named, or under
    /// `.` any of Home's.
    pub fn writes(&self, name: &str) -> bool {
        self.write.iter().any(|n| n == name) || (self.whole() && !seen_collection(name))
    }

    /// Whether memory may write the agent's notes
    /// (`memory/notes.md`): where it may write its instructions, or
    /// where they aren't kept from it, as by default.
    pub fn writes_notes(&self) -> bool {
        self.writes(layout::INSTRUCTIONS) || !self.read.iter().any(|n| n == layout::INSTRUCTIONS)
    }

    /// Whether it writes the whole of Home (`.`).
    pub fn whole(&self) -> bool {
        self.write.iter().any(|n| n == WHOLE)
    }
}

/// What `link` exits with when the connection needs linking and there's no
/// terminal to link it on: the connections' own code in `link-format`'s
/// list, which no other failure exits with.
pub const NOT_LINKED: i32 = 10;

/// What `lnk agent move` carries of a harness's box to another machine:
/// its state, not its install, which is made fresh there (a Mac's
/// programs don't run on Linux). Paths start with `~/`, its home.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// Folders or files that hold the agent.
    pub include: Vec<String>,
    /// Paths under those that don't: the harness's programs, its own
    /// runtimes, logs, caches, and the keys `configure` writes again.
    #[serde(default)]
    pub exclude: Vec<String>,
}

/// The port of an adapter's `endpoint`, or why it isn't one: `http` on
/// this machine's loopback (`127.0.0.1` or `localhost`), with a port.
pub fn endpoint_port(url: &str) -> anyhow::Result<u16> {
    let rest = url
        .strip_prefix("http://127.0.0.1:")
        .or_else(|| url.strip_prefix("http://localhost:"))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{url:?} isn't an address on this machine's loopback (http://127.0.0.1:<port>/...)"
            )
        })?;
    let (port, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let port: u16 = port
        .parse()
        .map_err(|_| anyhow::anyhow!("{url:?} has no port"))?;
    anyhow::ensure!(
        port >= 1024 && path.bytes().all(|b| b.is_ascii_graphic()),
        "{url:?} isn't a plain address with an unprivileged port"
    );
    Ok(port)
}

fn seen_collection(path: &str) -> bool {
    let path = path.trim_start_matches("./");
    [layout::INSTRUCTIONS, layout::SKILLS, layout::CONVERSATIONS]
        .contains(&path.split('/').next().unwrap_or(""))
}
