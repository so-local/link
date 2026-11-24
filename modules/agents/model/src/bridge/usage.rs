//! The tokens of the model answers the model routes carry, from the usage
//! each provider's answer gives: numbers only, never a word of the prompt
//! or the answer. Each call's are handed to the measure module, which
//! keeps them as the agent's ([`link_measure_contract::client::tokens`]).
//!
//! The answer is read as it's passed to the program, never held back or
//! changed: a [`Tapped`] connection hands each piece the bridge writes to
//! a [`Meter`], which follows the answer's head, its chunks and its
//! body, and keeps the few numbers of its usage:
//!
//! - **Anthropic's Messages:** `usage` in a whole answer; in a stream,
//!   `message_start`'s and `message_delta`'s. Its prompt is its three
//!   parts, as Link Harness counts it: `input_tokens`, and what was read
//!   from the cache (cached) and written to it (written).
//! - **OpenAI's Chat Completions and Responses, OpenRouter's, a local
//!   runtime's OpenAI-compatible API:** `usage` in a whole answer, and
//!   in a stream the last event's (`response.usage` for Responses). A
//!   Chat Completions stream has it only when its request asked
//!   (`stream_options.include_usage`): nothing is added to a request,
//!   so one that didn't ask is counted as a request without usage.
//! - **Ollama's own API:** `prompt_eval_count` and `eval_count`.
//!
//! Each count is cumulative, so the last one an answer gives is its own.
//! A stream's event is looked at only if it names usage, and only up to
//! [`MAX_EVENT`]; a whole answer is scanned for its top-level usage
//! without being kept.

use std::sync::atomic::{AtomicU64, Ordering};

use link_measure_contract::client::Tokens;
use link_sandbox_route::http::AnswerHead;
use serde_json::Value;

mod reading;
mod scanning;
mod tapping;

use scanning::Scan;
pub use tapping::{Counter, Tapped};

/// How a provider's usage is written, by the API the request asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Anthropic's Messages: `input_tokens` is the prompt past its cache;
    /// what was read from or written to the cache is apart.
    Anthropic,
    /// OpenAI's Chat Completions, Completions and Responses, and those
    /// that speak them (OpenRouter, a local runtime): the prompt's
    /// tokens include what was cached.
    OpenAi,
    /// Ollama's own `/api/chat` and `/api/generate`.
    Ollama,
}

impl Format {
    /// The format of a request's answer, by its method and path (no
    /// `?query`), when it's a request for a model's answer; none for
    /// anything else (the models' list, embeddings, counting tokens).
    pub fn of(method: &str, path: &str) -> Option<Format> {
        if method != "POST" {
            return None;
        }
        match path {
            "/v1/messages" => Some(Format::Anthropic),
            "/v1/chat/completions" | "/v1/completions" | "/v1/responses" => Some(Format::OpenAi),
            "/api/chat" | "/api/generate" => Some(Format::Ollama),
            _ => None,
        }
    }
}

/// The largest answer head read.
const MAX_HEAD: usize = 64 * 1024;
/// The longest line of a chunked body read (a chunk's size).
const MAX_LINE: usize = 4096;
/// The largest event of a stream, or line of Ollama's, looked at: one
/// past it can't be read, and if it held the usage, the request is
/// counted as one without.
pub const MAX_EVENT: usize = 4 * 1024 * 1024;
/// The largest usage value kept of a whole answer.
const MAX_USAGE: usize = 64 * 1024;
/// The most tokens one count may say: past it, an answer says what
/// isn't so, and the request is counted as one without usage.
const MOST: u64 = 1_000_000_000;

/// Where the answer is.
#[derive(Debug)]
enum State {
    /// In its head (or a 1xx's), these bytes of it so far.
    Head(Vec<u8>),
    /// In its body.
    Body(Body),
    /// Nothing more to read: not a 2xx, not plain, or broken.
    Done,
}

/// How the body is framed.
#[derive(Debug)]
enum Framing {
    /// As it comes, to the connection's end or its length.
    Plain,
    /// In chunks: where in them.
    Chunked(Chunk),
}

#[derive(Debug)]
enum Chunk {
    /// A size line, these bytes of it so far.
    Size(Vec<u8>),
    /// This many bytes of data left.
    Data(u64),
    /// The line end after a chunk's data.
    DataEnd(Vec<u8>),
    /// Past the last chunk.
    Trailers,
}

