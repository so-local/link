use super::*;

/// A chat marking text as CommonMark does, for the layout's tests.
struct Plainish {
    channel: Channel,
}

impl Plainish {
    fn limit(limit: usize) -> Self {
        Self {
            channel: Channel {
                limit,
                counted: Counted::Chars,
            },
        }
    }
}

impl Markup for Plainish {
    fn text(&self, text: &str) -> String {
        text.replace('*', "\\*")
    }
    fn mark(&self, mark: &Mark) -> (&str, &str) {
        match mark {
            Mark::Bold => ("**", "**"),
            Mark::Italic => ("_", "_"),
            _ => ("~~", "~~"),
        }
    }
    fn code(&self, text: &str) -> String {
        format!("`{text}`")
    }
    fn link(&self, url: &str, shown: &str, _: &str) -> String {
        format!("[{shown}]({url})")
    }
    fn heading(&self, level: u8, shown: &str, _: &str) -> String {
        format!("{} {shown}", "#".repeat(level.into()))
    }
    fn code_block(&self, language: Option<&str>, text: &str) -> String {
        format!("```{}\n{text}\n```", language.unwrap_or_default())
    }
    fn quote(&self) -> Quoted {
        Quoted::Prefixed("> ".into())
    }
    fn bullet(&self, number: Option<u64>) -> String {
        number.map_or("- ".into(), |n| format!("{n}. "))
    }
    fn rule(&self) -> String {
        "---".into()
    }
}

impl Dialect for Plainish {
    type Out = String;
    fn render(&self, blocks: &[Block]) -> String {
        markup(self, blocks)
    }
    fn text<'a>(&self, out: &'a String) -> &'a str {
        out
    }
    fn channel(&self) -> &Channel {
        &self.channel
    }
}

fn span(text: &str, marks: &[Mark]) -> Span {
    Span {
        text: text.into(),
        marks: marks.to_vec(),
    }
}

#[test]
fn markdown_is_read_into_blocks_and_marked_spans() {
    let document = Document::parse(
        "# Plan\n\nSome **bold _and nested_** text, `code` and [a link](https://x.io).\n\n\
         ```rust\nfn main() {}\n```\n\n- one\n- two\n\n> quoted\n\n~~gone~~",
    );
    assert_eq!(
        document.blocks,
        vec![
            Block::Heading {
                level: 1,
                spans: vec![span("Plan", &[])]
            },
            Block::Paragraph(vec![
                span("Some ", &[]),
                span("bold ", &[Mark::Bold]),
                span("and nested", &[Mark::Bold, Mark::Italic]),
                span(" text, ", &[]),
                span("code", &[Mark::Code]),
                span(" and ", &[]),
                span("a link", &[Mark::Link("https://x.io".into())]),
                span(".", &[]),
            ]),
            Block::Code {
                language: Some("rust".into()),
                text: "fn main() {}".into()
            },
            Block::List {
                start: None,
                items: vec![
                    vec![Block::Paragraph(vec![span("one", &[])])],
                    vec![Block::Paragraph(vec![span("two", &[])])],
                ]
            },
            Block::Quote(vec![Block::Paragraph(vec![span("quoted", &[])])]),
            Block::Paragraph(vec![span("gone", &[Mark::Strike])]),
        ]
    );
}

#[test]
fn emphasis_leaves_its_spaces_unmarked() {
    // Not emphasis to CommonMark at all: spaces inside the stars.
    let document = Document::parse("a ** b ** c");
    assert_eq!(plain_blocks(&document.blocks), "a ** b ** c");
    // Space at emphasis's edges, as a cut leaves it, goes outside it.
    let spans = vec![
        span("a ", &[Mark::Bold]),
        span("b ", &[Mark::Bold, Mark::Italic]),
        span(" ", &[]),
    ];
    assert_eq!(
        parse::hoisted(spans),
        vec![
            span("a ", &[Mark::Bold]),
            span("b", &[Mark::Bold, Mark::Italic]),
            span("  ", &[]),
        ]
    );
}

fn plain_blocks(blocks: &[Block]) -> String {
    styled(blocks).text
}

#[test]
fn a_table_is_aligned_text() {
    let document = Document::parse("| Name | Age |\n|---|--:|\n| Alice | 30 |\n| Bo | 7 |");
    let Block::Table(table) = &document.blocks[0] else {
        panic!("{document:?}")
    };
    assert_eq!(
        table.monospace(),
        "Name  | Age\n------+----\nAlice | 30\nBo    | 7"
    );
}

#[test]
fn an_answer_cut_short_still_reads() {
    // An unclosed fence is code to the end; unclosed stars are text.
    let document = Document::parse("**half\n\n```sh\nls -l");
    assert_eq!(
        document.blocks,
        vec![
            Block::Paragraph(vec![span("**half", &[])]),
            Block::Code {
                language: Some("sh".into()),
                text: "ls -l".into()
            },
        ]
    );
}

#[test]
fn raw_html_stays_text() {
    let document = Document::parse("<script>alert(1)</script>\n\nhi <b>x</b>");
    assert_eq!(
        plain_blocks(&document.blocks),
        "<script>alert(1)</script>\n\nhi <b>x</b>"
    );
}

#[test]
fn lists_and_quotes_are_laid_out() {
    let chat = Plainish::limit(1000);
    let text = "3. first\n4. second\n   - inner\n\n> a\n>\n> b";
    assert_eq!(
        markup(&chat, &Document::parse(text).blocks),
        "3. first\n4. second\n   - inner\n\n> a\n>\n> b"
    );
}

