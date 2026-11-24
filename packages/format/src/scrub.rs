//! What leaves the machine in `lnk issue create`, scrubbed in two passes. The
//! first is by type and certain: a [`Name`] a person chose is its kind and
//! a number, the same all through the issue; a [`Path`] under home is
//! `~/...`, anywhere else `<path>`; an [`Address`] is `<address>`; a
//! secret was never there. The second is by pattern, over text that came
//! from elsewhere: the home folder, the username, the hostname, IP and
//! email addresses and anything shaped like a token, each its kind in
//! angle brackets, and a line that still looks like a secret left out
//! whole. It errs to removing: a line too few costs a question, a secret
//! too many can't be taken back.
//!
//! [`Name`]: crate::Name
//! [`Path`]: crate::Path
//! [`Address`]: crate::Address

use std::cell::RefCell;
use std::path::PathBuf;

use crate::{Command, Span, Value, Word};

/// What the scrubber takes for a secret, from `scrub.toml`.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Shapes {
    /// The words that make a line look like a secret, lowercase.
    secret_like: Vec<String>,
    /// The openings of credentials whose shape is known.
    token_openings: Vec<String>,
    /// Words that name a code that opens something, lowercase.
    code_words: Vec<String>,
    /// The fewest digits in a row that a code is taken to be.
    code_digits: usize,
    /// How many words from a code word its digits may be.
    code_reach: usize,
}

/// The shapes, read once.
fn shapes() -> &'static Shapes {
    static SHAPES: std::sync::LazyLock<Shapes> = std::sync::LazyLock::new(|| {
        toml::from_str(include_str!("scrub.toml")).expect("scrub.toml is the scrubber's shapes")
    });
    &SHAPES
}

/// Whether `lower`, a line in lowercase, holds a word a secret is said
/// with.
fn secret_like(lower: &str) -> bool {
    shapes()
        .secret_like
        .iter()
        .any(|w| lower.contains(w.as_str()))
}

/// Why `text` looks like it holds a secret, for what's kept where a model
/// reads it again (notes kept for later): a word a secret is said with
/// (`password`, `pw:`), a word shaped like a credential, an identity
/// number, or a code that opens something (`the locker code is 4821`),
/// its digits within a few words of the word for it. None when it looks
/// like none: a path, a UUID or a commit's hash is not a credential.
pub fn secret_shaped(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    if secret_like(&lower) {
        return Some("it names a password, a token or a key");
    }
    let apart = |c: char| c.is_whitespace() || "\"'`()[]{}<>,;=|".contains(c);
    let words = || {
        text.split(apart)
            .map(|word| word.trim_end_matches(['.', ':', '!', '?']))
    };
    if words().any(|word| token(word) && !ordinary(word)) {
        return Some("it holds something shaped like a credential");
    }
    if words().any(identity_number) {
        return Some("it holds something shaped like an identity number");
    }
    let shapes = shapes();
    let said: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|w| !w.is_empty())
        .collect();
    let digits = |word: &str| {
        let mut run = 0;
        word.chars().any(|c| {
            run = if c.is_ascii_digit() { run + 1 } else { 0 };
            run >= shapes.code_digits
        })
    };
    let reach = shapes.code_reach;
    let coded = shapes.code_words.iter().any(|code| {
        let code: Vec<&str> = code.split(' ').collect();
        (0..said.len()).any(|at| {
            said[at..].starts_with(&code) && {
                let before = at.saturating_sub(reach)..at;
                let after = at + code.len()..(at + code.len() + reach).min(said.len());
                before.chain(after).any(|i| digits(said[i]))
            }
        })
    });
    coded.then_some("it holds a code that opens something")
}

/// A word shaped like a credential that's ordinary all the same: a path
/// from a folder (`/`, `~/`, `./`, `../`) none of whose names is shaped
/// like one, a UUID, or a commit's or a file's hash (40 or 64 hex digits).
fn ordinary(word: &str) -> bool {
    let path = ["/", "~/", "./", "../"]
        .iter()
        .any(|from| word.starts_with(from))
        && word.split('/').all(|name| !token(name));
    let hex = |part: &str| part.chars().all(|c| c.is_ascii_hexdigit());
    let parts: Vec<&str> = word.split('-').collect();
    let uuid = parts.iter().map(|p| p.len()).eq([8, 4, 4, 4, 12]) && parts.iter().all(|p| hex(p));
    let hash = matches!(word.len(), 40 | 64) && hex(word);
    path || uuid || hash
}