/// What the body is, and how it's read.
#[derive(Debug)]
enum Reader {
    /// Server-sent events: the current line, and the event's data.
    Events {
        line: Vec<u8>,
        data: Vec<u8>,
        over: bool,
    },
    /// A line of JSON each (Ollama's stream).
    Lines { line: Vec<u8>, over: bool },
    /// One JSON value, scanned for its top-level usage.
    Whole(Scan),
}

#[derive(Debug)]
struct Body {
    framing: Framing,
    reader: Reader,
}

/// The numbers of an answer's usage, each as last given.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Usage {
    input: Option<u64>,
    output: Option<u64>,
    /// Read from the provider's cache.
    cache_read: Option<u64>,
    /// Written to it (Anthropic's), part of the prompt.
    cache_write: Option<u64>,
}

/// One answer, followed as it's passed to the program.
#[derive(Debug)]
pub struct Meter {
    format: Format,
    state: State,
    /// Whether the answer was a 2xx.
    answered: bool,
    usage: Usage,
    /// A count past [`MOST`] was seen.
    absurd: bool,
}

impl Meter {
    pub fn new(format: Format) -> Self {
        Self {
            format,
            state: State::Head(Vec::new()),
            answered: false,
            usage: Usage::default(),
            absurd: false,
        }
    }

