link_test_kit::program!(system);

mod bridged;
mod calls;
mod hosted;
mod session;

/// A call's text as the model reads it plainly: what a file says is
/// fenced as third-party text, given back here as the text it holds.
pub(crate) fn said(result: &serde_json::Value) -> String {
    link_untrusted::unfenced(result["content"][0]["text"].as_str().unwrap_or_default())
}

/// A call's answer as the model reads it, as JSON: what a file says (a
/// line found, a change's diff) is in it alone, never in the
/// structured content.
pub(crate) fn answered(result: &serde_json::Value) -> serde_json::Value {
    serde_json::from_str(&said(result)).unwrap_or_else(|_| panic!("{result}"))
}

/// What a `glob` or `grep` found, as its structured content gives it:
/// each path, with a line's number or a count where it has one, never
/// what a file says.
pub(crate) fn finds(result: &serde_json::Value) -> serde_json::Value {
    result["structuredContent"]["found"].clone()
}

/// `text`'s lines as `read` gives them, each after its number.
pub(crate) fn numbered(text: &str) -> String {
    text.split_inclusive('\n')
        .zip(1..)
        .map(|(line, n)| format!("{n:>6}→{line}"))
        .collect()
}