/// A United States Social Security number's shape: `123-45-6789`.
fn identity_number(word: &str) -> bool {
    let parts: Vec<&str> = word.split('-').collect();
    parts.iter().map(|p| p.len()).eq([3, 2, 4])
        && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit()))
}

/// One issue's scrubbing: what's private on this machine, and each name
/// met so far with the number it stands in as.
pub struct Scrub {
    home: Option<String>,
    user: Option<String>,
    host: Option<String>,
    names: RefCell<Vec<(String, String)>>,
}

impl Scrub {
    /// For this machine: its home folder, username and hostname.
    pub fn here() -> Self {
        let host = std::fs::read_to_string("/etc/hostname")
            .ok()
            .or_else(|| std::env::var("HOSTNAME").ok());
        Scrub::new(
            std::env::var_os("HOME").map(PathBuf::from),
            std::env::var("USER")
                .or_else(|_| std::env::var("LOGNAME"))
                .ok(),
            host,
        )
    }

    pub fn new(home: Option<PathBuf>, user: Option<String>, host: Option<String>) -> Self {
        let word = |w: Option<String>| w.map(|w| w.trim().to_string()).filter(|w| w.len() >= 2);
        Scrub {
            home: home
                .map(|h| h.display().to_string())
                .filter(|h| h.len() > 1)
                .map(|h| h.trim_end_matches('/').to_string()),
            user: word(user),
            host: word(host),
            names: RefCell::default(),
        }
    }

    /// Every name met, by kind and name, with what stands in for it.
    pub fn names(&self) -> Vec<(String, String, String)> {
        let names = self.names.borrow().clone();
        names
            .into_iter()
            .map(|(kind, name)| {
                let shown = self.name(&kind, &name);
                (kind, name, shown)
            })
            .collect()
    }

    /// `<agent 1>`: the first agent met, the same each time.
    fn name(&self, kind: &str, name: &str) -> String {
        let mut names = self.names.borrow_mut();
        if !names.iter().any(|(k, n)| k == kind && n == name) {
            names.push((kind.to_string(), name.to_string()));
        }
        let n = names
            .iter()
            .filter(|(k, _)| k == kind)
            .position(|(_, n)| n == name)
            .map_or(1, |i| i + 1);
        format!("<{kind} {n}>")
    }

    /// The names in `spans`, met in order, so text read later knows them.
    pub(crate) fn meet(&self, spans: &[Span]) {
        for span in spans {
            if let Span::Value(v) = span {
                self.meet_value(v);
            }
        }
    }

    fn meet_value(&self, v: &Value) {
        match v {
            Value::Name(n) => {
                self.name(n.kind, &n.name);
            }
            Value::List(l) => l.iter().for_each(|v| self.meet_value(v)),
            _ => {}
        }
    }

    /// `spans` as plain text, each value by its type and the words by
    /// pattern: a sentence adopted from an error is all words, and may
    /// hold a path, an address or a secret, so words that look like one
    /// are left out as a line of a trace is.
    pub(crate) fn spans(&self, spans: &[Span]) -> String {
        spans
            .iter()
            .map(|s| match s {
                Span::Text(t) => self.line(t).unwrap_or_else(|| "<left out>".into()),
                Span::Value(v) => self.value(v),
            })
            .collect()
    }

    fn value(&self, v: &Value) -> String {
        match v {
            Value::Name(n) => self.name(n.kind, &n.name),
            Value::Path(p) => self.path(&p.0.display().to_string()),
            Value::Address(a) if a.0.starts_with("localhost:") => a.0.clone(),
            Value::Address(_) => "<address>".into(),
            Value::Secret => "<secret>".into(),
            Value::Command(c) => self.command(c),
            Value::Foreign(f) => self.line(&f.0).unwrap_or_else(|| "<left out>".into()),
            Value::Text(t) => self.line(t).unwrap_or_else(|| "<left out>".into()),
            Value::List(l) => l
                .iter()
                .map(|v| self.value(v))
                .collect::<Vec<_>>()
                .join(", "),
            v => crate::render::value(v, &crate::render::Ctx::plain()),
        }
    }

