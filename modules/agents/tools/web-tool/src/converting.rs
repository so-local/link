//! A page made markdown, a few at a time, and text in parts.

use super::*;
use crate::page::Page;
use crate::writing::{self, compact};

/// The pages being made markdown, a few at once (`limits.toml`'s
/// `conversions`): a page's tree and its markdown take many times the
/// page, so the rest wait their turn.
pub(super) static CONVERTING: std::sync::LazyLock<Slots> =
    std::sync::LazyLock::new(|| Slots::new(crate::limits().conversions));

/// At most so many of something at once: [`Slots::take`] waits for one.
pub struct Slots {
    taken: std::sync::Mutex<usize>,
    freed: std::sync::Condvar,
    most: usize,
}

impl Slots {
    pub const fn new(most: usize) -> Self {
        Self {
            taken: std::sync::Mutex::new(0),
            freed: std::sync::Condvar::new(),
            most,
        }
    }

    /// A slot, once one is free: given back when it's dropped.
    pub fn take(&self) -> Slot<'_> {
        let mut taken = self.taken.lock().unwrap_or_else(|e| e.into_inner());
        while *taken >= self.most {
            taken = self.freed.wait(taken).unwrap_or_else(|e| e.into_inner());
        }
        *taken += 1;
        Slot(self)
    }

    /// How many are taken now.
    pub fn taken(&self) -> usize {
        *self.taken.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// A slot of [`Slots`], held until it's dropped.
pub struct Slot<'a>(&'a Slots);

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        *self.0.taken.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
        self.0.freed.notify_one();
    }
}

/// Page `html`, fetched from `url`, as markdown, each link made whole
/// against the page's base (its `<base href>`, else `url`) so the model
/// can fetch it.
pub fn markdown(html: &str, url: &Url) -> String {
    let page = Page::parse(html);
    let base = &page.base(url);
    // Markdown is written by recursion: a page nested deeper than a page
    // of content ever is would overflow the stack, so it's read as text,
    // its scripts, styles, and hidden parts left out as ever.
    let depth_max = crate::limits().depth_max;
    if page.depth(depth_max) > depth_max {
        return writing::text(&page);
    }
    compact(&writing::write(&page, base)).trim().to_string()
}

/// The page's `<title>`, as plain text.
pub(super) fn title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let open = lower.find("<title")?;
    let start = open + lower[open..].find('>')? + 1;
    let end = start + lower[start..].find("</title")?;
    let title = plain(&html[start..end]);
    (!title.is_empty()).then(|| title.chars().take(300).collect())
}

/// `text` without tags, its entities read and its spaces one: a
/// snippet's `<strong>`, a title's `&amp;`.
pub fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = entities(&out);
    out.split_whitespace()
        .map(|word| word.chars().filter(|c| shown(*c)).collect::<String>())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The characters that change how text around them reads without being
/// read: Unicode's directional marks, embeddings, overrides, and
/// isolates, with which a title can show its words in another order.
const DIRECTIONAL: [char; 12] = [
    '\u{061C}', '\u{200E}', '\u{200F}', '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}',
    '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
];

/// Whether `c` is kept in a title or snippet: never a control character
/// or a directional one.
fn shown(c: char) -> bool {
    !c.is_control() && !DIRECTIONAL.contains(&c)
}

