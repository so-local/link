//! Registry-owned model routes. The grant supplies the backend; bodies contain only model inputs.
use anyhow::ensure;
use link_format::WithinExt;
use link_model_contract::{
    api, chat,
    route::actions::{Api, Backend},
};
use link_sandbox_route::{Hangup, Verdict, Verdicts, http::RequestHead};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

mod endpoint;

const REQUEST_MAX: usize = 32 << 20;
const ANSWER_MAX: usize = 8 << 20;
/// The most of a provider's own error message a refusal repeats.
const SAID_MAX: usize = 300;

/// Why an action was refused, in words safe to give its caller: Link's
/// own about the caller's request, or a provider's status and message
/// with its credential taken out. Every other error is refused in
/// general words, since it may name a URL, a host path or a credential.
#[derive(Debug)]
struct Said(String);

impl std::fmt::Display for Said {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Said {}

/// `result`'s error as a [`Said`]: an error in Link's words about the
/// caller's own request.
fn said<T>(result: anyhow::Result<T>) -> anyhow::Result<T> {
    result.map_err(|error| Said(format!("{error:#}")).into())
}

/// A proved action call, of `agent` acting as `service`: its key is
/// what the store grants `service`. Neither URL, key, provider nor agent
/// identity comes from its body.
#[expect(
    clippy::too_many_arguments,
    reason = "the request, its grant and who it's for, each set apart by the callers"
)]
pub async fn serve<S>(
    mut stream: S,
    head: &[u8],
    rest: Vec<u8>,
    grant: &Api,
    settings: &Path,
    agent: &str,
    service: &str,
    via: &str,
) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Verdicts + Hangup + Unpin,
{
    let request = RequestHead::parse(head).map_err(anyhow::Error::msg)?;
    let path = if grant.fallback {
        request.path().replacen("/model/fallback/", "/model/", 1)
    } else {
        request.path().to_owned()
    };
    let action = link_model_contract::program_route(&path).within("model action is not granted")?;
    ensure!(
        action.bridge.as_deref() == Some("model"),
        "wrong action bridge"
    );
    let limits = action.limits.within("model action has no budget")?;
    let mut streaming = false;
    let result=tokio::time::timeout(Duration::from_millis(limits.time_ms),async {
        let read=matches!(path.as_str(),"/_link/model/show"|"/_link/model/list"|"/_link/model/usage");
        ensure!(request.method==if read {"GET"} else {"POST"},"wrong action method");
        ensure!(request.path()==request.target || path==chat_show(),"unexpected action query");
        let length=usize::try_from(request.length().map_err(anyhow::Error::msg)?)?;
        ensure!(length<=REQUEST_MAX && rest.len()<=length,"model request exceeds its byte budget");
        ensure!(!read || length==0,"read actions take no body");
        let mut body=rest;
        if length>body.len() {let old=body.len();body.resize(length,0);stream.read_exact(&mut body[old..]).await?;}
        let mut endpoint=endpoint::target(grant,service,via).await?;
        let client=endpoint.client()?;
        let model=&mut endpoint.model;
        if path=="/_link/model/chat" {
            let mut input:chat::Chat=said(parse(&body))?;
            said(input.check())?;
            if let Some(max)=grant.window.filter(|max|input.window>*max) {return Err(Said(format!("the request's window, {} tokens, is more than the {max} granted",input.window)).into());}
            if let Some(max)=grant.answer.filter(|max|input.answer>*max) {return Err(Said(format!("the request's answer, {} tokens, is more than the {max} granted",input.answer)).into());}
            model.name=Some(said(grant.name(input.name.as_deref()).map(str::to_owned))?);
            let facts=link_model_contract::known(model.name.as_deref().unwrap_or_default())?.map(|known|known.facts).unwrap_or_default();
            input.price=facts.price;
            input.above=facts.above;
            if !model.provider.is_empty() {ensure!(!link_model_contract::route::searches_by_nature(&model.provider,model.name.as_deref().unwrap_or_default()),"server search is not granted");}
            stream.say(&Verdict::allowed("model/chat")).await?;
            streaming=true;
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n").await?;
            return chatting(&mut stream,&client,model,&grant.backend,&input,limits.bytes,settings,agent).await;
        }
        let answer=match path.as_str() {
            "/_link/model/show"=> {
                let name=match request.target.split_once('?') {Some((_,query))=>Some(query.strip_prefix("name=").within("show takes only name")?),None=>None};
                let name=grant.name(name)?;
                let mut shown=match &grant.backend {
                    Backend::Local{port}=>super::show::ask(*port,name).await?,
                    Backend::Hosted{..}=>link_model_contract::show::Shown{name:name.into(),context:link_model_contract::known(name)?.and_then(|f|f.facts.window),..Default::default()},
                };
                shown.facts=link_model_contract::known(name)?.map(|known|known.facts);
                // Whether the endpoint is ready, and what it has loaded.
                let available=listed(&client,model,grant).await;
                let loaded=if let Backend::Local{port}=grant.backend {
                    let ps=get(&client,&format!("http://127.0.0.1:{port}/api/ps")).await.ok();
                    ps.into_iter().flat_map(|ps|ps["models"].as_array().cloned().unwrap_or_default()).filter_map(|model|model["name"].as_str().map(str::to_owned)).filter(|name|grant.name(Some(name)).is_ok()).collect()
                } else {Vec::new()};
                shown.endpoint=Some(api::Status{ready:available.is_ok(),loaded});
                serde_json::to_vec(&shown)?
            }
            "/_link/model/list"=>serde_json::to_vec(&listed(&client,model,grant).await?)?,
            "/_link/model/embed"=> {
                let input:api::Embed=parse(&body)?;
                model.name=Some(grant.name(input.name.as_deref())?.to_owned());
                if model.provider=="anthropic" {return unsupported(&mut stream).await;}
                ensure!(!input.texts.is_empty(),"embed needs texts");
                let url=format!("{}/embeddings",chat::client::openai_base(model)?);
                let data=post(&client,model,&url,json!({"model":model.name,"input":input.texts})).await?;
                let vectors=data["data"].as_array().within("no embeddings")?.iter().map(|row|serde_json::from_value::<Vec<f64>>(row["embedding"].clone())).collect::<Result<Vec<_>,_>>()?;
                ensure!(vectors.len()==input.texts.len() && vectors.iter().all(|v|!v.is_empty() && v.iter().all(|n|n.is_finite())),"invalid embeddings");
                serde_json::to_vec(&api::Embedded{vectors})?
            }
            "/_link/model/transcribe"=> {
                let input:api::Transcribe=parse(&body)?;
                ensure!(link_model_contract::show::plain_name(&input.name),"invalid transcriber");
                ensure!(grant.transcribe.as_deref()==Some(input.name.as_str()) || grant.name(Some(&input.name)).is_ok(),"transcriber is outside the grant");
                ensure!(input.audio.kind==chat::Kind::Voice,"transcribe needs audio");
                let audio=chat::Media::new(&input.audio.media,input.audio.name.as_deref(),input.audio.bytes).map_err(anyhow::Error::msg)?;
                if model.provider=="anthropic" {return unsupported(&mut stream).await;}
                let text=chat::client::heard::heard(&client,model,&input.name,&audio).await?;
                serde_json::to_vec(&api::Heard{text})?
            }
            "/_link/model/count"=> {
                let input:api::Count=parse(&body)?;
                model.name=Some(grant.name(input.name.as_deref())?.to_owned());
                let count=if model.provider=="anthropic" {
                    let url=chat::client::anthropic_url(model).replace("/messages","/messages/count_tokens");
                    post(&client,model,&url,json!({"model":model.name,"messages":[{"role":"user","content":input.text}]})).await.ok().and_then(|v|v["input_tokens"].as_u64())
                } else if let Backend::Local{port}=grant.backend {
                    let mut count=None;
                    for path in ["/tokenize","/v1/tokenize"] {
                        if let Ok(value)=post(&client,model,&format!("http://127.0.0.1:{port}{path}"),json!({"model":model.name,"content":input.text,"prompt":input.text})).await {
                            count=value["count"].as_u64().or_else(||value["tokens"].as_array().map(|tokens|tokens.len() as u64));
                            if count.is_some() {break;}
                        }
                    }
                    count
                } else {None};
                let Some(tokens)=count else {return unsupported(&mut stream).await;};
                serde_json::to_vec(&api::Counted{tokens})?
            }
            "/_link/model/context"=> {
                let input:api::ContextOf=if body.is_empty() {api::ContextOf::default()} else {parse(&body)?};
                let settings=settings.to_owned();
                let agent=agent.to_owned();
                let read=tokio::task::spawn_blocking(move||super::context::read(&settings,&agent,input.chat.as_deref())).await??;
                serde_json::to_vec(&read)?
            }
            "/_link/model/usage"=> {
                let usage=link_measure_contract::client::tokens::read(settings,agent).unwrap_or_default();
                serde_json::to_vec(&api::Usage{requests:usage.requests,unmetered:usage.unmetered,tokens:chat::Tokens{input:usage.input,output:usage.output,cached:usage.cached}})?
            }
            _=>anyhow::bail!("unknown model action"),
        };
        ensure!(answer.len()<=limits.bytes,"model answer exceeds byte budget");
        stream.say(&Verdict::allowed(path.clone())).await?;
        json_answer(&mut stream,"200 OK",&answer).await
    }).await;
    match result {
        Ok(Ok(())) => Ok(()),
        _ if streaming => anyhow::bail!("model stream ended before completion"),
        // What it's said is safe to give; anything else is refused in
        // general words, never disclosing a provider URL, host path or
        // credential.
        Ok(Err(error)) => {
            let why = match error.downcast_ref::<Said>() {
                Some(said) => format!("model action refused: {said}"),
                None => "model action failed or its input was refused".into(),
            };
            super::local::refuse(&mut stream, String::new(), &why).await
        }
        Err(_) => {
            super::local::refuse(
                &mut stream,
                String::new(),
                "model action exceeded its time budget",
            )
            .await
        }
    }
}
fn chat_show() -> &'static str {
    "/_link/model/show"
}
fn parse<T: DeserializeOwned>(bytes: &[u8]) -> anyhow::Result<T> {
    Ok(serde_json::from_slice(bytes)?)
}
async fn listed(
    client: &reqwest::Client,
    model: &chat::client::Model,
    grant: &Api,
) -> anyhow::Result<Vec<String>> {
    Ok(chat::client::listed(client, model)
        .await?
        .into_iter()
        .filter(|name| grant.name(Some(name)).is_ok())
        .collect())
}
async fn read(response: reqwest::Response) -> anyhow::Result<Value> {
    read_as(response, None).await
}
/// A provider's answer: its JSON, or, refused, its status and its own
/// message as a [`Said`], with `key`, the credential sent, taken out.
async fn read_as(mut response: reqwest::Response, key: Option<&str>) -> anyhow::Result<Value> {
    let status = response.status();
    if !status.is_success() {
        let mut bytes = Vec::new();
        while let Ok(Some(chunk)) = response.chunk().await {
            bytes.extend_from_slice(&chunk);
            if bytes.len() > ANSWER_MAX {
                break;
            }
        }
        return Err(Said(refused_by_provider(status, &bytes, key)).into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= ANSWER_MAX,
            "provider answer exceeds byte budget"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&bytes)?)
}
async fn get(client: &reqwest::Client, url: &str) -> anyhow::Result<Value> {
    read(
        client
            .get(url)
            .timeout(Duration::from_secs(2))
            .send()
            .await?,
    )
    .await
}
async fn post(
    client: &reqwest::Client,
    model: &chat::client::Model,
    url: &str,
    body: Value,
) -> anyhow::Result<Value> {
    let request = chat::client::authorized(
        client
            .post(url)
            .header("content-type", "application/json")
            .body(serde_json::to_vec(&body)?),
        model,
    );
    let key = model.api_key.expose().as_str();
    read_as(request.send().await?, Some(key).filter(|k| !k.is_empty())).await
}
/// What a provider that answered `status` with `body` said: its status,
/// and the message of its JSON error when it gave one, cleaned, cut short
/// and with `key` taken out.
fn refused_by_provider(status: reqwest::StatusCode, body: &[u8], key: Option<&str>) -> String {
    let value: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let message = [
        &value["error"]["message"],
        &value["error"],
        &value["message"],
    ]
    .into_iter()
    .find_map(|v| v.as_str())
    .map(|m| {
        let m = key.map_or(m.to_owned(), |key| m.replace(key, "<credential>"));
        let m = link_format::clean(m.trim());
        match m.char_indices().nth(SAID_MAX) {
            Some((at, _)) => format!("{}...", &m[..at]),
            None => m,
        }
    })
    .filter(|m| !m.is_empty());
    match message {
        Some(message) => format!("the model provider answered {status}: {message}"),
        None => format!("the model provider answered {status}"),
    }
}
async fn json_answer<S: AsyncWrite + Unpin>(
    stream: &mut S,
    status: &str,
    body: &[u8],
) -> anyhow::Result<()> {
    stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n",body.len()).as_bytes()).await?;
    stream.write_all(body).await?;
    Ok(())
}
async fn unsupported<S: AsyncWrite + Verdicts + Unpin>(stream: &mut S) -> anyhow::Result<()> {
    stream.say(&Verdict::allowed("model")).await?;
    json_answer(
        stream,
        "501 Not Implemented",
        br#"{"error":"the granted model has no mapping for this action"}"#,
    )
    .await
}
#[expect(
    clippy::too_many_arguments,
    reason = "the chat, where it goes and whose it is, each the caller's"
)]
async fn chatting<S: AsyncRead + AsyncWrite + Hangup + Unpin>(
    stream: &mut S,
    client: &reqwest::Client,
    model: &chat::client::Model,
    backend: &Backend,
    input: &chat::Chat,
    limit: usize,
    settings: &Path,
    agent: &str,
) -> anyhow::Result<()> {
    let socket = stream.hangup_socket();
    let (mut read, mut write) = tokio::io::split(stream);
    let gone = async {
        let mut byte = [0; 1];
        if let Ok(0) = read.read(&mut byte).await
            && let Some(socket) = socket
        {
            link_sandbox_route::io::until_hung_up(socket).await;
        }
    };
    tokio::pin!(gone);
    let (pieces, mut events) = tokio::sync::mpsc::channel(32);
    let (_cancel, cancelled) = tokio::sync::watch::channel(false);
    let prompt = input.prompt();
    let asked = asked(model, &prompt);
    let answering = chat::client::answer(client, model, &prompt, &pieces, false, cancelled);
    tokio::pin!(answering);
    let mut sent = 0;
    let answer = loop {
        tokio::select! {
            answer=&mut answering=>break answer,
        _=&mut gone=>anyhow::bail!("model caller left"),
            Some(text)=events.recv()=>write_event(&mut write,&chat::Stream::Text{text},&mut sent,limit).await?,
        }
    };
    // The final text pieces can be queued when the answer resolves.
    while let Ok(text) = events.try_recv() {
        write_event(&mut write, &chat::Stream::Text { text }, &mut sent, limit).await?;
    }
    let event = match answer {
        Ok(mut answer) => {
            answer.model = model.name.clone();
            if !agent.is_empty() {
                link_measure_contract::client::tokens::add(
                    settings,
                    agent,
                    &link_measure_contract::client::Tokens {
                        requests: 1,
                        input: answer.tokens.input,
                        output: answer.tokens.output,
                        cached: answer.tokens.cached,
                        ..Default::default()
                    },
                )?;
                if let Some(asked) = asked {
                    asked.keep(settings, agent, Some(answer.tokens.clone()))?;
                }
            }
            chat::Stream::Answer { answer }
        }
        Err(error) => {
            let kind = if error.is::<chat::client::Unreached>() {
                chat::Failure::Unreached
            } else if error.is::<chat::client::TooLong>() {
                chat::Failure::TooLong
            } else if error.is::<chat::client::Overloaded>() {
                chat::Failure::Overloaded
            } else {
                chat::Failure::Refused
            };
            chat::Stream::Error {
                kind,
                message: failed(kind, backend),
            }
        }
    };
    write_event(&mut write, &event, &mut sent, limit).await
}
/// What a chat that failed as `kind` says, in Link's own words, never the
/// provider's or its client's, which may name a URL, a host path or a
/// credential: what went wrong, and what shows or fixes it.
fn failed(kind: chat::Failure, backend: &Backend) -> String {
    match (kind, backend) {
        (chat::Failure::Unreached, Backend::Local { port }) => format!(
            "nothing answers on port {port}: start the model's runtime there, and lnk model runtime list shows what runs here"
        ),
        (chat::Failure::Unreached, Backend::Hosted { .. }) => {
            "its provider wasn't reached: lnk agent show shows whether the model answers".into()
        }
        (chat::Failure::TooLong, _) => "the prompt is longer than the model's window".into(),
        (chat::Failure::Overloaded, _) => "the model is busy or failing: try again shortly".into(),
        (chat::Failure::Refused, Backend::Local { .. }) => {
            "the model's runtime refused the request: lnk model runtime list shows what it serves"
                .into()
        }
        (chat::Failure::Refused, Backend::Hosted { .. }) => {
            "its provider refused the request: lnk agent show shows whether the model answers, and lnk agent model set picks another".into()
        }
    }
}

