//! Link Memory as an MCP server: a JSON-RPC message a line, each way,
//! answered through a [`Reader`] ([`crate::serve::session`]). Its tools,
//! each answering from what's indexed, never waiting for indexing, all
//! read-only but `remember`, offered only where Link names the agent's
//! notes:
//!
//! - `search`: the documents matching a query's words or its meaning,
//!   of a kind, under a folder or changed after a time, best first, up
//!   to a size in tokens and where to carry on (`next`, then `skip`), each with its kind (`file` or `conversation`), where it
//!   matched and why (`matched`); and, while embedding is behind or off,
//!   how far it got (`embedded`) or why (`semantic`).
//! - `read`: a document's text, or some of its lines, up to a size in
//!   tokens, and where to carry on (`next`); a skill's file too, by its
//!   path, though it's never indexed.
//! - `list`: the documents changed most recently, of a kind, under a
//!   folder or changed after a time; with a time, each new or changed,
//!   and what was removed after it.
//! - `links`: what a document links to and what links to it, inside
//!   the folder.
//! - `remember`: a note added to the agent's notes, one changed or one
//!   forgotten ([`crate::notes`]), which its prompt holds from its next
//!   message on.

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::notes::Notes;
use crate::pages::Page;
use crate::{Filter, Index, Kind, Reader, parse_since, tokens};

/// The MCP versions it speaks, the newest first.
const VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// The most each call is asked for, as its declaration bounds it
/// (`actions.toml`'s `maximum`), read once from the calls it renders.
struct Bounds {
    /// The most results a search gives.
    search: usize,
    /// The most documents a list gives.
    list: usize,
    /// The most a call gives, in tokens.
    tokens: usize,
}

/// [`Bounds`], from [`TEMPLATE`].
fn bounds() -> &'static Bounds {
    static BOUNDS: std::sync::LazyLock<Bounds> = std::sync::LazyLock::new(|| {
        let template =
            link_actions::tools::Template::parse(TEMPLATE).expect("memory's tools template");
        let maximum = |call: &str, argument: &str| {
            template
                .tools
                .iter()
                .find(|tool| tool.name == call)
                .and_then(|tool| tool.input_schema["properties"][argument]["maximum"].as_u64())
                .unwrap_or_else(|| panic!("memory's {call} declares the most {argument}"))
                as usize
        };
        Bounds {
            search: maximum("search", "limit"),
            list: maximum("list", "limit"),
            tokens: maximum("search", "tokens"),
        }
    });
    &BOUNDS
}

/// The most a search or a read gives, in tokens, unless asked for
/// another, when Link doesn't say (`serve --tokens`, from the agent's
/// spec): the most any call is asked for (its declared `maximum`), under
/// what Link Harness keeps of a tool's answer (32 KiB).
pub fn tokens_max() -> usize {
    bounds().tokens
}

/// Its call that writes the agent's notes, and nothing else.
pub const REMEMBER: &str = "remember";

/// How it serves: the most a search or a read gives unless asked for
/// another, and the agent's notes, when it may write them.
#[derive(Clone, Debug)]
pub struct Options {
    pub tokens: usize,
    pub notes: Option<Notes>,
    /// Whether the agent's harness is sent messages from others
    /// (`--from-others`, [`link_memory_tool_contract::FROM_ENV`]): then `remember`
    /// writes only in a conversation of the owner's ([`owners`]).
    pub from: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            tokens: tokens_max(),
            notes: None,
            from: false,
        }
    }
}

/// Whether a call made in the conversation `meta` names (its `_meta`'s
/// `link.local/thread`) may write the agent's notes: only one of the
/// owner's. Never a teammate's (`agent-<id>`), a team's (`team-<id>`), a
/// subagent's (`task-<id>`, which can't show its parent was the owner's)
/// or a stranger's (named as its sender, `webhook:<name>`), and never a
/// call that names none: the harness names each, so one that doesn't
/// isn't taken for the owner's.
fn owners(meta: Option<&Value>) -> Result<()> {
    let thread = meta
        .and_then(|meta| meta.get(link_connection_contract::access::THREAD_META))
        .and_then(Value::as_str)
        .filter(|thread| !thread.is_empty());
    let others = !thread.is_some_and(link_connection_contract::access::is_owners_thread);
    anyhow::ensure!(
        !others,
        "remembering is the owner's alone, and this conversation isn't the owner's (a \
         teammate's, a team's, a subagent's or a stranger's, or one the call doesn't name): \
         nothing was noted, and it won't be here; tell the owner instead, in your answer"
    );
    Ok(())
}

