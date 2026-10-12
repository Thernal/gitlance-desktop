//! Markdown in comments: GitLab writes discussions in it, and a comment written here may use it too.
//! `parse` turns a body into blocks the UI draws; `apply` and `newline` are the composer's helpers,
//! which put the markers in for the writer (the composer only types at the end of its text).

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Paragraph(Inline),
    Heading(u8, Inline),
    /// A fenced or indented code block; `lang` is the fence's first word (`rust`, `suggestion:-0+0`).
    Code {
        lang: String,
        text: String,
    },
    Quote(Vec<Block>),
    List {
        /// The first number of an ordered list.
        start: Option<u64>,
        items: Vec<Item>,
    },
    Table {
        head: Vec<Inline>,
        rows: Vec<Vec<Inline>>,
    },
    Rule,
    /// Raw HTML, shown as written.
    Html(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    /// `Some(done)` for a task list item.
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

/// A run of text with its styled ranges.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inline {
    pub text: String,
    pub spans: Vec<Span>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub range: Range<usize>,
    pub style: Style,
    /// Where a link goes.
    pub link: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
}

impl Inline {
    fn push(&mut self, text: &str, style: Style, link: Option<&String>) {
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(text);
        if style == Style::default() && link.is_none() {
            return;
        }
        let end = self.text.len();
        match self.spans.last_mut() {
            Some(s) if s.range.end == start && s.style == style && s.link.as_ref() == link => {
                s.range.end = end
            }
            _ => self.spans.push(Span {
                range: start..end,
                style,
                link: link.cloned(),
            }),
        }
    }
}

enum Frame {
    Quote(Vec<Block>),
    List(Option<u64>, Vec<Item>),
    Item(Item),
    Table {
        head: Vec<Inline>,
        rows: Vec<Vec<Inline>>,
        row: Vec<Inline>,
    },
}

#[derive(Default)]
struct Builder {
    root: Vec<Block>,
    frames: Vec<Frame>,
    /// The paragraph, heading or table cell being filled.
    inline: Option<Inline>,
    heading: Option<u8>,
    code: Option<(String, String)>,
    html: Option<String>,
    bold: u32,
    italic: u32,
    strike: u32,
    links: Vec<String>,
}

impl Builder {
    fn style(&self, code: bool) -> Style {
        Style {
            bold: self.bold > 0,
            italic: self.italic > 0,
            strike: self.strike > 0,
            code,
        }
    }

    fn block(&mut self, block: Block) {
        match self.frames.last_mut() {
            Some(Frame::Quote(blocks)) => blocks.push(block),
            Some(Frame::Item(item)) => item.blocks.push(block),
            _ => self.root.push(block),
        }
    }

    /// Text in a tight list item comes without a paragraph around it; it gets one here.
    fn close_loose(&mut self) {
        if let Some(inline) = self.inline.take() {
            self.block(Block::Paragraph(inline));
        }
    }

