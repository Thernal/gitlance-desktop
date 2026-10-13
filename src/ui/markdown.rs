//! Comment bodies drawn as markdown (`crate::markdown`): GitLab discussions are written in it, so
//! the raw text reads badly. *Rich* draws it formatted, *Raw* shows the text as written; the choice
//! is a setting (⌥M), and a comment without markdown looks the same either way.

use super::px;
use super::theme;
use crate::highlight::{self, palette};
use crate::markdown::{self, Block, Helper, Inline};
use gpui::{
    AnyElement, App, Div, ElementId, FontStyle, FontWeight, HighlightStyle, InteractiveText,
    MouseButton, SharedString, StrikethroughStyle, StyledText, TextLayout, UnderlineStyle, div,
    prelude::*,
};
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

/// A selection in the text of one comment. The comment's text blocks (paragraphs, headings, list
/// items, lines of code) are numbered in reading order; an end is (block, byte in it).
#[derive(Clone, Debug)]
pub struct TextSel {
    pub note: SharedString,
    pub anchor: (usize, usize),
    pub head: (usize, usize),
    /// The blocks' text, for copying.
    pub texts: Rc<RefCell<Vec<String>>>,
}

impl TextSel {
    fn ends(&self) -> ((usize, usize), (usize, usize)) {
        (self.anchor.min(self.head), self.anchor.max(self.head))
    }

    /// What is selected in block `block` of length `len`.
    fn in_block(&self, block: usize, len: usize) -> Option<Range<usize>> {
        let (start, end) = self.ends();
        if block < start.0 || block > end.0 || start == end {
            return None;
        }
        let from = if block == start.0 { start.1 } else { 0 };
        let to = if block == end.0 { end.1 } else { len };
        (from < to).then_some(from..to.min(len))
    }

    /// The selected text, blocks joined by line breaks; `None` when nothing is selected.
    pub fn text(&self) -> Option<String> {
        let (start, end) = self.ends();
        if start == end {
            return None;
        }
        let texts = self.texts.borrow();
        let parts: Vec<&str> = (start.0..=end.0)
            .filter_map(|b| {
                let t = texts.get(b)?;
                let range = self.in_block(b, t.len())?;
                t.get(range)
            })
            .collect();
        Some(parts.join("\n"))
    }
}

/// A press in a comment's text.
pub enum Press {
    Down {
        note: SharedString,
        block: usize,
        at: usize,
        clicks: usize,
        shift: bool,
        texts: Rc<RefCell<Vec<String>>>,
    },
    Drag {
        note: SharedString,
        block: usize,
        at: usize,
    },
}

/// Where a comment's text sends its presses.
pub type OnPress = Rc<dyn Fn(Press, &mut App)>;

/// Selecting in comments: the selection now, and where presses go.
#[derive(Clone)]
pub struct Selecting {
    pub current: Option<TextSel>,
    pub on: OnPress,
}

/// Where relative links of a comment point: GitLab writes uploads as `/uploads/…`, relative to the
/// project, and other pages relative to the host.
fn resolve(url: &str, base: Option<&str>) -> String {
    let Some(base) = base.filter(|_| url.starts_with('/')) else {
        return url.to_owned();
    };
    if url.starts_with("/uploads/") {
        return format!("{base}{url}");
    }
    let host_end = base
        .find("://")
        .and_then(|s| base[s + 3..].find('/').map(|i| s + 3 + i))
        .unwrap_or(base.len());
    format!("{}{url}", &base[..host_end])
}

/// A comment's body: formatted when `rich` and it has markdown, else the text as written.
/// `key` makes the ids of its links unique among the comments on screen.
/// With `sel`, its text can be selected with the mouse (a drag, a double click for a word, three
/// for a block) and copied.
pub fn body(
    text: &str,
    rich: bool,
    key: impl Into<SharedString>,
    base: Option<&str>,
    sel: Option<Selecting>,
) -> AnyElement {
    let mut ctx = Ctx {
        key: key.into(),
        next: 0,
        base: base.map(str::to_owned),
        sel,
        blocks: 0,
        texts: Rc::default(),
    };
    let blocks = rich.then(|| markdown::parse(text));
    match blocks.filter(|b| markdown::is_rich(b)) {
        Some(blocks) => blocks_of(&blocks, &mut ctx).into_any_element(),
        None => {
            let styled = StyledText::new(text.to_owned());
            let layout = styled.layout().clone();
            text_block(styled.into_any_element(), layout, text, &mut ctx)
        }
    }
}

