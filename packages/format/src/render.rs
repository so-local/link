//! Rendering: each block as text, styled for a terminal or plain for a
//! pipe. How it looks is [`look`](crate::look)'s; this is where it's
//! applied.

use std::path::PathBuf;
use std::time::SystemTime;

use anstyle::Style;

use crate::block::*;
use crate::look::{self, Mark, Symbols};
use crate::report::{Kind, Report};
use crate::{Span, Value};

/// How one stream renders.
#[derive(Clone, Debug)]
pub struct Ctx {
    pub color: bool,
    pub bold: bool,
    pub symbols: Symbols,
    pub links: bool,
    /// Where prose wraps; `usize::MAX` for a pipe, which gets it whole.
    pub width: usize,
    pub home: Option<PathBuf>,
    pub now: SystemTime,
}

impl Ctx {
    /// A pipe or a file: no styles, plain marks, nothing wrapped or cut.
    pub fn plain() -> Self {
        Ctx {
            color: false,
            bold: false,
            symbols: Symbols::Plain,
            links: false,
            width: usize::MAX,
            home: std::env::var_os("HOME").map(PathBuf::from),
            now: SystemTime::now(),
        }
    }

    /// A terminal `width` wide, as `settings` have it.
    pub fn terminal(settings: &look::Settings, width: usize) -> Self {
        use look::Choice::*;
        Ctx {
            color: settings.color != Never,
            bold: settings.emphasis != Never,
            symbols: settings.symbols,
            links: settings.links == Always,
            width,
            ..Ctx::plain()
        }
    }

    fn paint(&self, text: &str, style: Style) -> String {
        let color = style.get_fg_color().is_some();
        let bold = style.get_effects().contains(anstyle::Effects::BOLD);
        let style = match (color && self.color, bold && self.bold) {
            (true, true) => style,
            (true, false) => style.effects(anstyle::Effects::new()),
            (false, true) => Style::new().bold(),
            (false, false) => return text.to_string(),
        };
        if text.is_empty() {
            return String::new();
        }
        format!("{style}{text}{style:#}")
    }

    fn bold(&self, text: &str) -> String {
        self.paint(text, look::BOLD)
    }

    fn tone(&self, text: &str, tone: crate::Tone) -> String {
        self.paint(text, look::tone(tone))
    }

    /// `text` faint, on a terminal with color: what's there to place a
    /// line, not to read first (a log line's time and IDs).
    pub(crate) fn faint(&self, text: &str) -> String {
        match self.color && !text.is_empty() {
            true => format!("{}{text}{:#}", look::FAINT, look::FAINT),
            false => text.to_string(),
        }
    }

    pub(crate) fn mark(&self, mark: Mark) -> String {
        let (symbol, tone) = look::mark(mark, self.symbols);
        match self.symbols {
            // An emoji has colors of its own.
            Symbols::Emoji => symbol.to_string(),
            _ => self.tone(symbol, tone),
        }
    }
}

/// How wide `text` shows, without its styles.
pub(crate) fn width(text: &str) -> usize {
    anstream::adapter::strip_str(text)
        .to_string()
        .chars()
        .count()
}

pub(crate) fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "kB", "MB", "GB", "TB"];
    let mut n = bytes as f64;
    let mut unit = 0;
    while n >= 1000.0 && unit < UNITS.len() - 1 {
        n /= 1000.0;
        unit += 1;
    }
    match (unit, n < 10.0 && n.fract() >= 0.05) {
        (0, _) => format!("{bytes} B"),
        (_, true) => format!("{n:.1} {}", UNITS[unit]),
        (_, false) => format!("{} {}", n.round() as u64, UNITS[unit]),
    }
}

