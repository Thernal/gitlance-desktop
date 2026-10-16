//! Markdown in comments: GitLab writes discussions in it, and a comment written here may use it too.
//! `parse` turns a body into blocks the UI draws; `apply` and `newline` are the composer's helpers,
//! which put the markers in for the writer at the caret or around the selection.

use crate::textedit::TextEdit;
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
        Block::Paragraph(inline) => !inline.spans.is_empty() || !mentions(&inline.text).is_empty(),
        _ => true,
    })
}

/// The `@username` mentions in `text` (#51): an `@` at the start or after a space or an opening
/// bracket, then letters, digits, `_`, `-` or `.` (not ending in `.`).
pub fn mentions(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let starts = bytes[i] == b'@'
            && (i == 0 || matches!(bytes[i - 1], b' ' | b'\n' | b'\t' | b'(' | b'['));
        if !starts {
            i += 1;
            continue;
        }
        let mut end = i + 1;
        while end < bytes.len()
            && (bytes[end].is_ascii_alphanumeric() || matches!(bytes[end], b'_' | b'-' | b'.'))
        {
            end += 1;
        }
        while end > i + 1 && bytes[end - 1] == b'.' {
            end -= 1;
        }
        if end > i + 1 {
            out.push(i..end);
        }
        i = end.max(i + 1);
    }
    out
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
    Bullet,
    Numbered,
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
            Helper::Bullet => Some("- "),
            Helper::Quote => Some("> "),
            Helper::Numbered => Some("1. "),
            _ => None,
        }
    }
}

/// Applies `helper` at the caret or over the selection, the way GitHub's toolbar does: an inline
/// marker wraps the selection (or goes in empty with the caret between), and comes off when the
/// selection is already wrapped; a line marker goes at the start of every line the selection
/// touches, and comes off when they all have it.
pub fn apply(edit: &mut TextEdit, helper: Helper) {
    let text = edit.text().to_owned();
    let sel = edit.selection().unwrap_or(edit.caret()..edit.caret());
    let inner = text[sel.clone()].to_owned();
    match helper {
        Helper::Link => {
            if inner.starts_with("http://") || inner.starts_with("https://") {
                let with = format!("[]({inner})");
                let at = sel.start + 1;
                edit.replace(sel, &with, at..at);
            } else {
                let with = format!("[{inner}](url)");
                let url = sel.start + inner.len() + 3;
                edit.replace(sel, &with, url..url + 3);
            }
        }
        Helper::CodeBlock => {
            let lead = if sel.start == 0 || text[..sel.start].ends_with('\n') {
                ""
            } else {
                "\n"
            };
            let with = format!("{lead}```\n{inner}\n```");
            let at = sel.start + lead.len() + 4;
            edit.replace(sel, &with, at..at + inner.len());
        }
        Helper::Code if inner.contains('\n') => apply(edit, Helper::CodeBlock),
        _ => match helper.marker() {
            Some(m) => {
                let wrapped = sel.start >= m.len()
                    && text[..sel.start].ends_with(m)
                    && text[sel.end..].starts_with(m);
                if wrapped {
                    let outer = sel.start - m.len()..sel.end + m.len();
                    let at = outer.start;
                    edit.replace(outer, &inner, at..at + inner.len());
                } else {
                    let with = format!("{m}{inner}{m}");
                    let at = sel.start + m.len();
                    edit.replace(sel, &with, at..at + inner.len());
                }
            }
            None => prefix_lines(edit, &text, sel, helper),
        },
    }
}

fn prefix_lines(edit: &mut TextEdit, text: &str, sel: std::ops::Range<usize>, helper: Helper) {
    let start = text[..sel.start].rfind('\n').map_or(0, |i| i + 1);
    let end = text[sel.end..]
        .find('\n')
        .map_or(text.len(), |i| sel.end + i);
    let lines: Vec<&str> = text[start..end].split('\n').collect();
    let have = |line: &str| match helper {
        Helper::Numbered => numbered_prefix(line).is_some(),
        _ => helper.prefix().is_some_and(|p| line.starts_with(p)),
    };
    let all = lines.iter().all(|l| have(l));
    let block: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(n, line)| {
            if all {
                let len = match helper {
                    Helper::Numbered => numbered_prefix(line).unwrap_or(0),
                    _ => helper.prefix().map_or(0, str::len),
                };
                line[len..].to_owned()
            } else if helper == Helper::Numbered {
                format!("{}. {line}", n + 1)
            } else {
                format!("{}{line}", helper.prefix().unwrap_or_default())
            }
        })
        .collect();
    let with = block.join("\n");
    let caret = start + with.len();
    edit.replace(start..end, &with, caret..caret);
}