    /// A command as it shows, its fixed words kept and each value read as
    /// text from elsewhere.
    pub fn command(&self, c: &Command) -> String {
        c.words()
            .iter()
            .map(|w| match w {
                Word::Fixed(f) => f.clone(),
                Word::Placeholder(p) => format!("<{p}>"),
                Word::Value(v) => self.line(v).unwrap_or_else(|| "<left out>".into()),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// `~/...` under home, `<path>` anywhere else.
    fn path(&self, path: &str) -> String {
        match &self.home {
            Some(home) if path == home => "~".into(),
            Some(home) => match path.strip_prefix(&format!("{home}/")) {
                Some(rest) => format!("~/{}", self.line(rest).unwrap_or_else(|| "<path>".into())),
                None => "<path>".into(),
            },
            None => "<path>".into(),
        }
    }

    /// A line of text from elsewhere, by pattern: none when it still looks
    /// like a secret, to be left out whole.
    pub fn line(&self, line: &str) -> Option<String> {
        if secret_like(&line.to_lowercase()) {
            return None;
        }
        Some(self.patterns(line))
    }

    /// `line` with its home, names, patterns, username and hostname each
    /// stood in for.
    fn patterns(&self, line: &str) -> String {
        let mut line = line.to_string();
        if let Some(home) = &self.home {
            line = line.replace(&format!("{home}/"), "~/");
            line = replace_word(&line, home, "~");
        }
        let names = self.names.borrow().clone();
        for (kind, name) in names {
            line = replace_word(&line, &name, &self.name(&kind, &name));
        }
        line = words(&line, pattern);
        if let Some(user) = &self.user {
            line = replace_word(&line, user, "<user>");
        }
        if let Some(host) = &self.host {
            line = replace_word(&line, host, "<host>");
        }
        line
    }

    /// Takes `kind`'s `name` as met, in turn: the names a record's issue
    /// numbered, met again in their order, keep their numbers.
    pub fn know(&self, kind: &str, name: &str) {
        self.name(kind, name);
    }

    /// A diagnosis's `--json` (typed blocks), scrubbed: each value by its
    /// type as its JSON says it (`{"path": ...}`, `{"address": ...}`, a
    /// name met as `{"agent": ...}`), every other string by pattern, and
    /// how many strings were left out.
    pub fn json(&self, value: &mut serde_json::Value) -> usize {
        use serde_json::Value as Json;
        match value {
            Json::String(text) => match self.line(text) {
                Some(line) => {
                    *text = line;
                    0
                }
                None => {
                    *text = "<left out>".into();
                    1
                }
            },
            Json::Array(items) => items.iter_mut().map(|v| self.json(v)).sum(),
            Json::Object(map) if map.len() == 1 => {
                let (key, inner) = map.iter_mut().next().expect("one entry");
                let known = self.names.borrow().iter().any(|(k, _)| k == key);
                match (key.as_str(), inner) {
                    ("path", Json::String(p)) => {
                        *p = self.path(p);
                        0
                    }
                    ("address", Json::String(a)) if !a.starts_with("localhost:") => {
                        *a = "<address>".into();
                        0
                    }
                    (_, Json::String(name)) if known => {
                        *name = self.name(key, name);
                        0
                    }
                    (_, inner) => self.json(inner),
                }
            }
            Json::Object(map) => map.values_mut().map(|v| self.json(v)).sum(),
            _ => 0,
        }
    }

    /// Lines from elsewhere (a tail, a log), each scrubbed, and how many
    /// were left out.
    pub fn lines<'a>(&self, lines: impl IntoIterator<Item = &'a str>) -> (Vec<String>, usize) {
        let mut kept = Vec::new();
        let mut left_out = 0;
        for line in lines {
            match self.line(line) {
                Some(l) => kept.push(l),
                None => left_out += 1,
            }
        }
        (kept, left_out)
    }
}

/// What a word stands in as, when it's one of the patterns: `<ip>`,
/// `<email>`, `<token>`.
fn pattern(word: &str) -> Option<&'static str> {
    let bare = word.trim_end_matches(['.', ':']);
    if ipv4(bare) || ipv6(bare) {
        return Some("<ip>");
    }
    if let Some((user, domain)) = bare.split_once('@')
        && !user.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
    {
        return Some("<email>");
    }
    if token(bare) {
        return Some("<token>");
    }
    None
}

fn ipv4(word: &str) -> bool {
    let host = word.split_once(':').map_or(word, |(h, _)| h);
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 3 && p.parse::<u8>().is_ok())
        && !matches!(host, "127.0.0.1" | "0.0.0.0")
}

fn ipv6(word: &str) -> bool {
    let word = word
        .trim_start_matches('[')
        .split(']')
        .next()
        .unwrap_or(word);
    let colons = word.matches(':').count();
    word != "::1"
        && (word.contains("::") || colons >= 5)
        && colons >= 2
        && word.chars().all(|c| c.is_ascii_hexdigit() || c == ':')
}