/// A value as it shows.
pub fn value(v: &Value, ctx: &Ctx) -> String {
    match v {
        Value::Name(n) => match n.typed {
            true => format!("\"{}\"", n.name),
            false => n.name.clone(),
        },
        Value::Product(p) => p.0.clone(),
        Value::Command(c) => ctx.bold(&c.text()),
        Value::Path(p) => path(&p.0, ctx),
        Value::Address(a) => a.0.clone(),
        Value::State(w) => ctx.tone(w.word, w.tone),
        Value::Size(b) => size(b.0),
        Value::Elapsed(e) => crate::time::elapsed(e.0),
        Value::Time(t) => crate::time::ago(t.0, ctx.now),
        Value::Money(m) => {
            let money = format!("${}.{:02}", thousands(m.cents / 100), m.cents % 100);
            match m.per {
                Some(per) => format!("{money}/{per}"),
                None => money,
            }
        }
        Value::Count(c) => match c.n {
            1 => format!("1 {}", c.one),
            n => format!("{} {}", thousands(n), c.many),
        },
        Value::Version(v) => v.0.clone(),
        Value::Secret => "<secret>".into(),
        Value::Foreign(f) => f.0.clone(),
        Value::Text(t) => t.clone(),
        Value::List(l) => {
            let items: Vec<String> = l.iter().map(|v| value(v, ctx)).collect();
            match items.split_last() {
                None => String::new(),
                Some((last, [])) => last.clone(),
                Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
            }
        }
        Value::Mark(m) => {
            let (symbol, tone) = look::value_mark(*m, ctx.symbols);
            ctx.tone(symbol, tone)
        }
    }
}

fn path(p: &std::path::Path, ctx: &Ctx) -> String {
    let shown = match ctx.home.as_deref().and_then(|h| p.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => p.display().to_string(),
    };
    let shown = crate::clean(&shown);
    match ctx.links && p.is_absolute() {
        true => format!("\x1b]8;;file://{}\x1b\\{shown}\x1b]8;;\x1b\\", p.display()),
        false => shown,
    }
}

/// Words, each value as it shows.
pub fn spans(spans: &[Span], ctx: &Ctx) -> String {
    spans
        .iter()
        .map(|s| match s {
            Span::Text(t) => t.clone(),
            Span::Value(v) => value(v, ctx),
        })
        .collect()
}

/// `text` wrapped to the context's width, each line after the first
/// under `indent`.
fn wrap(text: &str, indent: &str, ctx: &Ctx) -> String {
    let room = match ctx.width {
        usize::MAX => usize::MAX,
        terminal => terminal.min(look::WIDTH),
    }
    .saturating_sub(width(indent))
    .max(20);
    let mut out = String::new();
    let mut line = 0;
    for word in text.split(' ') {
        let w = width(word);
        if line > 0 && line + 1 + w > room {
            out.push('\n');
            out.push_str(indent);
            line = 0;
        } else if line > 0 {
            out.push(' ');
            line += 1;
        }
        out.push_str(word);
        line += w;
    }
    out
}

/// A block, ending in a newline.
pub fn block(b: &Block, ctx: &Ctx) -> String {
    match b {
        Block::Success(s) => success(s, ctx),
        Block::Fields(f) => fields(f, look::INDENT, ctx),
        Block::Table(t) => table(t, ctx),
        Block::List(l) => list(l, ctx),
        Block::Tree(t) => tree(t, ctx),
        Block::Text(t) => format!(
            "{}{}\n",
            look::INDENT,
            wrap(&spans(&t.0.0, ctx), look::INDENT, ctx)
        ),
        Block::Section(s) => {
            let mut out = format!("{}{}\n", look::INDENT, ctx.bold(&spans(&s.heading.0, ctx)));
            let nested = Ctx {
                width: ctx.width.saturating_sub(width(look::INDENT)),
                ..ctx.clone()
            };
            for b in &s.blocks {
                for line in block(b, &nested).lines() {
                    out += &format!("{}{line}\n", look::INDENT);
                }
            }
            out
        }
        Block::Tail(t) => tail(t, look::INDENT, ctx),
        Block::Next(n) => next(n, ctx),
        Block::Report(r) => report(r, None, ctx),
        Block::Progress(p) => progress(p, ctx),
        Block::Note(n) => format!("{}{}\n", look::INDENT, spans(&n.0.0, ctx)),
        Block::Verbatim(v) => verbatim(v, ctx),
        Block::Pairing(p) => pairing(p, ctx),
    }
}

fn list(list: &List, ctx: &Ctx) -> String {
    if list.items.is_empty() {
        return block(&Text(list.empty.clone().into()).into(), ctx);
    }
    let prefix = format!("{}{}", look::INDENT, look::BULLET);
    let indent = " ".repeat(width(&prefix));
    let mut out = String::new();
    for item in &list.items {
        let text = match item {
            ListItem::Value(v) => value(v, ctx),
            ListItem::Text(p) => spans(p.spans(), ctx),
        };
        let text = wrap(&text, &indent, ctx);
        out += &format!("{prefix}{text}\n");
    }
    out
}

