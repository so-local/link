//! What a call reads and writes: a file's text, bytes or media, its lines
//! from one on, and a change's diff.

use super::*;

/// Which of a file's lines a `read` gives: from line `offset` (from 1),
/// at most `limit` of them (as many as fit, left out), as Read's
/// `offset` and `limit` say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) offset: usize,
    pub(crate) limit: Option<usize>,
}

impl Span {
    /// From line `offset`, as many as fit.
    #[cfg(test)]
    pub(crate) fn from(offset: usize) -> Self {
        Self {
            offset,
            limit: None,
        }
    }
}

/// A `read`'s [`Span`], from its `offset` and `limit`: the first line,
/// and as many as fit, where each is left out.
pub(crate) fn span_of(arguments: &Value) -> anyhow::Result<Span> {
    let number = |key: &str| -> anyhow::Result<Option<usize>> {
        match arguments.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(n) => Ok(Some(
                n.as_u64()
                    .filter(|n| *n >= 1)
                    .with_context(|| format!("'{key}' is a number of lines, from 1"))?
                    as usize,
            )),
        }
    };
    Ok(Span {
        offset: number("offset")?.unwrap_or(1),
        limit: number("limit")?,
    })
}

/// The text of file `name` in `dir` (`path` to the model), as
/// [`bytes_of`] reads it.
pub(super) fn text_of(dir: &Dir, name: &OsStr, path: &str) -> anyhow::Result<String> {
    String::from_utf8(bytes_of(dir, name, path)?)
        .with_context(|| format!("{path} isn't text (UTF-8)"))
}

/// The bytes of file `name` in `dir` (`path` to the model), at most
/// `file_max` ([`Limits`]), read on the handle it was opened as: never through a
/// link, and a pipe refused, never waited on.
fn bytes_of(dir: &Dir, name: &OsStr, path: &str) -> anyhow::Result<Vec<u8>> {
    let mut opened = match dir.open_file(name, libc::O_RDONLY, 0) {
        Ok(opened) => opened,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!("no file {path}"),
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => bail!("{path} isn't a file"),
        Err(e) => return Err(e).with_context(|| format!("reading {path}")),
    };
    ensure!(
        opened.metadata()?.len() <= limits().file_max,
        "{path} is over {} MiB, larger than file reads",
        limits().file_max >> 20
    );
    let mut bytes = Vec::new();
    (&mut opened)
        .take(limits().file_max + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("reading {path}"))?;
    ensure!(
        bytes.len() as u64 <= limits().file_max,
        "{path} grew past the file read limit"
    );
    Ok(bytes)
}

/// The media type of `bytes` when they're an image or a PDF, which
/// `read` gives whole, as they are: known by their first bytes.
pub(super) fn media_of(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else if bytes.starts_with(b"%PDF-") {
        Some("application/pdf")
    } else {
        None
    }
}

/// `read`'s answer of file `path`, `bytes`: an image or a PDF whole, its
/// media type and its bytes in base64 (`data`); else its lines in
/// `span` ([`lines`]).
pub(super) fn given(bytes: Vec<u8>, path: &str, span: Span) -> anyhow::Result<Value> {
    use base64::Engine;
    if let Some(media) = media_of(&bytes) {
        ensure!(
            bytes.len() as u64 <= limits().media_max,
            "{path} is {media} of {} bytes, past the {} MiB file reads of an image or a PDF, \
             which a model's provider may refuse",
            bytes.len(),
            limits().media_max >> 20
        );
        return Ok(json!({"path": path, "media": media, "bytes": bytes.len(),
            "data": base64::engine::general_purpose::STANDARD.encode(&bytes)}));
    }
    let text = String::from_utf8(bytes).with_context(|| format!("{path} isn't text (UTF-8)"))?;
    lines(&text, path, span)
}

/// Writes `content` to file `name` in `dir` whole: beside it first, on
/// disk before it's moved in place, so a reader (or a crash) never sees
/// half of it; its mode kept, but never setuid, setgid or sticky.
/// Through the folder's handle: a link there is replaced, never followed.
pub(super) fn put(dir: &Dir, name: &OsStr, content: &str) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    ensure!(
        content.len() as u64 <= limits().file_max,
        "a file written is at most {} MiB",
        limits().file_max >> 20
    );
    let shown = name.to_string_lossy();
    let aside = link_files::layout::temporary_name();
    // Drawn at random and made new, never an existing file replaced: a
    // name another session is writing is never removed or written here.
    let aside = OsStr::new(&aside);
    let mode = dir
        .stat(name)
        .ok()
        .filter(|stat| stat.kind == Kind::File)
        .map(|stat| stat.mode & 0o777);
    let written = (|| -> anyhow::Result<()> {
        let mut file = dir.create_new(aside, 0o666)?;
        if let Some(mode) = mode {
            file.set_permissions(std::fs::Permissions::from_mode(mode))?;
        }
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        dir.rename(aside, name)?;
        Ok(())
    })();
    written
        .inspect_err(|_| {
            let _ = dir.remove_file(aside);
        })
        .with_context(|| format!("writing {shown}"))
}

