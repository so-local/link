//! What counts as a translation: a page's `<page>.<language>.md` beside
//! its English `<page>.md`, its language one of `languages.toml`'s.

use std::sync::LazyLock;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Languages {
    languages: Vec<String>,
}

static LANGUAGES: LazyLock<Vec<String>> = LazyLock::new(|| {
    toml::from_str::<Languages>(include_str!("../languages.toml"))
        .expect("languages.toml lists the languages")
        .languages
});

/// The languages a page may be translated into. English, the pages' own
/// language, is never one.
pub fn languages() -> &'static [String] {
    &LANGUAGES
}

/// Whether `name` is a translation's language: one of [`languages`],
/// alone or with a region in capitals (`pt-BR`).
pub fn is_language(name: &str) -> bool {
    let (base, region) = match name.split_once('-') {
        Some((base, region)) => (base, Some(region)),
        None => (name, None),
    };
    languages().iter().any(|language| language == base)
        && region.is_none_or(|region| {
            region.len() == 2 && region.bytes().all(|b| b.is_ascii_uppercase())
        })
}

/// A translation's English page name and its language, from its file
/// name: `guide.es.md` is `guide.md` in `es`; `foo.js.md` is no
/// translation, and neither is anything named for English.
pub fn translation(name: &str) -> Option<(String, &str)> {
    let (page, language) = name.strip_suffix(".md")?.rsplit_once('.')?;
    is_language(language).then(|| (format!("{page}.md"), language))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listed_language_alone_or_with_a_region_in_capitals_is_a_translation() {
        assert_eq!(translation("guide.es.md"), Some(("guide.md".into(), "es")));
        assert_eq!(
            translation("guide.pt-BR.md"),
            Some(("guide.md".into(), "pt-BR"))
        );
        assert_eq!(translation("guide.pt-br.md"), None);
    }

    #[test]
    fn english_and_unlisted_suffixes_are_pages() {
        assert_eq!(translation("guide.en.md"), None);
        assert_eq!(translation("foo.js.md"), None);
        assert_eq!(translation("guide.md"), None);
        assert!(!languages().iter().any(|language| language == "en"));
    }
}