/// Does `text` look different as markdown? Only then is the Rich/Raw switch worth showing.
pub fn has_markdown(text: &str) -> bool {
    markdown::is_rich(&markdown::parse(text))
}

struct Ctx {
    key: SharedString,
    next: usize,
    base: Option<String>,
    sel: Option<Selecting>,
    /// Text blocks drawn so far: the next one's number.
    blocks: usize,
    texts: Rc<RefCell<Vec<String>>>,
}

/// A block of text that can be selected: numbered, its selection painted under it, the mouse
/// turned into presses. Without a `Selecting`, the text alone.
fn text_block(text_el: AnyElement, layout: TextLayout, text: &str, ctx: &mut Ctx) -> AnyElement {
    let Some(sel) = ctx.sel.clone() else {
        return div().child(text_el).into_any_element();
    };
    let block = ctx.blocks;
    ctx.blocks += 1;
    {
        let mut texts = ctx.texts.borrow_mut();
        if texts.len() <= block {
            texts.resize(block + 1, String::new());
        }
        texts[block] = text.to_owned();
    }
    let note = ctx.key.clone();
    let range = sel
        .current
        .as_ref()
        .filter(|s| s.note == note)
        .and_then(|s| s.in_block(block, text.len()));
    let (for_paint, for_down, for_move) = (layout.clone(), layout.clone(), layout);
    let (on_down, on_move) = (sel.on.clone(), sel.on);
    let (note_down, note_move) = (note.clone(), note);
    let texts = ctx.texts.clone();
    let len = text.len();
    div()
        .relative()
        .cursor_text()
        .child(
            gpui::canvas(
                |_, _, _| (),
                move |_, _, window, _| {
                    if let Some(r) = range.clone() {
                        paint_selection(&for_paint, r, window);
                    }
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
        .child(text_el)
        .on_mouse_down(MouseButton::Left, move |event, _, cx| {
            let at = super::input::nearest_index(&for_down, event.position).min(len);
            on_down(
                Press::Down {
                    note: note_down.clone(),
                    block,
                    at,
                    clicks: event.click_count,
                    shift: event.modifiers.shift,
                    texts: texts.clone(),
                },
                cx,
            );
        })
        .on_mouse_move(move |event, _, cx| {
            if event.pressed_button == Some(MouseButton::Left) {
                let at = super::input::nearest_index(&for_move, event.position).min(len);
                on_move(
                    Press::Drag {
                        note: note_move.clone(),
                        block,
                        at,
                    },
                    cx,
                );
            }
        })
        .into_any_element()
}

/// The selection's tint over `range` of a laid-out text, line by line as it wraps.
fn paint_selection(layout: &TextLayout, range: Range<usize>, window: &mut gpui::Window) {
    use gpui::{Bounds, fill, point};
    let (Some(start), Some(end)) = (
        layout.position_for_index(range.start),
        layout.position_for_index(range.end),
    ) else {
        return;
    };
    let height = layout.line_height();
    let bounds = layout.bounds();
    let tint = theme::selection();
    let mut rect = |x0: gpui::Pixels, x1: gpui::Pixels, y: gpui::Pixels| {
        if x1 > x0 {
            window.paint_quad(fill(
                Bounds::from_corners(point(x0, y), point(x1, y + height)),
                tint,
            ));
        }
    };
    if start.y == end.y {
        rect(start.x, end.x, start.y);
        return;
    }
    rect(start.x, bounds.right(), start.y);
    let mut y = start.y + height;
    while y < end.y {
        rect(bounds.left(), bounds.right(), y);
        y += height;
    }
    rect(bounds.left(), end.x, end.y);
}

fn blocks_of(blocks: &[Block], ctx: &mut Ctx) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .children(blocks.iter().map(|b| block(b, ctx)))
}

fn block(block: &Block, ctx: &mut Ctx) -> AnyElement {
    match block {
        Block::Paragraph(i) => inline(i, ctx),
        Block::Heading(level, i) => div()
            .font_weight(FontWeight::BOLD)
            .text_size(px(match level {
                1 => 16.,
                2 => 14.,
                _ => 13.,
            }))
            .line_height(px(match level {
                1 => 22.,
                2 => 20.,
                _ => 18.,
            }))
            .child(inline(i, ctx))
            .into_any_element(),
        Block::Code { lang, text } => code_block(lang, text, ctx),
        Block::Quote(blocks) => blocks_of(blocks, ctx)
            .pl(px(10.))
            .border_l_2()
            .border_color(theme::faint())
            .text_color(theme::muted())
            .into_any_element(),
        Block::List { start, items } => div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .children(items.iter().enumerate().map(|(n, item)| {
                let marker = match (item.task, start) {
                    (Some(true), _) => "☑".to_owned(),
                    (Some(false), _) => "☐".to_owned(),
                    (None, Some(s)) => format!("{}.", s + n as u64),
                    (None, None) => "•".to_owned(),
                };
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex_none()
                            .min_w(px(14.))
                            .text_color(if item.task == Some(true) {
                                theme::added()
                            } else {
                                theme::faint()
                            })
                            .child(marker),
                    )
                    .child(
                        blocks_of(&item.blocks, ctx)
                            .flex_1()
                            .min_w_0()
                            .gap(px(2.))
                            .when(item.task == Some(true), |s| s.text_color(theme::muted())),
                    )
            }))
            .into_any_element(),
        Block::Table { head, rows } => {
            let row = |cells: &[Inline], header: bool, ctx: &mut Ctx| {
                div()
                    .flex()
                    .when(header, |s| {
                        s.font_weight(FontWeight::SEMIBOLD).bg(theme::hover())
                    })
                    .children(cells.iter().map(|c| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .px(px(6.))
                            .py(px(2.))
                            .border_r_1()
                            .border_color(theme::island_border())
                            .child(inline(c, ctx))
                    }))
            };
            let header = row(head, true, ctx);
            let body: Vec<Div> = rows
                .iter()
                .map(|r| {
                    row(r, false, ctx)
                        .border_t_1()
                        .border_color(theme::island_border())
                })
                .collect();
            div()
                .flex()
                .flex_col()
                .rounded(px(4.))
                .border_1()
                .border_color(theme::island_border())
                .overflow_hidden()
                .child(header)
                .children(body)
                .into_any_element()
        }
        Block::Rule => div()
            .h(px(1.))
            .my(px(2.))
            .bg(theme::island_border())
            .into_any_element(),
        Block::Html(text) => div()
            .font_family(theme::CODE_FONT)
            .text_size(px(11.))
            .text_color(theme::faint())
            .child(text.clone())
            .into_any_element(),
    }
}

