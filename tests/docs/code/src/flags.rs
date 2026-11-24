//! Check flags on explicit `lnk` examples against their command declarations.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

use link_docs_check::{Finding, Tree, fail, md};

/// Flag spelling and whether the following word is its value.
type Flags = BTreeMap<String, bool>;
type Catalog = BTreeMap<String, Flags>;
/// Each command's spec shapes, by its path, which resolve a field flag
/// its listing stops short of: one below a type within itself, as
/// `--machine.gcp.size` in `[machine.<cloud>]`.
type Shapes = BTreeMap<String, Vec<serde_json::Value>>;

fn add(catalog: &mut Catalog, path: &str, command: &link_component::Command) {
    let flags = catalog.entry(path.to_owned()).or_default();
    // A command a spec binds takes its field flags, a free key as `<name>`.
    if let Some(kind) = &command.spec
        && let Some(page) = crate::references::kinds::kinds()
            .into_iter()
            .find(|k| k.kind.kind == *kind)
    {
        for field in link_spec::fields::fields(&page.kind.shape) {
            flags.insert(format!("--{}", field.flag), !field.hint.is_empty());
        }
    }
    for flag in command.flags.keys().filter(|flag| flag.starts_with('-')) {
        flags
            .entry(flag.split_whitespace().next().unwrap().to_owned())
            .or_insert(false);
    }
    for option in command.options.values() {
        let takes_value = !matches!(option.r#type, link_component::ValueType::Boolean);
        if let Some(long) = &option.long {
            flags.insert(format!("--{long}"), takes_value);
        }
        if let Some(short) = &option.short {
            flags.insert(format!("-{short}"), takes_value);
        }
    }
}

/// The spec shape of each command a spec binds, and of its group where
/// it is the group's default.
fn shapes() -> Shapes {
    let kinds = crate::references::kinds::kinds();
    let mut shapes = Shapes::new();
    for component in link_cli::registry().values() {
        for (path, command) in &component.commands {
            let Some(page) = command
                .spec
                .as_ref()
                .and_then(|kind| kinds.iter().find(|k| k.kind.kind == *kind))
            else {
                continue;
            };
            let mut paths = vec![path.as_str()];
            if command.default
                && let Some((group, _)) = path.rsplit_once(' ')
            {
                paths.push(group);
            }
            for path in paths {
                shapes
                    .entry(path.to_owned())
                    .or_default()
                    .push(page.kind.shape.clone());
            }
        }
    }
    shapes
}

/// Whether `word`, a field flag `option` didn't find listed, is one a
/// spec `path` binds resolves, and if so whether its value follows.
fn field(shapes: &Shapes, path: &str, word: &str) -> Option<bool> {
    let (name, attached) = word
        .split_once('=')
        .map_or((word, false), |(flag, _)| (flag, true));
    if !name.starts_with("--") || !name.contains('.') {
        return None;
    }
    shapes
        .get(path)
        .into_iter()
        .flatten()
        .find_map(|shape| link_spec::fields::flag_takes(shape, name))
        .map(|takes| takes && !attached)
}

fn catalog() -> Catalog {
    let mut catalog = Catalog::new();
    for component in link_cli::registry().values() {
        for (path, command) in &component.commands {
            add(&mut catalog, path, command);
            // Its group, typed bare, runs its default with its flags.
            if command.default
                && let Some((group, _)) = path.rsplit_once(' ')
            {
                add(&mut catalog, group, command);
            }
        }
    }
    let core = link_component::actions::parse(link_cli::cli::HELP.toml)
        .expect("the core's command declarations");
    for (path, command) in link_component::actions::commands(&core) {
        add(&mut catalog, &path, &command);
    }
    catalog.entry(String::new()).or_default().extend([
        (link_cli::REGISTRY_FLAG.to_owned(), false),
        (link_cli::DOCUMENTATION_FLAG.to_owned(), false),
    ]);
    catalog
}

fn flags(catalog: &Catalog, path: &str) -> Flags {
    let mut flags = Flags::from([("--help".into(), false), ("-h".into(), false)]);
    if !path.contains(' ') {
        flags.extend([("--version".into(), false), ("-V".into(), false)]);
    }
    let mut prefix = String::new();
    if path.is_empty() {
        flags.extend(
            catalog
                .get("")
                .into_iter()
                .flatten()
                .map(|(k, v)| (k.clone(), *v)),
        );
    }
    for word in path.split_whitespace() {
        if !prefix.is_empty() {
            prefix.push(' ');
        }
        prefix.push_str(word);
        flags.extend(
            catalog
                .get(&prefix)
                .into_iter()
                .flatten()
                .map(|(k, v)| (k.clone(), *v)),
        );
    }
    flags
}

fn option(word: &str, declared: &Flags) -> Result<bool, String> {
    let (name, attached) = word
        .split_once('=')
        .map_or((word, false), |(flag, _)| (flag, true));
    if let Some(takes_value) = declared.get(name) {
        return Ok(*takes_value && !attached);
    }
    // A field flag's free key is any key: `--variables.REGION`.
    if name.contains('.')
        && let Some(takes_value) = declared.iter().find_map(|(flag, takes)| {
            let (want, got): (Vec<&str>, Vec<&str>) =
                (flag.split('.').collect(), name.split('.').collect());
            (want.len() == got.len()
                && want.iter().zip(&got).all(|(w, g)| w == g || *w == "<name>"))
            .then_some(*takes)
        })
    {
        return Ok(takes_value && !attached);
    }
    if word.starts_with("--") {
        return Err(name.to_owned());
    }
    // A value-taking short flag consumes the rest of its word: -aNAME.
    let mut letters = word[1..].chars().peekable();
    while let Some(letter) = letters.next() {
        let name = format!("-{letter}");
        let takes_value = declared.get(&name).ok_or_else(|| name.clone())?;
        if *takes_value {
            return Ok(letters.peek().is_none());
        }
    }
    Ok(false)
}

/// Give unquoted shell operators their own words before shell-word parsing.
pub(crate) fn operators(text: &str) -> String {
    let mut out = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if escaped {
            out.push(ch);
            escaped = false;
        } else if ch == '\\' && quote != Some('\'') {
            out.push(ch);
            escaped = true;
        } else if quote == Some(ch) {
            out.push(ch);
            quote = None;
        } else if quote.is_none() && matches!(ch, '\'' | '"') {
            out.push(ch);
            quote = Some(ch);
        } else if quote.is_none() && matches!(ch, ';' | '|' | '&') {
            out.push(' ');
            out.push(ch);
            if chars.peek() == Some(&ch) && ch != ';' {
                out.push(chars.next().unwrap());
            }
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    out
}

pub(crate) fn boundary(word: &str) -> bool {
    [";", "&&", "||", "|", "&"].contains(&word)
}

fn line(catalog: &Catalog, shapes: &Shapes, text: &str) -> Vec<String> {
    let Some(words) = shlex::split(&operators(text)) else {
        // Fragments with an unfinished quote are not a complete invocation.
        return Vec::new();
    };
    let mut wrong = Vec::new();
    let mut at = 0;
    while at < words.len() {
        if words[at] != "lnk" {
            at += 1;
            continue;
        }
        at += 1;
        let mut path = String::new();
        while at < words.len() && !boundary(&words[at]) {
            let word = &words[at];
            at += 1;
            if word == "--" {
                // The rest is another program's argv, not Link's flags.
                while at < words.len() && !boundary(&words[at]) {
                    at += 1;
                }
                break;
            }
            if word.starts_with('-') && word.len() > 1 && word.parse::<f64>().is_err() {
                let found = option(word, &flags(catalog, &path))
                    .or_else(|flag| field(shapes, &path, word).ok_or(flag));
                match found {
                    Ok(true) if at < words.len() => at += 1,
                    Ok(_) => {}
                    Err(flag) => wrong.push(format!("lnk {path}: unknown flag {flag}")),
                }
                continue;
            }
            let candidate = if path.is_empty() {
                word.clone()
            } else {
                format!("{path} {word}")
            };
            if catalog.contains_key(&candidate) {
                path = candidate;
            } else if path.is_empty() {
                // The command-existence rule owns an unknown command.
                break;
            }
        }
    }
    wrong
}

fn examples(text: &str) -> Vec<String> {
    let (prose, code) = md::split(text);
    let mut examples = md::spans(&prose.join("\n"));
    let mut continued = String::new();
    for (language, line) in code {
        if !matches!(language, "" | "sh" | "bash" | "console") {
            continue;
        }
        if line.trim_start().starts_with('#') {
            continue;
        }
        let line = line.trim_end();
        if let Some(part) = line.strip_suffix('\\') {
            continued.push_str(part);
            continued.push(' ');
        } else {
            continued.push_str(line);
            examples.push(std::mem::take(&mut continued));
        }
    }
    if !continued.is_empty() {
        examples.push(continued);
    }
    examples
}

pub fn check(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    if !tree.is_doc(file) {
        return Vec::new();
    }
    static CATALOG: LazyLock<Catalog> = LazyLock::new(catalog);
    static SHAPES: LazyLock<Shapes> = LazyLock::new(shapes);
    examples(text)
        .iter()
        .flat_map(|example| line(&CATALOG, &SHAPES, example))
        .map(|message| fail(format!("{}: {message}", tree.shown(file))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Catalog {
        BTreeMap::from([
            (
                "agent".into(),
                Flags::from([("--agent".into(), true), ("-a".into(), true)]),
            ),
            (
                "agent start".into(),
                Flags::from([("--model".into(), true), ("--json".into(), false)]),
            ),
            ("sandbox".into(), Flags::new()),
            ("sandbox run".into(), Flags::from([("--read".into(), true)])),
        ])
    }

    #[test]
    fn misspelled_and_wrong_command_flags_fail() {
        let catalog = fixture();
        for text in [
            "lnk agent start --modle qwen3",
            "lnk agent start --read .",
            "lnk agent start -x",
        ] {
            assert_eq!(line(&catalog, &Shapes::new(), text).len(), 1, "{text}");
        }
        assert_eq!(
            line(&catalog, &Shapes::new(), "lnk agent start --modle=x").len(),
            1
        );
        assert_eq!(
            line(&catalog, &Shapes::new(), "lnk --made-up-global-flag").len(),
            1
        );
    }

    #[test]
    fn inherited_flags_values_and_other_program_argv_are_distinguished() {
        let catalog = fixture();
        for text in [
            "lnk agent start --agent work --model=qwen3 --json",
            "lnk agent --agent work start --model qwen3",
            "lnk agent start -awork --model '--not-a-flag'",
            "lnk agent start --model 'words # not a comment' # --not-a-flag",
            "lnk sandbox run --read . -- cargo test --not-a-link-flag",
            "lnk agent start --help | jq --not-a-link-flag",
            "lnk agent start --json;echo --not-a-link-flag",
            "lnk agent start --model 'a;b|c&d'",
        ] {
            assert!(
                line(&catalog, &Shapes::new(), text).is_empty(),
                "{text}: {:?}",
                line(&catalog, &Shapes::new(), text)
            );
        }
        assert_eq!(
            line(
                &catalog,
                &Shapes::new(),
                "lnk agent start --json && lnk agent start --modle x"
            )
            .len(),
            1
        );
    }

    #[test]
    fn prose_and_continued_examples_are_checked_but_code_in_other_languages_is_not() {
        let text = "Use `lnk agent start --modle x`.\n\n```sh\nlnk agent start \\\n  --modle x\n```\n\n```json\nlnk agent start --modle x\n```\n";
        let wrong: Vec<_> = examples(text)
            .iter()
            .flat_map(|example| line(&fixture(), &Shapes::new(), example))
            .collect();
        assert_eq!(wrong.len(), 2);
    }

    /// A field flag below a type within itself resolves through the
    /// command's spec shape, and a key that shape lacks is still refused.
    #[test]
    fn a_field_flag_below_a_type_within_itself_is_known() {
        let catalog = fixture();
        let shape = serde_json::json!({
            "type": "object",
            "properties": {"machine": {"$ref": "#/$defs/Machine"}},
            "$defs": {"Machine": {
                "type": "object",
                "properties": {
                    "size": {"type": "string"},
                    "cloud": {
                        "type": "object",
                        "additionalProperties": {"$ref": "#/$defs/Machine"}
                    }
                },
                "additionalProperties": false
            }}
        });
        let shapes = Shapes::from([("agent start".to_owned(), vec![shape])]);
        assert!(
            line(
                &catalog,
                &shapes,
                "lnk agent start --machine.cloud.gcp.size e2-small"
            )
            .is_empty()
        );
        assert!(
            line(
                &catalog,
                &shapes,
                "lnk agent start --machine.cloud.gcp.size=e2-small --json"
            )
            .is_empty()
        );
        assert_eq!(
            line(
                &catalog,
                &shapes,
                "lnk agent start --machine.cloud.gcp.colour red"
            )
            .len(),
            1
        );
        assert_eq!(
            line(
                &catalog,
                &shapes,
                "lnk sandbox run --machine.cloud.gcp.size x"
            )
            .len(),
            1
        );
    }
}
