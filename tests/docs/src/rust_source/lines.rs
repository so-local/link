//! Count source lines through Rust syntax, keeping test-only items separate.

use syn::{spanned::Spanned, visit::Visit};

#[derive(Debug, Default, Clone, Copy)]
pub struct Lines {
    pub production: usize,
    pub tests: usize,
    /// Nonblank source lines, including inline tests, for the existing ratio.
    pub source: usize,
}

pub fn lines(text: &str, test_file: bool) -> Result<Lines, String> {
    let parsed = syn::parse_file(text).map_err(|error| error.to_string())?;
    let source: Vec<_> = text.lines().collect();
    let mut marked = vec![test_file || super::test_only(&parsed.attrs); source.len()];
    struct Tests<'a>(&'a mut [bool]);
    impl Tests<'_> {
        fn mark(&mut self, node: &impl Spanned) {
            let span = node.span();
            let first = span.start().line.saturating_sub(1);
            let last = span.end().line.min(self.0.len());
            for line in &mut self.0[first.min(last)..last] {
                *line = true;
            }
        }
    }
    impl<'ast> Visit<'ast> for Tests<'_> {
        fn visit_item(&mut self, item: &'ast syn::Item) {
            let attrs: &[syn::Attribute] = match item {
                syn::Item::Const(v) => &v.attrs,
                syn::Item::Enum(v) => &v.attrs,
                syn::Item::ExternCrate(v) => &v.attrs,
                syn::Item::Fn(v) => &v.attrs,
                syn::Item::ForeignMod(v) => &v.attrs,
                syn::Item::Impl(v) => &v.attrs,
                syn::Item::Macro(v) => &v.attrs,
                syn::Item::Mod(v) => &v.attrs,
                syn::Item::Static(v) => &v.attrs,
                syn::Item::Struct(v) => &v.attrs,
                syn::Item::Trait(v) => &v.attrs,
                syn::Item::TraitAlias(v) => &v.attrs,
                syn::Item::Type(v) => &v.attrs,
                syn::Item::Union(v) => &v.attrs,
                syn::Item::Use(v) => &v.attrs,
                _ => &[],
            };
            if super::test_only(attrs) {
                self.mark(item);
            } else {
                syn::visit::visit_item(self, item);
            }
        }
    }
    Tests(&mut marked).visit_file(&parsed);
    let mut found = Lines::default();
    for (line, test) in source.iter().zip(marked) {
        if line.trim().is_empty() {
            continue;
        }
        if test {
            found.tests += 1;
        } else {
            found.production += 1;
        }
    }
    // A crate-level explanation alone is not a one-line implementation.
    if parsed.items.is_empty() {
        found.production = 0;
    }
    found.source = if parsed.items.is_empty() {
        0
    } else {
        source.iter().filter(|line| !line.trim().is_empty()).count()
    };
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_test_text_and_test_only_modules_are_not_production() {
        let source = "pub fn value() {}\n\n#[cfg(test)]\nmod tests {\n    const TEXT: &str = \"pub fn not_code() {}\";\n    #[test]\n    fn a_test() {}\n}\n";
        let found = lines(source, false).unwrap();
        assert_eq!(found.production, 1);
        assert_eq!(found.tests, 6);
        assert_eq!(lines(source, true).unwrap().production, 0);
        assert_eq!(lines("//! Test program.\n", false).unwrap().source, 0);
    }

    #[test]
    fn unsupported_source_keeps_its_parse_error() {
        assert!(lines("pub fn broken(", false).is_err());
    }
}
