//! The docs' references: each module's command reference and each spec
//! kind's reference page, in the module's own `docs/`, and one variable
//! index for the workspace. Each is one page, however long: a reader
//! looks an entry up by its heading, so a reference is never split.

mod commands;
mod page;
mod specs;
mod variables;

use std::path::{Component, Path, PathBuf};

use anyhow::Context;
use link_actions::{Catalog, Module};

use crate::{Files, config::SETTINGS};

/// Every reference page for the modules in `catalog` whose folders are in
/// `root`, the workspace, and its variable index, by its path from `root`.
/// A module elsewhere, as core's shapes in `link-actions`, has its pages
/// written by the workspace holding it. A domain hub holds at least
/// `least` modules, the fewest pages the docs allow a page holding pages.
pub fn generate(catalog: &Catalog, root: &Path, least: usize) -> anyhow::Result<Files> {
    let mut files = Files::new();
    let mut hub = Vec::new();
    for module in catalog.modules().filter(|m| m.directory.starts_with(root)) {
        let pages = module_pages(catalog, module, root)?;
        let folder = folder(module, root)?;
        let page = folder.join(commands::page(module, &folder));
        if pages.get(&page).is_some() {
            hub.push((module.identity.id.clone(), commands::domain(&folder), page));
        }
        files.extend(Path::new(""), pages)?;
    }
    files.extend(Path::new(""), commands::hub(&hub, least)?)?;
    files.extend(Path::new(""), variables::generate(catalog, root)?)?;
    if let Some(index) = Path::new(&SETTINGS.reference.variables).parent() {
        files.own(index);
    }
    Ok(files)
}

/// One module's command reference and spec pages, by their paths from
/// `root`.
pub fn module_pages(catalog: &Catalog, module: &Module, root: &Path) -> anyhow::Result<Files> {
    let mut files = commands::generate(catalog, module, root)?;
    files.extend(Path::new(""), specs::generate(catalog, module, root)?)?;
    // Its folders are the generator's own, so a page nothing writes any
    // more, its kind renamed or its commands gone, is found and removed.
    let folder = folder(module, root)?;
    files.own(folder.join(&SETTINGS.reference.commands));
    files.own(folder.join(&SETTINGS.reference.specs));
    Ok(files)
}

/// A module's folder from `root`.
fn folder(module: &Module, root: &Path) -> anyhow::Result<PathBuf> {
    module
        .directory
        .strip_prefix(root)
        .map(Path::to_path_buf)
        .with_context(|| {
            format!(
                "module {} at {} is outside {}",
                module.identity.id,
                module.directory.display(),
                root.display()
            )
        })
}

/// The link from the page at `from` to the file at `to`, both from one root.
fn relative(from: &Path, to: &Path) -> String {
    let from: Vec<Component> = from
        .parent()
        .unwrap_or(Path::new(""))
        .components()
        .collect();
    let to: Vec<Component> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut out = PathBuf::new();
    for _ in common..from.len() {
        out.push("..");
    }
    for part in &to[common..] {
        out.push(part);
    }
    out.to_string_lossy().replace('\\', "/")
}

/// `text` as a page writes it: each bare command (`lnk model list`), path
/// (`~/Link/Files`, `/v1/models`) and environment variable (`AWS_PROFILE`)
/// in a code span, as the docs write code, leaving text already in one as
/// it is.
pub(crate) fn code(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, piece) in text.split('`').enumerate() {
        if i > 0 {
            out.push('`');
        }
        match i % 2 {
            1 => out.push_str(piece),
            _ => spans(piece, &mut out),
        }
    }
    out
}

/// `piece`, text outside code, with its code spanned into `out`.
fn spans(piece: &str, out: &mut String) {
    let words: Vec<&str> = piece.split(' ').collect();
    let mut i = 0;
    while i < words.len() {
        if i > 0 {
            out.push(' ');
        }
        let (lead, core, tail) = punctuated(words[i]);
        let starts = |word: &&str| word.starts_with(|c: char| c.is_ascii_lowercase());
        if core == "lnk" && tail.is_empty() && words.get(i + 1).is_some_and(starts) {
            // A command runs through each word that continues it: a lowercase
            // word, a flag or a placeholder, up to punctuation.
            let mut command = vec![core];
            let mut end = "";
            let mut next = i + 1;
            while let Some(word) = words.get(next) {
                let (lead, core, tail) = punctuated(word);
                let continues = core
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-<>_.=/".contains(c));
                if !lead.is_empty() || core.is_empty() || !continues {
                    break;
                }
                command.push(core);
                next += 1;
                if !tail.is_empty() {
                    end = tail;
                    break;
                }
            }
            out.push_str(&format!("{lead}`{}`{end}", command.join(" ")));
            i = next;
            continue;
        }
        match path(core) || variable(core) {
            true => out.push_str(&format!("{lead}`{core}`{tail}")),
            false => out.push_str(words[i]),
        }
        i += 1;
    }
}