/// A fenced block: highlighted by its language; GitLab's `suggestion` blocks as the lines they
/// would put in.
fn code_block(lang: &str, text: &str, ctx: &mut Ctx) -> AnyElement {
    let suggestion = lang.starts_with("suggestion");
    let spans = if suggestion {
        vec![Vec::new(); text.lines().count()]
    } else {
        highlight::highlight_lang(lang, text)
    };
    let lines: Vec<Div> = text
        .split('\n')
        .enumerate()
        .map(|(n, line)| {
            let runs = spans.get(n).cloned().unwrap_or_default();
            let styled = StyledText::new(if line.is_empty() {
                " ".to_owned()
            } else {
                line.to_owned()
            })
            .with_highlights(
                runs.into_iter()
                    .filter(|s| s.range.end <= line.len())
                    .map(|s| {
                        (
                            s.range,
                            HighlightStyle {
                                color: Some(theme::code(s.color)),
                                font_style: s.italic.then_some(FontStyle::Italic),
                                ..Default::default()
                            },
                        )
                    }),
            );
            let layout = styled.layout().clone();
            div()
                .px(px(8.))
                .when(suggestion, |s| s.bg(theme::added_line()))
                .child(text_block(styled.into_any_element(), layout, line, ctx))
        })
        .collect();
    div()
        .flex()
        .flex_col()
        .rounded(px(4.))
        .border_1()
        .border_color(theme::island_border())
        .bg(theme::editor())
        .overflow_hidden()
        .children(suggestion.then(|| {
            div()
                .px(px(8.))
                .py(px(2.))
                .bg(theme::hover())
                .font_family(theme::UI_FONT)
                .text_size(px(11.))
                .text_color(theme::muted())
                .child("Suggested change")
        }))
        .child(
            div()
                .py(px(4.))
                .font_family(theme::CODE_FONT)
                .text_size(px(11.5))
                .line_height(px(16.))
                .text_color(theme::code(palette::FOREGROUND))
                .children(lines),
        )
        .into_any_element()
}