/// `read`: file `path`'s lines in `span`, at most `read_max` ([`Limits`]). Its
/// lines are counted, and the first found, by scanning for their ends,
/// never kept in a list of every line. Its answer names the file's
/// version (`version`, its SHA-256), which `serve` keeps.
pub(super) fn read(files: &Files, path: &str, span: Span) -> anyhow::Result<Value> {
    let place = inside(files, path)?;
    let (dir, name) = place.parent(false)?;
    let bytes = bytes_of(&dir, name, path)?;
    let version = link_digest::sha256(&bytes);
    let mut answer = given(bytes, path, span)?;
    answer[VERSION] = version.into();
    answer[FILE] = file_of(&dir, name).into();
    Ok(answer)
}

/// An error unless file `name` in `dir` (`path` to the model) is the
/// version `expect` names (its SHA-256): changed or removed since.
pub(super) fn unchanged(dir: &Dir, name: &OsStr, path: &str, expect: &str) -> anyhow::Result<()> {
    let now = bytes_of(dir, name, path)
        .ok()
        .map(|b| link_digest::sha256(&b));
    ensure!(
        now.as_deref() == Some(expect),
        "{path} changed since it was read: read it again, then write"
    );
    Ok(())
}

/// Lines `span` of file `path`, `content`, each after its number
/// ([`numbered`]), at most `read_max` ([`Limits`]) whole lines, a single
/// line past it clipped. Where it isn't the whole file, a first line says
/// which lines these are and where `offset` reads on. Its receipt names
/// the numbered text, so it's checked against what the model was given;
/// the source it names is the file as it is, which the stale check reads.
pub(super) fn lines(content: &str, path: &str, span: Span) -> anyhow::Result<Value> {
    let from = span.offset;
    // A last line without its end counts.
    let total =
        newlines(content.as_bytes()) + usize::from(!content.is_empty() && !content.ends_with('\n'));
    ensure!(
        from <= total.max(1),
        "{path} has {total} lines; 'offset' is past them"
    );
    let start = line_start(content.as_bytes(), from);
    let mut text = String::new();
    let mut end = start;
    let mut to = from - 1;
    let mut clipped = false;
    for line in content[start..].split_inclusive('\n') {
        if span.limit.is_some_and(|limit| to + 1 - from >= limit) {
            break;
        }
        let shown = numbered(to + 1, line);
        if !text.is_empty() && text.len() + shown.len() > limits().read_max {
            break;
        }
        // A single line past the most: its start, its number with it.
        let shown = match shown.len() > limits().read_max {
            true => {
                clipped = true;
                let room = limits().read_max - (shown.len() - line.len());
                let kept = &line[..line.floor_char_boundary(room)];
                end += kept.len();
                numbered(to + 1, kept)
            }
            false => {
                end += line.len();
                shown
            }
        };
        text.push_str(&shown);
        to += 1;
    }
    let lines = total;
    let selected = link_files::reading::ContentDigest::of(text.as_bytes());
    let whole = start == 0 && end == content.len();
    if !whole {
        let clipped = if clipped {
            format!("; line {to} is clipped")
        } else {
            String::new()
        };
        let next = if to < lines {
            format!("; 'offset' {} reads on", to + 1)
        } else {
            String::new()
        };
        text = format!("[{path}: lines {from} to {to} of {lines}{clipped}{next}]\n{text}");
    }
    let receipt = link_files::reading::ReadReceipt {
        path: path.into(),
        source: link_files::reading::ReadSnapshot::Whole {
            content: link_files::reading::ContentDigest::of(content.as_bytes()),
        },
        span: link_files::reading::ByteSpan {
            start: start as u64,
            end: end as u64,
        },
        text_prefix_bytes: text.len() as u64 - selected.bytes,
        text: selected,
        transformations: vec![link_files::reading::TextTransform::Numbered],
    };
    receipt.validate(&text)?;
    Ok(
        json!({"path": path, "text": text, "from": from, "to": to, "lines": lines,
        "cut": !whole, "read": receipt}),
    )
}