    fn text(&mut self, text: &str, code: bool) {
        if let Some((_, body)) = self.code.as_mut() {
            return body.push_str(text);
        }
        if let Some(html) = self.html.as_mut() {
            return html.push_str(text);
        }
        let style = self.style(code);
        let link = self.links.last().cloned();
        self.inline
            .get_or_insert_with(Inline::default)
            .push(text, style, link.as_ref());
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph | Tag::TableCell => self.inline = Some(Inline::default()),
            Tag::Heading { level, .. } => {
                self.close_loose();
                self.heading = Some(level as u8);
                self.inline = Some(Inline::default());
            }
            Tag::BlockQuote(_) => {
                self.close_loose();
                self.frames.push(Frame::Quote(Vec::new()));
            }
            Tag::CodeBlock(kind) => {
                self.close_loose();
                let lang = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(info) => info
                        .split_whitespace()
                        .next()
                        .unwrap_or_default()
                        .to_owned(),
                    pulldown_cmark::CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Tag::HtmlBlock => {
                self.close_loose();
                self.html = Some(String::new());
            }
            Tag::List(start) => {
                self.close_loose();
                self.frames.push(Frame::List(start, Vec::new()));
            }
            Tag::Item => self.frames.push(Frame::Item(Item::default())),
            Tag::Table(_) => self.frames.push(Frame::Table {
                head: Vec::new(),
                rows: Vec::new(),
                row: Vec::new(),
            }),
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { dest_url, .. } => self.links.push(dest_url.into_string()),
            Tag::Image { dest_url, .. } => {
                self.links.push(dest_url.into_string());
                self.text("🖼 ", false);
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.close_loose(),
            TagEnd::Heading(_) => {
                let inline = self.inline.take().unwrap_or_default();
                let level = self.heading.take().unwrap_or(1);
                self.block(Block::Heading(level, inline));
            }
            TagEnd::BlockQuote(_) => {
                self.close_loose();
                if let Some(Frame::Quote(blocks)) = self.frames.pop() {
                    self.block(Block::Quote(blocks));
                }
            }
            TagEnd::CodeBlock => {
                if let Some((lang, text)) = self.code.take() {
                    let text = text.strip_suffix('\n').unwrap_or(&text).to_owned();
                    self.block(Block::Code { lang, text });
                }
            }
            TagEnd::HtmlBlock => {
                if let Some(html) = self.html.take() {
                    self.block(Block::Html(html.trim_end().to_owned()));
                }
            }
            TagEnd::Item => {
                self.close_loose();
                if let Some(Frame::Item(item)) = self.frames.pop()
                    && let Some(Frame::List(_, items)) = self.frames.last_mut()
                {
                    items.push(item);
                }
            }
            TagEnd::List(_) => {
                if let Some(Frame::List(start, items)) = self.frames.pop() {
                    self.block(Block::List { start, items });
                }
            }
            TagEnd::TableCell => {
                let cell = self.inline.take().unwrap_or_default();
                if let Some(Frame::Table { row, .. }) = self.frames.last_mut() {
                    row.push(cell);
                }
            }
            TagEnd::TableHead => {
                if let Some(Frame::Table { head, row, .. }) = self.frames.last_mut() {
                    *head = std::mem::take(row);
                }
            }
            TagEnd::TableRow => {
                if let Some(Frame::Table { rows, row, .. }) = self.frames.last_mut() {
                    rows.push(std::mem::take(row));
                }
            }
            TagEnd::Table => {
                if let Some(Frame::Table { head, rows, .. }) = self.frames.pop() {
                    self.block(Block::Table { head, rows });
                }
            }
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link | TagEnd::Image => {
                self.links.pop();
            }
            _ => {}
        }
    }
}

/// The blocks of a markdown body, GitLab-flavoured: tables, task lists and strikethrough included.
pub fn parse(source: &str) -> Vec<Block> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM;
    let mut b = Builder::default();
    for event in Parser::new_ext(source, options) {
        match event {
            Event::Start(tag) => b.start(tag),
            Event::End(tag) => b.end(tag),
            Event::Text(t) => b.text(&t, false),
            Event::Code(t) => b.text(&t, true),
            Event::Html(t) => b.text(&t, false),
            Event::InlineHtml(t) => {
                let tag = t.trim().to_ascii_lowercase();
                if matches!(tag.as_str(), "<br>" | "<br/>" | "<br />") {
                    b.text("\n", false)
                } else {
                    b.text(&t, true)
                }
            }
            Event::SoftBreak => b.text(" ", false),
            Event::HardBreak => b.text("\n", false),
            Event::Rule => {
                b.close_loose();
                b.block(Block::Rule);
            }
            Event::TaskListMarker(done) => {
                if let Some(Frame::Item(item)) = b.frames.last_mut() {
                    item.task = Some(done);
                }
            }
            Event::FootnoteReference(t) => b.text(&format!("[^{t}]"), false),
            _ => {}
        }
    }
    b.close_loose();
    b.root
}

/// Does `body` use markdown beyond plain paragraphs? Plain text needs no Rich/Raw switch.
pub fn is_rich(blocks: &[Block]) -> bool {
    blocks.iter().any(|b| match b {
        Block::Paragraph(inline) => !inline.spans.is_empty(),
        _ => true,
    })
}

