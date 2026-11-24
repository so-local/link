//! Source fallback for serde maps/private owner types that hide struct metadata.
//! Unknown nested types and unsupported serde transformations fail closed.

use super::fields::{Binding, Fields};
use std::collections::{BTreeMap, BTreeSet};

fn type_name<T>() -> Result<&'static str, String> {
    std::any::type_name::<T>()
        .rsplit("::")
        .next()
        .ok_or_else(|| "missing type name".into())
}

fn field_name(field: &syn::Field) -> Result<Option<String>, String> {
    let mut name = field
        .ident
        .as_ref()
        .ok_or("a reference struct needs named fields")?
        .to_string();
    let mut omitted = false;
    for attr in field.attrs.iter().filter(|a| a.path().is_ident("serde")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                name = meta.value()?.parse::<syn::LitStr>()?.value();
            } else if meta.path.is_ident("with") || meta.path.is_ident("deserialize_with") {
                return Err(meta.error("custom field reader needs an owning type binding"));
            } else if meta.path.is_ident("flatten") {
                // Only an explicitly arbitrary extension map can be omitted.
                let syn::Type::Path(path) = &field.ty else {
                    return Err(meta.error("flattened owner type needs a binding"));
                };
                let segment = path.path.segments.last().unwrap();
                let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
                    return Err(meta.error("flattened owner type needs a binding"));
                };
                let arbitrary = segment.ident == "BTreeMap"
                    && arguments
                        .args
                        .last()
                        .is_some_and(|argument| match argument {
                            syn::GenericArgument::Type(syn::Type::Path(path)) => path
                                .path
                                .segments
                                .last()
                                .is_some_and(|s| s.ident == "Value"),
                            _ => false,
                        });
                if !arbitrary {
                    return Err(meta.error("flattened owner type needs a binding"));
                }
                omitted = true;
            } else if meta.path.is_ident("skip") || meta.path.is_ident("skip_deserializing") {
                omitted = true;
            } else if meta.input.peek(syn::Token![=]) {
                let _ = meta.value()?.parse::<syn::Expr>()?;
            } else if meta.input.peek(syn::token::Paren) {
                return Err(meta.error("nested serde metadata needs explicit support"));
            }
            Ok(())
        })
        .map_err(|e| format!("{name}: serde attribute: {e}"))?;
    }
    Ok((!omitted).then_some(name))
}

/// Whether `name` is one of core's shared shapes, `pub use link_types::<name>`.
fn shared(file: &syn::File, name: &str) -> bool {
    file.items.iter().any(|item| match item {
        syn::Item::Use(item) => match &item.tree {
            syn::UseTree::Path(path) if path.ident == "link_types" => match &*path.tree {
                syn::UseTree::Name(used) => used.ident == name,
                syn::UseTree::Rename(used) => used.rename == name,
                _ => false,
            },
            _ => false,
        },
        _ => false,
    })
}

fn scalar(file: &syn::File, name: &str, depth: usize) -> bool {
    if depth > 8 {
        return false;
    }
    if [
        "String", "bool", "u8", "u16", "u32", "u64", "usize", "i32", "i64", "f32", "f64", "PathBuf",
    ]
    .contains(&name)
    {
        return true;
    }
    // Core's shared shapes (`link-types`) are each one value: an OS, a
    // user, a duration.
    if shared(file, name) {
        return true;
    }
    file.items
        .iter()
        .find_map(|item| match item {
            syn::Item::Enum(item) if item.ident == name => {
                let scalar_encoding = item
                    .attrs
                    .iter()
                    .filter(|a| a.path().is_ident("serde"))
                    .all(|attr| {
                        attr.parse_nested_meta(|meta| {
                            if ["tag", "content", "from", "try_from"]
                                .iter()
                                .any(|name| meta.path.is_ident(name))
                            {
                                return Err(
                                    meta.error("table or transformed enum needs an owning binding")
                                );
                            }
                            if meta.input.peek(syn::Token![=]) {
                                let _ = meta.value()?.parse::<syn::Expr>()?;
                            }
                            Ok(())
                        })
                        .is_ok()
                    });
                Some(
                    scalar_encoding
                        && item.variants.iter().all(|variant| match &variant.fields {
                            syn::Fields::Unit => true,
                            syn::Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                                match &fields.unnamed[0].ty {
                                    syn::Type::Path(path) => {
                                        path.path.segments.last().is_some_and(|segment| {
                                            scalar(file, &segment.ident.to_string(), depth + 1)
                                        })
                                    }
                                    _ => false,
                                }
                            }
                            _ => false,
                        }),
                )
            }
            _ => None,
        })
        .unwrap_or(false)
}