/// Line `number`, `line`, as `read` gives it: its number right-aligned
/// in `number_width` columns, `number_mark`, then the line, as Claude
/// Code's Read gives it.
fn numbered(number: usize, line: &str) -> String {
    let width = limits().number_width;
    format!("{number:>width$}{}{line}", limits().number_mark)
}

/// How many line ends `bytes` holds, counted 255 bytes at a time in a
/// byte, which the compiler counts many at once.
fn newlines(bytes: &[u8]) -> usize {
    bytes
        .chunks(255)
        .map(|block| block.iter().fold(0u8, |n, b| n + u8::from(*b == b'\n')) as usize)
        .sum()
}

/// Where line `from` (from 1) of `bytes` starts: past the line ends
/// before it, skipped a block at a time; the end if there are fewer.
fn line_start(bytes: &[u8], from: usize) -> usize {
    let mut skip = from - 1;
    let mut at = 0;
    for block in bytes.chunks(255) {
        let ends = newlines(block);
        if ends < skip {
            skip -= ends;
            at += block.len();
            continue;
        }
        if skip == 0 {
            return at;
        }
        for (n, b) in block.iter().enumerate() {
            if *b == b'\n' {
                skip -= 1;
                if skip == 0 {
                    return at + n + 1;
                }
            }
        }
    }
    bytes.len()
}