/// The words of `source` without its markers, on one line: for the one-line previews of a comment.
pub fn plain(source: &str) -> String {
    fn walk(blocks: &[Block], out: &mut Vec<String>) {
        for b in blocks {
            match b {
                Block::Paragraph(i) | Block::Heading(_, i) => out.push(i.text.clone()),
                Block::Code { text, .. } | Block::Html(text) => out.push(text.clone()),
                Block::Quote(blocks) => walk(blocks, out),
                Block::List { items, .. } => {
                    for item in items {
                        walk(&item.blocks, out)
                    }
                }
                Block::Table { head, rows } => {
                    for cell in head.iter().chain(rows.iter().flatten()) {
                        out.push(cell.text.clone())
                    }
                }
                Block::Rule => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&parse(source), &mut out);
    out.join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// ---- the composer's helpers ------------------------------------------------------------------

/// A button (or key) of the composer's toolbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Helper {
    Bold,
    Italic,
    Strike,
    Code,
    CodeBlock,
    Heading,
    Bullet,
    Numbered,
    Task,
    Quote,
    Link,
}

impl Helper {
    /// What wraps a run of text, for the inline ones.
    fn marker(self) -> Option<&'static str> {
        match self {
            Helper::Bold => Some("**"),
            Helper::Italic => Some("_"),
            Helper::Strike => Some("~~"),
            Helper::Code => Some("`"),
            _ => None,
        }
    }

    /// What starts each line, for the line ones.
    fn prefix(self) -> Option<&'static str> {
        match self {
            Helper::Heading => Some("### "),
            Helper::Bullet => Some("- "),
            Helper::Task => Some("- [ ] "),
            Helper::Quote => Some("> "),
            Helper::Numbered => Some("1. "),
            _ => None,
        }
    }
}

/// A marker the writer opened and has not closed yet: `at` is where its text starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Open {
    pub helper: Helper,
    /// The link's address is being typed (after `](`).
    pub url: bool,
    pub at: usize,
}

/// Drops the open markers that editing by hand took away.
pub fn prune(text: &str, open: &mut Vec<Open>) {
    open.retain(|o| {
        o.at <= text.len()
            && text.is_char_boundary(o.at)
            && match (o.helper.marker(), o.url) {
                (Some(m), _) => text[..o.at].ends_with(m),
                (None, true) => text[..o.at].ends_with("]("),
                (None, false) => text[..o.at].ends_with('['),
            }
    });
}

/// Applies `helper` to the composer's `text`. With `all` (⌘A), it wraps or prefixes the whole
/// text; otherwise an inline marker opens at the end and the same helper closes it, and a line
/// marker starts a new line.
pub fn apply(text: &mut String, all: bool, open: &mut Vec<Open>, helper: Helper) {
    prune(text, open);
    if all && !text.is_empty() {
        open.clear();
        *text = wrap_all(text, helper);
        return;
    }
    if helper == Helper::CodeBlock {
        // The same fence opens a block and, pressed again, closes it.
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str("```\n");
        return;
    }
    if let Some(prefix) = helper.prefix() {
        let last = text.rsplit('\n').next().unwrap_or_default();
        let bare = match helper {
            Helper::Numbered => line_marker(last).is_some_and(|m| m == last && m.ends_with(". ")),
            _ => last == prefix,
        };
        let prefix = match helper {
            Helper::Numbered => numbered_after(last),
            _ => prefix.to_owned(),
        };
        if last.is_empty() {
            text.push_str(&prefix);
        } else if bare {
            // Pressed again on an empty line of its own: it comes off.
            text.truncate(text.len() - last.len());
        } else {
            text.push('\n');
            text.push_str(&prefix);
        }
        return;
    }
    if let Some(ix) = open.iter().rposition(|o| o.helper == helper) {
        let o = open.remove(ix);
        if o.at == text.len() {
            // Nothing written since it opened: the marker comes off again.
            let len = match (helper.marker(), o.url) {
                (Some(m), _) => m.len(),
                (None, true) => 2,
                (None, false) => 1,
            };
            text.truncate(o.at - len);
            if o.url {
                let at = text.rfind('[').map_or(text.len(), |i| i + 1);
                open.push(Open {
                    helper,
                    url: false,
                    at,
                });
            }
            return;
        }
        match (helper.marker(), o.url) {
            (Some(m), _) => text.push_str(m),
            (None, true) => text.push(')'),
            (None, false) => {
                text.push_str("](");
                open.push(Open {
                    helper,
                    url: true,
                    at: text.len(),
                });
            }
        }
        return;
    }
    if !text.is_empty() && !text.ends_with(char::is_whitespace) {
        text.push(' ');
    }
    text.push_str(helper.marker().unwrap_or("["));
    open.push(Open {
        helper,
        url: false,
        at: text.len(),
    });
}

