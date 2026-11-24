//! Keep specification field inventories and checked defaults with their owners.

mod catalog;
pub mod kinds;
mod source;

/// Composed specification definitions already bound to independent owning readers.
/// The generated catalogue exposes these logical homes to the text-only tree check.
pub fn composed_pages() -> Result<Vec<&'static str>, String> {
    Ok(catalog::references()?
        .into_iter()
        .filter(|reference| reference.page.contains("/docs/config/specs/"))
        .map(|reference| reference.page)
        .collect())
}

/// The serde probe `link_spec` owns, strict here: a reference lists every
/// table, so a value read by hand needs an owning type binding.
mod fields {
    use std::collections::BTreeMap;

    pub use link_spec::probe::{Binding, Fields};

    pub fn discover<T: serde::de::DeserializeOwned>(
        overrides: &BTreeMap<String, Binding>,
    ) -> Result<Fields, String> {
        let found = link_spec::probe::discover::<T>(overrides, link_spec::probe::Expand::Once)?;
        match found.opaque.iter().next() {
            Some((at, why)) => Err(format!(
                "{} at {at}: {why}, so it needs an owning type binding",
                std::any::type_name::<T>()
            )),
            None => Ok(found.fields),
        }
    }
}

use link_docs_check::navigation::Update;
use link_docs_check::{Finding, Tree, fail};

/// Stale or missing checked reference summaries, including unbound spec pages.
pub fn check(tree: &Tree) -> Vec<Finding> {
    let mut findings = scope(tree);
    findings.extend(match updates(tree) {
        Ok(updates) => updates
            .into_iter()
            .map(|update| {
                fail(format!(
                    "{}: checked spec fields or defaults are missing or stale: run \
                     lnk-docs-inventories --write",
                    tree.shown(&update.path)
                ))
            })
            .collect(),
        Err(error) => vec![fail(error)],
    });
    findings
}

/// Update each specification's marked, source-backed summary.
pub fn updates(tree: &Tree) -> Result<Vec<Update>, String> {
    let mut updates = Vec::new();
    for reference in catalog::references()? {
        let path = tree.root.join(reference.page);
        let old = tree
            .text(&path)
            .ok_or_else(|| format!("{}: missing spec reference", reference.page))?;
        let block = reference.render();
        let text = replace(old, &block)?;
        if text != old {
            updates.push(Update { path, text });
        }
    }
    Ok(updates)
}

fn scope(tree: &Tree) -> Vec<Finding> {
    let Ok(references) = catalog::references() else {
        return Vec::new();
    };
    let owners: std::collections::BTreeSet<_> = references.iter().map(|r| r.owner).collect();
    tree.pages.values().flatten().filter_map(|path| {
        let shown = tree.shown(path);
        if !shown.contains("/docs/config/specs/") || path.ends_with("decisions.md") { return None; }
        if references.iter().any(|r| shown == r.page) { return None; }
        let text = tree.text(path)?;
        // A page written from its module's declarations is bound to them.
        if link_docs_check::generated_reference(text) { return None; }
        let declared = text.lines().find_map(|line| line.trim().strip_prefix("<!-- spec-reference: ").and_then(|line| line.strip_suffix(" -->")));
        let valid = match declared {
            Some("proposed") => {
                let opening = text.split("\n## ").next().unwrap_or(text).to_lowercase();
                (opening.contains("proposed") || opening.contains("not built yet"))
                    && !toml_fence(text)
            }
            // A Rust owner the catalog binds, or a kind its module's
            // declarations own, whose page link-actions-surfaces writes.
            Some(owner) => {
                owners.contains(owner)
                    || kinds::kinds().iter().any(|k| k.kind.kind == owner)
            }
            None => false,
        };
        (!valid).then(|| fail(format!("{shown}: bind the reference to its released owning type or visibly declare the proposed, unimplemented format")))
    }).collect()
}

