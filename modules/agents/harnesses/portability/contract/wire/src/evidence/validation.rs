//! Checks on captured observations shared by producers and consumers.

use super::*;
use anyhow::ensure;

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl<T> Captured<T> {
    pub fn validate(&self) -> anyhow::Result<()> {
        if let Self::Omitted { sha256, reason, .. } = self {
            ensure!(digest(sha256), "an omitted field has no valid SHA-256");
            ensure!(!reason.is_empty(), "an omitted field has no reason");
        }
        Ok(())
    }
}

impl Reply {
    pub fn validate(&self) -> anyhow::Result<()> {
        self.body.validate()?;
        for field in self
            .body
            .retained()
            .into_iter()
            .flat_map(|body| body.fields.values())
        {
            field.validate()?;
        }
        for (index, content) in self.content().into_iter().flatten().enumerate() {
            ensure!(
                content.index as usize == index,
                "reply content indices are not contiguous"
            );
            content.value.validate()?;
            if let Some(part) = &content.media {
                ensure!(
                    self.model.media.contains(part),
                    "reply media is absent from its model view"
                );
            }
        }
        ensure!(
            digest(&self.model.text_sha256),
            "a model view has no valid text SHA-256"
        );
        ensure!(
            self.model.cut_text_bytes <= self.model.original_text_bytes,
            "a model view cuts more text than it received"
        );
        if let Some(Captured::Retained { value }) = self.field("isError") {
            ensure!(
                value.as_bool() == Some(self.is_error),
                "reply isError contradicts its captured field"
            );
        }
        Ok(())
    }
}

impl SkillLoad {
    pub fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            digest(&self.model_view.sha256),
            "a skill's model view has no valid digest"
        );
        if self.complete {
            ensure!(
                self.read.covers_source() && self.supplied.is_some() && self.omissions.is_empty(),
                "a complete skill load has partial or omitted contents"
            );
        }
        Ok(())
    }
}

impl request::ModelRequest {
    pub fn validate(&self) -> anyhow::Result<()> {
        use request::{Coverage, Slot, Source};
        ensure!(
            digest(&self.system.sha256),
            "a model request has no valid system digest"
        );
        let mut tools = std::collections::BTreeSet::new();
        for tool in &self.tools {
            ensure!(
                !tool.name.is_empty() && tools.insert(&tool.name),
                "model request tool names are empty or repeated"
            );
            ensure!(
                digest(&tool.declaration.sha256),
                "a model request tool declaration has no valid digest"
            );
        }
        self.projection.validate()?;
        let Some(projection) = self.projection.retained() else {
            return Ok(());
        };
        // The messages it lists, and the place of the first of them among
        // all it sent: after those it keeps, when it goes on from another.
        let (messages, sources, first) = match projection {
            request::RequestProjection::Whole(whole) => (&whole.messages, &whole.sources, 0),
            request::RequestProjection::Continued(continued) => {
                ensure!(
                    continued.after > 0,
                    "a continued model request names no request"
                );
                (&continued.messages, &continued.sources, continued.kept)
            }
        };
        for message in messages {
            ensure!(
                digest(&message.text.sha256),
                "a model message has no valid text digest"
            );
            for answer in &message.answers {
                ensure!(
                    digest(&answer.text.sha256),
                    "a model answer has no valid digest"
                );
            }
            for call in &message.calls {
                ensure!(
                    digest(&call.arguments.sha256),
                    "model call arguments have no valid digest"
                );
            }
        }
        for source in sources {
            ensure!(
                digest(&source.source_text.sha256) && digest(&source.projected_text.sha256),
                "a model source has no valid text digests"
            );
            if let Source::Native { seq } | Source::Imported { seq } = source.source {
                ensure!(seq > 0, "a stored model source has no sequence");
            }
            ensure!(
                source.message >= first,
                "a continued model request's source names a message it keeps"
            );
            if source.coverage != Coverage::Whole {
                continue;
            }
            let message = messages
                .get((source.message - first) as usize)
                .ok_or_else(|| anyhow::anyhow!("a model source names no message"))?;
            match &source.slot {
                Slot::Text { start, end } => ensure!(
                    start <= end
                        && *end <= message.text.bytes
                        && source.projected_text.bytes == end - start,
                    "a model source text span is invalid"
                ),
                Slot::Answer { index, call } => {
                    let answer = message
                        .answers
                        .get(*index as usize)
                        .ok_or_else(|| anyhow::anyhow!("a model source names no tool answer"))?;
                    ensure!(
                        &answer.call == call && answer.text == source.projected_text,
                        "a model source contradicts its tool answer"
                    );
                }
            }
        }
        Ok(())
    }
}