async fn write_event<S: AsyncWrite + Unpin>(
    stream: &mut S,
    event: &chat::Stream,
    sent: &mut usize,
    limit: usize,
) -> anyhow::Result<()> {
    let mut bytes = serde_json::to_vec(event)?;
    bytes.push(b'\n');
    *sent += bytes.len();
    ensure!(*sent <= limit, "model stream exceeds byte budget");
    stream.write_all(&bytes).await?;
    stream.flush().await?;
    Ok(())
}

/// What `prompt` asks `model`, as the provider's request the client
/// writes for it: its parts, its instructions split by its spans, in the
/// chat it names.
fn asked(model: &chat::client::Model, prompt: &chat::Prompt) -> Option<super::context::Asked> {
    use link_model_contract::context::Api;
    let name = model.name.as_deref().unwrap_or_default();
    let (api, body) = match model.provider == "anthropic" {
        true => (Api::Anthropic, chat::client::anthropic_body(name, prompt)),
        false => (
            Api::Chat,
            chat::client::openai_body(name, &model.provider, prompt),
        ),
    };
    let mut asked = super::context::Asked::of(api, &body, prompt.chat)?;
    // The instructions as composed, in place of their one part, where
    // they are the system prompt whole.
    let whole = prompt.system.len() as u64;
    if let Some(split) = link_model_contract::context::split(prompt.system, prompt.spans)
        && let Some(at) = asked
            .parts
            .iter()
            .position(|p| p.path == "instructions" && p.bytes == whole)
    {
        asked.parts.splice(at..=at, split);
    }
    asked.window = Some(u64::from(prompt.window));
    if prompt.price.is_some() {
        asked.price = prompt.price;
        asked.above = prompt.above.to_vec();
    }
    asked.parent = prompt.parent.map(str::to_owned);
    Some(asked)
}

#[cfg(test)]
mod tests;
