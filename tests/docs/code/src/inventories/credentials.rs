//! Runtime credential categories must keep their owning explanation and code.

mod bindings;

use super::{Items, entry};
use link_core::uninstall::CredentialKind;
use link_docs_check::{Tree, md};
use std::collections::{BTreeMap, BTreeSet};
use syn::visit::Visit;

use crate::syntax::test_only;

#[derive(Default)]
struct Symbols(BTreeSet<String>);

impl<'ast> Visit<'ast> for Symbols {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        let attrs = match item {
            syn::Item::Fn(v) => &v.attrs,
            syn::Item::Struct(v) => &v.attrs,
            syn::Item::Enum(v) => &v.attrs,
            syn::Item::Type(v) => &v.attrs,
            syn::Item::Mod(v) => &v.attrs,
            syn::Item::Impl(v) => &v.attrs,
            _ => return syn::visit::visit_item(self, item),
        };
        if test_only(attrs) {
            return;
        }
        let ident = match item {
            syn::Item::Fn(v) => Some(&v.sig.ident),
            syn::Item::Struct(v) => Some(&v.ident),
            syn::Item::Enum(v) => Some(&v.ident),
            syn::Item::Type(v) => Some(&v.ident),
            _ => None,
        };
        if let Some(ident) = ident {
            self.0.insert(ident.to_string());
        }
        syn::visit::visit_item(self, item);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if !test_only(&item.attrs) {
            self.0.insert(item.sig.ident.to_string());
            syn::visit::visit_impl_item_fn(self, item);
        }
    }
}

fn symbols(text: &str) -> Result<BTreeSet<String>, String> {
    let file = syn::parse_file(text).map_err(|error| error.to_string())?;
    let mut symbols = Symbols::default();
    if !test_only(&file.attrs) {
        symbols.visit_file(&file);
    }
    Ok(symbols.0)
}

fn documentation<'a>(tree: &'a Tree, target: &str) -> Result<&'a str, String> {
    let (path, anchor) = target
        .split_once('#')
        .map_or((target, None), |(p, a)| (p, Some(a)));
    let text = tree
        .text(&tree.root.join(path))
        .ok_or_else(|| format!("credential documentation missing: {path}"))?;
    if anchor.is_some_and(|anchor| !md::anchors(text).contains(anchor)) {
        return Err(format!("credential documentation anchor missing: {target}"));
    }
    Ok(text)
}

pub(super) fn catalog(tree: &Tree) -> Result<Items, String> {
    let mut items = Items::new();
    let mut ids = BTreeSet::new();
    let mut labels = BTreeSet::new();
    implementations(tree)?;
    for kind in CredentialKind::ALL {
        if !ids.insert(kind.id()) || !labels.insert(kind.label()) {
            return Err(format!(
                "duplicate credential identifier or label: {}",
                kind.id()
            ));
        }
        documentation(tree, kind.documentation())?;
        entry(
            &mut items,
            format!("{} (`{}`)", kind.label(), kind.id()),
            &tree.root.join(kind.documentation()),
        );
    }
    Ok(items)
}

/// Check producers owned by this workspace. Credential categories remain a
/// shared wire contract even when their optional provider is released separately.
pub(super) fn implementations(tree: &Tree) -> Result<(), String> {
    let manifest: toml::Value = toml::from_str(
        tree.text(&tree.root.join("Cargo.toml"))
            .ok_or("workspace manifest missing")?,
    )
    .map_err(|error| error.to_string())?;
    let members = manifest["workspace"]["members"]
        .as_array()
        .ok_or("workspace members missing")?;
    let mut sources = BTreeMap::new();
    for kind in CredentialKind::ALL {
        let Some((relative, symbol)) = bindings::source(*kind) else {
            continue;
        };
        if !members
            .iter()
            .filter_map(|member| member.as_str())
            .any(|member| {
                relative
                    .strip_prefix(member)
                    .is_some_and(|rest| rest.starts_with('/'))
            })
        {
            continue;
        }
        let source = tree.root.join(relative);
        if !sources.contains_key(&source) {
            let text = tree.text(&source).ok_or_else(|| {
                format!("credential implementation missing: {}", source.display())
            })?;
            sources.insert(source.clone(), symbols(text)?);
        }
        if !sources[&source].contains(symbol) {
            return Err(format!(
                "{}: credential {} has no production symbol {}",
                tree.shown(&source),
                kind.id(),
                symbol
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_checks_real_explanations_and_implementations() {
        let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut tree = Tree::read(
            &here.join("../../.."),
            false,
            &here.join("../large-files.toml"),
        );
        assert!(catalog(&tree).is_ok());
        let kind = CredentialKind::InstallationSealingKey;
        let (source, _) = bindings::source(kind).unwrap();
        let source = tree.root.join(source);
        let original = tree.files[&source].clone();
        tree.files.insert(
            source.clone(),
            original.replace("pub struct Recipient", "pub struct RenamedRecipient"),
        );
        assert!(catalog(&tree).unwrap_err().contains("no production symbol"));
        tree.files.insert(source, original);
        let (page, _) = kind.documentation().split_once('#').unwrap();
        let page = tree.root.join(page);
        let original = tree.files[&page].clone();
        tree.files.insert(
            page,
            original.replace("## The sealing key", "## A renamed explanation"),
        );
        assert!(catalog(&tree).unwrap_err().contains("anchor missing"));
    }

    #[test]
    fn removed_and_test_only_credential_producers_cannot_satisfy_the_binding() {
        let source = r#"
            struct Holder;
            impl Holder { fn held() {} }
            fn mint() {}
            #[cfg(test)] mod tests { fn fake() {} }
            #[test] fn also_fake() {}
            #[cfg(test)] impl Holder { fn fake_method() {} }
            #[cfg(all(test, unix))] fn nested_test_only() {}
            #[cfg(any(test, all(test, unix)))] fn all_branches_test_only() {}
            #[cfg(not(test))] fn shipped() {}
            #[cfg(any(test, unix))] fn can_ship() {}
        "#;
        let original = symbols(source).unwrap();
        assert_eq!(
            original,
            BTreeSet::from(["Holder", "held", "mint", "shipped", "can_ship"].map(String::from))
        );
        assert!(
            !symbols(&source.replace("fn mint() {}", ""))
                .unwrap()
                .contains("mint")
        );
        assert!(symbols("fn broken(").is_err());
    }
}