/// `answer`, a `write`'s or `edit`'s, with the change it made: its
/// `diff`, or where there's none to give (past `diff_max` ([`Limits`]), or what the
/// file held wasn't text), `diff_cut` and `after`'s SHA-256.
pub(super) fn with_change(mut answer: Value, diff: Option<String>, after: &str) -> Value {
    match diff.filter(|diff| diff.len() <= limits().diff_max) {
        Some(diff) => answer["diff"] = diff.into(),
        None => {
            answer["diff_cut"] = true.into();
            answer["sha256"] = link_digest::sha256(after.as_bytes()).into();
        }
    }
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `read` finds its lines by scanning for their ends, as splitting
    /// the file into every line would give them.
    #[test]
    fn read_scans_to_its_lines() {
        let dir = link_test_kit::TempDir::new("file-read-scans");
        std::fs::create_dir_all(dir.join("work")).unwrap();
        let files = Files {
            folder: link_agent_contract::folder::Folder {
                home: dir.to_path_buf(),
                agent: dir.join("agent"),
            },
            write: vec![],
            teams: vec![],
        };
        let long = format!("{}\n", "x".repeat(limits().read_max + 10));
        let many: String = (1..=3000).map(|n| format!("line {n}\n")).collect();
        for content in [
            "",
            "one",
            "one\n",
            "one\ntwo",
            "one\ntwo\n",
            "\n\n\n",
            "é\nü\n",
            &long,
            &many,
            &format!("{many}tail"),
        ] {
            std::fs::write(dir.join("work/f.md"), content).unwrap();
            let lines: Vec<&str> = content.split_inclusive('\n').collect();
            let last = lines.len().max(1);
            for from in (1..=last).filter(|n| *n <= 5 || n % 97 == 0 || n + 2 >= last) {
                let got = read(&files, "work/f.md", Span::from(from)).unwrap();
                assert_eq!(got["lines"], lines.len(), "{content:?} from {from}");
                let to = got["to"].as_u64().unwrap() as usize;
                let text = got["text"].as_str().unwrap();
                let receipt: link_files::reading::ReadReceipt =
                    serde_json::from_value(got["read"].clone()).unwrap();
                receipt.validate(text).unwrap();
                let body = &text[receipt.text_prefix_bytes as usize..];
                let want: String = lines[from - 1..to]
                    .iter()
                    .zip(from..)
                    .map(|(l, n)| numbered(n, l))
                    .map(|l| l[..l.floor_char_boundary(limits().read_max)].to_string())
                    .collect();
                assert_eq!(body, want, "{content:?} from {from}");
                assert!(body.len() <= limits().read_max, "{content:?} from {from}");
                assert!(to >= from || lines.is_empty(), "{content:?} from {from}");
            }
            let past = read(&files, "work/f.md", Span::from(last + 1));
            assert!(past.is_err(), "{content:?}");
        }
        assert_eq!(line_start(b"a\nb\nc", 3), 4);
        assert_eq!(line_start(b"a\nb\n", 3), 4);
        assert_eq!(line_start(b"a", 1), 0);
    }

    /// Writers sharing one folder each get their own file whole, and a
    /// file another writer has beside it, by a temporary name, is left
    /// alone: names are drawn at random and made new.
    #[test]
    fn writers_in_one_folder_never_meet() {
        let dir = link_test_kit::TempDir::new("file-writers-meet");
        let other = link_files::layout::temporary_name();
        std::fs::write(dir.join(&other), "theirs").unwrap();
        std::thread::scope(|scope| {
            for n in 0..16 {
                let path = dir.to_path_buf();
                scope.spawn(move || {
                    let folder = Dir::open(&path).unwrap();
                    for round in 0..20 {
                        let content = format!("writer {n} round {round}\n");
                        put(&folder, OsStr::new(&format!("f{n}.md")), &content).unwrap();
                        let read = std::fs::read_to_string(path.join(format!("f{n}.md"))).unwrap();
                        assert_eq!(read, content);
                    }
                });
            }
        });
        assert_eq!(std::fs::read_to_string(dir.join(&other)).unwrap(), "theirs");
        let left: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| link_files::layout::is_temporary(name) && *name != other)
            .collect();
        assert!(left.is_empty(), "{left:?}");
    }

    #[test]
    fn a_clipped_single_line_never_claims_a_whole_read() {
        let source = "é".repeat(limits().read_max);
        let answer = lines(&source, "skills/example/SKILL.md", Span::from(1)).unwrap();
        let receipt: link_files::reading::ReadReceipt =
            serde_json::from_value(answer["read"].clone()).unwrap();
        receipt.validate(answer["text"].as_str().unwrap()).unwrap();
        assert_eq!(answer["to"], answer["lines"]);
        assert_eq!(answer["cut"], true);
        assert!(
            answer["text"]
                .as_str()
                .unwrap()
                .contains("line 1 is clipped")
        );
        assert!(!receipt.covers_source());
        // Its number takes room in the most a read gives.
        let room = limits().read_max - (numbered(1, "").len());
        assert_eq!(receipt.span.end, source.floor_char_boundary(room) as u64);
        let changed = lines("changed", "skills/example/SKILL.md", Span::from(1)).unwrap();
        assert_ne!(answer["read"]["source"], changed["read"]["source"]);
    }

    /// `limit` gives at most that many lines from `offset`, its answer
    /// saying where it stopped and that `offset` reads on; past the end,
    /// it gives the rest; a limit or offset that isn't a number from 1 is
    /// refused.
    #[test]
    fn a_read_gives_its_limit_of_lines_from_its_offset() {
        let content = "one\ntwo\nthree\nfour\n";
        let span = |offset, limit| Span {
            offset,
            limit: Some(limit),
        };
        let two = lines(content, "f.md", span(2, 2)).unwrap();
        assert_eq!(
            (two["from"].clone(), two["to"].clone()),
            (json!(2), json!(3))
        );
        let text = two["text"].as_str().unwrap();
        assert!(text.ends_with("     2→two\n     3→three\n"), "{text}");
        assert!(text.contains("'offset' 4 reads on"), "{text}");
        let rest = lines(content, "f.md", span(3, 10)).unwrap();
        assert_eq!(rest["to"], 4);
        assert!(
            !rest["text"].as_str().unwrap().contains("reads on"),
            "{rest}"
        );
        let whole = lines(content, "f.md", span(1, 4)).unwrap();
        assert_eq!(
            whole["text"],
            "     1→one\n     2→two\n     3→three\n     4→four\n"
        );
        assert_eq!(whole["cut"], false);
        assert_eq!(
            span_of(&json!({"offset": 3, "limit": 5})).unwrap(),
            span(3, 5)
        );
        assert_eq!(span_of(&json!({})).unwrap(), Span::from(1));
        for bad in [json!({"limit": 0}), json!({"offset": "2"})] {
            assert!(span_of(&bad).is_err(), "{bad}");
        }
    }

    /// A file written keeps its mode but never setuid, setgid or sticky.
    #[test]
    fn a_write_drops_setuid() {
        use std::os::unix::fs::PermissionsExt;
        let dir = link_test_kit::TempDir::new("file-setuid");
        let path = dir.join("run.sh");
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o6755)).unwrap();
        put(&Dir::open(dir.path()).unwrap(), OsStr::new("run.sh"), "new").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o7777, 0o755);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    }

    /// An image or a PDF past `media_max` ([`Limits`]) is refused, never given to a
    /// provider that may refuse it.
    #[test]
    fn media_past_its_most_is_refused() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.resize(limits().media_max as usize, 0);
        assert!(given(png.clone(), "a.png", Span::from(1)).is_ok());
        png.push(0);
        let why = given(png, "a.png", Span::from(1)).unwrap_err().to_string();
        assert!(why.contains("past the 2 MiB"), "{why}");
    }
}
