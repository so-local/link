//! Store message media before committing the event that names it.

use super::*;

impl Harness {
    /// Keeps `media`, the images and documents of a message or a tool's
    /// answer for `agent`'s conversation `thread`, each a file beside its
    /// events, before the event that names them is written: their parts,
    /// for that event, and a line for its text naming each left out and
    /// why (none when each was kept).
    pub fn keep(
        &self,
        agent: &Agent,
        thread: &Thread,
        media: Vec<event::Media>,
    ) -> (Vec<event::Part>, Option<String>) {
        if media.is_empty() {
            return (Vec::new(), None);
        }
        // A name no other message's part has, whichever number its
        // event takes.
        let token =
            link_files::random_token().map(|token| token[..12.min(token.len())].to_string());
        let (mut parts, mut left) = (Vec::new(), Vec::new());
        for (n, media) in media.into_iter().enumerate() {
            let kept = token
                .as_ref()
                .map_err(|err| anyhow::anyhow!("{err}"))
                .and_then(|token| {
                    let file = format!(
                        "{}/{token}-{}.{}",
                        crate::store::PARTS,
                        n + 1,
                        media.extension()
                    );
                    off_runtime(|| self.store.keep_part(agent, thread, &file, &media.bytes))?;
                    Ok(file)
                });
            match kept {
                Ok(file) => parts.push(media.part(file)),
                Err(err) => left.push(format!("{} was left out: {err:#}", media.named())),
            }
        }
        (parts, (!left.is_empty()).then(|| left.join("\n")))
    }
}
