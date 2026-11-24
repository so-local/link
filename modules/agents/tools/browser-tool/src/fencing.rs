//! What the page wrote, fenced as third-party text in the browser's
//! notes: the browser's own words stay outside the fence, and each note
//! holds at most `note_bytes` of the page's (`browser.toml`).

use anyhow::Result;
use serde_json::Value;

/// A download refused, said.
pub(crate) fn refused_download(download: &Value) -> Result<String> {
    let name = download["params"]["suggestedFilename"]
        .as_str()
        .unwrap_or("a file");
    Ok(format!(
        "A download was refused: this browser downloads nothing. Its name:\n{}",
        by_page(link_untrusted::Kind::Dialog, PAGE, name)?
    ))
}

/// What the fence names as the source of a dialog or a name the page
/// gave, where its address isn't at hand.
pub(crate) const PAGE: &str = "the page";

/// `text`, which the page wrote, from `from`, in the fence every tool
/// puts around third-party text, at most `note_bytes` of it: the
/// browser's own notes stay outside, so one the page forges stays
/// inside, and what was cut is said after it.
pub(crate) fn by_page(kind: link_untrusted::Kind, from: &str, text: &str) -> Result<String> {
    let most = crate::settings::get().note_bytes;
    let (text, cut) = crate::capped(text, most);
    let (from, _) = crate::capped(from, most);
    let fenced = page_text(kind, from, text)?;
    Ok(match cut {
        0 => fenced,
        cut => format!("{fenced}\n[The page's words were cut at {most} bytes: {cut} more.]"),
    })
}

/// `text`, which the page wrote, from `from`, fenced whole: a read's
/// part, which its own limit holds ([`crate::READ`]).
pub(crate) fn page_text(kind: link_untrusted::Kind, from: &str, text: &str) -> Result<String> {
    Ok(link_untrusted::fence(
        link_untrusted::Source { kind, name: from },
        text,
    )?)
}