#[test]
fn short_answers_are_one_piece() {
    let chat = Plainish::limit(100);
    let pieces = pieces("Hello **there**.", &chat);
    assert_eq!(pieces.len(), 1);
    assert_eq!(pieces[0].out, "Hello **there**.");
    assert!(super::pieces("  \n", &chat).is_empty());
}

#[test]
fn a_long_code_block_is_closed_and_opened_again() {
    let chat = Plainish::limit(60);
    let code: String = (0..20).map(|n| format!("line {n}\n")).collect();
    let pieces = pieces(&format!("Intro.\n\n```sh\n{code}```\n\nOutro."), &chat);
    assert!(pieces.len() > 2, "{pieces:#?}");
    for piece in &pieces {
        assert!(piece.out.chars().count() <= 60, "{}", piece.out);
        // Every fence that opens closes in the same message.
        assert_eq!(piece.out.matches("```").count() % 2, 0, "{}", piece.out);
    }
    let lines: Vec<String> = pieces
        .iter()
        .flat_map(|p| p.out.lines().map(str::to_string).collect::<Vec<_>>())
        .filter(|l| l.starts_with("line"))
        .collect();
    assert_eq!(lines.len(), 20, "no line lost or cut");
    assert!(pieces[0].out.starts_with("Intro."));
    assert!(pieces.last().unwrap().out.ends_with("Outro."));
}

#[test]
fn a_long_paragraph_is_cut_between_words_its_marks_kept() {
    let chat = Plainish::limit(30);
    let text = format!("**{}**", "word ".repeat(20).trim_end());
    let pieces = pieces(&text, &chat);
    assert!(pieces.len() > 1);
    for piece in &pieces {
        assert!(piece.out.chars().count() <= 30, "{}", piece.out);
        assert!(
            piece.out.starts_with("**word") && piece.out.ends_with("word**"),
            "{}",
            piece.out
        );
    }
}

#[test]
fn a_long_table_is_code_cut_by_lines() {
    let chat = Plainish::limit(50);
    let rows: String = (0..10).map(|n| format!("| row {n} | {n} |\n")).collect();
    let pieces = pieces(&format!("| a | b |\n|---|---|\n{rows}"), &chat);
    assert!(pieces.len() > 1, "{pieces:#?}");
    for piece in &pieces {
        assert!(piece.out.starts_with("```\n") && piece.out.ends_with("\n```"));
        assert!(piece.out.chars().count() <= 50);
    }
}

#[test]
fn a_long_list_is_cut_between_items_numbering_on() {
    let chat = Plainish::limit(40);
    let text: String = (1..=10)
        .map(|n| format!("{n}. item number {n}\n"))
        .collect();
    let pieces = pieces(&text, &chat);
    assert!(pieces.len() > 1);
    let second = &pieces[1].out;
    let Some(Block::List { start: Some(n), .. }) = pieces[1].blocks.first() else {
        panic!("{pieces:#?}")
    };
    assert!(
        second.starts_with(&format!("{n}. item number {n}")),
        "{second}"
    );
}

#[test]
fn earlier_pieces_stay_as_the_answer_grows() {
    let chat = Plainish::limit(80);
    let text = "First paragraph here.\n\nSecond paragraph, a bit longer than that.\n\n".repeat(4);
    let before = pieces(&text, &chat);
    let after = pieces(&format!("{text}And more to come"), &chat);
    assert_eq!(before[0], after[0]);
}

#[test]
fn styles_count_utf16_units() {
    // 😀 is two UTF-16 units, é one.
    let styled = styled(&Document::parse("😀 é **bold** `x`").blocks);
    assert_eq!(styled.text, "😀 é bold x");
    assert_eq!(
        styled.utf16(),
        vec![(5, 4, Style::Bold), (10, 1, Style::Monospace)]
    );
}

#[test]
fn styled_text_shows_links_headings_and_lists() {
    let styled = styled(
        &Document::parse("## Head\n\n- [docs](https://d.io) and <https://e.io>\n\n> q").blocks,
    );
    assert_eq!(
        styled.text,
        "Head\n\n• docs (https://d.io) and https://e.io\n\n> q"
    );
    assert_eq!(styled.styles[0].style, Style::Bold);
}

#[test]
fn only_web_links_are_linked_their_markup_breakers_encoded() {
    assert_eq!(web_url("https://x.io/a"), Some("https://x.io/a".into()));
    assert_eq!(
        web_url("https://x.io/a_(b)|<!channel>"),
        Some("https://x.io/a_%28b%29%7C%3C!channel%3E".into())
    );
    assert_eq!(web_url("mailto:a@b.c"), Some("mailto:a@b.c".into()));
    for refused in [
        "javascript:alert(1)",
        "<!channel>",
        "x",
        "https:",
        "tg://user?id=1",
    ] {
        assert_eq!(web_url(refused), None, "{refused}");
    }
    assert_eq!(web_url("https://x.io/\u{7}"), None);
}

#[test]
fn a_link_that_may_not_be_linked_is_text() {
    let chat = Plainish::limit(1000);
    let out = markup(&chat, &Document::parse("[x](javascript:alert(1))").blocks);
    assert_eq!(out, "x (javascript:alert(1))");
}

#[test]
fn counted_as_the_chat_counts() {
    assert_eq!(Counted::Chars.count("😀é"), 2);
    assert_eq!(Counted::Utf16.count("😀é"), 3);
    let channel: Channel = link_files::schema::parse("limit = 10\ncounted = \"utf16\"").unwrap();
    assert_eq!(channel.counted, Counted::Utf16);
}

#[test]
fn a_fence_inside_code_ends_no_block() {
    assert_eq!(unfenced("a ``` b", "```"), "a ``\u{200b}` b");
    assert_eq!(unfenced("plain", "```"), "plain");
}
