//! Follow out-of-line module cfgs before classifying a source as production.
use super::*;

pub(super) fn production(parsed: &BTreeMap<PathBuf, syn::File>) -> BTreeSet<PathBuf> {
    let mut edges = BTreeMap::new();
    let mut targets = BTreeSet::new();
    for (path, file) in parsed {
        let parent = path.parent().unwrap_or(Path::new(""));
        let stem = path.file_stem().unwrap_or_default();
        let folder = if ["lib", "main", "mod", "build"]
            .iter()
            .any(|name| stem == *name)
        {
            parent.to_path_buf()
        } else {
            parent.join(stem)
        };
        let mut links = Vec::new();
        modules(&file.items, parent, &folder, true, parsed, &mut links);
        targets.extend(links.iter().map(|(target, _)| target.clone()));
        edges.insert(path.clone(), links);
    }
    let mut pending: Vec<_> = parsed
        .keys()
        .filter(|path| !targets.contains(*path))
        .cloned()
        .collect();
    let mut reached = BTreeSet::new();
    while let Some(path) = pending.pop() {
        if !reached.insert(path.clone()) {
            continue;
        }
        if let Some(links) = edges.get(&path) {
            pending.extend(
                links
                    .iter()
                    .filter(|(_, active)| *active)
                    .map(|(path, _)| path.clone()),
            );
        }
    }
    reached
}

fn modules(
    items: &[syn::Item],
    parent: &Path,
    folder: &Path,
    active: bool,
    parsed: &BTreeMap<PathBuf, syn::File>,
    links: &mut Vec<(PathBuf, bool)>,
) {
    for item in items {
        let syn::Item::Mod(module) = item else {
            continue;
        };
        let active = active && !crate::rust_source::test_only(&module.attrs);
        if let Some((_, items)) = &module.content {
            let nested = folder.join(module.ident.to_string());
            modules(items, &nested, &nested, active, parsed, links);
            continue;
        }
        let explicit = module.attrs.iter().find_map(|attribute| {
            if !attribute.path().is_ident("path") {
                return None;
            }
            let syn::Meta::NameValue(meta) = &attribute.meta else {
                return None;
            };
            let syn::Expr::Lit(literal) = &meta.value else {
                return None;
            };
            let syn::Lit::Str(path) = &literal.lit else {
                return None;
            };
            joined(parent, &path.value())
        });
        let name = module.ident.to_string();
        let choices = explicit.map_or_else(
            || {
                vec![
                    folder.join(format!("{name}.rs")),
                    folder.join(name).join("mod.rs"),
                ]
            },
            |path| vec![path],
        );
        for path in choices.into_iter().filter(|path| parsed.contains_key(path)) {
            links.push((path, active));
        }
    }
}