/// The reply to message `line`, from `index`'s own reader: `None` for a
/// notification, or a reply the client sent to a request of ours (it
/// sends none: we ask nothing).
pub fn answer(index: &Index, line: &str) -> Option<Value> {
    answer_with(index.reader(), &Options::default(), line)
}

/// [`answer`], through `reader`, as `options` say.
pub fn answer_with(reader: &Reader, options: &Options, line: &str) -> Option<Value> {
    let tokens = options.tokens;
    let Ok(message) = serde_json::from_str::<Value>(line) else {
        return Some(error(Value::Null, -32700, "not JSON".into()));
    };
    if !message.is_object() {
        return Some(error(Value::Null, -32600, "not a JSON-RPC message".into()));
    }
    let method = message.get("method").and_then(Value::as_str)?;
    let id = message.get("id")?.clone();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str);
            let version = VERSIONS
                .iter()
                .find(|v| Some(**v) == asked)
                .unwrap_or(&VERSIONS[0]);
            result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "link-memory", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": match options.notes {
                        None => INSTRUCTIONS.to_string(),
                        Some(_) => format!("{INSTRUCTIONS} {REMEMBERING}"),
                    },
                }),
            )
        }
        "ping" => result(id, json!({})),
        "tools/list" => match offered(tokens, options.notes.as_ref()) {
            Ok(tools) => result(id, json!({"tools": tools})),
            Err(why) => error(id, -32603, format!("{why:#}")),
        },
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            // Its arguments, refused by their declaration before its handler.
            let checked = || crate::cli::check_call(name, &arguments).map_err(anyhow::Error::from);
            let outcome = match (name, &options.notes) {
                (REMEMBER, Some(notes)) => checked().and_then(|()| {
                    match options.from {
                        true => owners(params.get("_meta")),
                        false => Ok(()),
                    }
                    .and_then(|()| remember(notes, &arguments, params.get("_meta")))
                }),
                // The calls that only read, the index's to answer.
                _ if link_memory_tool_contract::reads()
                    .is_ok_and(|reads| reads.iter().any(|r| r == name)) =>
                {
                    checked().and_then(|()| call(reader, tokens, name, &arguments))
                }
                _ => return Some(error(id, -32602, format!("no tool {name:?}"))),
            };
            let outcome = outcome.and_then(|value| {
                let mut presented = value.clone();
                if name == "read"
                    && let Some(presented) = presented.as_object_mut()
                {
                    presented.remove("read");
                }
                let text = fenced(name, &arguments, &presented.to_string())?;
                Ok((text, value))
            });
            result(
                id,
                match outcome {
                    Ok((text, value)) => json!({
                        "content": [{"type": "text", "text": text}],
                        "structuredContent": structured(name, value),
                    }),
                    Err(err) => json!({
                        "content": [{"type": "text", "text": format!("{err:#}")}],
                        "isError": true,
                    }),
                },
            )
        }
        other => error(id, -32601, format!("no method {other:?}")),
    })
}

/// Call `name`'s answer as its structured content: without what the
/// documents say, which only the fenced text gives (a title, a snippet,
/// a link as written, a document's text), so a client that gives a
/// model this sees none of it bare. `read`'s read receipt (`read`) stays,
/// naming its text by its digest, which a reader checks against the
/// fenced text.
fn structured(name: &str, mut value: Value) -> Value {
    if name == "read"
        && let Some(document) = value.as_object_mut()
    {
        document.remove("text");
        document.remove("title");
        return value;
    }
    let said: &[(&str, &str)] = match name {
        "search" => &[("results", "title"), ("results", "snippet")],
        "list" => &[("documents", "title"), ("removed", "title")],
        "links" => &[("to", "title"), ("from", "title"), ("missing", "target")],
        _ => return value,
    };
    if name == "links"
        && let Some(links) = value.as_object_mut()
    {
        links.remove("title");
    }
    for (list, field) in said {
        let items = value.get_mut(*list).and_then(Value::as_array_mut);
        for item in items.into_iter().flatten() {
            if let Some(item) = item.as_object_mut() {
                item.remove(*field);
            }
        }
    }
    value
}

/// What the server says of itself, in its handshake.
const INSTRUCTIONS: &str = "Search, read and list the files in your memory folder, and your past \
    conversations, and follow their links. Each result says which it is.";