/// `word` as its punctuation before, itself, and its punctuation after.
fn punctuated(word: &str) -> (&str, &str, &str) {
    let core = word.trim_start_matches(|c: char| "([\"'*".contains(c));
    let lead = &word[..word.len() - core.len()];
    let trimmed = core.trim_end_matches(|c: char| ",.;:!?)]\"'*".contains(c));
    (lead, trimmed, &core[trimmed.len()..])
}

/// Whether `word` is a path: from the home folder, here, or the root,
/// with a folder in it (`~/.aws`, `/v1/models`), or a route (`/v1`),
/// after any placeholder (`<url>/v1/metrics`).
fn path(word: &str) -> bool {
    let word = match word.strip_prefix('<').and_then(|w| w.split_once('>')) {
        Some((_, rest)) => rest,
        None => word,
    };
    let nested = ["~/", "./", "/"].iter().any(|p| word.starts_with(p))
        && word[1..].contains('/')
        && !word.starts_with("//");
    let route = word.len() > 1
        && word.starts_with('/')
        && word[1..].starts_with(|c: char| c.is_ascii_alphanumeric());
    nested || route
}

/// Whether `word` is an environment variable's name: `AWS_PROFILE`.
fn variable(word: &str) -> bool {
    word.contains('_')
        && word.starts_with(|c: char| c.is_ascii_uppercase())
        && word
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// Items as a sentence's list, with an Oxford comma: `a, b, and c`.
fn sentence(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    }
}

