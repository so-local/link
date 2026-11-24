//! Every file Link saves names its schema: it's written through
//! `link_files::saved`, never as a bare TOML or JSON value
//! (`link_repository_check::saved`, by `saved.toml`).

use std::path::Path;

use link_repository_check::saved::{bare_writes, problems, refused};

#[test]
fn every_saved_file_names_its_schema_but_the_allowed_and_the_held() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = here.join("../..").canonicalize().unwrap();
    let problems = problems(&root, &here.join("saved.toml"));
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn a_bare_toml_or_json_write_is_refused() {
    assert!(refused(
        "link_files::write_private(&path, toml::to_string(self)?.as_bytes())"
    ));
    assert!(refused(
        "    std::fs::write(&path, serde_json::to_vec(&list)?)?;"
    ));
    assert!(!refused("link_files::saved::write(&path, self)?;"));
    assert!(!refused("let text = toml::to_string(&value)?;"));
    assert!(!refused(
        "// link_files::write_private(&p, toml::to_string(x))"
    ));
}

/// A bare line appended to a ledger is refused, whether the
/// serialization is handed to `write_all` at once or through a variable,
/// or to an `append`; a ledger line written by `link_files::saved::line`,
/// a value that's never text, and a bare wire message are not.
#[test]
fn an_appended_bare_line_is_refused() {
    let at_once = r#"
pub fn append(path: &Path, event: &Event) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all((serde_json::to_string(event)? + "\n").as_bytes())?;
    Ok(())
}
"#;
    assert_eq!(bare_writes(at_once), [7]);
    let through = r#"
fn log(path: &Path, message: &Message) -> anyhow::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    let mut bytes = serde_json::to_vec(message)?;
    bytes.push(b'\n');
    options.open(path)?.write_all(&bytes)?;
    Ok(())
}

fn send(stream: &mut UnixStream, message: &Message) -> anyhow::Result<()> {
    let mut bytes = serde_json::to_vec(message)?;
    bytes.push(b'\n');
    stream.write_all(&bytes)?;
    Ok(())
}
"#;
    assert_eq!(bare_writes(through), [5]);
    let handed = r#"
pub(crate) async fn keep(made: &Path, event: &Event) -> anyhow::Result<()> {
    frames::append(made, event.seq, &toml::to_string(event)?)?;
    Ok(())
}
"#;
    assert_eq!(bare_writes(handed), [3]);
    let named = r#"
fn append(&self, thread: &str, event: &Value) -> anyhow::Result<Reply> {
    let size = serde_json::to_vec(event)?.len() as u64;
    self.store.put(&self.event(thread), &serde_json::to_vec(event)?)
}
"#;
    assert!(bare_writes(named).is_empty());
    let saved = r#"
pub fn append(path: &Path, event: &Event) -> Result<()> {
    let mut file = std::fs::OpenOptions::new().append(true).open(path)?;
    file.write_all(&link_files::saved::line(event)?)?;
    report["phases"] = serde_json::to_value(phases)?;
    // file.write_all((serde_json::to_string(event)? + "\n").as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    fn bare(file: &mut File) {
        file.write_all(&serde_json::to_vec(&1).unwrap()).unwrap();
        let _ = OpenOptions::new().append(true);
    }
}
"#;
    assert!(bare_writes(saved).is_empty());
}

/// A function that serializes bare and writes a file is refused wherever
/// the serialization sits: split across lines from its write, or handed
/// to a `write_all` on a file it opens. A `#[cfg(test)]` on one item
/// leaves the rest of the file read; only a test module is left out.
#[test]
fn a_bare_write_split_or_made_by_write_all_is_refused() {
    let split = r#"
fn keep(path: &Path, kept: &Kept) -> anyhow::Result<()> {
    link_files::write_private(
        path,
        &serde_json::to_vec(kept)?,
    )?;
    Ok(())
}
"#;
    assert_eq!(bare_writes(split), [5]);
    let opened = r#"
fn save(&self, counts: &Counts) -> anyhow::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&aside)?;
    file.write_all(&serde_json::to_vec(counts)?)?;
    Ok(())
}
"#;
    assert_eq!(bare_writes(opened), [7]);
    let after_a_test_item = r#"
#[cfg(test)]
fn fixture() {}

fn keep(path: &Path, kept: &Kept) -> anyhow::Result<()> {
    let text = toml::to_string(kept)?;
    std::fs::write(path, text)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    fn bare(path: &Path) {
        std::fs::write(path, serde_json::to_vec(&1).unwrap()).unwrap();
    }
}

fn after(path: &Path) -> anyhow::Result<()> {
    std::fs::write(path, serde_json::to_vec(&2)?)?;
    Ok(())
}
"#;
    assert_eq!(bare_writes(after_a_test_item), [6, 19]);
    let digested = r#"
fn keep(path: &Path, note: &Note) -> anyhow::Result<()> {
    let id = link_digest::sha256(&serde_json::to_vec(&note.source)?);
    link_files::write_private(&path.join(id), note.text.as_bytes())?;
    Ok(())
}
"#;
    assert!(bare_writes(digested).is_empty());
}

/// A bare value handed to another function of the file that writes it is
/// refused, at once or through a name, as are `File::options()`,
/// `toml::ser::to_string` and a JSON value's `to_string()`; a key that
/// function never writes, and JSON sent on a stream, are not.
#[test]
fn a_bare_write_through_another_function_or_another_spelling_is_refused() {
    let through = r#"
fn keep(path: &Path, user: &User) -> anyhow::Result<()> {
    root_file(path, toml::to_string(user)?.as_bytes())?;
    let text = serde_json::to_vec(user)?;
    let key = serde_json::to_string(&user.name)?;
    remember(&key);
    root_file(path, &text)
}

fn root_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    link_files::write_private(path, bytes)?;
    Ok(())
}
"#;
    assert_eq!(bare_writes(through), [3, 4]);
    let spelled = r#"
fn keep(path: &Path, kept: &Kept) -> anyhow::Result<()> {
    let mut file = std::fs::File::options().write(true).open(path)?;
    file.write_all(toml::ser::to_string(kept)?.as_bytes())?;
    let value = serde_json::json!({"kept": kept.name});
    file.write_all(value.to_string().as_bytes())?;
    stream.write_all(&serde_json::to_vec(kept)?)?;
    println!("{}", serde_json::to_string(kept)?);
    Ok(())
}
"#;
    assert_eq!(bare_writes(spelled), [4, 6]);
}

/// A test module's braces in a string or a character never end it early.
#[test]
fn a_test_modules_braces_in_strings_are_not_its_own() {
    let quoted = r##"
#[cfg(test)]
mod tests {
    fn bare(path: &Path) {
        let close = '}';
        let text = "}}";
        let raw = r#"}
        "#;
        std::fs::write(path, serde_json::to_vec(&1).unwrap()).unwrap();
    }
}

fn after(path: &Path) -> anyhow::Result<()> {
    std::fs::write(path, serde_json::to_vec(&2)?)?;
    Ok(())
}
"##;
    assert_eq!(bare_writes(quoted), [14]);
}