/// ... and of `remember`, where it has it.
const REMEMBERING: &str = "Note with 'remember' what you'll want in every conversation: what \
    your owner tells you to keep in mind, their preferences, facts about them and their work. \
    When your owner gives a stable fact such as their name or favorite color, call 'remember' \
    in that turn before claiming it is saved. Conversation history is not a saved memory: \
    a new chat or another connection will not contain this transcript. Only say a fact was \
    saved after 'remember' succeeds; if it fails, explain that it was not saved. \
    Your notes are in your instructions from the next message on; keep them short, and change \
    or forget one that no longer holds. Never note a secret (a password, a key, a token, a PIN \
    or a code that opens something): a note that looks like one is refused.";

/// `remember`'s call, made in the conversation `meta` names, which the
/// note says it was saved in.
fn remember(notes: &Notes, arguments: &Value, meta: Option<&Value>) -> Result<Value> {
    let text = |key: &str| match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(_) => anyhow::bail!("'{key}' is text"),
    };
    let note = text("note")?.unwrap_or_default();
    let thread = meta
        .and_then(|meta| meta.get(link_connection_contract::access::THREAD_META))
        .and_then(Value::as_str);
    Ok(serde_json::to_value(notes.remember_from(
        note,
        text("replaces")?,
        thread,
    )?)?)
}

/// `text`, call `name`'s answer, fenced as third-party text where it
/// holds what was kept (conversations, documents and their names): a
/// search's from its query, a read's and a document's links from its
/// path, a listing as the memory's. `remember`'s says only what it did.
fn fenced(name: &str, arguments: &Value, text: &str) -> Result<String> {
    use link_untrusted::Kind;
    let asked = |key: &str| arguments.get(key).and_then(Value::as_str);
    let (kind, from) = match name {
        "search" => (Kind::Search, asked("query")),
        "read" | "links" => (Kind::Document, asked("path")),
        "list" => (Kind::File, None),
        _ => return Ok(text.to_string()),
    };
    let source = link_untrusted::Source {
        kind,
        name: from.unwrap_or("your memory"),
    };
    Ok(link_untrusted::fence(source, text)?)
}