fn wrap_all(text: &str, helper: Helper) -> String {
    let body = text.trim_end_matches('\n');
    match helper {
        Helper::Code if body.contains('\n') => format!("```\n{body}\n```"),
        Helper::CodeBlock => format!("```\n{body}\n```"),
        Helper::Link if body.starts_with("http://") || body.starts_with("https://") => {
            format!("[link]({body})")
        }
        Helper::Link => format!("[{body}](url)"),
        Helper::Heading => match body.split_once('\n') {
            Some((first, rest)) => format!("### {first}\n{rest}"),
            None => format!("### {body}"),
        },
        Helper::Numbered => body
            .lines()
            .enumerate()
            .map(|(i, l)| format!("{}. {l}", i + 1))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => match (helper.marker(), helper.prefix()) {
            (Some(m), _) => format!("{m}{body}{m}"),
            (_, Some(p)) => body
                .lines()
                .map(|l| format!("{p}{l}"))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => body.to_owned(),
        },
    }
}

/// `N. ` after a line numbered `N-1. `, else `1. `.
fn numbered_after(line: &str) -> String {
    let n = line
        .split_once(". ")
        .and_then(|(n, _)| n.trim_start().parse::<u64>().ok());
    format!("{}. ", n.map_or(1, |n| n + 1))
}

/// The list or quote marker a line starts with (`- `, `- [ ] `, `3. `, `> `), with its indent.
fn line_marker(line: &str) -> Option<String> {
    let indent = &line[..line.len() - line.trim_start().len()];
    let rest = &line[indent.len()..];
    for p in ["- [ ] ", "- [x] ", "* [ ] ", "- ", "* ", "+ ", "> "] {
        if rest.starts_with(p) {
            let p = p.replace("[x]", "[ ]");
            return Some(format!("{indent}{p}"));
        }
    }
    let (n, _) = rest.split_once(". ")?;
    let n: u64 = n.parse().ok()?;
    Some(format!("{indent}{n}. "))
}

