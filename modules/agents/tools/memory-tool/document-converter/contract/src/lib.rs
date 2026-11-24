//! What Link Document Converter, `lnk-document-converter`, answers: the text of a
//! PDF or a Word document. Its caller runs `lnk-document-converter text
//! <pdf|docx>` as a child, the document on its stdin, and reads one
//! [`Extracted`] on its stdout.

pub mod cli;

pub use cli::{Extracted, Format, Text, Unread};

/// The module, `lnk-<MODULE>`.
pub const MODULE: &str = "document-converter";

/// What it reads and keeps of a document, as `limits.toml` sets them.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Limits {
    /// The largest document read, in bytes.
    pub document_max_bytes: u64,
    /// The most text kept of one, in bytes.
    pub text_max_bytes: usize,
}

/// [`Limits`], read once.
pub fn limits() -> &'static Limits {
    static LIMITS: std::sync::LazyLock<Limits> = std::sync::LazyLock::new(|| {
        toml::from_str(include_str!("limits.toml")).expect("the document converter's limits.toml")
    });
    &LIMITS
}

impl Format {
    /// The format of `path`, by its extension; none for one read as text.
    pub fn of(path: &str) -> Option<Format> {
        let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
        // Its word on the command line is its usual extension, as its
        // declaration (`types.toml`) writes it.
        <Format as ::link_actions::value::Words>::from_word(&extension)
    }
}

/// The command line asking the text of a document of `format`.
pub fn text_argv(format: Format) -> Vec<String> {
    cli::TextShowArgs { format }.argv()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_is_known_by_its_name() {
        assert_eq!(Format::of("memory/Taxes 2025.PDF"), Some(Format::Pdf));
        assert_eq!(Format::of("memory/letter.docx"), Some(Format::Docx));
        assert_eq!(Format::of("memory/letter.doc"), None);
        assert_eq!(Format::of("memory/notes"), None);
    }

    #[test]
    fn its_limits_read() {
        let limits = limits();
        assert!(limits.text_max_bytes > 0);
        // What's kept of a document is never more than the document.
        assert!(limits.text_max_bytes as u64 <= limits.document_max_bytes);
    }

    /// The line Link Memory runs it with, from the declaration.
    #[test]
    fn its_line_names_the_format() {
        assert_eq!(text_argv(Format::Pdf), ["text", "show", "pdf"]);
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