fn call(index: &Reader, most: usize, name: &str, arguments: &Value) -> Result<Value> {
    let text = |key: &str| arguments.get(key).and_then(Value::as_str);
    // A number given as another type is refused, never taken as left out.
    let number = |key: &str| -> Result<Option<usize>> {
        match arguments.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(given) => Ok(Some(
                given
                    .as_u64()
                    .with_context(|| format!("'{key}' is a whole number, not {given}"))?
                    as usize,
            )),
        }
    };
    let since = match arguments.get("since") {
        None | Some(Value::Null) => None,
        Some(Value::Number(n)) => Some(n.as_i64().context("'since' is seconds since 1970")?),
        Some(Value::String(s)) => Some(parse_since(s)?),
        Some(_) => anyhow::bail!("'since' is a date ('2026-09-01') or seconds since 1970"),
    };
    let filter = Filter {
        kind: text("kind").map(Kind::parse).transpose()?,
        under: text("under").map(str::to_string),
        since,
    };
    let budget = number("tokens")?.unwrap_or(most).max(1);
    let page = |kind: &str| text("page").map(|page| Page::parse(page, kind)).transpose();
    Ok(match name {
        "search" => {
            let query = text("query").context("search needs a 'query'")?;
            let limit = number("limit")?.unwrap_or(10).clamp(1, bounds().search);
            let from = match page("search")? {
                Some(Page::Search { at }) => at,
                _ => 0,
            };
            // How far indexing has got, read first: what the search finds is
            // at least as current as what it says is indexed.
            let status = serde_json::to_value(index.status()?)?;
            // One more than asked for, to know whether there are more.
            let searched = index.search_from(query, filter, from, limit + 1)?;
            let mut hits = searched.hits;
            let found = hits.len();
            hits.truncate(limit);
            // Whole results while they fit, at least one.
            let mut used = 0;
            let fit = hits
                .iter()
                .take_while(|hit| {
                    used += tokens(&serde_json::to_string(hit).unwrap_or_default());
                    used <= budget
                })
                .count()
                .max(1);
            hits.truncate(fit);
            let next = hits.last().map(|hit| Page::Search { at: hit.at }.token());
            let mut reply = json!({"results": hits});
            match (
                found > reply["results"].as_array().map_or(0, Vec::len),
                next,
            ) {
                (true, Some(next)) => reply["next"] = json!(next),
                _ if searched.beyond => reply["more"] = json!(PAST_SEARCH),
                _ => {}
            }
            if searched.stopped {
                reply["stopped"] = json!(STOPPED);
            }
            // How far embedding has got, or why meaning is off.
            if let Value::Object(status) = status {
                reply.as_object_mut().expect("an object").extend(status);
            }
            reply
        }
        "read" => {
            let path = text("path").context("read needs a 'path'")?;
            let from = number("from")?.unwrap_or(1);
            // A document holds at most one line a byte.
            let lines = number("lines")?
                .map(|n| n.clamp(1, crate::limits::limits().file_max_bytes as usize));
            match index.read(path, from, lines, Some(budget))? {
                Some(document) => serde_json::to_value(document)?,
                None => anyhow::bail!("nothing at {path:?}: 'search' or 'list' give each path"),
            }
        }
        "links" => {
            let path = text("path").context("links needs a 'path'")?;
            let Some(mut links) = index.links(path)? else {
                anyhow::bail!("nothing at {path:?}: 'search' or 'list' give each path");
            };
            // Counted through `to`, `from`, then `missing`.
            let skip = number("skip")?.unwrap_or(0);
            let mut left = skip;
            left -= drain(&mut links.to, left);
            left -= drain(&mut links.from, left);
            drain(&mut links.missing, left);
            // In order: once one is cut, none after it is shown, so `next`
            // counts only what came before where it stopped.
            let mut used = 0;
            let mut cut = fit(&mut links.to, budget, &mut used);
            match cut {
                true => links.from.clear(),
                false => cut = fit(&mut links.from, budget, &mut used),
            }
            match cut {
                true => links.missing.clear(),
                false => cut = fit(&mut links.missing, budget, &mut used),
            }
            let shown = links.to.len() + links.from.len() + links.missing.len();
            let next = cut.then_some(skip + shown);
            with_next(serde_json::to_value(links)?, cut, next)
        }
        _ => {
            let limit = number("limit")?.unwrap_or(50).clamp(1, bounds().list);
            let after = match page("list")? {
                Some(Page::List { changed, path }) => Some((changed, path)),
                _ => None,
            };
            let mut listing = index.list_after(
                &filter,
                after.as_ref().map(|(c, p)| (*c, p.as_str())),
                limit,
            )?;
            // What was removed first: it comes only with the first page,
            // so what's cut of it no `next` reaches. Then the documents,
            // at least one, so a `next` always moves on.
            let mut used = 0;
            let mut cut = false;
            if let Some(removed) = &mut listing.removed {
                cut |= fit(removed, budget, &mut used);
            }
            let spent = used;
            used = 0;
            cut |= fit(
                &mut listing.documents,
                budget.saturating_sub(spent),
                &mut used,
            );
            // Where it stopped, short of the size or the limit.
            let next = listing
                .documents
                .last()
                .filter(|_| cut || listing.more)
                .map(|last| {
                    Page::List {
                        changed: last.changed,
                        path: last.path.clone(),
                    }
                    .token()
                });
            with_next(serde_json::to_value(listing)?, cut, next)
        }
    })
}

/// Keeps the first of `items` while they fit in `budget` tokens, with
/// `used` spent before them: whether any were dropped. A page keeps its
/// first item whatever it costs, so a `next` always moves on, never
/// naming the page it's on.
fn fit<T: serde::Serialize>(items: &mut Vec<T>, budget: usize, used: &mut usize) -> bool {
    let fits = items
        .iter()
        .take_while(|item| {
            let cost = tokens(&serde_json::to_string(item).unwrap_or_default());
            let fits = *used == 0 || *used + cost <= budget;
            if fits {
                *used += cost.max(1);
            }
            fits
        })
        .count();
    let cut = fits < items.len();
    items.truncate(fits);
    cut
}

/// `reply`, saying `cut` when some of it didn't fit, and `next`, what
/// carries on (a `skip` or a `page`), when there's more.
fn with_next(mut reply: Value, cut: bool, next: Option<impl serde::Serialize>) -> Value {
    if let Some(object) = reply.as_object_mut() {
        if cut {
            object.insert("cut".into(), json!(true));
        }
        if let Some(next) = next {
            object.insert("next".into(), json!(next));
        }
    }
    reply
}

/// Drops the first `n` of `items`, at most all: how many it dropped.
fn drain<T>(items: &mut Vec<T>, n: usize) -> usize {
    let n = n.min(items.len());
    items.drain(..n);
    n
}