fn verbatim(v: &Verbatim, ctx: &Ctx) -> String {
    let mut out = match &v.lead {
        Some(lead) => format!("{}{}:\n\n", look::INDENT, spans(&lead.0, ctx)),
        None => String::new(),
    };
    for line in &v.lines {
        out += &format!("{}{}", look::INDENT, line.text);
        if let Some(c) = &line.comment {
            out += &format!("  {}", ctx.tone(&format!("# {c}"), crate::Tone::Quiet));
        }
        out.push('\n');
    }
    out
}

fn pairing(p: &Pairing, ctx: &Ctx) -> String {
    let expires = value(&Value::Time(p.expires), ctx);
    format!(
        "{i}{}\n\n{i}{i}{}\n\n{i}It expires {expires}.\n",
        spans(p.how.spans(), ctx),
        p.code,
        i = look::INDENT,
    )
}

fn success(s: &Success, ctx: &Ctx) -> String {
    let mark = ctx.mark(match s.waiting {
        true => Mark::Waiting,
        false => Mark::Success,
    });
    let done = spans(&s.done.0, ctx);
    let mut out = match mark.is_empty() {
        true => format!("{done}\n"),
        false => format!("{mark} {done}\n"),
    };
    for c in &s.changed {
        out += &format!(
            "{}{}: {}\n",
            look::INDENT,
            spans(&c.what.0, ctx),
            value(&c.kept, ctx)
        );
    }
    out
}

fn fields(f: &Fields, indent: &str, ctx: &Ctx) -> String {
    let labels: Vec<String> = f.rows.iter().map(|r| spans(&r.label.0, ctx)).collect();
    let wide = labels.iter().map(|l| width(l)).max().unwrap_or(0);
    let mut out = String::new();
    for (row, label) in f.rows.iter().zip(&labels) {
        let shown = value(&row.value, ctx);
        let shown = match row.key {
            true => ctx.bold(&shown),
            false => shown,
        };
        let pad = " ".repeat(wide - width(label));
        let under = format!("\n{indent}{}", " ".repeat(wide + width(look::GAP)));
        out += &format!(
            "{indent}{label}{pad}{}{}\n",
            look::GAP,
            shown.replace('\n', &under)
        );
    }
    out
}

fn table(t: &Table, ctx: &Ctx) -> String {
    if t.rows.is_empty() {
        return format!("{}{}\n", look::INDENT, spans(&t.empty.0, ctx));
    }
    let cells: Vec<Vec<String>> = t
        .rows
        .iter()
        .map(|r| r.iter().map(|v| value(v, ctx)).collect())
        .collect();
    let headers: Vec<String> = t.columns.iter().map(|c| c.header.to_uppercase()).collect();
    let mut wide: Vec<usize> = headers.iter().map(|h| width(h)).collect();
    for row in &cells {
        for (i, c) in row.iter().enumerate() {
            wide[i] = wide[i].max(width(c));
        }
    }
    let total = width(look::INDENT)
        + wide.iter().sum::<usize>()
        + width(look::GAP) * wide.len().saturating_sub(1);
    if total > ctx.width {
        // Keep every value readable when the columns do not fit.
        let mut out = String::new();
        for row in &t.rows {
            let mut f = Fields::new();
            for (col, v) in t.columns.iter().zip(row) {
                f.rows.push(Field {
                    label: crate::Phrase(vec![Span::Text(col.header.clone())]),
                    value: v.clone(),
                    key: false,
                });
            }
            out += &fields(&f, look::INDENT, ctx);
            out.push('\n');
        }
        out.pop();
        return out;
    }
    let line = |cells: &[String]| {
        let mut text = String::from(look::INDENT);
        for (i, cell) in cells.iter().enumerate() {
            let pad = " ".repeat(wide[i] - width(cell));
            let last = i + 1 == cells.len();
            match (t.columns[i].align, last) {
                (Align::Right, _) => text += &format!("{pad}{cell}"),
                (Align::Left, true) => text += cell,
                (Align::Left, false) => text += &format!("{cell}{pad}"),
            }
            if !last {
                text += look::GAP;
            }
        }
        format!("{}\n", text.trim_end())
    };
    let mut out = line(&headers);
    for row in &cells {
        out += &line(row);
    }
    out
}