/// `text` with its character references read, in one pass so `&amp;lt;`
/// stays `&lt;`: the named ones titles and snippets use, and every
/// numeric one, decimal (`&#8217;`) or hexadecimal (`&#x2019;`), its
/// digits alone (`&#+65;` isn't one). One it can't read, or naming no
/// character, is kept as it is.
fn entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let read = rest[1..]
            .find(';')
            .filter(|end| *end <= crate::limits::limits().entity_name_max)
            .and_then(|end| {
                let name = &rest[1..1 + end];
                let c = match name {
                    "lt" => '<',
                    "gt" => '>',
                    "quot" => '"',
                    "apos" => '\'',
                    "nbsp" => ' ',
                    "amp" => '&',
                    _ => {
                        let number = name.strip_prefix('#')?;
                        let (digits, radix) = match number.strip_prefix(['x', 'X']) {
                            Some(hex) => (hex, 16),
                            None => (number, 10),
                        };
                        if !digits.chars().all(|c| c.is_digit(radix)) {
                            return None;
                        }
                        let code = u32::from_str_radix(digits, radix).ok()?;
                        char::from_u32(code).filter(|c| *c != '\0')?
                    }
                };
                Some((c, end + 2))
            });
        match read {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Where a part asked from byte `start` of `text` begins: there, or the
/// next character's start, never inside one; at most the end.
pub fn boundary(text: &str, start: usize) -> usize {
    let mut start = start.min(text.len());
    while !text.is_char_boundary(start) {
        start += 1;
    }
    start
}

/// Of `text`, the part from byte `start` ([`boundary`]), at most `most`
/// bytes, cut at a line's end where one is near: the part, and where the
/// next starts (none at the end).
pub fn part(text: &str, start: usize, most: usize) -> (&str, Option<usize>) {
    let start = boundary(text, start);
    let rest = &text[start..];
    if rest.len() <= most {
        return (rest, None);
    }
    let mut end = most;
    while !rest.is_char_boundary(end) {
        end -= 1;
    }
    // A line's end in the last fifth, so a line isn't split.
    if let Some(newline) = rest[..end].rfind('\n').filter(|n| *n >= end - end / 5) {
        end = newline + 1;
    }
    (&rest[..end], Some(start + end))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page nested deeper than any page of content is read as text,
    /// never overflowing the stack.
    #[test]
    fn a_deeply_nested_page_is_read_as_text() {
        let html = format!(
            "<html><body>{}deep words{}</body></html>",
            "<div>".repeat(4_000),
            "</div>".repeat(4_000)
        );
        let base = Url::parse("https://example.com/").unwrap();
        let text = std::thread::Builder::new()
            .stack_size(256 << 10)
            .spawn(move || markdown(&html, &base))
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(text, "deep words");
        // Just within the limit, written on a thread's usual 2 MB.
        let html = format!(
            "<html><body>{}near words{}</body></html>",
            "<blockquote><ul><li>".repeat(169),
            "</li></ul></blockquote>".repeat(169)
        );
        let text = std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(move || markdown(&html, &Url::parse("https://example.com/").unwrap()))
            .unwrap()
            .join()
            .unwrap();
        assert!(text.ends_with("- near words"), "{text}");
        let shallow = markdown(
            "<p><a href=\"/x\">x</a></p>",
            &Url::parse("https://example.com/a/").unwrap(),
        );
        assert!(shallow.contains("https://example.com/x"), "{shallow}");
    }

    /// No more than its most at once; each waits for a slot.
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn slots_hold_at_most_their_most() {
        let slots = Slots::new(2);
        let most = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    let _slot = slots.take();
                    most.fetch_max(slots.taken(), std::sync::atomic::Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(20));
                });
            }
        });
        assert_eq!(most.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(slots.taken(), 0);
    }

    #[test]
    fn a_page_is_markdown_its_links_whole() {
        let base = Url::parse("https://example.com/news/today.html").unwrap();
        let html = "<html><head><title>Today &amp; now</title><script>evil()</script>\
            <style>p{}</style></head><body><nav><a href=\"/\">Home</a></nav>\
            <h1>Headline</h1><p>First <b>bold</b> line, <a href=\"../about\">about</a>.</p>\
            <img src=\"x.png\"><ul><li>one</li><li>two</li></ul>\
            <a href=\"javascript:alert(1)\">click</a><footer>(c)</footer></body></html>";
        let md = markdown(html, &base);
        assert!(md.starts_with("# Headline"), "{md}");
        assert!(md.contains("**bold**"), "{md}");
        assert!(md.contains("[about](https://example.com/about)"), "{md}");
        assert!(md.contains("one") && md.contains("two"), "{md}");
        for gone in ["evil", "p{}", "Home", "x.png", "javascript", "(c)"] {
            assert!(!md.contains(gone), "{gone} in {md}");
        }
        assert_eq!(title(html).as_deref(), Some("Today & now"));
    }

    /// A page's `<base href>` is what its links are made whole against,
    /// itself made whole against where the page came from; a base that
    /// isn't the web's is ignored.
    #[test]
    fn links_follow_the_pages_base() {
        let url = Url::parse("https://example.com/news/today.html").unwrap();
        let page = |base: &str| {
            format!(
                "<html><head><base href=\"{base}\"></head>\
                 <body><p><a href=\"about\">about</a></p></body></html>"
            )
        };
        let md = markdown(&page("https://cdn.example.org/docs/"), &url);
        assert!(
            md.contains("[about](https://cdn.example.org/docs/about)"),
            "{md}"
        );
        let md = markdown(&page("/archive/"), &url);
        assert!(
            md.contains("[about](https://example.com/archive/about)"),
            "{md}"
        );
        let md = markdown(&page("javascript:void(0)/"), &url);
        assert!(
            md.contains("[about](https://example.com/news/about)"),
            "{md}"
        );
    }

    #[test]
    fn a_long_page_comes_in_parts() {
        let text = "line one\nline two\nline three\n";
        assert_eq!(part(text, 0, 100), (text, None));
        // Cut at a line's end near the most.
        let (first, next) = part(text, 0, 20);
        assert_eq!(first, "line one\nline two\n");
        let (second, end) = part(text, next.unwrap(), 20);
        assert_eq!(second, "line three\n");
        assert_eq!(end, None);
        // Never inside a character.
        let text = "ééééé";
        let (first, next) = part(text, 0, 3);
        assert_eq!(first, "é");
        assert_eq!(part(text, next.unwrap(), 100).0, "éééé");
        assert_eq!(part(text, 1, 100).0, "éééé");
        assert_eq!(boundary(text, 1), 2);
        assert_eq!(boundary(text, 99), text.len());
        assert_eq!(part(text, 99, 100), ("", None));
    }

    #[test]
    fn snippets_are_plain() {
        assert_eq!(
            plain("The <strong>news</strong> &amp; more\n  today"),
            "The news & more today"
        );
        // Numeric references read too, and read once.
        assert_eq!(
            plain("It&#8217;s &#x201C;here&#X201D; &#39;now&#39; &amp;lt; &#0; &bogus; & &#xZZ;"),
            "It\u{2019}s \u{201C}here\u{201D} 'now' &lt; &#0; &bogus; & &#xZZ;"
        );
        // A reference is its digits alone, signed ones kept as written.
        assert_eq!(plain("&#+65; &#x+41; &#65;"), "&#+65; &#x+41; A");
        // Control and directional characters never reach a title or a
        // snippet, sent as they are or as references.
        assert_eq!(
            plain("safe\u{7}\u{202E}txt.exe &#x202E;gpj &#x1b;[31mred"),
            "safetxt.exe gpj [31mred"
        );
    }

    /// Hidden text is never read to the model, written as markdown or as
    /// text for a page nested too deep, and the text fallback keeps out
    /// scripts and styles.
    #[test]
    fn hidden_text_scripts_and_styles_are_left_out() {
        let base = Url::parse("https://example.com/").unwrap();
        let body = "<p>shown</p><p hidden>attr</p><p aria-hidden=\"true\">aria</p>\
            <div style=\"color: red; DISPLAY : none !important\">styled</div>\
            <span style=\"visibility:hidden\">invisible</span>\
            <template>templated</template><script>run()</script><style>p{}</style>";
        let md = markdown(&format!("<html><body>{body}</body></html>"), &base);
        assert_eq!(md, "shown");
        let deep = format!(
            "<html><head><style>q{{}}</style></head><body>{}{body}{}</body></html>",
            "<div>".repeat(600),
            "</div>".repeat(600)
        );
        let text = std::thread::Builder::new()
            .stack_size(256 << 10)
            .spawn(move || markdown(&deep, &Url::parse("https://example.com/").unwrap()))
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(text, "shown");
    }
}