/// What a search says past the most it ranks, where no `next` reaches.
const PAST_SEARCH: &str =
    "more match, past the most a search goes: narrow it with 'under', 'kind' or 'since'";

/// What a search says when it stopped ranking at its deadline.
const STOPPED: &str = "the search stopped ranking at its deadline: these are the best of what \
    it ranked, and more may match: narrow it with 'under', 'kind' or 'since', or add words";

/// The calls `actions.toml` declares, as its contract renders them.
const TEMPLATE: &str = link_memory_tool_contract::TOOLS;

/// The tools, as `tools/list` gives them, from `actions.toml`:
/// `remember` with `notes`.
fn offered(tokens: usize, notes: Option<&Notes>) -> Result<Value> {
    use link_actions::tools::{Fill, Fills};
    // Not the notes' path, which `read` doesn't serve: the notes reach
    // the model in its instructions.
    let most = notes
        .map(|notes| notes.most.to_string())
        .unwrap_or_default();
    let fills = Fills {
        values: [
            ("tokens", Fill::Text(tokens.to_string())),
            ("most", Fill::Text(most)),
        ]
        .into(),
        conditions: notes.map(|_| "notes").into_iter().collect(),
    };
    link_actions::tools::offer(TEMPLATE, &fills)
}

fn result(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error(id: Value, code: i64, message: String) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

#[cfg(test)]
mod read_receipts;

#[cfg(test)]
mod tests {
    use super::*;

    mod declared;

    /// An item past the budget alone is still kept, first on its page,
    /// so a model following `next` moves on; after it, nothing past.
    #[test]
    fn a_page_keeps_its_first_item_whatever_it_costs() {
        let mut items = vec!["x".repeat(400), "y".into(), "z".into()];
        let mut used = 0;
        assert!(fit(&mut items, 10, &mut used));
        assert_eq!(items.len(), 1);
        let mut more = vec!["w".to_string()];
        assert!(fit(&mut more, 10, &mut used));
        assert!(more.is_empty());
    }

    /// The declared calls as serve offers them: `remember` only with
    /// notes, saying where they are and how much they hold, each limit
    /// the one the handler keeps to, and each title naming what its call
    /// works on.
    #[test]
    fn since_is_declared_as_the_handler_reads_it() {
        let tools = offered(6000, None).unwrap();
        let since = &tools[1]["inputSchema"]["properties"]["since"];
        let pattern = regex::Regex::new(since["pattern"].as_str().unwrap()).unwrap();
        for good in [
            "2026-09-01",
            "2026-09-01T14:30:00Z",
            "2026-09-01 14:30",
            "1788000000",
        ] {
            assert!(pattern.is_match(good), "{good}");
            crate::reading::parse_since(good).unwrap();
        }
        for bad in ["yesterday", "2026-09", "-5", ""] {
            assert!(!pattern.is_match(bad), "{bad}");
        }
    }

    #[test]
    fn the_declared_tools_are_filled_as_serve_says() {
        let root = link_test_kit::TempDir::new("memory-declared");
        let notes = Notes::new(root.path(), &root.path().join("notes/notes.md"), 4096).unwrap();
        let names = |tools: &Value| -> Vec<String> {
            tools
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["name"].as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(
            names(&offered(6000, None).unwrap()),
            ["links", "list", "read", "search"]
        );
        let tools = offered(6000, Some(&notes)).unwrap();
        let named = |name: &str| {
            tools
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["name"] == name)
                .cloned()
                .unwrap()
        };
        let remember = named("remember");
        let said = remember["description"].as_str().unwrap();
        assert!(
            !said.contains(&notes.path()) && said.contains("4096 bytes"),
            "{said}"
        );
        assert_eq!(remember["annotations"]["destructiveHint"], true);
        let search = named("search");
        let most = |call: &str, argument: &str| {
            named(call)["inputSchema"]["properties"][argument]["maximum"].clone()
        };
        assert_eq!(most("search", "limit"), json!(bounds().search));
        assert!(search.to_string().contains("6000 if left out"));
        // No call asks for more than the most it's given unasked: each
        // declares the same most.
        for call in ["search", "read", "list", "links"] {
            assert_eq!(most(call, "tokens"), json!(tokens_max()), "{call}");
        }
        assert_eq!(most("list", "limit"), json!(bounds().list));
        assert!(bounds().search > 0 && bounds().list >= bounds().search);
        assert_eq!(search["_meta"]["link.local/summary"], json!(["query"]));
    }
}