fn tree(t: &Tree, ctx: &Ctx) -> String {
    if t.roots.is_empty() {
        return format!("{}{}\n", look::INDENT, spans(&t.empty.0, ctx));
    }
    fn node(n: &Node, lead: &str, last: bool, top: bool, ctx: &Ctx, out: &mut String) {
        let cells: Vec<String> = n.cells.iter().map(|v| value(v, ctx)).collect();
        let branch = match (top, last) {
            (true, _) => "",
            (false, true) => look::LAST_BRANCH,
            (false, false) => look::BRANCH,
        };
        *out += &format!("{}{lead}{branch}{}\n", look::INDENT, cells.join(look::GAP));
        let lead = match (top, last) {
            (true, _) => lead.to_string(),
            (false, true) => format!("{lead}{}", " ".repeat(width(look::TRUNK))),
            (false, false) => format!("{lead}{}", look::TRUNK),
        };
        for (i, c) in n.children.iter().enumerate() {
            node(c, &lead, i + 1 == n.children.len(), false, ctx, out);
        }
    }
    let mut out = String::new();
    for r in &t.roots {
        node(r, "", true, true, ctx, &mut out);
    }
    out
}

fn tail(t: &Tail, indent: &str, ctx: &Ctx) -> String {
    let mut out = String::new();
    if let (true, Some(empty)) = (t.lines.is_empty(), &t.empty) {
        out += &format!("{indent}{}\n", spans(&empty.0, ctx));
    }
    for l in &t.lines {
        out += &format!("{indent}{indent}{}\n", l.0);
    }
    if let Some(log) = &t.log {
        out += &format!("{indent}Its log: {}\n", path(&log.0, ctx));
    }
    out
}

fn next(n: &Next, ctx: &Ctx) -> String {
    if n.is_empty() {
        return String::new();
    }
    let shown: Vec<_> = n.0.iter().take(look::MOST_NEXT).collect();
    let wide = shown
        .iter()
        .map(|s| s.command.text().chars().count())
        .max()
        .unwrap_or(0);
    let mut out = format!("{}{}\n", look::INDENT, ctx.bold(look::NEXT));
    for s in shown {
        let text = s.command.text();
        let pad = " ".repeat(wide - text.chars().count() + look::WHY_GAP);
        out += &format!(
            "{0}{0}{1}{pad}{2}\n",
            look::INDENT,
            ctx.bold(&text),
            spans(&s.why.0, ctx)
        );
    }
    out
}

fn progress(p: &Progress, ctx: &Ctx) -> String {
    let what = spans(&p.what.0, ctx);
    match (p.step, p.of) {
        (Some(step), Some(of)) => format!("[{step}/{of}] {what}{}\n", look::ELLIPSIS),
        _ => format!("{what}{}\n", look::ELLIPSIS),
    }
}

/// The last two lines of a report: where it came from, and how to report
/// it.
#[derive(Clone, Debug)]
pub struct Footer {
    pub version: String,
    pub platform: String,
    pub command: String,
    pub record: Option<PathBuf>,
}