/// A paragraph's text with its styles; links open in the browser.
fn inline(i: &Inline, ctx: &mut Ctx) -> AnyElement {
    let code_bg: gpui::Hsla = gpui::rgba(0xffffff14).into();
    let accent: gpui::Hsla = theme::accent().into();
    let highlights: Vec<_> = i
        .spans
        .iter()
        .map(|s| {
            let st = s.style;
            (
                s.range.clone(),
                HighlightStyle {
                    font_weight: st.bold.then_some(FontWeight::BOLD),
                    font_style: st.italic.then_some(FontStyle::Italic),
                    background_color: st.code.then_some(code_bg),
                    strikethrough: st.strike.then_some(StrikethroughStyle {
                        thickness: px(1.),
                        color: None,
                    }),
                    color: s.link.is_some().then_some(accent),
                    underline: s.link.is_some().then_some(UnderlineStyle {
                        thickness: px(1.),
                        color: Some(accent),
                        wavy: false,
                    }),
                    ..Default::default()
                },
            )
        })
        .collect();
    let fonts: Vec<_> = i
        .spans
        .iter()
        .filter(|s| s.style.code)
        .map(|s| (s.range.clone(), SharedString::from(theme::CODE_FONT)))
        .collect();
    let styled = StyledText::new(i.text.clone())
        .with_highlights(highlights)
        .with_font_family_overrides(fonts);
    let links: Vec<_> = i
        .spans
        .iter()
        .filter_map(|s| {
            let url = s.link.as_deref()?;
            Some((s.range.clone(), resolve(url, ctx.base.as_deref())))
        })
        .collect();
    let layout = styled.layout().clone();
    if links.is_empty() {
        return text_block(styled.into_any_element(), layout, &i.text, ctx);
    }
    ctx.next += 1;
    let id = ElementId::NamedInteger(ctx.key.clone(), ctx.next as u64);
    let (ranges, urls): (Vec<_>, Vec<_>) = links.into_iter().unzip();
    let linked = InteractiveText::new(id, styled).on_click(ranges, move |ix, _, cx| {
        if let Some(url) = urls.get(ix) {
            cx.open_url(url);
        }
    });
    text_block(linked.into_any_element(), layout, &i.text, ctx)
}

/// The icon and the name (with its key, if it has one) of a composer helper.
pub fn helper_look(h: Helper) -> (&'static str, &'static str) {
    match h {
        Helper::Bold => ("bold", "Bold  ⌘B"),
        Helper::Italic => ("italic", "Italic  ⌘I"),
        Helper::Strike => ("strikethrough", "Strikethrough  ⌘⇧X"),
        Helper::Code => ("code", "Code  ⌘⇧E"),
        Helper::Link => ("link", "Link — text, then again for the address  ⌘⇧K"),
        Helper::Quote => ("quote", "Quote"),
        Helper::Bullet => ("list", "Bulleted list"),
        Helper::Numbered => ("list-ordered", "Numbered list"),
        Helper::CodeBlock => ("code-block", "Code block — again to close it"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_links_point_into_the_project_or_the_host() {
        let base = Some("https://gitlab.x.io/group/app");
        assert_eq!(
            resolve("/uploads/ab/shot.png", base),
            "https://gitlab.x.io/group/app/uploads/ab/shot.png"
        );
        assert_eq!(
            resolve("/group/app/-/issues/3", base),
            "https://gitlab.x.io/group/app/-/issues/3"
        );
        assert_eq!(resolve("https://a.b/c", base), "https://a.b/c");
        assert_eq!(resolve("/x", None), "/x");
    }

    fn sel(anchor: (usize, usize), head: (usize, usize)) -> TextSel {
        TextSel {
            note: "n".into(),
            anchor,
            head,
            texts: Rc::new(RefCell::new(vec![
                "first para".into(),
                "second one".into(),
                "third".into(),
            ])),
        }
    }

    #[test]
    fn a_selection_copies_across_blocks_in_reading_order() {
        // Dragged upwards: the head is before the anchor.
        assert_eq!(
            sel((2, 3), (0, 6)).text().as_deref(),
            Some("para\nsecond one\nthi")
        );
        assert_eq!(sel((1, 0), (1, 6)).text().as_deref(), Some("second"));
        assert_eq!(sel((1, 4), (1, 4)).text(), None);
    }

    #[test]
    fn a_block_shows_only_its_part() {
        let s = sel((0, 6), (2, 3));
        assert_eq!(s.in_block(0, 10), Some(6..10));
        assert_eq!(s.in_block(1, 10), Some(0..10));
        assert_eq!(s.in_block(2, 5), Some(0..3));
        assert_eq!(s.in_block(3, 5), None);
    }
}