/// Recognize fenced blocks without interpreting nested-looking text as a fence.
fn toml_fence(text: &str) -> bool {
    let mut open: Option<(char, usize)> = None;
    for line in text.lines() {
        let indent = line.chars().take_while(|c| *c == ' ').count();
        if indent > 3 {
            continue;
        }
        let line = &line[indent..];
        let Some(marker @ ('`' | '~')) = line.chars().next() else {
            continue;
        };
        let width = line.chars().take_while(|c| *c == marker).count();
        if let Some((opened, length)) = open {
            if marker == opened && width >= length && line[width..].trim().is_empty() {
                open = None;
            }
            continue;
        }
        if width < 3 {
            continue;
        }
        let info = line[width..].trim();
        if marker == '`' && info.contains('`') {
            continue;
        }
        if info
            .split_whitespace()
            .next()
            .is_some_and(|lang| lang.eq_ignore_ascii_case("toml"))
        {
            return true;
        }
        open = Some((marker, width));
    }
    false
}

fn replace(text: &str, block: &str) -> Result<String, String> {
    const START: &str = "<!-- spec-reference:start -->";
    const END: &str = "<!-- spec-reference:end -->";
    let starts: Vec<_> = text.match_indices(START).collect();
    let ends: Vec<_> = text.match_indices(END).collect();
    match (starts.as_slice(), ends.as_slice()) {
        ([(start, _)], [(end, _)]) if end > start => Ok(format!(
            "{}{}{}",
            &text[..*start],
            block,
            &text[end + END.len()..]
        )),
        ([], []) => Ok(format!("{}\n\n{block}\n", text.trim_end())),
        _ => Err("spec reference markers must be one unique ordered pair".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn summary_replacement_preserves_implementation_explanations() {
        let text = "Before\n<!-- spec-reference:start -->old<!-- spec-reference:end -->\nAfter";
        assert_eq!(
            replace(text, "replacement").unwrap(),
            "Before\nreplacement\nAfter"
        );
        assert!(replace("<!-- spec-reference:start -->", "").is_err());
    }
    #[test]
    fn every_binding_can_discover_its_own_fields() {
        catalog::references().unwrap();
    }
    #[test]
    fn missing_misspelled_stale_fields_and_incorrect_defaults_fail_the_check() {
        let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut tree = Tree::read(
            &here.join("../../.."),
            true,
            &here.join("../large-files.toml"),
        );
        for update in updates(&tree).unwrap() {
            tree.files.insert(update.path, update.text);
        }
        let findings = check(&tree);
        assert!(findings.is_empty(), "{findings:?}");
        let path = tree
            .root
            .join("modules/agents/agent/contract/identity/docs/config/specs/access.md");
        let original = tree.files[&path].clone();
        for (old, wrong) in [
            ("root: member, message", "root: membr, message"),
            ("message = []", "message = [\"read\"]"),
        ] {
            assert!(
                original.contains(old),
                "fixture assertion {old} must exercise its target"
            );
            tree.files
                .insert(path.clone(), original.replace(old, wrong));
            let findings = check(&tree);
            assert_eq!(findings.len(), 1, "{old} -> {wrong}: {findings:?}");
            assert!(findings[0].text.contains("access.md"));
        }
    }

    #[test]
    fn a_new_reference_needs_a_real_owner_or_a_visible_proposal() {
        let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut tree = Tree::read(
            &here.join("../../.."),
            true,
            &here.join("../large-files.toml"),
        );
        for update in updates(&tree).unwrap() {
            tree.files.insert(update.path, update.text);
        }
        let path = tree.root.join("modules/example/docs/config/specs/new.md");
        tree.pages
            .insert("/config/specs/new".into(), vec![path.clone()]);
        tree.files.insert(
            path.clone(),
            "# New Spec\n\nAn undocumented implementation.\n".into(),
        );
        let findings = scope(&tree);
        assert_eq!(findings.len(), 1, "{findings:?}");
        tree.files.insert(path, "# New Spec\n\nThis is a proposed format, not built yet.\n\n<!-- spec-reference: proposed -->\n".into());
        assert!(scope(&tree).is_empty());
    }
    #[test]
    fn proposals_reject_tilde_toml_fences_and_ignore_fences_inside_other_blocks() {
        assert!(toml_fence("~~~toml\nname = 'example'\n~~~"));
        assert!(toml_fence("````toml\nname = 'example'\n````"));
        assert!(!toml_fence("~~~~text\n~~~toml\n~~~~"));
        assert!(!toml_fence("````text\n```toml\n```\n````"));
        assert!(!toml_fence("The literal ```toml is only mentioned here."));
    }
}
