//! Receipts for text read from one observed snapshot. A receipt identifies
//! the source bytes and the selected text separately from its presentation.

use anyhow::ensure;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentDigest {
    pub sha256: String,
    pub bytes: u64,
}

impl ContentDigest {
    pub fn of(bytes: &[u8]) -> Self {
        Self {
            sha256: link_digest::sha256(bytes),
            bytes: bytes.len() as u64,
        }
    }

    pub fn matches(&self, bytes: &[u8]) -> bool {
        self.bytes == bytes.len() as u64 && self.sha256 == link_digest::sha256(bytes)
    }

    fn check(&self) -> anyhow::Result<()> {
        ensure!(
            self.sha256.len() == 64
                && self
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "a content digest is a lowercase SHA-256"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReadSnapshot {
    Whole {
        content: ContentDigest,
    },
    Prefix {
        content: ContentDigest,
        total_bytes: Option<u64>,
    },
}

impl ReadSnapshot {
    pub fn content(&self) -> &ContentDigest {
        match self {
            Self::Whole { content } | Self::Prefix { content, .. } => content,
        }
    }
}

/// Byte offsets in the observed source snapshot, with an exclusive end.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ByteSpan {
    pub start: u64,
    pub end: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextTransform {
    Lines,
    /// Each line after its number, as a file tool's read gives it.
    Numbered,
    LossyUtf8,
    Trim,
}

/// `text_prefix_bytes` indexes the returned text's UTF-8 bytes. The
/// suffix is exactly `text`; a tool's pagination label is only a prefix.
/// This describes what a reader returned, not what a later model received.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadReceipt {
    pub path: String,
    pub source: ReadSnapshot,
    pub span: ByteSpan,
    pub text: ContentDigest,
    pub text_prefix_bytes: u64,
    pub transformations: Vec<TextTransform>,
}

impl ReadReceipt {
    /// Refuses inconsistent ranges, digests, and presentation boundaries.
    pub fn validate(&self, returned: &str) -> anyhow::Result<()> {
        ensure!(!self.path.is_empty(), "a read receipt names its source");
        self.source.content().check()?;
        self.text.check()?;
        ensure!(
            self.span.start <= self.span.end && self.span.end <= self.source.content().bytes,
            "a read span lies inside its observed snapshot"
        );
        if let ReadSnapshot::Prefix {
            content,
            total_bytes: Some(total),
        } = &self.source
        {
            ensure!(content.bytes <= *total, "a source prefix exceeds its total");
        }
        let prefix = usize::try_from(self.text_prefix_bytes)?;
        let selected = returned
            .get(prefix..)
            .ok_or_else(|| anyhow::anyhow!("a read prefix ends at a UTF-8 boundary"))?;
        ensure!(
            self.text.matches(selected.as_bytes()),
            "a read receipt does not match its returned text"
        );
        if self.transformations.is_empty() {
            ensure!(
                self.text.bytes == self.span.end - self.span.start,
                "an unchanged read has its selected source's length"
            );
            if self.covers_source() {
                ensure!(
                    self.text == *self.source.content(),
                    "an unchanged whole read has its source's digest"
                );
            }
        }
        Ok(())
    }

    /// Full source coverage, before any later delivery limit. Callers also
    /// validate the returned text and account for model-facing clipping.
    pub fn covers_source(&self) -> bool {
        matches!(self.source, ReadSnapshot::Whole { .. })
            && self.span.start == 0
            && self.span.end == self.source.content().bytes
            && !self.transformations.contains(&TextTransform::LossyUtf8)
    }
}

#[cfg(test)]
mod tests;
