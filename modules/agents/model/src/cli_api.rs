//! CLI callers use the same typed requests and responses as the model routes.
use link_format::Out;
use link_format::WithinExt;
use link_model_contract::{api, chat};
use std::io::Read;

/// A missing agent namespace is expected CLI usage, never a Link bug.
pub fn missing_context() -> link_format::Report {
    use link_format::{Exit, Report, cmd, sentence};
    Report::error(sentence!("This command needs an agent's model context."))
        .exit(Exit::Usage)
        .next(cmd!("lnk agent model status"), "inspect your agent's model")
        .next(cmd!("lnk model list"), "list models running here")
}

fn url() -> anyhow::Result<String> {
    std::env::var("LNK_MODEL_URL")
        .within("LNK_MODEL_URL must name the granted model namespace (an agent proxy supplies it)")
}
fn input(request: Option<&str>) -> anyhow::Result<Vec<u8>> {
    let reader: Box<dyn Read> = match request {
        Some(path) => Box::new(std::fs::File::open(path)?),
        None => Box::new(std::io::stdin()),
    };
    let mut bytes = Vec::new();
    reader.take((32 << 20) + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= 32 << 20, "request exceeds 32 MiB");
    Ok(bytes)
}
pub async fn read(action: &str) -> anyhow::Result<serde_json::Value> {
    let client = chat::client::canonical::client()?;
    let url = url()?;
    Ok(match action {
        "usage" => serde_json::to_value(
            chat::client::canonical::get::<api::Usage>(&client, &url, action).await?,
        )?,
        _ => anyhow::bail!("unknown read action"),
    })
}
pub async fn context(chat: Option<String>) -> anyhow::Result<serde_json::Value> {
    let client = chat::client::canonical::client()?;
    let asked = api::ContextOf { chat };
    let answer: api::Context =
        chat::client::canonical::post(&client, &url()?, "context", &asked).await?;
    Ok(serde_json::to_value(answer)?)
}
pub async fn request(
    out: &mut Out,
    action: &str,
    request: Option<&str>,
) -> anyhow::Result<serde_json::Value> {
    let bytes = input(request)?;
    let client = chat::client::canonical::client()?;
    let url = url()?;
    Ok(match action {
        "embed" => serde_json::to_value(
            chat::client::canonical::post::<api::Embedded>(
                &client,
                &url,
                action,
                &serde_json::from_slice::<api::Embed>(&bytes)?,
            )
            .await?,
        )?,
        "transcribe" => serde_json::to_value(
            chat::client::canonical::post::<api::Heard>(
                &client,
                &url,
                action,
                &serde_json::from_slice::<api::Transcribe>(&bytes)?,
            )
            .await?,
        )?,
        "count" => serde_json::to_value(
            chat::client::canonical::post::<api::Counted>(
                &client,
                &url,
                action,
                &serde_json::from_slice::<api::Count>(&bytes)?,
            )
            .await?,
        )?,
        "chat" => {
            let input: chat::Chat = serde_json::from_slice(&bytes)?;
            input.check()?;
            let (pieces, mut events) = tokio::sync::mpsc::channel(32);
            let (_cancel, cancelled) = tokio::sync::watch::channel(false);
            let prompt = input.prompt();
            let answer = chat::client::canonical::answer(
                &client,
                chat::client::canonical::Target {
                    url: &url,
                    name: input.name.as_deref(),
                },
                &prompt,
                &pieces,
                false,
                cancelled,
                None,
            );
            tokio::pin!(answer);
            let mut said = false;
            let answer = loop {
                tokio::select! {answer=&mut answer=>break answer?,Some(text)=events.recv()=>said|=streamed(out,text)?}
            };
            // The last pieces can be queued when the answer resolves.
            while let Ok(text) = events.try_recv() {
                said |= streamed(out, text)?;
            }
            // A person reads its text whole, once, however it came.
            if !out.is_json() {
                let mut live = out.live();
                if !said {
                    live.piece(&answer.text);
                }
                if !answer.text.is_empty() && !answer.text.ends_with('\n') {
                    live.piece("\n");
                }
            }
            serde_json::to_value(chat::Stream::Answer { answer })?
        }
        _ => anyhow::bail!("unknown request action"),
    })
}

/// A piece of a chat's text as it streams: with `--json` a line of JSON
/// at once, its event as its declaration writes it; for a person the
/// words. Whether it said any.
fn streamed(out: &mut Out, text: String) -> anyhow::Result<bool> {
    match out.is_json() {
        true => {
            let piece: crate::cli::Stream =
                link_actions::answer::output(&chat::Stream::Text { text })?;
            out.reply_line(&piece);
        }
        false => out.live().piece(&text),
    }
    Ok(true)
}

/// A single model's public details, from its granted runtime or known numeric facts.
pub async fn show(model: Option<String>) -> anyhow::Result<link_model_contract::show::Shown> {
    if let Ok(url) = std::env::var("LNK_MODEL_URL") {
        let action = match &model {
            Some(name) => {
                anyhow::ensure!(
                    link_model_contract::show::plain_name(name),
                    "invalid model name"
                );
                format!("show?name={name}")
            }
            None => "show".into(),
        };
        return chat::client::canonical::get(&chat::client::canonical::client()?, &url, &action)
            .await;
    }
    let name = model.within("name a model to inspect its details")?;
    anyhow::ensure!(
        link_model_contract::show::plain_name(&name),
        "invalid model name"
    );
    let facts = link_model_contract::known(&name)?.map(|known| known.facts);
    Ok(link_model_contract::show::Shown {
        name,
        context: facts.as_ref().and_then(|facts| facts.window),
        facts,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn missing_namespace_is_usage_not_a_bug_report() {
        let report = super::missing_context();
        assert!(!report.is_bug());
        assert_eq!(report.exit, link_format::Exit::Usage);
    }
}
