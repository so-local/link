//! A voice note heard: its speech as words, by its provider's
//! transcription in OpenAI's shape (`POST {base}/v1/audio/transcriptions`,
//! which a runtime here may serve too), before its message's turn.

use super::*;

/// The longest a voice note takes to be heard.
const HEARING: std::time::Duration = std::time::Duration::from_secs(120);

/// The most of a voice note's words kept.
const WORDS_MAX: usize = 64 << 10;

/// `voice`'s words, heard by `transcriber`, a model of `model`'s
/// provider; an error when it hears none, or can't be reached.
pub async fn heard(
    client: &reqwest::Client,
    model: &Model,
    transcriber: &str,
    voice: &Media,
) -> anyhow::Result<String> {
    anyhow::ensure!(
        model.provider != "anthropic",
        "Anthropic hears no voice notes"
    );
    let (boundary, body) = form(transcriber, voice);
    let request = client
        .post(format!("{}/audio/transcriptions", openai_base(model)?))
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(body)
        .timeout(HEARING);
    let mut response = authorized(request, model)
        .send()
        .await
        .map_err(|err| anyhow::anyhow!(with_causes(&err.without_url())))?;
    let status = response.status();
    anyhow::ensure!(status.is_success(), "it answered {status}");
    let heard: Value =
        serde_json::from_slice(&bounded(&mut response).await?).within("its answer isn't JSON")?;
    let words = heard["text"].as_str().within("its answer has no text")?;
    let words: String = link_format::clean(words.trim())
        .chars()
        .take(WORDS_MAX)
        .collect();
    anyhow::ensure!(!words.is_empty(), "it heard no words");
    Ok(words)
}

/// A form of `transcriber` and `voice`'s file, and the boundary its
/// parts are between: one its bytes don't hold.
fn form(transcriber: &str, voice: &Media) -> (String, Vec<u8>) {
    let boundary = (0u32..)
        .map(|n| format!("link-voice-note-{n}"))
        .find(|b| !voice.bytes.windows(b.len()).any(|w| w == b.as_bytes()))
        .unwrap_or_default();
    let mut body = format!(
        "--{boundary}\r\ncontent-disposition: form-data; name=\"model\"\r\n\r\n{transcriber}\r\n\
         --{boundary}\r\ncontent-disposition: form-data; name=\"file\"; filename=\"voice.{}\"\r\n\
         content-type: {}\r\n\r\n",
        voice.extension(),
        voice.media
    )
    .into_bytes();
    body.extend_from_slice(&voice.bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (boundary, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_voice_note_is_sent_as_a_form() {
        let voice = Media::new("audio/ogg", None, b"OggS link-voice-note-0".to_vec()).unwrap();
        let (boundary, body) = form("whisper-1", &voice);
        assert_eq!(boundary, "link-voice-note-1");
        let body = String::from_utf8(body).unwrap();
        assert!(
            body.contains("name=\"model\"\r\n\r\nwhisper-1\r\n"),
            "{body}"
        );
        assert!(body.contains("filename=\"voice.ogg\"\r\ncontent-type: audio/ogg\r\n\r\nOggS"));
        assert!(body.ends_with("\r\n--link-voice-note-1--\r\n"));
    }
}
