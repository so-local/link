//! Bounded instruction text, retaining source identity before trimming.

use link_files::reading::{ByteSpan, ContentDigest, ReadReceipt, ReadSnapshot, TextTransform};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstructionRead {
    pub text: String,
    pub receipt: ReadReceipt,
}

impl InstructionRead {
    pub(crate) fn from_bytes(
        path: &str,
        source: ReadSnapshot,
        selected: &[u8],
        lossy: bool,
    ) -> anyhow::Result<Option<Self>> {
        let decoded = match lossy {
            true => String::from_utf8_lossy(selected),
            false => std::borrow::Cow::Borrowed(std::str::from_utf8(selected)?),
        };
        let text = decoded.trim();
        if text.is_empty() {
            return Ok(None);
        }
        let mut transformations = Vec::new();
        if matches!(decoded, std::borrow::Cow::Owned(_)) {
            transformations.push(TextTransform::LossyUtf8);
        }
        if text != decoded {
            transformations.push(TextTransform::Trim);
        }
        let receipt = ReadReceipt {
            path: path.into(),
            source,
            span: ByteSpan {
                start: 0,
                end: selected.len() as u64,
            },
            text: ContentDigest::of(text.as_bytes()),
            text_prefix_bytes: 0,
            transformations,
        };
        receipt.validate(text)?;
        Ok(Some(Self {
            text: text.into(),
            receipt,
        }))
    }

    /// A local read takes one extra byte, so reaching its bound cannot
    /// be mistaken for EOF. No full digest is claimed for a prefix.
    pub(crate) fn bounded(
        path: &str,
        observed: &[u8],
        limit: usize,
        lossy: bool,
    ) -> anyhow::Result<Option<Self>> {
        let selected = &observed[..observed.len().min(limit)];
        let content = ContentDigest::of(selected);
        let source = match observed.len() <= limit {
            true => ReadSnapshot::Whole { content },
            false => ReadSnapshot::Prefix {
                content,
                total_bytes: None,
            },
        };
        Self::from_bytes(path, source, selected, lossy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_instruction_receipts_keep_trim_and_prefix_separate() {
        let read = InstructionRead::bounded("skills/a/SKILL.md", b" body \n", 20, false)
            .unwrap()
            .unwrap();
        assert_eq!(read.text, "body");
        assert!(read.receipt.covers_source());
        assert_eq!(read.receipt.transformations, [TextTransform::Trim]);
        let cut = InstructionRead::bounded("skills/a/SKILL.md", b"123456", 5, false)
            .unwrap()
            .unwrap();
        assert_eq!(cut.text, "12345");
        assert!(!cut.receipt.covers_source());
        assert!(matches!(cut.receipt.source, ReadSnapshot::Prefix { .. }));
        let lossy = InstructionRead::bounded("skills/a/SKILL.md", b"a\xffb", 20, true)
            .unwrap()
            .unwrap();
        assert!(!lossy.receipt.covers_source());
        assert!(InstructionRead::bounded("skills/a/SKILL.md", b"a\xffb", 20, false).is_err());
    }
}