    /// The answer's next bytes, as they go to the program.
    pub fn feed(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            match &mut self.state {
                State::Done => return,
                State::Head(head) => {
                    // Up to the end of the head: a blank line.
                    let before = head.len();
                    head.extend_from_slice(bytes);
                    let start = before.saturating_sub(3);
                    let Some(end) = head[start..].windows(4).position(|w| w == b"\r\n\r\n") else {
                        if head.len() > MAX_HEAD {
                            self.state = State::Done;
                        }
                        return;
                    };
                    let end = start + end + 4;
                    let used = end - before;
                    head.truncate(end);
                    let head = std::mem::take(head);
                    self.state = self.after_head(&head);
                    bytes = &bytes[used..];
                }
                State::Body(body) => {
                    let mut events = Vec::new();
                    let ended = body.feed(bytes, &mut events);
                    for event in events {
                        self.event(&event);
                    }
                    if ended {
                        self.end();
                    }
                    return;
                }
            }
        }
    }

    /// What follows a head: another head after a 1xx, the body of a
    /// 2xx, else nothing.
    fn after_head(&mut self, head: &[u8]) -> State {
        let Ok(answer) = AnswerHead::parse(head) else {
            return State::Done;
        };
        let code = answer.code;
        if (100..200).contains(&code) && code != 101 {
            return State::Head(Vec::new());
        }
        if !(200..300).contains(&code) {
            return State::Done;
        }
        self.answered = true;
        let (mut chunked, mut kind, mut encoded) = (false, String::new(), false);
        for header in &answer.headers {
            let value = header.value.to_ascii_lowercase();
            match header.name.as_str() {
                "transfer-encoding" => {
                    chunked = value
                        .rsplit(',')
                        .next()
                        .is_some_and(|t| t.trim() == "chunked");
                }
                "content-encoding" => encoded = !value.is_empty() && value != "identity",
                "content-type" => kind = value,
                _ => {}
            }
        }
        // Compressed: nothing in it can be read here.
        if encoded {
            return State::Done;
        }
        let reader = if kind.starts_with("text/event-stream") {
            Reader::Events {
                line: Vec::new(),
                data: Vec::new(),
                over: false,
            }
        } else if kind.contains("ndjson") {
            Reader::Lines {
                line: Vec::new(),
                over: false,
            }
        } else {
            Reader::Whole(Scan::default())
        };
        State::Body(Body {
            framing: match chunked {
                true => Framing::Chunked(Chunk::Size(Vec::new())),
                false => Framing::Plain,
            },
            reader,
        })
    }

    /// One event of a stream, a line of Ollama's, or a whole answer's
    /// usage: its numbers kept.
    fn event(&mut self, json: &[u8]) {
        let Ok(value) = serde_json::from_slice::<Value>(json) else {
            return;
        };
        match self.format {
            Format::Ollama => self.take(&value, &[("prompt_eval_count", In), ("eval_count", Out)]),
            Format::Anthropic | Format::OpenAi => {
                for usage in [
                    &value["usage"],
                    &value["message"]["usage"],
                    &value["response"]["usage"],
                ] {
                    if usage.is_object() {
                        self.usage_of(usage);
                    }
                }
            }
        }
    }

    fn usage_of(&mut self, usage: &Value) {
        match self.format {
            Format::Anthropic => self.take(
                usage,
                &[
                    ("input_tokens", In),
                    ("output_tokens", Out),
                    ("cache_read_input_tokens", Read),
                    ("cache_creation_input_tokens", Write),
                ],
            ),
            Format::OpenAi => {
                self.take(
                    usage,
                    &[
                        ("prompt_tokens", In),
                        ("input_tokens", In),
                        ("completion_tokens", Out),
                        ("output_tokens", Out),
                    ],
                );
                for details in ["prompt_tokens_details", "input_tokens_details"] {
                    self.take(&usage[details], &[("cached_tokens", Read)]);
                }
            }
            Format::Ollama => {}
        }
    }

    /// Each of `fields` that `value` has as a whole number, kept as the
    /// count it names: the largest given, since each is cumulative, and a
    /// later event that says 0 (a compatible server's `message_delta`)
    /// never erases the prompt's count.
    fn take(&mut self, value: &Value, fields: &[(&str, Field)]) {
        for (key, field) in fields {
            let Some(n) = value.get(*key).and_then(Value::as_u64) else {
                continue;
            };
            if n > MOST {
                self.absurd = true;
                continue;
            }
            let slot = match field {
                In => &mut self.usage.input,
                Out => &mut self.usage.output,
                Read => &mut self.usage.cache_read,
                Write => &mut self.usage.cache_write,
            };
            *slot = Some(slot.map_or(n, |was| was.max(n)));
        }
    }

    /// The body ended: anything it held to the end is read.
    fn end(&mut self) {
        if let State::Body(body) = &mut self.state {
            let mut events = Vec::new();
            body.reader.end(&mut events);
            self.state = State::Done;
            for event in events {
                self.event(&event);
            }
        }
        self.state = State::Done;
    }

    /// What this answer counts: none if it wasn't answered (not a 2xx);
    /// else one request, and its tokens, or one without usage.
    pub fn finish(mut self) -> Tokens {
        self.end();
        if !self.answered {
            return Tokens::default();
        }
        let u = self.usage;
        if self.absurd || u == Usage::default() {
            return Tokens {
                requests: 1,
                unmetered: 1,
                ..Tokens::default()
            };
        }
        let read = u.cache_read.unwrap_or(0);
        let input = match self.format {
            // Anthropic's prompt is its three parts.
            Format::Anthropic => u
                .input
                .unwrap_or(0)
                .saturating_add(read)
                .saturating_add(u.cache_write.unwrap_or(0)),
            Format::OpenAi | Format::Ollama => u.input.unwrap_or(0),
        };
        Tokens {
            requests: 1,
            unmetered: 0,
            input,
            output: u.output.unwrap_or(0),
            cached: read,
            written: u.cache_write.unwrap_or(0),
        }
    }
}

