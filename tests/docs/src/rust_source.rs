//! Source checks share the same conservative production/test cfg boundary.

mod lines;
pub use lines::{Lines, lines};

pub fn test_only(attrs: &[syn::Attribute]) -> bool {
    fn enabled(meta: &syn::Meta) -> Option<bool> {
        use syn::parse::Parser;
        match meta {
            syn::Meta::Path(path) if path.is_ident("test") => Some(false),
            syn::Meta::List(list) => {
                let args =
                    syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated
                        .parse2(list.tokens.clone())
                        .ok()?;
                let values: Vec<_> = args.iter().map(enabled).collect();
                if list.path.is_ident("not") && values.len() == 1 {
                    values[0].map(|v| !v)
                } else if list.path.is_ident("all") {
                    if values.contains(&Some(false)) {
                        Some(false)
                    } else if values.iter().all(|v| *v == Some(true)) {
                        Some(true)
                    } else {
                        None
                    }
                } else if list.path.is_ident("any") {
                    if values.contains(&Some(true)) {
                        Some(true)
                    } else if values.iter().all(|v| *v == Some(false)) {
                        Some(false)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    }
    attrs.iter().any(|a| {
        a.path().is_ident("test")
            || a.path().is_ident("cfg")
                && a.parse_args::<syn::Meta>().ok().as_ref().and_then(enabled) == Some(false)
    })
}
