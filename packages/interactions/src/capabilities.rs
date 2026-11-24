//! Source and surface registrations are the applicability input for delivery and matrix proof.
use crate::{delivery::SurfaceCapabilities, *};
use serde::Deserialize;
use std::{collections::BTreeSet, sync::LazyLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Host {
    Linux,
    Macos,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: SourceId,
    pub answer_kinds: BTreeSet<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Surface {
    pub id: String,
    pub on: Vec<Host>,
    pub review_bytes: Option<usize>,
    #[serde(default = "enabled")]
    pub sets: bool,
    #[serde(default)]
    pub inline: bool,
    #[serde(default)]
    pub reply_messages: bool,
}
fn enabled() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    review_bytes: usize,
    source: Vec<Source>,
    surface: Vec<Surface>,
}
static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    toml::from_str(include_str!("../capabilities.toml")).expect("interaction capabilities")
});

pub fn sources() -> &'static [Source] {
    &CATALOG.source
}
pub fn surfaces() -> &'static [Surface] {
    &CATALOG.surface
}
pub fn current_host() -> Option<Host> {
    if cfg!(target_os = "linux") {
        Some(Host::Linux)
    } else if cfg!(target_os = "macos") {
        Some(Host::Macos)
    } else {
        None
    }
}
pub fn answer_kind(shape: &AnswerShape) -> &'static str {
    match shape {
        AnswerShape::Confirmation => "confirmation",
        AnswerShape::Text { .. } => "text",
        AnswerShape::Choice { .. } => "choice",
    }
}
pub fn accepts(source: &SourceId, shape: &AnswerShape) -> bool {
    sources()
        .iter()
        .find(|s| s.id == *source)
        .is_some_and(|s| s.answer_kinds.contains(answer_kind(shape)))
}
pub fn surface(id: &str, host: Host) -> Option<&'static Surface> {
    surfaces()
        .iter()
        .find(|s| s.id == id && s.on.contains(&host))
}
impl Surface {
    pub fn form(&self) -> SurfaceCapabilities {
        SurfaceCapabilities {
            confirmation: true,
            text: true,
            choice: true,
            sets: self.sets,
            free_response: true,
            inline: self.inline,
            max_review_bytes: self.review_bytes.unwrap_or(CATALOG.review_bytes),
        }
    }
}

#[cfg(test)]
mod tests;
