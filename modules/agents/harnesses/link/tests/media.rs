//! Images and documents end to end, in process: sent over ACP as
//! image blocks and embedded resources, kept beside the conversation's
//! events, and sent to a fake model as its provider takes them.

use crate::common;

use std::sync::Arc;

use link_agent_state_protocol::acp::schema::v1::{
    AudioContent, BlobResourceContents, ContentBlock, EmbeddedResource, EmbeddedResourceResource,
    ImageContent, ResourceLink, TextContent,
};
use link_harness::{
    agent::{Agent, Agents},
    config::Config,
    server::{self, Server},
    store::Files,
    turn::Harness,
};
use link_harness_contract::harness::Model;
use link_test_kit::TempDir;
use serde_json::json;

/// A 1×1 PNG, in base64.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

/// The start of a PDF, in base64 (`%PDF-1.4`).
const PDF: &str = "JVBERi0xLjQK";

struct World {
    root: TempDir,
    endpoint: String,
    asked: common::Asked,
}

/// An agent on `provider`'s fake model, which answers "Seen".
async fn world(name: &str, provider: &str) -> World {
    world_with(name, provider, axum::Router::new()).await
}

/// [`world`], its provider serving `routes` too.
async fn world_with(name: &str, provider: &str, routes: axum::Router) -> World {
    let root = TempDir::new(&format!("media-{name}"));
    let (port, asked) = common::fake_model_with(routes, std::time::Duration::ZERO, |_, _| {
        vec![
            common::delta(json!({"content": "Seen"})),
            json!({"id": "gen-1", "choices": [], "usage": {"prompt_tokens": 3,
                "completion_tokens": 1}}),
        ]
    })
    .await;
    // A hosted provider's base holds its `/v1`, a runtime's doesn't.
    let base_url = match provider {
        "local" => format!("http://127.0.0.1:{port}"),
        _ => format!("http://127.0.0.1:{port}/v1"),
    };
    let (home, files) = (root.join("home"), root.join("files"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&files).unwrap();
    let agent = Agent::new("main").unwrap();
    let mut agents = Agents::default();
    agents.add(agent.clone(), home, common::flat(&files));
    let store = Files::new(agents.clone());
    link_harness::store::Store::set_config(
        &store,
        &agent,
        &Config {
            model: Model {
                provider: provider.into(),
                name: Some("fake".into()),
                base_url: Some(base_url),
                ..Model::default()
            },
            endpoint_key: Some("the-key".to_string().into()),
            ..Config::default()
        },
    )
    .unwrap();
    let server = Arc::new(Server::new(
        Harness::new(agents, Arc::new(store)).unwrap(),
        agent,
    ));
    let port = common::serve(server::router(server)).await;
    World {
        root,
        endpoint: common::acp(port),
        asked,
    }
}

/// Sends `blocks` as the owner's message on `telegram:42`: its answer,
/// or why it was refused.
async fn send(world: &World, blocks: Vec<ContentBlock>) -> Result<String, String> {
    common::prompt(&world.endpoint, "the-key", "telegram:42", blocks).await
}

fn text(text: &str) -> ContentBlock {
    ContentBlock::Text(TextContent::new(text))
}

fn image(data: &str) -> ContentBlock {
    ContentBlock::Image(ImageContent::new(data, "image/png"))
}

/// A document, `data` in base64, named by the last part of its URI.
fn document(name: &str, media: &str, data: &str) -> ContentBlock {
    ContentBlock::Resource(EmbeddedResource::new(
        EmbeddedResourceResource::BlobResourceContents(
            BlobResourceContents::new(data, format!("file:///{name}")).mime_type(media),
        ),
    ))
}

fn folder(world: &World) -> std::path::PathBuf {
    world.root.join("files/chats/telegram-42")
}

#[tokio::test]
async fn an_image_and_a_document_reach_the_model() {
    let world = world("reach", "openai").await;
    let answer = send(
        &world,
        vec![
            text("What's this?"),
            image(PNG),
            document("plan.pdf", "application/pdf", PDF),
            document("list.md", "text/markdown", "LSBlZ2dz"),
        ],
    )
    .await;
    assert_eq!(answer.unwrap(), "Seen");

    // Kept beside its events, which name them.
    let events = common::events(&folder(&world));
    let parts = events[0]["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 3);
    for part in parts {
        let file = folder(&world).join(part["file"].as_str().unwrap());
        assert_eq!(
            std::fs::metadata(&file).unwrap().len(),
            part["bytes"].as_u64().unwrap()
        );
    }
    assert_eq!(parts[0]["kind"], "image");
    assert_eq!(parts[1]["name"], "plan.pdf");
    let copy = std::fs::read_to_string(folder(&world).join("conversation.md")).unwrap();
    assert!(
        copy.contains("[image]\n[document: plan.pdf]\n[document: list.md]\nWhat's this?"),
        "{copy}"
    );

    // Sent as OpenAI takes them, before the text: a text document as its
    // text.
    let asked = world.asked.lock().unwrap().clone();
    let content = asked[0]["messages"][1]["content"]
        .as_array()
        .unwrap()
        .clone();
    let kinds: Vec<&str> = content
        .iter()
        .map(|p| p["type"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["image_url", "file", "text", "text"]);
    assert_eq!(
        content[0]["image_url"]["url"],
        format!("data:image/png;base64,{PNG}")
    );
    assert_eq!(content[1]["file"]["filename"], "plan.pdf");
    assert_eq!(content[2]["text"], "[document: list.md]\n- eggs");
    let text = content[3]["text"].as_str().unwrap();
    assert!(text.ends_with("[image]\n[document: plan.pdf]\n[document: list.md]\nWhat's this?"));

    // A photo needs no caption, and the next turn sends both messages'.
    assert_eq!(send(&world, vec![image(PNG)]).await.unwrap(), "Seen");
    let asked = world.asked.lock().unwrap().clone();
    let images = asked[1]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["content"].as_array())
        .flatten()
        .filter(|p| p["type"] == "image_url")
        .count();
    assert_eq!(images, 2);
}

#[tokio::test]
async fn a_model_that_takes_none_is_told_what_was_left_out() {
    let world = world("left", "local").await;
    let answer = send(&world, vec![image(PNG), text("Hi")]).await;
    assert_eq!(answer.unwrap(), "Seen");
    let asked = world.asked.lock().unwrap().clone();
    let content = asked[0]["messages"][1]["content"].as_str().unwrap();
    assert!(
        content.starts_with("[image] was left out: this model takes no images through Link yet"),
        "{content}"
    );
    assert!(content.ends_with("[image]\nHi"), "{content}");
}

#[tokio::test]
async fn what_isnt_taken_is_refused_and_says_why() {
    let world = world("refused", "openai").await;
    for (blocks, why) in [
        (
            vec![ContentBlock::ResourceLink(ResourceLink::new(
                "a.png",
                "https://example.com/a.png",
            ))],
            "embedded, not as a link",
        ),
        (vec![image("aGVsbG8=")], "isn't what it says"),
        (
            vec![document("a.exe", "application/octet-stream", "aGVsbG8=")],
            "images are taken as PNG",
        ),
        (
            vec![ContentBlock::Audio(AudioContent::new(
                "aGVsbG8=",
                "audio/wav",
            ))],
            "isn't what it says",
        ),
        (vec![image(PNG); 9], "at most 8"),
    ] {
        let said = send(&world, blocks).await.unwrap_err();
        assert!(said.contains(why), "{said}");
    }
    assert!(world.asked.lock().unwrap().is_empty());
    assert!(!folder(&world).join("parts").exists());
}

#[tokio::test]
async fn a_connections_photo_reaches_the_model() {
    use base64::Engine;
    let world = world("connection", "openai").await;
    let bridge = link_connection_contract::Bridge {
        endpoint: world.endpoint.clone(),
        key: "the-key".to_string().into(),
        parts: true,
        ..Default::default()
    };
    let photo = link_connection_contract::Part {
        media: "image/png".into(),
        name: Some("photo.png".into()),
        bytes: base64::engine::general_purpose::STANDARD
            .decode(PNG)
            .unwrap(),
    };
    let (progress, _) = tokio::sync::watch::channel(String::new());
    let asked = link_connection_contract::ask_holding(
        &bridge,
        "telegram:42",
        "What's this?",
        &[photo],
        None,
        &progress,
    )
    .await
    .unwrap();
    assert_eq!(
        asked,
        link_connection_contract::Asked::Answer("Seen".into())
    );
    let events = common::events(&folder(&world));
    assert_eq!(events[0]["parts"][0]["kind"], "image");
    assert_eq!(events[0]["text"], "What's this?");
    let sent = world.asked.lock().unwrap()[0].to_string();
    assert!(
        sent.contains(&format!("data:image/png;base64,{PNG}")),
        "{sent}"
    );
}

/// The start of an Ogg file, in base64 (`OggS`, then a page's header).
const OGG: &str = "T2dnUwACAAAAAAAAAAA=";

/// A voice note is heard by its provider's transcription before its
/// turn: its words are the message's text, its audio a part kept and
/// never sent to the model.
#[tokio::test]
async fn a_voice_note_is_heard_and_its_words_reach_the_model() {
    use axum::routing::post;
    let heard = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let routes = axum::Router::new().route(
        "/v1/audio/transcriptions",
        post({
            let heard = heard.clone();
            move |body: axum::body::Bytes| async move {
                heard
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&body).into_owned());
                axum::Json(json!({"text": "buy milk on the way home"}))
            }
        }),
    );
    let world = world_with("voice", "openai", routes).await;
    let voice = ContentBlock::Audio(AudioContent::new(OGG, "audio/ogg"));
    assert_eq!(
        send(&world, vec![text("and eggs"), voice]).await.unwrap(),
        "Seen"
    );
    let form = heard.lock().unwrap()[0].clone();
    assert!(
        form.contains("\r\n\r\ngpt-4o-mini-transcribe\r\n"),
        "{form}"
    );
    assert!(
        form.contains("filename=\"voice.ogg\"") && form.contains("OggS"),
        "{form}"
    );
    let events = common::events(&folder(&world));
    assert_eq!(events[0]["parts"][0]["kind"], "voice");
    assert_eq!(
        events[0]["text"],
        "[voice note] buy milk on the way home\n\nand eggs"
    );
    let sent = world.asked.lock().unwrap()[0].to_string();
    assert!(sent.contains("buy milk on the way home"), "{sent}");
    assert!(!sent.contains(OGG), "{sent}");
}

/// A voice note on a provider its spec names no transcription for is
/// left out, and the model told so.
#[tokio::test]
async fn a_voice_note_nothing_hears_is_left_out() {
    let world = world("unheard", "local").await;
    let voice = ContentBlock::Audio(AudioContent::new(OGG, "audio/ogg"));
    assert_eq!(send(&world, vec![voice]).await.unwrap(), "Seen");
    let events = common::events(&folder(&world));
    let said = events[0]["text"].as_str().unwrap();
    assert!(
        said.contains("[voice note] was left out: [model.local] transcribe names no model"),
        "{said}"
    );
}