struct Graph<'a> {
    file: &'a syn::File,
    bindings: &'a BTreeMap<&'a str, Binding>,
    expanded: BTreeSet<String>,
    fields: Fields,
}
impl Graph<'_> {
    fn structure(&mut self, name: &str, path: &str, depth: usize) -> Result<(), String> {
        if depth > 12 || self.fields.len() > 256 {
            return Err("source field discovery exceeded its bound".into());
        }
        let item = self
            .file
            .items
            .iter()
            .find_map(|item| match item {
                syn::Item::Struct(item) if item.ident == name => Some(item),
                _ => None,
            })
            .ok_or_else(|| format!("owner source no longer defines {name}"))?;
        for attr in item.attrs.iter().filter(|a| a.path().is_ident("serde")) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename_all")
                    || meta.path.is_ident("from")
                    || meta.path.is_ident("try_from")
                {
                    return Err(meta.error("transformed owner type needs its own binding"));
                }
                if meta.input.peek(syn::Token![=]) {
                    let _ = meta.value()?.parse::<syn::Expr>()?;
                }
                Ok(())
            })
            .map_err(|e| e.to_string())?;
        }
        let entries = item
            .fields
            .iter()
            .filter_map(|field| match field_name(field) {
                Ok(Some(name)) => Some(Ok((name, &field.ty))),
                Ok(None) => None,
                Err(e) => Some(Err(e)),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut keys: Vec<_> = entries.iter().map(|(name, _)| name.clone()).collect();
        keys.sort();
        self.fields.insert(path.into(), keys);
        if self.expanded.insert(name.into()) {
            for (name, ty) in entries {
                self.follow(ty, &join(path, &name), depth + 1)?;
            }
        }
        Ok(())
    }
    fn follow(&mut self, ty: &syn::Type, path: &str, depth: usize) -> Result<(), String> {
        let syn::Type::Path(ty) = ty else {
            return Err(format!("{path}: owner field shape needs a binding"));
        };
        let segment = ty.path.segments.last().ok_or("missing field type")?;
        let name = segment.ident.to_string();
        if let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments {
            let inner = arguments
                .args
                .iter()
                .filter_map(|arg| match arg {
                    syn::GenericArgument::Type(ty) => Some(ty),
                    _ => None,
                })
                .next_back();
            if ["Option", "Vec", "Box", "BTreeMap"].contains(&name.as_str()) {
                return self.follow(
                    inner.ok_or("missing owner element type")?,
                    &if name == "BTreeMap" {
                        join(path, "*")
                    } else {
                        path.into()
                    },
                    depth + 1,
                );
            }
        }
        if let Some(binding) = self.bindings.get(name.as_str()) {
            for (relative, keys) in binding()? {
                self.fields.insert(join(path, &relative), keys);
            }
            return Ok(());
        }
        if scalar(self.file, &name, 0) {
            return Ok(());
        }
        self.structure(&name, path, depth)
    }
}
fn join(prefix: &str, suffix: &str) -> String {
    [prefix, suffix]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(".")
}

/// The tag and named fields of every variant of the actual owner enum.
pub fn tagged<T>(text: &str) -> Result<Fields, String> {
    let name = type_name::<T>()?;
    let file = syn::parse_file(text).map_err(|e| e.to_string())?;
    let item = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Enum(item) if item.ident == name => Some(item),
            _ => None,
        })
        .ok_or_else(|| format!("missing owner enum {name}"))?;
    let mut keys = Vec::new();
    for attr in item.attrs.iter().filter(|a| a.path().is_ident("serde")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("tag") {
                keys.push(meta.value()?.parse::<syn::LitStr>()?.value());
            } else if meta.path.is_ident("rename_all_fields") {
                return Err(meta.error("renamed variant fields need a binding"));
            } else if meta.input.peek(syn::Token![=]) {
                let _ = meta.value()?.parse::<syn::Expr>()?;
            }
            Ok(())
        })
        .map_err(|e| e.to_string())?;
    }
    if keys.is_empty() {
        return Err(format!("{name} no longer has an internal serde tag"));
    }
    let bindings = BTreeMap::new();
    let mut graph = Graph {
        file: &file,
        bindings: &bindings,
        expanded: BTreeSet::new(),
        fields: Fields::new(),
    };
    for variant in &item.variants {
        match &variant.fields {
            syn::Fields::Named(fields) => {
                for field in &fields.named {
                    if let Some(name) = field_name(field)? {
                        keys.push(name.clone());
                        graph.follow(&field.ty, &name, 0)?;
                    }
                }
            }
            syn::Fields::Unit => {}
            _ => {
                return Err(format!(
                    "{name}: tuple variant needs an owning shape binding"
                ));
            }
        }
    }
    keys.sort();
    keys.dedup();
    graph.fields.insert(String::new(), keys);
    Ok(graph.fields)
}
