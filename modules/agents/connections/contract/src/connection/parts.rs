//! A message's images, documents and voice notes: what a bridge
//! downloads for a harness that takes them ([`Bridge::parts`]), and how
//! it sends them.

use super::{Asked, Bridge, Sent, Speaker, accepted};

/// An image or document a message holds, as a bridge downloaded it, for
/// a harness that takes them ([`Bridge::parts`]).
#[derive(Clone, PartialEq, Eq)]
pub struct Part {
    /// Its media type, as its connection said it (`image/jpeg`).
    pub media: String,
    /// Its name as it came (`photo.jpg`), if it had one.
    pub name: Option<String>,
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for Part {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Part")
            .field("media", &self.media)
            .field("name", &self.name)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

/// The most bytes a part holds.
pub use link_model_contract::chat::media::{MEDIA, PART_MAX, PARTS_MAX, is};

/// Whether a file a message holds, of media type `media` (its
/// parameters, `; charset=utf-8`, aside) and `size` bytes when its
/// connection says, is one a harness that takes parts takes: said before
/// it's downloaded. Else why not, in words for its sender, naming it as
/// `name`.
pub fn takes(name: &str, media: &str, size: Option<u64>) -> Result<(), String> {
    let media = media.split(';').next().unwrap_or_default().trim();
    if !MEDIA.iter().any(|taken| taken.eq_ignore_ascii_case(media)) {
        return Err(match media.is_empty() {
            true => format!("{name} is of no media type"),
            false => format!("{name} is {}", link_format::clean(media)),
        });
    }
    match size {
        Some(size) if size > PART_MAX => {
            Err(format!("{name} is larger than {} MiB", PART_MAX >> 20))
        }
        _ => Ok(()),
    }
}

/// A file a message holds, as its connection tells of it before it's
/// downloaded: `at` is how its connection gets it (a URL, an id).
#[derive(Clone, Debug, PartialEq)]
pub struct Held<T> {
    /// Its media type, as its connection says (`image/jpeg`).
    pub media: String,
    pub name: Option<String>,
    /// Its size, in bytes, when its connection says.
    pub size: Option<u64>,
    pub at: T,
}

impl<T> Held<T> {
    /// What it's called in words: its name, else its kind.
    fn called(&self) -> String {
        match &self.name {
            Some(name) => link_format::clean(name),
            None if self.media.starts_with("image/") => "an image".into(),
            None if self.media.starts_with("audio/") => "a voice note".into(),
            None => "a file".into(),
        }
    }
}

/// What a bridge does with a message: asks the agent, its text and its
/// images and documents, or answers it itself, when the agent would get
/// nothing of it.
#[derive(Debug, PartialEq)]
pub enum Holding {
    Ask { text: String, parts: Vec<Part> },
    Answer(String),
}

/// What a bridge answers a message holding only what the agent can't
/// read.
pub const TEXT_ONLY: &str = "I can read text only, for now.";

/// A message's `text` and `files`, as a bridge sends them on
/// ([`ask_holding`]). To a harness that takes parts ([`Bridge::parts`]),
/// each file it takes ([`takes`]) is downloaded by `get`, at most
/// [`PARTS_MAX`], `get` given where it's got (`at`); to one that reads
/// text only, none is. Each file left
/// out is named in a line of the text, and why, never dropped unsaid; a
/// message left with neither text nor parts is answered instead.
pub async fn holding<T, F, Fut>(
    bridge: &Bridge,
    text: Option<&str>,
    files: &[Held<T>],
    get: F,
) -> Holding
where
    T: Clone,
    F: Fn(T) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Vec<u8>>>,
{
    let text = text.map(str::trim).unwrap_or_default();
    let (mut parts, mut left) = (Vec::new(), Vec::new());
    for file in files {
        let called = file.called();
        if !bridge.parts {
            left.push(format!("{called} (the agent reads text only)"));
            continue;
        }
        if let Err(why) = takes(&called, &file.media, file.size) {
            left.push(why);
            continue;
        }
        if parts.len() == PARTS_MAX {
            left.push(format!("{called} (a message holds at most {PARTS_MAX})"));
            continue;
        }
        match get(file.at.clone()).await {
            Ok(bytes) if bytes.len() as u64 > PART_MAX => {
                left.push(format!("{called} is larger than {} MiB", PART_MAX >> 20));
            }
            Ok(bytes) => {
                let media = file.media.split(';').next().unwrap_or_default().trim();
                let media = media.to_ascii_lowercase();
                match is(&media, &bytes) {
                    true => parts.push(Part {
                        media,
                        name: file.name.clone(),
                        bytes,
                    }),
                    false => left.push(format!("{called} isn't what it says it is ({media})")),
                }
            }
            Err(err) => {
                super::warn(&format!("Downloading {called}: {err:#}"));
                left.push(format!("{called} couldn't be downloaded"));
            }
        }
    }
    if text.is_empty() && parts.is_empty() {
        return Holding::Answer(match bridge.parts {
            false => TEXT_ONLY.into(),
            true => {
                let read = "I can read text, images (PNG, JPEG, GIF or WebP), documents \
                            (PDF or text) and voice notes only, for now";
                match left.is_empty() {
                    true => format!("{read}."),
                    false => format!("{read}: {}.", left.join("; ")),
                }
            }
        });
    }
    let text = match left.is_empty() {
        true => text.to_string(),
        false if text.is_empty() => format!("(Left out: {}.)", left.join("; ")),
        false => format!("{text}\n\n(Left out: {}.)", left.join("; ")),
    };
    Holding::Ask { text, parts }
}

/// [`ask_from`](super::ask_from), the message holding `parts`, its images and documents:
/// only for a harness that takes them ([`Bridge::parts`]), else an
/// error before anything is sent.
pub async fn ask_holding(
    bridge: &Bridge,
    thread: &str,
    text: &str,
    parts: &[Part],
    from: Option<Speaker<'_>>,
    progress: &tokio::sync::watch::Sender<String>,
) -> anyhow::Result<Asked> {
    accepted(
        bridge,
        thread,
        text,
        parts,
        Sent {
            occurrence: None,
            from,
            ..Sent::default()
        },
    )
    .await?
    .answer(progress)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(media: &str, name: Option<&str>, size: Option<u64>, at: &str) -> Held<String> {
        Held {
            media: media.into(),
            name: name.map(str::to_string),
            size,
            at: at.into(),
        }
    }

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n....";

    #[tokio::test]
    async fn a_message_holds_what_its_harness_takes() {
        let get = |at: String| async move {
            match at.as_str() {
                "png" => Ok(PNG.to_vec()),
                "fake" => Ok(b"<html>".to_vec()),
                _ => anyhow::bail!("gone"),
            }
        };
        let files = [
            held("image/png", Some("a.png"), Some(12), "png"),
            held("video/mp4", Some("b.mp4"), Some(9), "png"),
            held("application/pdf", Some("c.pdf"), Some(PART_MAX + 1), "png"),
            held("image/png", None, None, "fake"),
            held("text/plain; charset=utf-8", Some("d.txt"), None, "gone"),
        ];
        let taking = Bridge {
            parts: true,
            ..Default::default()
        };
        let Holding::Ask { text, parts } =
            super::holding(&taking, Some(" look "), &files, get).await
        else {
            panic!("not asked");
        };
        assert_eq!(
            parts,
            [Part {
                media: "image/png".into(),
                name: Some("a.png".into()),
                bytes: PNG.to_vec(),
            }]
        );
        // Each left out is said, and why.
        assert_eq!(
            text,
            "look\n\n(Left out: b.mp4 is video/mp4; c.pdf is larger than 10 MiB; an image isn't \
             what it says it is (image/png); d.txt couldn't be downloaded.)"
        );

        // Nothing it reads: answered by the bridge.
        let video = [held("video/mp4", None, None, "png")];
        let Holding::Answer(answer) = super::holding(&taking, None, &video, get).await else {
            panic!("asked");
        };
        assert!(
            answer.ends_with("only, for now: a file is video/mp4."),
            "{answer}"
        );

        // A harness that reads text only is sent none, and told of them.
        let text_only = Bridge::default();
        let photo = [held("image/png", Some("a.png"), None, "png")];
        assert_eq!(
            super::holding(&text_only, None, &photo, get).await,
            Holding::Answer(TEXT_ONLY.into())
        );
        assert_eq!(
            super::holding(&text_only, Some("hi"), &photo, get).await,
            Holding::Ask {
                text: "hi\n\n(Left out: a.png (the agent reads text only).)".into(),
                parts: vec![],
            }
        );

        // At most PARTS_MAX.
        let many = vec![held("image/png", None, None, "png"); PARTS_MAX + 1];
        let Holding::Ask { text, parts } = super::holding(&taking, None, &many, get).await else {
            panic!("not asked");
        };
        assert_eq!(parts.len(), PARTS_MAX);
        assert!(text.contains("at most 8"), "{text}");
    }

    /// A voice note goes to the harness, which takes audio over ACP, and
    /// is known by its first bytes.
    #[tokio::test]
    async fn a_voice_note_reaches_a_harness_on_acp() {
        let get = |_: String| async move { Ok(b"OggS\0\x02".to_vec()) };
        let voice = [held("audio/ogg", None, Some(6), "v")];
        let acp = Bridge {
            parts: true,
            ..Default::default()
        };
        let Holding::Ask { parts, .. } = super::holding(&acp, None, &voice, get).await else {
            panic!("not asked");
        };
        assert_eq!(parts[0].media, "audio/ogg");
        for (media, bytes) in [
            ("audio/ogg", &b"OggS"[..]),
            ("audio/mpeg", b"ID3\x04"),
            ("audio/mpeg", b"\xff\xfb\x90"),
            ("audio/mp4", b"\0\0\0\x20ftypM4A "),
            ("audio/aac", b"\xff\xf1\x50"),
            ("audio/wav", b"RIFF\0\0\0\0WAVEfmt "),
            ("audio/webm", b"\x1a\x45\xdf\xa3"),
        ] {
            assert!(is(media, bytes), "{media}");
            assert!(!is(media, b"<html>"), "{media}");
        }
    }

    #[test]
    fn a_harness_that_reads_text_only_is_never_sent_a_part() {
        let parts = [Part {
            media: "image/png".into(),
            name: None,
            bytes: PNG.to_vec(),
        }];
        // Refused before anything's sent.
        let refused = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(super::accepted(
                &Bridge::default(),
                "",
                "hi",
                &parts,
                Sent::default(),
            ));
        assert!(refused.is_err_and(|e| e.to_string().contains("reads text only")));
    }
}