/// A report: its mark and headline, why under it, the tail, Next, and,
/// for an error, the footer.
pub fn report(r: &Report, footer: Option<&Footer>, ctx: &Ctx) -> String {
    let mark = ctx.mark(match &r.kind {
        Kind::Error => Mark::Error,
        Kind::Warning => Mark::Warning,
        Kind::Refusal(_) => Mark::Refusal,
    });
    let headline = spans(&r.headline.0, ctx);
    let headline = match r.kind {
        Kind::Warning => headline,
        _ => ctx.bold(&headline),
    };
    let mut out = match mark.is_empty() {
        true => format!("{headline}\n"),
        false => format!("{mark} {headline}\n"),
    };
    if let Some(why) = &r.why {
        out += &format!(
            "{}{}\n",
            look::INDENT,
            wrap(&spans(&why.0, ctx), look::INDENT, ctx)
        );
    }
    if let Some(t) = &r.tail {
        out.push('\n');
        out += &tail(t, look::INDENT, ctx);
    }
    if !r.next.is_empty() {
        out.push('\n');
        out += &next(&r.next, ctx);
    }
    // Only Link's own fault is worth reporting: the rest ends on its Next.
    if let (Some(f), true) = (footer, r.is_bug()) {
        let at = r.trace.steps.first().and_then(|s| s.at.clone());
        let mut context = vec![f.version.clone(), f.platform.clone(), f.command.clone()];
        context.extend(at);
        out += &format!("\n{}{}\n", look::INDENT, context.join(look::SEPARATOR));
        let issue = ctx.bold("lnk issue create");
        match &f.record {
            Some(record) => {
                out += &format!(
                    "{}Report it: {issue}{}everything is in {}\n",
                    look::INDENT,
                    look::SEPARATOR,
                    path(record, ctx)
                )
            }
            None => out += &format!("{}Report it: {issue}\n", look::INDENT),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    fn plain(b: impl Into<Block>) -> String {
        block(&b.into(), &Ctx::plain())
    }

    #[test]
    fn lists_wrap_under_the_item_and_keep_typed_values() {
        let list = List::new(sentence!("Nothing here."))
            .item(crate::prose!("Remove {} from this computer.", cmd!("lnk")))
            .item(crate::prose!("Keep your files."));
        assert_eq!(
            plain(list.clone()),
            "  - Remove lnk from this computer.\n  - Keep your files.\n"
        );
        let mut ctx = Ctx::plain();
        ctx.width = 25;
        assert_eq!(
            block(&list.clone().into(), &ctx),
            "  - Remove lnk from this\n    computer.\n  - Keep your files.\n"
        );
        assert!(
            block(
                &list.clone().into(),
                &Ctx::terminal(&look::Settings::default(), 80)
            )
            .contains("\x1b[1mlnk\x1b[0m")
        );
        assert!(
            serde_json::to_value(Block::from(list))
                .unwrap()
                .get("list")
                .is_some()
        );
        assert_eq!(
            plain(List::new(sentence!("Nothing here."))),
            "  Nothing here.\n"
        );
        assert_eq!(
            plain(List::new(sentence!("Nothing here.")).value(Product::new("lnk"))),
            "  - lnk\n"
        );
    }

    #[test]
    fn verbatim_lines_are_as_written_with_their_comments() {
        let v = Verbatim::new().line("\"git\",").commented(
            "\"python3\",",
            "an interpreter: it runs any code it's given",
        );
        assert_eq!(
            plain(v),
            "  \"git\",\n  \"python3\",  # an interpreter: it runs any code it's given\n"
        );
    }

    #[test]
    fn a_pairing_code_shows_with_its_expiry() {
        let mut ctx = Ctx::plain();
        ctx.now = SystemTime::UNIX_EPOCH;
        let expires = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(600);
        let p = Pairing::new(
            sentence!("In the chat, mention @link with this code."),
            "4FQ2",
            expires,
        );
        assert_eq!(
            block(&p.into(), &ctx),
            "  In the chat, mention @link with this code.\n\n    4FQ2\n\n  It expires in 10 min.\n"
        );
    }

    #[test]
    fn a_pairing_code_is_plain_even_when_terminal_emphasis_is_on() {
        let ctx = Ctx::terminal(&look::Settings::default(), 80);
        assert!(ctx.bold);
        let p = Pairing::new(
            sentence!("Send this code in the app."),
            "K7Q2M9XA",
            ctx.now + std::time::Duration::from_secs(600),
        );
        let shown = block(&p.into(), &ctx);
        assert_eq!(
            shown.lines().find(|line| line.contains("K7Q2M9XA")),
            Some("    K7Q2M9XA")
        );
    }

    #[test]
    fn a_mark_as_a_value_is_never_an_emoji() {
        let ctx = Ctx::terminal(&look::Settings::default(), 80);
        assert_eq!(
            value(&Mark::Success.into(), &ctx),
            "\u{1b}[32m\u{2713}\u{1b}[0m"
        );
        let words = Ctx {
            symbols: Symbols::None,
            ..Ctx::plain()
        };
        assert_eq!(value(&Mark::Warning.into(), &words), "warn");
    }

    #[test]
    fn values_show_as_people_read_them() {
        let ctx = Ctx::plain();
        assert_eq!(value(&Size(4_000_000_000).into(), &ctx), "4 GB");
        assert_eq!(value(&Size(4_200_000_000).into(), &ctx), "4.2 GB");
        assert_eq!(value(&Size(512).into(), &ctx), "512 B");
        assert_eq!(value(&Count::new(1, "box", "boxes").into(), &ctx), "1 box");
        assert_eq!(
            value(&Count::new(1270, "box", "boxes").into(), &ctx),
            "1,270 boxes"
        );
        assert_eq!(
            value(
                &Money {
                    cents: 12,
                    per: Some("h")
                }
                .into(),
                &ctx
            ),
            "$0.12/h"
        );
        let list = Value::List(vec!["telegram".into(), "slack".into(), "discord".into()]);
        assert_eq!(value(&list, &ctx), "telegram, slack and discord");
        assert_eq!(value(&Name::typed("box", "qa 1").into(), &ctx), "\"qa 1\"");
    }

    #[test]
    fn home_is_a_tilde() {
        let mut ctx = Ctx::plain();
        ctx.home = Some("/home/ana".into());
        assert_eq!(
            value(&Path::new("/home/ana/.lnk/x").into(), &ctx),
            "~/.lnk/x"
        );
        assert_eq!(value(&Path::new("/etc/x").into(), &ctx), "/etc/x");
    }

    #[test]
    fn fields_line_up_and_a_table_says_what_empty_is() {
        let f = Fields::new()
            .row("Name", Name::new("agent", "my-otter"))
            .key("Model", "qwen3:8b");
        assert_eq!(plain(f), "  Name   my-otter\n  Model  qwen3:8b\n");
        let t = Table::new(["Name", "State"], sentence!("No boxes yet."));
        assert_eq!(plain(t), "  No boxes yet.\n");
        let mut t = Table::new(["Name", "Size"], sentence!("No boxes yet.")).right(1);
        t.row(["qa-1", "4 GB"]);
        t.row(["build", "16 GB"]);
        assert_eq!(plain(t), "  NAME    SIZE\n  qa-1    4 GB\n  build  16 GB\n");
    }

    #[test]
    fn a_narrow_terminal_gets_a_table_as_fields() {
        let mut ctx = Ctx::plain();
        ctx.width = 20;
        let mut t = Table::new(["Name", "Place"], sentence!("No boxes yet."));
        t.row(["qa-1", "aws us-east-1 a long way"]);
        assert_eq!(
            block(&t.into(), &ctx),
            "  Name   qa-1\n  Place  aws us-east-1 a long way\n"
        );
    }

    #[test]
    fn next_lines_up_its_whys() {
        let mut n = Next::default();
        n.push(
            cmd!("lnk agent connect telegram"),
            "text it from your phone",
        );
        n.push(cmd!("lnk agent show"), "see what it's doing");
        assert_eq!(
            plain(n),
            "  Next\n    lnk agent connect telegram   text it from your phone\n    lnk agent show               see what it's doing\n"
        );
    }

    #[test]
    fn a_report_has_its_footer() {
        let r = Report::bug(sentence!(
            "Couldn't install the {} harness.",
            Name::new("module", "hermes")
        ))
        .why("Its installer stopped at the Python step.")
        .tail(Tail::last("a\nerror: externally-managed-environment\n", 1))
        .next(
            cmd!("lnk agent start --harness link"),
            "use Link's own harness",
        );
        let footer = Footer {
            version: "lnk 0.0.2".into(),
            platform: "linux x86_64".into(),
            command: "lnk agent start".into(),
            record: None,
        };
        let shown = report(&r, Some(&footer), &Ctx::plain());
        assert!(
            shown.starts_with("\u{2717} Couldn't install the hermes harness.\n  Its installer")
        );
        assert!(shown.contains("\n    error: externally-managed-environment\n"));
        assert!(shown.contains("lnk 0.0.2 \u{b7} linux x86_64 \u{b7} lnk agent start \u{b7} "));
        assert!(shown.ends_with("Report it: lnk issue create\n"));
    }

    #[test]
    fn a_terminal_gets_bold_and_color_only_in_their_slots() {
        let ctx = Ctx::terminal(&look::Settings::default(), 80);
        let s = sentence!("Started {}.", Name::new("agent", "my-otter"));
        assert_eq!(
            block(&Success::new(s).into(), &ctx),
            "\u{2705} Started my-otter.\n"
        );
        let c = value(&cmd!("lnk up").into(), &ctx);
        assert_eq!(c, "\x1b[1mlnk up\x1b[0m");
    }

    #[test]
    fn prose_wraps_on_a_terminal() {
        let mut ctx = Ctx::plain();
        ctx.width = 30;
        let t = Text(prose!(
            "The quick brown fox jumps over the lazy dog, twice over."
        ));
        assert_eq!(plain(t.clone()).lines().count(), 1);
        assert_eq!(
            block(&t.into(), &ctx),
            "  The quick brown fox jumps\n  over the lazy dog, twice\n  over.\n"
        );
    }

    #[test]
    fn a_tree_has_branches() {
        let t = Tree::new(sentence!("Nothing in it yet.")).root(
            Node::new(["bucket/"])
                .child(Node::new(["2019/"]))
                .child(Node::new(["2020/"])),
        );
        assert_eq!(plain(t), "  bucket/\n  ├── 2019/\n  └── 2020/\n");
    }
}
