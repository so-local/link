//! Link Memory's tests, one program: each file a module, so `cargo test`
//! links once, not once per file. One file: `cargo test --test memory
//! index::`.

link_test_kit::program!(system);

#[cfg(target_os = "linux")]
mod agent;
#[cfg(unix)]
mod home;
mod index;
mod indexer;
#[cfg(target_os = "linux")]
mod journal;
mod mcp;
mod notes;
mod passages;
mod quality;
mod reads_conversations;
#[cfg(target_os = "linux")]
mod sandbox;
mod semantic;
#[cfg(unix)]
mod serve;
#[cfg(unix)]
mod service;
mod vectors;
mod words;
mod writeback;

/// Counts what each thread holds, for a test of what one holds
/// ([`writeback`]).
#[global_allocator]
static COUNTED: writeback::Counted = writeback::Counted;

/// A `read`'s document as the model reads it: the fenced JSON of its
/// answer's `content`, given back plainly. What a document says is
/// there alone, never in the structured content.
pub(crate) fn document(reply: &serde_json::Value) -> serde_json::Value {
    let text = reply["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    serde_json::from_str(&link_untrusted::unfenced(text)).unwrap_or_else(|_| panic!("{reply}"))
}