/// Shaped like a credential: a known opening, or a long run of letters
/// and digits, both, with nothing to read in it.
fn token(word: &str) -> bool {
    let known = shapes()
        .token_openings
        .iter()
        .any(|o| word.starts_with(o.as_str()) && word.len() >= o.len() + 12);
    let long = word.len() >= 24
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_+/=".contains(c))
        && word.chars().any(|c| c.is_ascii_digit())
        && word.chars().any(|c| c.is_ascii_alphabetic());
    known || long
}

/// `text` with each word `stand_in` names replaced by what it stands in
/// as: a word is what lies between spaces, quotes and brackets.
fn words(text: &str, stand_in: impl Fn(&str) -> Option<&'static str>) -> String {
    let apart = |c: char| c.is_whitespace() || "\"'`()[]{}<>,;=|".contains(c);
    let mut out = String::new();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        match stand_in(word) {
            Some(kind) => {
                let end = word.len() - word.trim_end_matches(['.', ':']).len();
                out.push_str(kind);
                out.push_str(&word[word.len() - end..]);
            }
            None => out.push_str(word),
        }
        word.clear();
    };
    for c in text.chars() {
        if apart(c) {
            flush(&mut word, &mut out);
            out.push(c);
        } else {
            word.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// `text` with `word` replaced where it stands alone: not inside a longer
/// word.
fn replace_word(text: &str, word: &str, with: &str) -> String {
    if word.is_empty() {
        return text.to_string();
    }
    let part = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find(word) {
        let before = rest[..i].chars().next_back();
        let after = rest[i + word.len()..].chars().next();
        out.push_str(&rest[..i]);
        if before.is_some_and(part) || after.is_some_and(part) {
            out.push_str(word);
        } else {
            out.push_str(with);
        }
        rest = &rest[i + word.len()..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Address, Name, Path, sentence};

    /// A secret is told by its words, its shape, or a code beside the
    /// word for it; plain facts, numbers among them, are not.
    #[test]
    fn the_shapes_read() {
        let shapes = shapes();
        assert!(shapes.code_digits > 0 && shapes.code_reach > 0);
        assert!(!shapes.secret_like.is_empty() && !shapes.code_words.is_empty());
        let lower = |words: &[String]| words.iter().all(|w| *w == w.to_lowercase());
        assert!(lower(&shapes.secret_like) && lower(&shapes.code_words));
    }

    #[test]
    fn a_secret_is_told_by_its_words_or_shape() {
        for secret in [
            "The wifi password is hunter2.",
            "API_KEY is in the drawer",
            "Use sk-abcdefghijklmnopqrstuv for the account.",
            "token: 12ab34cd56ef78gh90ij12kl",
            "The locker code is 4821.",
            "Gym locker code: 4821",
            "My PIN is 0042",
            "Card number 4111 1111 1111 1111",
            "locker combo 4821",
            "pw: hunter2",
            "Her SSN is 123-45-6789.",
            "4821 is the garage code",
            "key /home/ada/abcdefghijklmnopqrstuvwxyz0123456789",
        ] {
            assert!(secret_shaped(secret).is_some(), "{secret}");
        }
        for fact in [
            "Maya is my sister.",
            "Pin the tomatoes to the trellis.",
            "The dentist is at 4 pm on the 21st, room 302.",
            "Favorite color is teal.",
            "The locker is by the door.",
            "Pin the tomatoes to the trellis by room 302.",
            "The project is at /srv/projects/2024backups/archive",
            "Run id 123e4567-e89b-12d3-a456-426614174000 failed.",
            "Fixed in commit 4f2a9c1e8b7d6a5f4e3d2c1b0a9f8e7d6c5b4a39.",
            "The PIN pad is by the door, in building 1200 on the far side.",
        ] {
            assert_eq!(secret_shaped(fact), None, "{fact}");
        }
    }

    fn scrub() -> Scrub {
        Scrub::new(
            Some(PathBuf::from("/home/ada")),
            Some("ada".into()),
            Some("ada-laptop\n".into()),
        )
    }

    #[test]
    fn a_diagnosis_is_scrubbed_by_its_json_types() {
        let s = scrub();
        s.know("agent", "my-otter");
        let mut doc = serde_json::json!({"blocks": [{"fields": [
            ["Name", {"agent": "my-otter"}],
            ["Files", {"path": "/home/ada/Link/agents/my-otter"}],
            ["Reached at", {"address": "203.0.113.4:22"}],
            ["Said", "token=abc"],
            ["Log", "ada ran it on ada-laptop"],
        ]}], "exit": 0});
        assert_eq!(s.json(&mut doc), 1);
        let shown = doc.to_string();
        assert!(shown.contains(r#"{"agent":"<agent 1>"}"#), "{shown}");
        assert!(shown.contains("~/Link/agents/<agent 1>"), "{shown}");
        assert!(
            shown.contains("<address>") && shown.contains("<left out>"),
            "{shown}"
        );
        assert!(shown.contains("<user> ran it on <host>"), "{shown}");
        assert!(
            !shown.contains("ada") && !shown.contains("203.0"),
            "{shown}"
        );
    }

    #[test]
    fn values_are_scrubbed_by_type() {
        let s = scrub();
        let otter = Name::new("agent", "my-otter");
        let said = sentence!(
            "Couldn't start {} or {} in {} from {}, at {}.",
            &otter,
            Name::new("agent", "beaver"),
            Path::new("/home/ada/Link/agents/my-otter"),
            Path::new("/srv/private/data"),
            Address::new("203.0.113.4:22"),
        );
        s.meet(&said.0);
        assert_eq!(
            s.spans(&said.0),
            "Couldn't start <agent 1> or <agent 2> in ~/Link/agents/<agent 1> from <path>, at <address>."
        );
        assert_eq!(
            s.spans(&sentence!("Again {}.", &otter).0),
            "Again <agent 1>."
        );
    }

    /// A sentence adopted from an error is words alone: they're scrubbed
    /// by pattern, as text from elsewhere is.
    #[test]
    fn an_adopted_sentence_is_scrubbed_by_pattern() {
        let s = scrub();
        let said = crate::Sentence::adopt("unknown module /home/ada/secret-place on 203.0.113.4");
        assert_eq!(s.spans(&said.0), "unknown module ~/secret-place on <ip>.");
        // Words a secret is said with leave the error's words out, as a
        // trace's line is, though no word is shaped like a credential.
        let said = crate::Sentence::adopt("login refused: password hunter2 is wrong");
        let shown = s.spans(&said.0);
        assert!(!shown.contains("hunter2"), "{shown}");
        assert!(shown.contains("<left out>"), "{shown}");
    }

    /// A corpus of real-shaped logs and errors, with the private words
    /// planted in them.
    #[test]
    fn text_from_elsewhere_is_scrubbed_by_pattern() {
        let s = scrub();
        s.meet(&sentence!("Started {}.", Name::new("agent", "my-otter")).0);
        let corpus = [
            (
                "pip: error: /home/ada/.lnk/safe/agents/harness-grants/hermes/bin/pip exited 1",
                Some("pip: error: ~/.lnk/safe/agents/harness-grants/hermes/bin/pip exited 1"),
            ),
            (
                "ssh: connect to host 203.0.113.4 port 22: Connection refused",
                Some("ssh: connect to host <ip> port 22: Connection refused"),
            ),
            (
                "ada@ada-laptop:~$ lnk agent start my-otter",
                Some("<user>@<host>:~$ lnk agent start <agent 1>"),
            ),
            (
                "mail sent to ada.lovelace@example.com.",
                Some("mail sent to <email>."),
            ),
            (
                "listening on [2001:db8::1]:8080 and 127.0.0.1:3000",
                Some("listening on [<ip>]:8080 and 127.0.0.1:3000"),
            ),
            (
                "using key sk-ant-api03-abcdefghijklmnop for the model",
                Some("using key <token> for the model"),
            ),
            (
                "commit 4f2a9c1e8b7d6a5f4e3d2c1b0a9f8e7d6c5b4a39 on ada's box",
                Some("commit <token> on <user>'s box"),
            ),
            ("Authorization: Bearer abc", None),
            ("export OPENAI_API_KEY=whatever", None),
            ("the password is hunter2", None),
            (
                "started at 2026-10-25T14:05:11Z in 3.2s",
                Some("started at 2026-10-25T14:05:11Z in 3.2s"),
            ),
            ("adapter ada_x readable", Some("adapter ada_x readable")),
        ];
        for (line, want) in corpus {
            assert_eq!(s.line(line).as_deref(), want, "{line}");
        }
        let (kept, left_out) = s.lines(["ok", "token=abc", "fine"]);
        assert_eq!((kept, left_out), (vec!["ok".to_string(), "fine".into()], 1));
    }
}
