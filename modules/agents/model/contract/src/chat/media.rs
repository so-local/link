//! Media and tool records used by model requests.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// An image or document of a message, or of a tool's answer: kept as a
/// file of the conversation's folder, which the event names.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub kind: Kind,
    /// Its media type (`image/png`).
    pub media: String,
    /// Its name as it came (`photo.jpg`), if it had one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Its file, in the conversation's folder (`parts/<name>.png`).
    pub file: String,
    /// Its size, in bytes.
    pub bytes: u64,
}

/// What a part is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// PNG, JPEG, GIF or WebP.
    Image,
    /// PDF, or text.
    Document,
    /// Speech: Ogg, MP3, MP4 audio, AAC, WAV or WebM. Heard before its
    /// turn, its words the message's text; never sent to a model.
    Voice,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Document => "document",
            Self::Voice => "voice note",
        }
    }
}

/// The most bytes a part holds, as its connection knows it.
pub const PART_MAX: u64 = 10 << 20;

/// The most parts a message, or a tool's answer, holds.
pub const PARTS_MAX: usize = 8;

/// The media types a part may be, its kind, and the extension of its
/// file: the contract's ([`MEDIA`]), which
/// connections download. A text document is any text in UTF-8; the rest
/// are known by their first bytes.
const TAKEN: [(&str, Kind, &str); 14] = [
    ("image/png", Kind::Image, "png"),
    ("image/jpeg", Kind::Image, "jpg"),
    ("image/gif", Kind::Image, "gif"),
    ("image/webp", Kind::Image, "webp"),
    ("application/pdf", Kind::Document, "pdf"),
    ("text/plain", Kind::Document, "txt"),
    ("text/markdown", Kind::Document, "md"),
    ("text/csv", Kind::Document, "csv"),
    ("audio/ogg", Kind::Voice, "ogg"),
    ("audio/mpeg", Kind::Voice, "mp3"),
    ("audio/mp4", Kind::Voice, "m4a"),
    ("audio/aac", Kind::Voice, "aac"),
    ("audio/wav", Kind::Voice, "wav"),
    ("audio/webm", Kind::Voice, "webm"),
];

/// The longest name of a part kept, in characters.
const NAME_MAX: usize = 128;

/// An image or document with its bytes: as it came, before it's kept,
/// or read back for the prompt. Only one whose bytes are what it says
/// is made ([`Media::new`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Media {
    pub kind: Kind,
    pub media: String,
    pub name: Option<String>,
    #[serde(with = "bytes")]
    pub bytes: Vec<u8>,
}

impl Media {
    /// `bytes`, said to be of media type `media`, named `name`: taken
    /// when they're a type Link takes, within [`PART_MAX`], and are what
    /// they say (an image or a PDF by its first bytes, text by being
    /// UTF-8); else why not, in words for the sender.
    pub fn new(media: &str, name: Option<&str>, bytes: Vec<u8>) -> Result<Self, String> {
        let name = name
            .map(|name| name.rsplit(['/', '\\']).next().unwrap_or_default())
            .map(|name| link_format::clean(name.trim()))
            .map(|name| name.chars().take(NAME_MAX).collect::<String>())
            .filter(|name| !name.is_empty());
        let what = name.as_deref().unwrap_or("a part").to_string();
        let media = media.trim().to_ascii_lowercase();
        let Some((media, kind, _)) = TAKEN.iter().find(|(taken, _, _)| *taken == media) else {
            return Err(format!(
                "{what} is {}: images are taken as PNG, JPEG, GIF or WebP, documents \
                 as PDF or text (plain, Markdown or CSV), and voice notes as Ogg, MP3, MP4 \
                 audio, AAC, WAV or WebM",
                match media.is_empty() {
                    true => "of no media type".to_string(),
                    false => link_format::clean(&media),
                }
            ));
        };
        if bytes.len() as u64 > PART_MAX {
            return Err(format!("{what} is larger than {} MiB", PART_MAX >> 20));
        }
        if !is(media, &bytes) {
            return Err(format!("{what} isn't what it says it is ({media})"));
        }
        Ok(Self {
            kind: *kind,
            media: media.to_string(),
            name,
            bytes,
        })
    }

