//! `lnk-document-converter`: Link Document Converter, the text of PDFs and Word
//! documents, for Link Memory's index:
//!
//! ```sh
//! lnk-document-converter text <pdf|docx> < document > answer
//! ```
//!
//! The document on its stdin, its text or why there's none on its stdout,
//! as `link_document_converter_contract::Extracted`. Its caller runs it as a child
//! holding no secret, with limits on its memory and time; anything a
//! parser prints goes to stderr, never into the answer. A module of its
//! own, so a build or an install without it compiles and includes no
//! document parser.

use std::io::Read;

use anyhow::{Context, Result, ensure};
use link_document_converter_contract::{Extracted, Format, limits};

use link_document_converter::cli::{self, Command};
use link_format::{Done, Out, Product, Prose, Report, Sentence, cmd, phrase, sentence};

/// The most a Word document's text part is, unpacked: a zip that packs
/// more is refused, not unpacked.
const XML_MAX: u64 = 64 << 20;

fn main() -> std::process::ExitCode {
    // A document is private: no core dump of it, no other process of this
    // user reading it.
    link_secretive::process::guard();
    link_core::describe_component!();
    link_settings::output::run(|out| try_main(out).map_err(failed))
}

/// A failure as a report: a usage error says where to read the usage,
/// anything else keeps its words.
fn failed(err: anyhow::Error) -> Report {
    let program = Product::new("lnk-document-converter");
    Report::error(sentence!("{} stopped.", program))
        .why(Prose::from(Sentence::adopt(&format!("{err:#}"))))
        .next(cmd!("lnk agent list"), phrase!("see which agents run"))
        .cause(err)
}

fn try_main(_out: &mut Out) -> Result<Done> {
    match cli::parse().command.command {
        Command::TextShow(given) => self::text(given.format)?,
    }
    Ok(Done::shown())
}

/// `text <pdf|docx>`: the document on stdin, its text or why there's
/// none on stdout, as [`Extracted`].
fn text(format: Format) -> Result<()> {
    // The answer's own descriptor; stdout, from here, is stderr.
    // SAFETY: dup and dup2 on this process's own descriptors, before
    // anything else writes to them.
    let answer = unsafe {
        let answer = libc::dup(1);
        ensure!(
            answer >= 0 && libc::dup2(2, 1) >= 0,
            "{}",
            std::io::Error::last_os_error()
        );
        <std::fs::File as std::os::fd::FromRawFd>::from_raw_fd(answer)
    };
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(limits().document_max_bytes + 1)
        .read_to_end(&mut bytes)
        .context("reading the document")?;
    let extracted = match parse(format, &bytes) {
        Ok(text) => Extracted::Text(link_document_converter_contract::Text { text }),
        Err(why) => Extracted::Unread(link_document_converter_contract::Unread {
            unread: format!("{why:#}"),
        }),
    };
    serde_json::to_writer(answer, &extracted).context("answering")?;
    Ok(())
}

/// The text of `bytes`, a document of `format`, at most `text-max-bytes` ([`limits`]).
fn parse(format: Format, bytes: &[u8]) -> Result<String> {
    ensure!(
        bytes.len() as u64 <= limits().document_max_bytes,
        "it's over {} MiB",
        limits().document_max_bytes >> 20
    );
    let text = match format {
        Format::Pdf => pdf(bytes),
        Format::Docx => word(bytes),
    }?;
    let text = text.trim();
    ensure!(!text.is_empty(), "it holds no text (a scan has none)");
    Ok(text[..text.floor_char_boundary(limits().text_max_bytes)].to_string())
}

/// A PDF's text, its pages in order.
fn pdf(bytes: &[u8]) -> Result<String> {
    let document = lopdf::Document::load_mem(bytes)
        .map_err(|err| anyhow::anyhow!("not a PDF it reads: {err}"))?;
    let pages: Vec<u32> = document.get_pages().keys().copied().collect();
    document
        .extract_text(&pages)
        .map_err(|err| anyhow::anyhow!("its text can't be read: {err}"))
}

/// A Word document's text: its paragraphs (`w:p`) a line each, from the
/// runs' text (`w:t`), tabs and breaks kept.
fn word(bytes: &[u8]) -> Result<String> {
    let mut zip =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).context("not a Word document (a zip)")?;
    let mut part = zip
        .by_name("word/document.xml")
        .context("not a Word document: it has no word/document.xml")?;
    let mut xml = String::new();
    (&mut part)
        .take(XML_MAX + 1)
        .read_to_string(&mut xml)
        .context("reading its text")?;
    ensure!(
        xml.len() as u64 <= XML_MAX,
        "its text is over {} MiB unpacked",
        XML_MAX >> 20
    );
    Ok(word_text(&xml))
}

/// The text of a Word document's `document.xml`.
fn word_text(xml: &str) -> String {
    let mut out = String::new();
    let mut rest = xml;
    let mut in_text = false;
    while let Some(open) = rest.find('<') {
        if in_text {
            out.push_str(&unescape(&rest[..open]));
        }
        let Some(close) = rest[open..].find('>') else {
            break;
        };
        let tag = &rest[open + 1..open + close];
        let name = tag
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("");
        let closing = tag.starts_with('/');
        let empty = tag.ends_with('/');
        match (name, closing) {
            ("w:t", false) => in_text = !empty,
            ("w:t", true) => in_text = false,
            ("w:p", true) => out.push('\n'),
            ("w:tab", false) => out.push('\t'),
            ("w:br" | "w:cr", false) => out.push('\n'),
            _ => {}
        }
        rest = &rest[open + close + 1..];
        if out.len() > limits().text_max_bytes * 2 {
            break;
        }
    }
    out
}

/// XML's escapes: its five names, and numbers.
fn unescape(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let Some(end) = after.find(';').filter(|end| *end <= 10) else {
            out.push('&');
            rest = after;
            continue;
        };
        let entity = &after[..end];
        let char = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .map(|hex| u32::from_str_radix(hex, 16))
                .or_else(|| entity.strip_prefix('#').map(str::parse))
                .and_then(Result::ok)
                .and_then(char::from_u32),
        };
        match char {
            Some(c) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_documents_paragraphs_are_its_lines() {
        let xml = r#"<?xml version="1.0"?><w:document><w:body>
            <w:p><w:r><w:t>Refund:</w:t></w:r><w:r><w:tab/><w:t xml:space="preserve"> 42 &amp; more</w:t></w:r></w:p>
            <w:p><w:r><w:t>Due &#x41;pril&#33;</w:t><w:br/><w:t/></w:r></w:p>
        </w:body></w:document>"#;
        assert_eq!(word_text(xml), "Refund:\t 42 & more\nDue April!\n\n");
    }

    #[test]
    fn what_isnt_a_document_has_no_text() {
        assert!(parse(Format::Pdf, b"not a pdf").is_err());
        assert!(parse(Format::Docx, b"not a zip").is_err());
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