#[derive(Clone, Copy)]
enum Field {
    In,
    Out,
    Read,
    Write,
}
use Field::{In, Out, Read, Write};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// What the program says and is told, in every answer below: never
    /// anywhere in what's counted.
    pub(super) const PHRASE: &str = "the purple walrus sings at noon";

    /// `body` as a whole answer, with its length.
    fn whole(kind: &str, body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\
             Connection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    /// `body` as a stream, in chunks of at most 40 bytes, as a provider
    /// sends one.
    pub(super) fn streamed(kind: &str, body: &str) -> Vec<u8> {
        let mut out = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nTransfer-Encoding: chunked\r\n\r\n"
        )
        .into_bytes();
        for piece in body.as_bytes().chunks(40) {
            out.extend_from_slice(format!("{:x}\r\n", piece.len()).as_bytes());
            out.extend_from_slice(piece);
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(b"0\r\n\r\n");
        out
    }

    /// Server-sent events, each `(event, data)`.
    pub(super) fn events(events: &[(&str, serde_json::Value)]) -> String {
        events
            .iter()
            .map(|(name, data)| match name.is_empty() {
                true => format!("data: {data}\n\n"),
                false => format!("event: {name}\ndata: {data}\n\n"),
            })
            .collect()
    }

    /// What `answer` counts, fed whole, split in two at every byte, and
    /// a byte at a time: the same however it arrives.
    fn counted(format: Format, answer: &[u8]) -> Tokens {
        let once = |pieces: &[&[u8]]| {
            let mut meter = Meter::new(format);
            for piece in pieces {
                meter.feed(piece);
            }
            meter.finish()
        };
        let all = once(&[answer]);
        for at in 0..answer.len() {
            let (a, b) = answer.split_at(at);
            assert_eq!(once(&[a, b]), all, "split at {at}");
        }
        let bytes: Vec<&[u8]> = answer.chunks(1).collect();
        assert_eq!(once(&bytes), all, "a byte at a time");
        all
    }

    pub(super) fn tokens(input: u64, output: u64, cached: u64) -> Tokens {
        Tokens {
            requests: 1,
            unmetered: 0,
            input,
            output,
            cached,
            written: 0,
        }
    }

    /// [`tokens`], with what was written to the cache.
    fn anthropic(input: u64, output: u64, cached: u64, written: u64) -> Tokens {
        Tokens {
            written,
            ..tokens(input, output, cached)
        }
    }

    const UNMETERED: Tokens = Tokens {
        requests: 1,
        unmetered: 1,
        input: 0,
        output: 0,
        cached: 0,
        written: 0,
    };

    #[test]
    fn a_request_for_an_answer_is_metered_and_nothing_else() {
        for (method, path, format) in [
            ("POST", "/v1/messages", Some(Format::Anthropic)),
            ("POST", "/v1/chat/completions", Some(Format::OpenAi)),
            ("POST", "/v1/responses", Some(Format::OpenAi)),
            ("POST", "/v1/completions", Some(Format::OpenAi)),
            ("POST", "/api/chat", Some(Format::Ollama)),
            ("POST", "/api/generate", Some(Format::Ollama)),
            ("POST", "/v1/messages/count_tokens", None),
            ("POST", "/v1/embeddings", None),
            ("GET", "/v1/models", None),
            ("GET", "/v1/messages", None),
        ] {
            assert_eq!(Format::of(method, path), format, "{method} {path}");
        }
    }

    /// Anthropic's whole message: its prompt is its three parts. A
    /// `usage` in its text, or in a tool's input, isn't its usage.
    #[test]
    fn anthropics_whole_message() {
        let body = json!({
            "id": "msg_01", "type": "message", "role": "assistant", "model": "claude-haiku-5-5",
            "content": [
                {"type": "text", "text": format!("{PHRASE}, \"usage\": {{\"input_tokens\": 9}}")},
                {"type": "tool_use", "id": "t1", "name": "f",
                 "input": {"usage": {"input_tokens": 999999}}},
            ],
            "stop_reason": "end_turn", "stop_sequence": null,
            "usage": {"input_tokens": 10, "cache_creation_input_tokens": 5,
                      "cache_read_input_tokens": 90, "output_tokens": 7,
                      "service_tier": "standard"},
        });
        let answer = whole("application/json", &body.to_string());
        // The prompt is its three parts; cached is what was read.
        assert_eq!(
            counted(Format::Anthropic, &answer),
            anthropic(105, 7, 90, 5)
        );
        // Its usage first, as JSON may order it, pretty-printed.
        let pretty = serde_json::to_string_pretty(&json!({
            "usage": {"input_tokens": 3, "output_tokens": 4}, "content": [{"text": PHRASE}],
        }))
        .unwrap();
        assert_eq!(
            counted(Format::Anthropic, &whole("application/json", &pretty)),
            tokens(3, 4, 0)
        );
    }

    /// Anthropic's stream: the prompt in `message_start`, the answer's
    /// count growing in `message_delta`, the last one its own.
    #[test]
    fn anthropics_stream() {
        let body = events(&[
            (
                "message_start",
                json!({"type": "message_start", "message": {
                "id": "msg_01", "type": "message", "role": "assistant", "content": [],
                "model": "claude-haiku-5-5", "stop_reason": null,
                "usage": {"input_tokens": 10, "cache_creation_input_tokens": 5,
                          "cache_read_input_tokens": 90, "output_tokens": 1}}}),
            ),
            (
                "content_block_start",
                json!({"type": "content_block_start", "index": 0,
                "content_block": {"type": "text", "text": ""}}),
            ),
            ("ping", json!({"type": "ping"})),
            (
                "content_block_delta",
                json!({"type": "content_block_delta", "index": 0,
                "delta": {"type": "text_delta", "text": format!("{PHRASE}: \"usage\"")}}),
            ),
            (
                "content_block_stop",
                json!({"type": "content_block_stop", "index": 0}),
            ),
            (
                "message_delta",
                json!({"type": "message_delta",
                "delta": {"stop_reason": "end_turn", "stop_sequence": null},
                "usage": {"output_tokens": 15}}),
            ),
            (
                "message_delta",
                json!({"type": "message_delta", "delta": {},
                "usage": {"input_tokens": 10, "cache_creation_input_tokens": 5,
                          "cache_read_input_tokens": 90, "output_tokens": 42}}),
            ),
            ("message_stop", json!({"type": "message_stop"})),
        ]);
        let answer = streamed("text/event-stream; charset=utf-8", &body);
        assert_eq!(
            counted(Format::Anthropic, &answer),
            anthropic(105, 42, 90, 5)
        );
    }

    #[test]
    fn openais_whole_chat_completion() {
        let body = json!({
            "id": "chatcmpl-1", "object": "chat.completion", "model": "gpt-6.1-sol",
            "choices": [{"index": 0, "finish_reason": "stop",
                         "message": {"role": "assistant", "content": PHRASE}}],
            "usage": {"prompt_tokens": 100, "completion_tokens": 20, "total_tokens": 120,
                      "prompt_tokens_details": {"cached_tokens": 64, "audio_tokens": 0},
                      "completion_tokens_details": {"reasoning_tokens": 8}},
        });
        let answer = whole("application/json", &body.to_string());
        assert_eq!(counted(Format::OpenAi, &answer), tokens(100, 20, 64));
    }

    /// A stream asked for its usage has it in its last event; one that
    /// wasn't is a request without usage, never zero tokens.
    #[test]
    fn openais_chat_stream_with_and_without_usage() {
        let chunk = |content: &str, usage: serde_json::Value| {
            (
                "",
                json!({"id": "chatcmpl-1", "object": "chat.completion.chunk",
                "choices": [{"index": 0, "delta": {"content": content}}], "usage": usage}),
            )
        };
        let mut asked = events(&[
            chunk("the purple ", json!(null)),
            chunk("walrus sings at noon", json!(null)),
            (
                "",
                json!({"id": "chatcmpl-1", "object": "chat.completion.chunk", "choices": [],
                "usage": {"prompt_tokens": 100, "completion_tokens": 20,
                          "prompt_tokens_details": {"cached_tokens": 0}}}),
            ),
        ]);
        asked.push_str("data: [DONE]\n\n");
        let answer = streamed("text/event-stream", &asked);
        assert_eq!(counted(Format::OpenAi, &answer), tokens(100, 20, 0));
        let mut not_asked = events(&[chunk(PHRASE, json!(null))]);
        not_asked.push_str("data: [DONE]\n\n");
        let answer = streamed("text/event-stream", &not_asked);
        assert_eq!(counted(Format::OpenAi, &answer), UNMETERED);
    }

    #[test]
    fn openais_responses_whole_and_streamed() {
        let response = |usage: serde_json::Value| {
            json!({"id": "resp_1", "object": "response", "status": "completed",
                "output": [{"type": "message", "role": "assistant",
                            "content": [{"type": "output_text", "text": PHRASE}]}],
                "usage": usage, "user": null, "metadata": {}})
        };
        let usage = json!({"input_tokens": 50, "input_tokens_details": {"cached_tokens": 10},
            "output_tokens": 5, "output_tokens_details": {"reasoning_tokens": 0},
            "total_tokens": 55});
        let answer = whole("application/json", &response(usage.clone()).to_string());
        assert_eq!(counted(Format::OpenAi, &answer), tokens(50, 5, 10));
        let body = events(&[
            (
                "response.created",
                json!({"type": "response.created",
                "response": response(json!(null))}),
            ),
            (
                "response.output_text.delta",
                json!({"type": "response.output_text.delta",
                "delta": PHRASE}),
            ),
            (
                "response.completed",
                json!({"type": "response.completed",
                "response": response(usage)}),
            ),
        ]);
        let answer = streamed("text/event-stream", &body);
        assert_eq!(counted(Format::OpenAi, &answer), tokens(50, 5, 10));
    }

    /// OpenRouter's: its comments between events, and its cost beside
    /// the usage, which is never read.
    #[test]
    fn openrouters_stream() {
        let mut body = String::from(": OPENROUTER PROCESSING\n\n");
        body += &events(&[
            (
                "",
                json!({"id": "gen-1", "choices": [{"delta": {"content": PHRASE}}]}),
            ),
            (
                "",
                json!({"id": "gen-1", "choices": [{"delta": {}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 12, "completion_tokens": 3, "total_tokens": 15,
                          "cost": 0.00042, "prompt_tokens_details": {"cached_tokens": 4}}}),
            ),
        ]);
        body += "data: [DONE]\n\n";
        let answer = streamed("text/event-stream", &body);
        assert_eq!(counted(Format::OpenAi, &answer), tokens(12, 3, 4));
        // Lines ended `\r\n`, and an answer to the connection's end.
        let crlf = body.replace('\n', "\r\n");
        let answer = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n{crlf}");
        assert_eq!(counted(Format::OpenAi, answer.as_bytes()), tokens(12, 3, 4));
    }

    #[test]
    fn ollamas_own_api() {
        let lines = [
            json!({"model": "qwen3", "message": {"role": "assistant", "content": PHRASE},
                   "done": false}),
            json!({"model": "qwen3", "message": {"role": "assistant", "content": ""},
                   "done": true, "prompt_eval_count": 26, "eval_count": 290}),
        ];
        let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
        let answer = streamed("application/x-ndjson", &body);
        assert_eq!(counted(Format::Ollama, &answer), tokens(26, 290, 0));
        let answer = whole("application/json", &lines[1].to_string());
        assert_eq!(counted(Format::Ollama, &answer), tokens(26, 290, 0));
    }

    /// What isn't an answer counts nothing; an answer that can't be
    /// read, or says what can't be, counts as one without usage.
    #[test]
    fn what_cant_be_read_is_a_gap_never_a_number() {
        let usage = json!({"usage": {"input_tokens": 10, "output_tokens": 2}}).to_string();
        let refused = format!(
            "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\n\r\n{usage}",
            usage.len()
        );
        assert_eq!(
            counted(Format::Anthropic, refused.as_bytes()),
            Tokens::default()
        );
        let compressed = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Encoding: gzip\r\n\
             Content-Length: {}\r\n\r\n{usage}",
            usage.len()
        );
        assert_eq!(counted(Format::Anthropic, compressed.as_bytes()), UNMETERED);
        let absurd = json!({"usage": {"input_tokens": 10_000_000_000_u64, "output_tokens": 2}});
        let answer = whole("application/json", &absurd.to_string());
        assert_eq!(counted(Format::Anthropic, &answer), UNMETERED);
        // A later event that says 0 erases no count.
        let body = events(&[
            (
                "",
                json!({"type": "message_start", "message": {"usage": {"input_tokens": 10,
                "cache_read_input_tokens": 4, "output_tokens": 1}}}),
            ),
            (
                "",
                json!({"type": "message_delta", "usage": {"input_tokens": 0,
                "cache_read_input_tokens": 0, "output_tokens": 6}}),
            ),
        ]);
        let answer = whole("text/event-stream", &body);
        assert_eq!(counted(Format::Anthropic, &answer), tokens(14, 6, 4));
        // A 100 Continue first: the answer after it.
        let mut answer = b"HTTP/1.1 100 Continue\r\n\r\n".to_vec();
        answer.extend(whole("application/json", &usage));
        assert_eq!(counted(Format::Anthropic, &answer), tokens(10, 2, 0));
        // An event past the most read: its usage isn't, and the gap shows.
        let big = format!(
            "data: {}\n\n",
            json!({"text": "x".repeat(MAX_EVENT), "usage": {"prompt_tokens": 1}})
        );
        let mut meter = Meter::new(Format::OpenAi);
        meter.feed(&whole("text/event-stream", &big));
        assert_eq!(meter.finish(), UNMETERED);
    }
}