/// The length of `12. ` at the start of `line`, if it has one.
fn numbered_prefix(line: &str) -> Option<usize> {
    let (n, _) = line.split_once(". ")?;
    (!n.is_empty() && n.chars().all(|c| c.is_ascii_digit())).then_some(n.len() + 2)
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
/// ends the list with a blank line instead. Inside a code fence it is a plain new line.
pub fn newline(edit: &mut TextEdit) {
    if edit.selection().is_some() {
        return edit.insert("\n");
    }
    let text = edit.text().to_owned();
    let caret = edit.caret();
    let start = text[..caret].rfind('\n').map_or(0, |i| i + 1);
    let line = &text[start..caret];
    let in_code = text[..caret]
        .lines()
        .filter(|l| l.trim_start().starts_with("```"))
        .count()
        % 2
        == 1;
    match line_marker(line).filter(|_| !in_code) {
        Some(marker) if line.trim_end() == marker.trim_end() => {
            edit.replace(start..caret, "\n", start + 1..start + 1);
        }
        Some(marker) => {
            let next = match marker.trim_start().split_once(". ") {
                Some((n, _)) if n.parse::<u64>().is_ok() => {
                    let indent = &marker[..marker.len() - marker.trim_start().len()];
                    format!("{indent}{}", numbered_after(marker.trim_start()))
                }
                _ => marker,
            };
            edit.insert(&format!("\n{next}"));
        }
        None => edit.insert("\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mentions_are_names_after_an_at_that_starts_a_word() {
        let text = "@juniper can you check, cc @ember.lind. mail a@b.io (@kestrel)";
        let found: Vec<&str> = mentions(text).into_iter().map(|r| &text[r]).collect();
        assert_eq!(found, ["@juniper", "@ember.lind", "@kestrel"]);
        assert!(mentions("@ alone").is_empty());
    }

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

    fn edit(text: &str, sel: std::ops::Range<usize>) -> TextEdit {
        let mut e = TextEdit::new(text);
        e.place(sel.start, false);
        e.place(sel.end, true);
        e
    }

    #[test]
    fn an_inline_helper_wraps_the_selection_and_unwraps_it() {
        let mut e = edit("fix this now", 4..8);
        apply(&mut e, Helper::Bold);
        assert_eq!(e.text(), "fix **this** now");
        assert_eq!(e.selected_text(), Some("this"));
        apply(&mut e, Helper::Bold);
        assert_eq!(e.text(), "fix this now");
    }

    #[test]
    fn with_nothing_selected_the_caret_lands_between_the_markers() {
        let mut e = edit("a ", 2..2);
        apply(&mut e, Helper::Code);
        e.insert("x");
        assert_eq!(e.text(), "a `x`");
    }

    #[test]
    fn a_link_wraps_its_text_and_selects_the_address() {
        let mut e = edit("see docs", 4..8);
        apply(&mut e, Helper::Link);
        assert_eq!(e.text(), "see [docs](url)");
        assert_eq!(e.selected_text(), Some("url"));
    }

    #[test]
    fn line_helpers_mark_every_selected_line_and_take_it_off_again() {
        let mut e = edit("one\ntwo", 1..5);
        apply(&mut e, Helper::Bullet);
        assert_eq!(e.text(), "- one\n- two");
        let len = e.text().len();
        e.place(0, false);
        e.place(len, true);
        apply(&mut e, Helper::Bullet);
        assert_eq!(e.text(), "one\ntwo");
        let mut e = edit("a\nb", 0..3);
        apply(&mut e, Helper::Numbered);
        assert_eq!(e.text(), "1. a\n2. b");
    }

    #[test]
    fn return_continues_a_list_and_ends_it_on_an_empty_item() {
        let mut e = TextEdit::new("- a");
        newline(&mut e);
        assert_eq!(e.text(), "- a\n- ");
        newline(&mut e);
        assert_eq!(e.text(), "- a\n\n");
        assert!(matches!(
            &parse(&format!("{}after", e.text()))[1],
            Block::Paragraph(_)
        ));
        let mut e = TextEdit::new("  3. c");
        newline(&mut e);
        assert_eq!(e.text(), "  3. c\n  4. ");
        let mut e = TextEdit::new("- [x] done");
        newline(&mut e);
        assert_eq!(e.text(), "- [x] done\n- [ ] ");
        let mut e = TextEdit::new("```\n- in code");
        newline(&mut e);
        assert_eq!(e.text(), "```\n- in code\n");
    }
}