/// Return in the composer: a list or a quote goes on to its next line; return on an empty item
/// ends the list instead.
pub fn newline(text: &mut String) {
    let last = text.rsplit('\n').next().unwrap_or_default().to_owned();
    let in_code = text
        .lines()
        .filter(|l| l.trim_start().starts_with("```"))
        .count()
        % 2
        == 1;
    match line_marker(&last).filter(|_| !in_code) {
        Some(marker) if last == marker || last.trim_end() == marker.trim_end() => {
            // A blank line ends the list: without it the next line would join the last item.
            text.truncate(text.len() - last.len());
            text.push('\n');
        }
        Some(marker) => {
            let next = match marker.trim_start().split_once(". ") {
                Some((n, _)) if n.parse::<u64>().is_ok() => {
                    let indent = &marker[..marker.len() - marker.trim_start().len()];
                    format!("{indent}{}", numbered_after(marker.trim_start()))
                }
                _ => marker,
            };
            text.push('\n');
            text.push_str(&next);
        }
        None => text.push('\n'),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(blocks: &[Block]) -> &Inline {
        match &blocks[0] {
            Block::Paragraph(i) => i,
            other => panic!("not a paragraph: {other:?}"),
        }
    }

    #[test]
    fn inline_styles_become_spans() {
        let blocks = parse("a **bold** _it_ `code` ~~gone~~ [link](https://x.y)");
        let p = para(&blocks);
        assert_eq!(p.text, "a bold it code gone link");
        let styled = |word: &str| {
            let at = p.text.find(word).unwrap();
            p.spans
                .iter()
                .find(|s| s.range.start == at)
                .unwrap_or_else(|| panic!("no span for {word}"))
                .clone()
        };
        assert!(styled("bold").style.bold);
        assert!(styled("it").style.italic);
        assert!(styled("code").style.code);
        assert!(styled("gone").style.strike);
        assert_eq!(styled("link").link.as_deref(), Some("https://x.y"));
    }

    #[test]
    fn blocks_of_a_gitlab_comment() {
        let src = "### Why\n\n- [x] done\n- [ ] todo\n\n> quoted\n\n```suggestion:-0+0\nlet x = 1;\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n---";
        let blocks = parse(src);
        assert!(matches!(&blocks[0], Block::Heading(3, i) if i.text == "Why"));
        match &blocks[1] {
            Block::List { start: None, items } => {
                assert_eq!(items[0].task, Some(true));
                assert_eq!(items[1].task, Some(false));
                assert!(matches!(&items[1].blocks[0], Block::Paragraph(i) if i.text == "todo"));
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(&blocks[2], Block::Quote(b) if b.len() == 1));
        assert_eq!(
            blocks[3],
            Block::Code {
                lang: "suggestion:-0+0".into(),
                text: "let x = 1;".into()
            }
        );
        assert!(
            matches!(&blocks[4], Block::Table { head, rows } if head.len() == 2 && rows.len() == 1)
        );
        assert_eq!(blocks[5], Block::Rule);
    }

    #[test]
    fn plain_text_is_not_rich() {
        assert!(!is_rich(&parse("just words\nover two lines")));
        assert!(is_rich(&parse("- a list")));
        assert!(is_rich(&parse("with `code`")));
    }

    #[test]
    fn a_preview_line_has_the_words_without_the_markers() {
        assert_eq!(
            plain("### Why\n\n- **one**\n- `two`\n\n> [three](https://x)"),
            "Why one two three"
        );
    }

    #[test]
    fn an_inline_helper_opens_then_closes() {
        let (mut text, mut open) = ("fix".to_owned(), Vec::new());
        apply(&mut text, false, &mut open, Helper::Bold);
        assert_eq!(text, "fix **");
        text.push_str("this");
        apply(&mut text, false, &mut open, Helper::Bold);
        assert_eq!(text, "fix **this**");
        assert!(open.is_empty());
        // Opened and closed with nothing between: undone.
        apply(&mut text, false, &mut open, Helper::Code);
        apply(&mut text, false, &mut open, Helper::Code);
        assert_eq!(text, "fix **this** ");
    }

    #[test]
    fn a_link_takes_its_text_then_its_address() {
        let (mut text, mut open) = (String::new(), Vec::new());
        apply(&mut text, false, &mut open, Helper::Link);
        text.push_str("docs");
        apply(&mut text, false, &mut open, Helper::Link);
        text.push_str("https://x.y");
        apply(&mut text, false, &mut open, Helper::Link);
        assert_eq!(text, "[docs](https://x.y)");
        assert!(open.is_empty());
    }

    #[test]
    fn backspacing_over_a_marker_forgets_it() {
        let (mut text, mut open) = ("a".to_owned(), Vec::new());
        apply(&mut text, false, &mut open, Helper::Italic);
        text.pop();
        prune(&text, &mut open);
        assert!(open.is_empty());
    }

    #[test]
    fn helpers_wrap_everything_when_all_is_selected() {
        let mut open = Vec::new();
        let mut text = "one\ntwo".to_owned();
        apply(&mut text, true, &mut open, Helper::Bullet);
        assert_eq!(text, "- one\n- two");
        let mut text = "one\ntwo".to_owned();
        apply(&mut text, true, &mut open, Helper::Numbered);
        assert_eq!(text, "1. one\n2. two");
        let mut text = "x".to_owned();
        apply(&mut text, true, &mut open, Helper::Bold);
        assert_eq!(text, "**x**");
        let mut text = "a\nb".to_owned();
        apply(&mut text, true, &mut open, Helper::Code);
        assert_eq!(text, "```\na\nb\n```");
    }

    #[test]
    fn line_helpers_start_a_line() {
        let (mut text, mut open) = ("intro".to_owned(), Vec::new());
        apply(&mut text, false, &mut open, Helper::Numbered);
        assert_eq!(text, "intro\n1. ");
        text.push_str("first");
        apply(&mut text, false, &mut open, Helper::Numbered);
        assert_eq!(text, "intro\n1. first\n2. ");
        apply(&mut text, false, &mut open, Helper::Numbered);
        assert_eq!(text, "intro\n1. first\n");
    }

    #[test]
    fn return_continues_a_list_and_ends_it_on_an_empty_item() {
        let mut text = "- a".to_owned();
        newline(&mut text);
        assert_eq!(text, "- a\n- ");
        newline(&mut text);
        assert_eq!(text, "- a\n\n");
        assert!(matches!(
            &parse(&format!("{text}after"))[1],
            Block::Paragraph(_)
        ));
        let mut text = "  3. c".to_owned();
        newline(&mut text);
        assert_eq!(text, "  3. c\n  4. ");
        let mut text = "- [x] done".to_owned();
        newline(&mut text);
        assert_eq!(text, "- [x] done\n- [ ] ");
        let mut text = "```\n- in code".to_owned();
        newline(&mut text);
        assert_eq!(text, "```\n- in code\n");
    }
}