    /// It as a part kept in `file`.
    pub fn part(&self, file: String) -> Part {
        Part {
            kind: self.kind,
            media: self.media.clone(),
            name: self.name.clone(),
            file,
            bytes: self.bytes.len() as u64,
        }
    }

    /// The extension of its file (`png`).
    pub fn extension(&self) -> &'static str {
        TAKEN
            .iter()
            .find(|(media, _, _)| *media == self.media)
            .map_or("bin", |(_, _, extension)| extension)
    }

    /// Its bytes in base64, as providers take them.
    pub fn encoded(&self) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(&self.bytes)
    }

    /// Its bytes as a `data:` URL, as OpenAI's API takes them.
    pub fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.media, self.encoded())
    }

    /// Whether it's a text document, which every model reads as text.
    pub fn is_text(&self) -> bool {
        self.media.starts_with("text/")
    }

    /// What it's called where it's named: `[image: photo.jpg]`, or
    /// `[image]` without a name.
    pub fn named(&self) -> String {
        named(self.kind, self.name.as_deref())
    }
}

impl Part {
    /// What it's called where it's named, as [`Media::named`].
    pub fn named(&self) -> String {
        named(self.kind, self.name.as_deref())
    }
}

/// `data` as base64 says it, within what a part holds.
pub fn decode(data: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    // Four characters for three bytes, and padding.
    if data.len() as u64 > PART_MAX / 3 * 4 + 8 {
        return Err(format!("it's larger than {} MiB", PART_MAX >> 20));
    }
    base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .map_err(|_| "its data isn't base64".to_string())
}

fn named(kind: Kind, name: Option<&str>) -> String {
    match name {
        Some(name) => format!("[{}: {name}]", kind.word()),
        None => format!("[{}]", kind.word()),
    }
}

/// A call of a tool, as the model made it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Call {
    /// The model's id for it, which its answer names.
    pub id: String,
    /// The tool, by the name the model was given (`memory__search`).
    pub name: String,
    /// What it's called with: a JSON object.
    pub arguments: Value,
}

/// What a response cost in tokens, as the provider counts them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tokens {
    /// The whole prompt, cached or not.
    pub input: u64,
    pub output: u64,
    /// Of `input`, those read from the provider's cache.
    pub cached: u64,
}

pub const MEDIA: [&str; 14] = [
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "application/pdf",
    "text/plain",
    "text/markdown",
    "text/csv",
    "audio/ogg",
    "audio/mpeg",
    "audio/mp4",
    "audio/aac",
    "audio/wav",
    "audio/webm",
];

/// Whether `bytes` are what media type `media` ([`MEDIA`]) says: an
/// image, a PDF or audio by its first bytes, text by being UTF-8.
pub fn is(media: &str, bytes: &[u8]) -> bool {
    match media {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "image/webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        "application/pdf" => bytes.starts_with(b"%PDF-"),
        "audio/ogg" => bytes.starts_with(b"OggS"),
        // An ID3 tag, or a frame's sync.
        "audio/mpeg" => {
            bytes.starts_with(b"ID3")
                || matches!(bytes, [0xff, second, ..] if second & 0xe0 == 0xe0)
        }
        "audio/mp4" => bytes.get(4..8) == Some(b"ftyp"),
        // ADTS's sync.
        "audio/aac" => matches!(bytes, [0xff, second, ..] if second & 0xf6 == 0xf0),
        "audio/wav" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE"),
        "audio/webm" => bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]),
        media if media.starts_with("text/") => std::str::from_utf8(bytes).is_ok(),
        _ => false,
    }
}

mod bytes {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(value: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        use base64::Engine;
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(value))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let encoded = String::deserialize(deserializer)?;
        super::decode(&encoded).map_err(serde::de::Error::custom)
    }
}
