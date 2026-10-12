//! Comment bodies drawn as markdown (`crate::markdown`): GitLab discussions are written in it, so
//! the raw text reads badly. *Rich* draws it formatted, *Raw* shows the text as written; the choice
//! is a setting (⌥M), and a comment without markdown looks the same either way.

use super::px;
use super::theme;
use crate::highlight::{self, palette};
use crate::markdown::{self, Block, Helper, Inline};
use gpui::{
    AnyElement, Div, ElementId, FontStyle, FontWeight, HighlightStyle, InteractiveText,
    SharedString, StrikethroughStyle, StyledText, UnderlineStyle, div, prelude::*,
};

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
pub fn body(
    text: &str,
    rich: bool,
    key: impl Into<SharedString>,
    base: Option<&str>,
) -> AnyElement {
    if !rich {
        return div().child(text.to_owned()).into_any_element();
    }
    let blocks = markdown::parse(text);
    if !markdown::is_rich(&blocks) {
        return div().child(text.to_owned()).into_any_element();
    }
    let mut ctx = Ctx {
        key: key.into(),
        next: 0,
        base: base.map(str::to_owned),
    };
    blocks_of(&blocks, &mut ctx).into_any_element()
}

/// Does `text` look different as markdown? Only then is the Rich/Raw switch worth showing.
pub fn has_markdown(text: &str) -> bool {
    markdown::is_rich(&markdown::parse(text))
}

struct Ctx {
    key: SharedString,
    next: usize,
    base: Option<String>,
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
        Block::Code { lang, text } => code_block(lang, text),
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
fn code_block(lang: &str, text: &str) -> AnyElement {
    let suggestion = lang.starts_with("suggestion");
    let spans = if suggestion {
        vec![Vec::new(); text.lines().count()]
    } else {
        highlight::highlight_lang(lang, text)
    };
    let lines = text.split('\n').enumerate().map(|(n, line)| {
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
        div()
            .px(px(8.))
            .when(suggestion, |s| s.bg(theme::added_line()))
            .child(styled)
    });
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
    if links.is_empty() {
        return div().child(styled).into_any_element();
    }
    ctx.next += 1;
    let id = ElementId::NamedInteger(ctx.key.clone(), ctx.next as u64);
    let (ranges, urls): (Vec<_>, Vec<_>) = links.into_iter().unzip();
    div()
        .cursor_pointer()
        .child(
            InteractiveText::new(id, styled).on_click(ranges, move |ix, _, cx| {
                if let Some(url) = urls.get(ix) {
                    cx.open_url(url);
                }
            }),
        )
        .into_any_element()
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
    use super::resolve;

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
}
