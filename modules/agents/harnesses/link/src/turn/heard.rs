//! A message's voice notes heard before its turn ([`crate::model::heard`]).

use super::Harness;
use crate::config::Config;
use crate::event::Media;

impl Harness {
    /// The words of each of `voices`, a line each (`[voice note] ...`),
    /// heard by its spec's `[model] transcribe` on `config`'s provider;
    /// and a line for each that couldn't be, saying why.
    pub async fn heard(&self, config: &Config, voices: &[Media]) -> (Vec<String>, Vec<String>) {
        let provider = config.model.provider.as_str();
        let transcriber = config
            .opinions()
            .model_for(provider)
            .transcribe
            .filter(|model| !model.is_empty() && model != "none");
        let (mut heard, mut left) = (Vec::new(), Vec::new());
        for voice in voices {
            let Some(transcriber) = &transcriber else {
                left.push(format!(
                    "{} was left out: [model.{provider}] transcribe names no model to hear it with",
                    voice.named()
                ));
                continue;
            };
            match crate::model::heard(&self.client, &config.model, transcriber, voice).await {
                Ok(words) => heard.push(format!("{} {words}", voice.named())),
                Err(err) => left.push(format!("{} couldn't be heard: {err:#}", voice.named())),
            }
        }
        (heard, left)
    }
}