/// A name in Title Case for a heading: `cloud-account` is `Cloud Account`,
/// a word `surfaces.toml` spells out is written in full, `spec` as
/// `Specification`, and an acronym it lists in capitals, `mcp` as `MCP`.
fn title(name: &str) -> String {
    let reference = &crate::config::SETTINGS.reference;
    name.split('-')
        .map(|word| {
            if let Some(full) = reference.spelled_out.get(word) {
                return full.clone();
            }
            if reference.acronyms.contains(word) {
                return word.to_ascii_uppercase();
            }
            let mut chars = word.chars();
            chars
                .next()
                .map(|c| c.to_ascii_uppercase().to_string() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A one-line summary of a shape's type for a reference: `string`,
/// `list of env.stage`, `one of "a", "b"`.
fn kind(schema: &link_actions::schema::Schema) -> String {
    use link_actions::schema::{Node, Told};
    match &schema.node {
        Node::Named(name) => format!("`{name}`"),
        Node::Record(_) => "table".into(),
        Node::Map(map) => format!("table of {}", kind(&map.values)),
        Node::List(list) => format!("list of {}", kind(&list.items)),
        Node::Enum(values) => format!(
            "one of {}",
            values
                .iter()
                .map(|v| format!("`{}`", plain(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Node::Const(value) => format!("`{}`", plain(value)),
        Node::OneOf(one) => match &one.told {
            Told::Tag(tag) => format!("one of {} variants by `{tag}`", one.variants.len()),
            Told::Markers(_) => format!(
                "one of {} variants, each by a key it alone has",
                one.variants.len()
            ),
            Told::Value => format!("one of {} variants, by its value", one.variants.len()),
        },
        Node::String(text) => match (text.format, &text.kind) {
            (_, Some(kind)) => format!("{kind} ID"),
            (Some(format), None) => format!("string, {}", format.as_str()),
            (None, None) if text.path.is_some() => "path".into(),
            (None, None) => "string".into(),
        },
        Node::Integer(_) => "integer".into(),
        Node::Number(_) => "number".into(),
        Node::Boolean => "boolean".into(),
        Node::Any => "any JSON value".into(),
    }
}

/// A JSON value as a reader would type it.
fn plain(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn a_bare_path_is_written_in_code_and_code_stays_as_it_is() {
        assert_eq!(
            super::code("needs OpenAI's /v1/models and /v1/chat/completions. Only /v1/ is."),
            "needs OpenAI's `/v1/models` and `/v1/chat/completions`. Only `/v1/` is."
        );
        assert_eq!(
            super::code("see `lnk model list` and /"),
            "see `lnk model list` and /"
        );
        assert_eq!(
            super::code(
                "a type (lnk storage rclone help backends), the ~/.aws profile [default: AWS_PROFILE, else default]"
            ),
            "a type (`lnk storage rclone help backends`), the `~/.aws` profile [default: `AWS_PROFILE`, else default]"
        );
        assert_eq!(
            super::code("its account's (see lnk cloud list). Files go in ~/Link/Files."),
            "its account's (see `lnk cloud list`). Files go in `~/Link/Files`."
        );
        assert_eq!(
            super::code("go to <url>/v1/metrics, as JSON"),
            "go to `<url>/v1/metrics`, as JSON"
        );
        assert_eq!(
            super::code("KEY=VALUE, lnk, and Link"),
            "KEY=VALUE, lnk, and Link"
        );
    }

    #[test]
    fn a_modules_page_is_under_its_domains_hub() {
        use std::path::PathBuf;
        assert_eq!(
            super::commands::domain(Path::new("modules/tooling/bundle")).as_deref(),
            Some("tooling")
        );
        assert_eq!(
            super::commands::domain(Path::new("interop/agents/connections/signal")).as_deref(),
            Some("agents")
        );
        assert_eq!(super::commands::domain(Path::new("box")), None);
        assert_eq!(super::commands::domain(Path::new("modules/box")), None);
        let pages = [
            (
                "bundle".to_string(),
                Some("tooling".to_string()),
                PathBuf::from(
                    "modules/tooling/bundle/docs/more/reference/modules/tooling/bundle.md",
                ),
            ),
            (
                "box".to_string(),
                None,
                PathBuf::from("box/docs/more/reference/modules/box.md"),
            ),
        ];
        let files = super::commands::hub(&pages, 1).unwrap();
        let hub = files.get("docs/more/reference/modules.md").unwrap();
        assert!(
            hub.contains("[Box](../../../box/docs/more/reference/modules/box.md) and [Tooling](modules/tooling.md)."),
            "{hub}"
        );
        let tooling = files.get("docs/more/reference/modules/tooling.md").unwrap();
        assert!(tooling.starts_with("<!--"), "{tooling}");
        assert!(tooling.contains("# Tooling Modules"), "{tooling}");
        assert!(
            tooling.contains(
                "[Bundle](../../../../modules/tooling/bundle/docs/more/reference/modules/tooling/bundle.md)"
            ),
            "{tooling}"
        );
    }

    #[test]
    fn a_domain_of_fewer_modules_than_a_group_has_no_hub() {
        use std::path::PathBuf;
        let pages = [(
            "bundle".to_string(),
            Some("tooling".to_string()),
            PathBuf::from("modules/tooling/bundle/docs/more/reference/modules/tooling/bundle.md"),
        )];
        let files = super::commands::hub(&pages, 2).unwrap();
        assert!(
            files
                .get("docs/more/reference/modules/tooling.md")
                .is_none()
        );
        let hub = files.get("docs/more/reference/modules.md").unwrap();
        assert!(
            hub.contains(
                "[Bundle](../../../modules/tooling/bundle/docs/more/reference/modules/tooling/bundle.md)"
            ),
            "{hub}"
        );
    }

    #[test]
    fn links_climb_to_the_folder_they_share() {
        assert_eq!(
            super::relative(
                Path::new("a/docs/more/reference/commands/env.md"),
                Path::new("a/docs/config/specs/env.md")
            ),
            "../../../config/specs/env.md"
        );
        assert_eq!(
            super::relative(Path::new("docs/x.md"), Path::new("b/docs/y.md")),
            "../b/docs/y.md"
        );
        assert_eq!(super::title("cloud-account"), "Cloud Account");
        assert_eq!(super::title("spec"), "Specification");
        assert_eq!(super::title("mcp"), "MCP");
        assert_eq!(super::title("vm-image"), "VM Image");
    }
}
