//! Settings → Appearance (Design #12 A + C, `docs/specs/themes.md`, flow 09): the Window themes as
//! cards three to a row — tabs down the left, the code as bars in the theme's syntax colours and
//! diff tints —, the Code themes as syntax samples with "Follow the window" first, several projects,
//! panes, and your themes (the folder, Copy One Dark as JSON, each file's problems). A theme opens in
//! the editor: its tokens by group, contrast per text token with a fix, and beside them a preview —
//! a request's header, a piece of diff and a discussion in the theme being edited, while the window
//! keeps the chosen themes; editing a built-in theme makes a new one that extends it.

use super::lists::Field;
use super::{ROW_RADIUS, Workspace, icons, px, theme};
use crate::themes::{self, T, TOKENS, Theme};
use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, HighlightStyle, Rgba, StyledText, div,
    prelude::*, relative, rgba,
};

/// A theme being edited.
pub struct ThemeEdit {
    pub theme: Theme,
    /// As it was when the editor opened: Revert goes back to it.
    start: Theme,
    /// What it extends, for "Use One Dark's".
    base: Theme,
    /// A copy made from a built-in theme (named `from`).
    from: Option<String>,
    /// The token whose value is open.
    pub selected: Option<usize>,
    /// Its colour as the picker holds it: hue, saturation, value, alpha — kept apart from the
    /// value, so a grey keeps the hue it was dragged to.
    hsva: [f32; 4],
    /// The last save's outcome, said beside Save.
    said: Option<Result<String, String>>,
}

fn c(t: &Theme, tok: T) -> Rgba {
    rgba(t.get(tok))
}

/// The editor's rows: the tokens, then the parse tree's roles (`TOKENS.len()` + n, #13).
const ROWS: usize = TOKENS.len() + themes::ROLES;

/// Row `i`'s colour in `t`: a token's, or a role's — the token it falls back to when `t` gives it
/// none.
fn value(t: &Theme, i: usize) -> u32 {
    use crate::highlight::role;
    let Some(r) = i.checked_sub(TOKENS.len()) else {
        return t.colors[i];
    };
    t.roles[r].unwrap_or(match role::ROLES[r].token {
        n @ 1..=8 => t.colors[T::Code1 as usize + n as usize - 1],
        role::MUTED => t.get(T::Text2),
        _ => t.get(T::Text1),
    })
}

/// Whether `t` sets row `i` itself.
fn own(t: &Theme, i: usize) -> bool {
    match i.checked_sub(TOKENS.len()) {
        None => t.own.contains(&i),
        Some(r) => t.own_roles.contains(&r),
    }
}

/// Whether row `i` is text, read on every surface: the text tokens and every role.
fn is_text(i: usize) -> bool {
    i >= TOKENS.len() || themes::TEXT.iter().any(|&x| x as usize == i)
}

/// Row `i`'s name and what it is for.
fn row_name(i: usize) -> (String, String) {
    use crate::highlight::role;
    match i.checked_sub(TOKENS.len()) {
        None => (TOKENS[i].0.to_owned(), TOKENS[i].2.to_owned()),
        Some(r) => {
            let look = &role::ROLES[r];
            let falls = match look.token {
                n @ 1..=8 => format!("code-0{n}"),
                role::MUTED => "content-02".to_owned(),
                _ => "content-01".to_owned(),
            };
            let style = match (look.bold, look.italic) {
                (true, _) => " · semibold",
                (_, true) => " · italic",
                _ => "",
            };
            (
                format!("code.{}", look.name),
                format!("role · falls back to {falls}{style}"),
            )
        }
    }
}

/// The Window card's face (`cards-5.html` A): tabs on the left, the selected one in the accent;
/// code as bars, a removed and two added lines in the diff's tints.
fn window_face(t: &Theme) -> Div {
    let bar = |tok: T, w: f32, ring: Option<T>| {
        let b = div().h(px(5.)).rounded(px(2.)).bg(c(t, tok));
        match ring {
            Some(r) => div()
                .w(relative(w))
                .p(px(2.))
                .rounded(px(3.))
                .bg(c(t, r))
                .child(b)
                .into_any_element(),
            None => b.w(relative(w)).into_any_element(),
        }
    };
    let line = |kind: i8, parts: Vec<AnyElement>, indent: bool| {
        let (bg, gutter) = match kind {
            -1 => (Some(c(t, T::Removed1)), Some(c(t, T::Removed2))),
            1 => (Some(c(t, T::Added1)), Some(c(t, T::Added2))),
            _ => (None, None),
        };
        div()
            .h(px(13.))
            .flex()
            .items_center()
            .gap(px(4.))
            .pr(px(8.))
            .when_some(bg, |s, bg| s.bg(bg))
            .child(
                div()
                    .w(px(18.))
                    .h_full()
                    .flex_none()
                    .when_some(gutter, |s, g| s.bg(g)),
            )
            .when(indent, |s| s.child(div().w(px(10.)).flex_none()))
            .children(parts)
    };
    let tab = |on: bool| {
        div()
            .h(px(18.))
            .rounded(px(4.))
            .bg(if on { c(t, T::FillHover) } else { c(t, T::Bg3) })
            .when(on, |s| s.border_l_2().border_color(c(t, T::Accent)))
    };
    div()
        .h(px(132.))
        .flex()
        .rounded(px(6.))
        .overflow_hidden()
        .border_1()
        .border_color(c(t, T::Border1))
        .bg(c(t, T::Bg1))
        .child(
            div()
                .w(px(54.))
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(4.))
                .px(px(5.))
                .py(px(6.))
                .bg(c(t, T::Bg2))
                .child(tab(true))
                .child(tab(false))
                .child(tab(false))
                .child(tab(false)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .py(px(6.))
                .flex()
                .flex_col()
                .gap(px(3.))
                .bg(c(t, T::Bg3))
                .child(line(
                    0,
                    vec![
                        bar(T::Code1, 0.12, None),
                        bar(T::Code2, 0.2, None),
                        bar(T::Text3, 0.1, None),
                    ],
                    false,
                ))
                .child(line(
                    0,
                    vec![
                        bar(T::Code1, 0.09, None),
                        bar(T::Text3, 0.14, None),
                        bar(T::Code5, 0.22, None),
                    ],
                    true,
                ))
                .child(line(
                    -1,
                    vec![
                        bar(T::Text3, 0.14, None),
                        bar(T::Code5, 0.18, Some(T::Removed3)),
                    ],
                    true,
                ))
                .child(line(
                    1,
                    vec![
                        bar(T::Text3, 0.14, None),
                        bar(T::Code2, 0.16, Some(T::Added3)),
                        bar(T::Text3, 0.08, None),
                    ],
                    true,
                ))
                .child(line(
                    1,
                    vec![bar(T::Code4, 0.1, None), bar(T::Code3, 0.26, None)],
                    true,
                ))
                .child(line(0, vec![bar(T::Code6, 0.4, None)], true)),
        )
}

/// Pieces of a sample line and the code token each is drawn in.
type Runs = Vec<(std::ops::Range<usize>, T)>;

/// D's Code card: a few lines of Kotlin in the theme's syntax colours on its diff surface.
fn code_face(t: &Theme) -> Div {
    let lines: [(&str, Runs); 4] = [
        ("fun retry() {", vec![(0..3, T::Code1), (4..9, T::Code2)]),
        (
            "  val m = \"failed\"",
            vec![(2..5, T::Code1), (10..18, T::Code3)],
        ),
        ("  tries += 1", vec![(11..12, T::Code4)]),
        ("}", vec![]),
    ];
    div()
        .h(px(84.))
        .px(px(10.))
        .py(px(8.))
        .rounded(px(6.))
        .overflow_hidden()
        .bg(c(t, T::Bg3))
        .font_family(theme::CODE_FONT)
        .text_size(px(12.))
        .line_height(px(17.))
        .text_color(c(t, T::Text1))
        .children(lines.into_iter().map(|(text, runs)| {
            div()
                .whitespace_nowrap()
                .child(
                    StyledText::new(text).with_highlights(runs.into_iter().map(|(r, tok)| {
                        (
                            r,
                            HighlightStyle {
                                color: Some(c(t, tok).into()),
                                ..Default::default()
                            },
                        )
                    })),
                )
        }))
}

/// Sample Kotlin coloured as the highlighter would: keywords, types (a capital), calls, members,
/// numbers, strings, comments and operators.
fn kotlin_runs(line: &str) -> Runs {
    const KEYWORDS: [&str; 7] = ["class", "private", "val", "var", "fun", "by", "return"];
    let b = line.as_bytes();
    let mut runs = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let start = i;
        if line[i..].starts_with("//") {
            runs.push((i..b.len(), T::Code6));
            break;
        }
        if b[i] == b'"' {
            i = line[i + 1..].find('"').map_or(b.len(), |e| i + e + 2);
            runs.push((start..i, T::Code3));
        } else if b[i].is_ascii_digit() {
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            runs.push((start..i, T::Code4));
        } else if b[i].is_ascii_alphabetic() || b[i] == b'_' {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let word = &line[start..i];
            let token = if KEYWORDS.contains(&word) {
                Some(T::Code1)
            } else if word.starts_with(|c: char| c.is_ascii_uppercase()) {
                Some(T::Code5)
            } else if b.get(i) == Some(&b'(') {
                Some(T::Code2)
            } else if start > 0 && b[start - 1] == b'.' {
                Some(T::Code7)
            } else {
                None
            };
            runs.extend(token.map(|t| (start..i, t)));
        } else {
            if matches!(b[i], b'=' | b'<' | b'>') {
                runs.push((i..i + 1, T::Code8));
            }
            i += 1;
        }
    }
    runs
}

/// The preview's piece of diff (Design #12 C and D's sample): (kind -1 / 0 / 1, old and new
/// numbers, the line, the word marked as changed).
const SAMPLE: [(i8, u32, u32, &str, &str); 9] = [
    (0, 12, 12, "class LoginViewModel(", ""),
    (0, 13, 13, "    private val auth: AuthRepository,", ""),
    (0, 14, 14, ") : ViewModel() {", ""),
    (
        -1,
        15,
        0,
        "    var state by mutableStateOf(LoginState())",
        "mutableStateOf",
    ),
    (
        1,
        0,
        15,
        "    private val _state = MutableStateFlow(LoginState())",
        "MutableStateFlow",
    ),
    (
        1,
        0,
        16,
        "    val state: StateFlow<LoginState> = _state",
        "",
    ),
    (0, 16, 17, "    // One source of truth for the screen", ""),
    (0, 17, 18, "    fun submit(email: String) {", ""),
    (
        0,
        18,
        19,
        "        if (email.isBlank()) return show(\"Email is empty\", 3)",
        "",
    ),
];

/// The sample drawn as the diff draws it: the surface and the plain text the window's, the tints
/// and the syntax colours the code theme's (D16).
fn sample_diff(window: &Theme, code: &Theme) -> Div {
    let number = |n: u32, tint: Option<Rgba>| {
        div()
            .w(px(34.))
            .h_full()
            .flex_none()
            .flex()
            .items_center()
            .justify_end()
            .pr(px(8.))
            .text_color(c(window, T::Text3))
            .when_some(tint, |s, t| s.bg(t))
            .child(if n == 0 { String::new() } else { n.to_string() })
    };
    div()
        .py(px(4.))
        .bg(c(window, T::Bg3))
        .font_family(theme::CODE_FONT)
        .text_size(px(12.))
        .text_color(c(window, T::Text1))
        .children(SAMPLE.iter().map(|&(kind, old, new, text, word)| {
            let (line, gutter, mark) = match kind {
                -1 => (Some(T::Removed1), Some(T::Removed2), T::Removed3),
                1 => (Some(T::Added1), Some(T::Added2), T::Added3),
                _ => (None, None, T::Added3),
            };
            let gutter = gutter.map(|g| c(code, g));
            // Each byte's colour and whether it is the changed word, run together.
            let mut look: Vec<(Option<T>, bool)> = vec![(None, false); text.len()];
            for (r, t) in kotlin_runs(text) {
                for l in &mut look[r] {
                    l.0 = Some(t);
                }
            }
            if let Some(at) = (!word.is_empty()).then(|| text.find(word)).flatten() {
                for l in &mut look[at..at + word.len()] {
                    l.1 = true;
                }
            }
            let mut runs = Vec::new();
            let mut from = 0;
            for i in 1..=look.len() {
                if i == look.len() || look[i] != look[from] {
                    let (color, marked) = look[from];
                    if color.is_some() || marked {
                        runs.push((
                            from..i,
                            HighlightStyle {
                                color: color.map(|t| c(code, t).into()),
                                background_color: marked.then(|| c(code, mark).into()),
                                ..Default::default()
                            },
                        ));
                    }
                    from = i;
                }
            }
            div()
                .h(px(19.))
                .flex()
                .items_center()
                .when_some(line, |s, l| s.bg(c(code, l)))
                .child(number(old, gutter))
                .child(number(new, gutter))
                .child(
                    div()
                        .pl(px(10.))
                        .whitespace_nowrap()
                        .child(StyledText::new(text).with_highlights(runs)),
                )
        }))
}

/// The preview of the editor (Design #12 C): the app in theme `t` — a request's header, the
/// sample diff and a selected discussion.
fn theme_preview(t: &Theme) -> Div {
    let header = div()
        .px(px(10.))
        .h(px(40.))
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(px(13.))
        .text_color(c(t, T::Text2))
        .child(
            icons::icon("pull-request")
                .size(px(16.))
                .text_color(c(t, T::Text2)),
        )
        .child("!398")
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(c(t, T::Text1))
                .child("Refactor the login screen state"),
        )
        .child(
            div()
                .px(px(10.))
                .h(px(24.))
                .flex()
                .items_center()
                .rounded(px(ROW_RADIUS))
                .bg(c(t, T::Accent))
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(c(t, T::Text5))
                .child("Approve"),
        );
    let discussion = div()
        .m(px(10.))
        .p(px(10.))
        .rounded(px(8.))
        .border_2()
        .border_color(c(t, T::Focus))
        .bg(c(t, T::Bg2))
        .flex()
        .flex_col()
        .gap(px(8.))
        .text_size(px(12.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .size(px(22.))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(c(t, T::Code5))
                        .text_size(px(10.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(t, T::Bg1))
                        .child("EL"),
                )
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(t, T::Text1))
                        .child("Ember Lind"),
                )
                .child(div().text_color(c(t, T::Text3)).child("2 h ago"))
                .child(div().flex_1())
                .child(
                    div()
                        .px(px(6.))
                        .rounded(px(4.))
                        .bg(c(t, T::AttentionBg))
                        .text_size(px(11.))
                        .text_color(c(t, T::Attention))
                        .child("open"),
                ),
        )
        .child(
            div()
                .text_color(c(t, T::Text1))
                .child("Does the old state survive a rotation? StateFlow keeps it, but the screen reads it once."),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .h(px(28.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .rounded(px(ROW_RADIUS))
                        .border_1()
                        .border_color(c(t, T::Border1))
                        .bg(c(t, T::Bg1))
                        .text_color(c(t, T::Text3))
                        .child("Reply…"),
                )
                .child(
                    div()
                        .px(px(10.))
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .rounded(px(ROW_RADIUS))
                        .bg(c(t, T::Bg4))
                        .text_color(c(t, T::Text1))
                        .child("Resolve"),
                ),
        );
    div()
        .rounded(px(10.))
        .overflow_hidden()
        .border_1()
        .border_color(c(t, T::Border1))
        .bg(c(t, T::Bg1))
        .child(header)
        .child(
            div()
                .mx(px(8.))
                .rounded(px(6.))
                .overflow_hidden()
                .child(sample_diff(t, t)),
        )
        .child(discussion)
}

/// A card's frame: the accent around the chosen one, its name and a word under it.
fn card(
    id: (&'static str, usize),
    face: Div,
    name: String,
    tag: Option<(String, bool)>,
    on: bool,
    extra: Option<AnyElement>,
) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex_1()
        .min_w_0()
        .p(px(6.))
        .rounded(px(10.))
        .border_2()
        .border_color(if on {
            theme::accent()
        } else {
            gpui::transparent_black().into()
        })
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .child(face)
        .child(
            div()
                .mt(px(6.))
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(12.))
                .child(div().flex_1().min_w_0().truncate().child(name))
                .children(tag.map(|(t, bad)| {
                    div()
                        .text_size(px(11.))
                        .text_color(if bad {
                            theme::removed()
                        } else {
                            theme::faint()
                        })
                        .child(t)
                }))
                .children(extra),
        )
}

/// Cards three to a row.
fn rows_of_three(cards: Vec<AnyElement>) -> Div {
    let mut rows: Vec<Div> = Vec::new();
    let mut cards = cards.into_iter().peekable();
    while cards.peek().is_some() {
        let mut row = div().flex().gap(px(12.));
        let mut n = 0;
        for card in cards.by_ref().take(3) {
            row = row.child(card);
            n += 1;
        }
        for _ in n..3 {
            row = row.child(div().flex_1());
        }
        rows.push(row);
    }
    div().flex().flex_col().gap(px(8.)).children(rows)
}

fn section(title: &'static str, sub: &'static str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(
            div()
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(theme::muted())
                .child(sub),
        )
}

fn small_button(
    id: impl Into<gpui::ElementId>,
    label: impl Into<gpui::SharedString>,
) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .px(px(10.))
        .h(px(26.))
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded(px(ROW_RADIUS))
        .border_1()
        .border_color(theme::island_border())
        .bg(theme::panel())
        .text_size(px(12.))
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .child(label.into())
}

/// A tag for a theme's card: default, light, yours, its problems (true: drawn as one).
fn tag_of(t: &Theme) -> Option<(String, bool)> {
    if !t.problems.is_empty() {
        return Some((super::plural(t.problems.len(), "problem"), true));
    }
    let word = if t.id == "one-dark" {
        "default"
    } else if !t.built_in {
        "JSON"
    } else if t.light() {
        "light"
    } else {
        return None;
    };
    Some((word.into(), false))
}

impl Workspace {
    /// The page: Window, Code, several projects, panes, your themes — or the editor.
    pub(super) fn render_appearance(
        &self,
        projects: AnyElement,
        panes: AnyElement,
        fold: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.theme_edit.is_some() {
            return self.render_theme_editor(cx);
        }
        let all = theme::all();
        let choice = theme::choice();
        let window = all
            .iter()
            .find(|t| t.id == choice.window)
            .unwrap_or(&all[0]);
        let code = choice
            .code
            .as_deref()
            .and_then(|id| all.iter().find(|t| t.id == id));
        let windows: Vec<AnyElement> = all
            .iter()
            .filter(|t| t.window)
            .enumerate()
            .map(|(i, t)| {
                let id = t.id.clone();
                let edit = t.id.clone();
                let edit_button = div()
                    .id(("win-edit", i))
                    .px(px(6.))
                    .rounded(px(4.))
                    .text_size(px(11.))
                    .text_color(theme::accent())
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::selected()))
                    .child("Edit")
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        this.start_theme_edit(&edit, cx);
                    }))
                    .into_any_element();
                card(
                    ("win-theme", i),
                    window_face(t),
                    t.label.clone(),
                    tag_of(t),
                    choice.window == t.id,
                    Some(edit_button),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let mut c = theme::choice();
                    c.window = id.clone();
                    this.choose_theme(c, cx);
                }))
                .into_any_element()
            })
            .collect();
        let mut codes: Vec<AnyElement> = vec![
            card(
                ("code-theme-follow", 0),
                div()
                    .h(px(84.))
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(theme::editor())
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child("Same as the window"),
                "Follow the window".into(),
                None,
                choice.code.is_none(),
                None,
            )
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                let mut c = theme::choice();
                c.code = None;
                this.choose_theme(c, cx);
            }))
            .into_any_element(),
        ];
        codes.extend(all.iter().filter(|t| t.code).enumerate().map(|(i, t)| {
            let id = t.id.clone();
            // Drawn on the window's diff surface (D16): what reads under 4.5:1 there is said.
            let low = themes::code_on(t, window).len();
            card(
                ("code-theme", i),
                code_face(t),
                t.label.clone(),
                (low > 0).then(|| (format!("{low} under 4.5:1 here"), true)),
                choice.code.as_deref() == Some(t.id.as_str()),
                None,
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                let mut c = theme::choice();
                c.code = Some(id.clone());
                this.choose_theme(c, cx);
            }))
            .into_any_element()
        }));
        let folder = themes::folder()
            .map(|f| super::format::home_relative(&f))
            .unwrap_or_default();
        let troubled: Vec<&Theme> = all
            .iter()
            .filter(|t| !t.built_in && !t.problems.is_empty())
            .collect();
        div()
            .flex()
            .flex_col()
            .gap_5()
            .child(section(
                "Window",
                "Surfaces, text, the accent and status. Text is 4.5:1 or more in every built-in theme.",
            ))
            .child(rows_of_three(windows))
            .child(section(
                "Code",
                "Syntax colours and the diff's tints. They follow the window until you pick another.",
            ))
            .child(rows_of_three(codes))
            .children(code.and_then(|code| {
                let low = themes::code_on(code, window);
                let (worst, ratio) = low
                    .iter()
                    .copied()
                    .min_by(|a, b| a.1.total_cmp(&b.1))?;
                // It opens out and fades in, rather than pushing the page down at once (the
                // developer, 2026-10-23); a new pair of themes opens it again.
                let id = gpui::SharedString::from(format!("code-contrast-{}-{}", code.id, window.id));
                Some(super::motion::appear(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .px(px(12.))
                        .py(px(8.))
                        .overflow_hidden()
                        .rounded(px(ROW_RADIUS))
                        .bg(theme::danger_bg())
                        .text_size(px(12.))
                        .child(div().flex_1().min_w_0().child(format!(
                            "{}'s code on {}'s diff: {} of {} syntax colours under 4.5:1, {} at {ratio:.1}:1.",
                            code.label,
                            window.label,
                            low.len(),
                            themes::SYNTAX.len(),
                            TOKENS[worst as usize].0
                        )))
                        .child(small_button("code-follow", "Follow the window").on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| {
                                let mut c = theme::choice();
                                c.code = None;
                                this.choose_theme(c, cx);
                            }),
                        )),
                    id,
                    super::motion::BASE,
                    |warning, v| warning.max_h(px(96. * v)).opacity(v),
                ))
            }))
            .child(
                div()
                    .rounded(px(8.))
                    .overflow_hidden()
                    .border_1()
                    .border_color(theme::island_border())
                    .child(sample_diff(window, code.unwrap_or(window))),
            )
            .child(projects)
            .child(panes)
            .child(fold)
            .child(section(
                "Your themes",
                "A .json file in the themes folder shows up above; its colors set any token by name, or window and code set those parts apart — a theme with only one shows up there only. A theme extends One Dark unless it says another.",
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        small_button("themes-folder", "Open themes folder").on_click(cx.listener(
                            |_, _: &ClickEvent, _, cx| {
                                if let Some(dir) = themes::folder() {
                                    let _ = std::fs::create_dir_all(&dir);
                                    cx.reveal_path(&dir);
                                }
                            },
                        )),
                    )
                    .child(
                        small_button("themes-copy", "Copy One Dark as JSON").on_click(cx.listener(
                            |_, _: &ClickEvent, _, cx| {
                                let json = theme::all()[0].to_json(true);
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(json));
                            },
                        )),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_family(theme::CODE_FONT)
                            .text_size(px(11.))
                            .text_color(theme::faint())
                            .child(folder),
                    ),
            )
            .children(troubled.into_iter().map(|t| {
                div()
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(ROW_RADIUS))
                    .bg(theme::danger_bg())
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .text_size(px(12.))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("{} — {}", t.label, tag_of(t).unwrap_or_default().0)),
                    )
                    .children(t.problems.iter().map(|p| div().child(format!("· {p}"))))
            }))
            .child(self.render_file_icons(cx))
            .into_any_element()
    }

    /// File icons (#95): what they are, a few of them, the user's folder and what it gives.
    fn render_file_icons(&self, cx: &mut Context<Self>) -> AnyElement {
        let (svgs, rules) = crate::file_icons::counts();
        let problems = crate::file_icons::problems();
        let folder = crate::file_icons::folder()
            .map(|d| super::format::home_relative(&d))
            .unwrap_or_default();
        let samples = [
            ("LoginViewModel.kt", false),
            ("build.gradle.kts", false),
            ("Main.java", false),
            ("package.json", false),
            ("README.md", false),
            ("Dockerfile", false),
            ("core", true),
            (".github", true),
        ];
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(section(
                "File icons",
                "IntelliJ Community's icons, by a file's name or extension. Yours go in the icons folder: name.svg for a whole name (Dockerfile.svg), an extension (kt.svg) or one of the icons replaced (folder.svg, kotlin.svg), name_dark.svg for dark themes; folders.txt and files.txt hold a regular expression on the path from the repository's root, then the icon, a line each — (^|/)src/test$ testRoot.",
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_x(px(14.))
                    .gap_y(px(6.))
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .children(samples.into_iter().map(|(name, is_folder)| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(if is_folder {
                                icons::folder_icon(name, 16.)
                            } else {
                                icons::file_icon(name, 16.)
                            })
                            .child(name)
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        small_button("icons-folder", "Open icons folder").on_click(cx.listener(
                            |_, _: &ClickEvent, _, cx| {
                                if let Some(dir) = crate::file_icons::folder() {
                                    crate::file_icons::start_folder(&dir);
                                    cx.reveal_path(&dir);
                                }
                            },
                        )),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(theme::muted())
                            .child(format!(
                                "{} · {}",
                                super::plural(svgs, "icon"),
                                super::plural(rules, "rule")
                            )),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_family(theme::CODE_FONT)
                            .text_size(px(11.))
                            .text_color(theme::faint())
                            .child(folder),
                    ),
            )
            .children((!problems.is_empty()).then(|| {
                div()
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(ROW_RADIUS))
                    .bg(theme::danger_bg())
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .text_size(px(12.))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Icon rules"))
                    .children(problems.into_iter().map(|p| div().child(format!("· {p}"))))
            }))
            .into_any_element()
    }

    /// Draws with `choice` in every window and keeps it.
    pub(super) fn choose_theme(&mut self, choice: theme::Choice, cx: &mut Context<Self>) {
        theme::choose(choice);
        cx.refresh_windows();
        cx.notify();
    }

    /// Opens theme `id` in the editor; a built-in theme is copied first.
    pub(super) fn start_theme_edit(&mut self, id: &str, cx: &mut Context<Self>) {
        let all = theme::all();
        let Some(t) = all.iter().find(|t| t.id == id).cloned() else {
            return;
        };
        let (theme, from) = if t.built_in {
            let mut name = format!("my-{}", t.id);
            let mut n = 2;
            while all.iter().any(|o| o.id == name) {
                name = format!("my-{}-{n}", t.id);
                n += 1;
            }
            (
                Theme {
                    id: name.clone(),
                    label: name,
                    built_in: false,
                    extends: Some(t.id.clone()),
                    own: Vec::new(),
                    file: None,
                    ..t.clone()
                },
                Some(t.label.clone()),
            )
        } else {
            (t.clone(), None)
        };
        let base = theme
            .extends
            .as_deref()
            .and_then(|e| all.iter().find(|o| o.id == e))
            .unwrap_or(&all[0])
            .clone();
        self.theme_edit = Some(ThemeEdit {
            start: theme.clone(),
            theme,
            base,
            from,
            selected: None,
            hsva: [0., 0., 0., 1.],
            said: None,
        });
        cx.notify();
    }

    /// Leaves the editor.
    /// Esc in the theme editor: the token being edited lets go, then the editor closes — Settings
    /// stays (the developer, 2026-10-23). Whether it was in the editor.
    pub(super) fn theme_edit_escape(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(edit) = &mut self.theme_edit else {
            return false;
        };
        if edit.selected.take().is_some() {
            if self.field == Some(Field::ThemeValue) {
                self.field = None;
            }
            cx.notify();
        } else {
            self.close_theme_edit(cx);
        }
        true
    }

    pub(super) fn close_theme_edit(&mut self, cx: &mut Context<Self>) {
        self.theme_edit = None;
        if self.field == Some(Field::ThemeValue) {
            self.field = None;
        }
        cx.notify();
    }

    /// The themes the one being edited may extend: any but itself and those built on it.
    fn theme_bases(&self) -> Vec<Theme> {
        let Some(edit) = &self.theme_edit else {
            return Vec::new();
        };
        let all = theme::all();
        let me = edit.theme.id.as_str();
        // Whether `t` is `me` or extends it, however far down.
        let built_on_me = |t: &Theme| {
            let mut at = Some(t);
            for _ in 0..all.len() + 1 {
                let Some(t) = at else {
                    return false;
                };
                if t.id == me {
                    return true;
                }
                at = t
                    .extends
                    .as_deref()
                    .and_then(|e| all.iter().find(|o| o.id == e));
            }
            true
        };
        all.iter().filter(|t| !built_on_me(t)).cloned().collect()
    }

    /// The extends ▾ menu: the themes it may be built on, the current one checked.
    pub(super) fn open_theme_bases(
        &mut self,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        use super::menu::{Act, Entry};
        let current = self
            .theme_edit
            .as_ref()
            .map(|e| e.base.id.clone())
            .unwrap_or_default();
        let (built_in, yours): (Vec<Theme>, Vec<Theme>) =
            self.theme_bases().into_iter().partition(|t| t.built_in);
        let entries = |themes: Vec<Theme>| {
            themes
                .into_iter()
                .map(|t| {
                    Entry::new(t.label.clone(), Act::ThemeBase(t.id.clone()))
                        .checked(t.id == current)
                })
                .collect::<Vec<_>>()
        };
        self.open_titled(
            at,
            vec![
                (Some("Extends"), entries(built_in)),
                (Some("Your themes"), entries(yours)),
            ],
            cx,
        );
    }

    /// The theme being edited is built on `id` now: what it does not set comes from there.
    pub(super) fn set_theme_base(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(base) = theme::find(id) else {
            return;
        };
        let Some(edit) = &mut self.theme_edit else {
            return;
        };
        for i in 0..TOKENS.len() {
            if !edit.theme.own.contains(&i) {
                edit.theme.colors[i] = base.colors[i];
            }
        }
        for r in 0..themes::ROLES {
            if !edit.theme.own_roles.contains(&r) {
                edit.theme.roles[r] = base.roles[r];
            }
        }
        edit.theme.extends = Some(base.id.clone());
        edit.base = base;
        edit.said = None;
        self.show_value();
        cx.notify();
    }

    pub(super) fn pick_token(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(edit) = &mut self.theme_edit else {
            return;
        };
        if edit.selected == Some(i) {
            edit.selected = None;
            self.field = None;
            return cx.notify();
        }
        edit.selected = Some(i);
        self.show_value();
        self.start_field(Field::ThemeValue, cx);
    }

    /// The selected token's value into the hex field and the picker, after it changed elsewhere.
    fn show_value(&mut self) {
        let Some(edit) = &mut self.theme_edit else {
            return;
        };
        if let Some(i) = edit.selected {
            let c = value(&edit.theme, i);
            self.theme_value = themes::hex(c);
            edit.hsva = themes::to_hsva(c);
        }
    }

    /// The picker moved: the token takes its colour, the hex field follows.
    fn pick_color(&mut self, hsva: [f32; 4], cx: &mut Context<Self>) {
        let Some(edit) = &mut self.theme_edit else {
            return;
        };
        let Some(i) = edit.selected else {
            return;
        };
        edit.hsva = hsva;
        let c = themes::from_hsva(hsva);
        self.theme_value = themes::hex(c);
        self.set_token(i, c, cx);
    }

    /// Token `i` of the theme being edited becomes `color`, drawn at once.
    fn set_token(&mut self, i: usize, color: u32, cx: &mut Context<Self>) {
        let Some(edit) = &mut self.theme_edit else {
            return;
        };
        match i.checked_sub(TOKENS.len()) {
            None => {
                edit.theme.colors[i] = color;
                if !edit.theme.own.contains(&i) {
                    edit.theme.own.push(i);
                }
            }
            Some(r) => {
                edit.theme.roles[r] = Some(color);
                if !edit.theme.own_roles.contains(&r) {
                    edit.theme.own_roles.push(r);
                }
            }
        }
        edit.said = None;
        cx.notify();
    }

    /// Row `i` back to what the theme it extends has.
    fn reset_token(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(edit) = &mut self.theme_edit else {
            return;
        };
        match i.checked_sub(TOKENS.len()) {
            None => {
                edit.theme.colors[i] = edit.base.colors[i];
                edit.theme.own.retain(|&o| o != i);
            }
            Some(r) => {
                edit.theme.roles[r] = edit.base.roles[r];
                edit.theme.own_roles.retain(|&o| o != r);
            }
        }
        edit.said = None;
        self.show_value();
        cx.notify();
    }

    /// The hex field changed: a whole colour is applied.
    pub(super) fn theme_value_changed(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.theme_edit.as_ref().and_then(|e| e.selected) else {
            return;
        };
        if let Some(color) = themes::parse_hex(&self.theme_value) {
            if let Some(edit) = &mut self.theme_edit {
                edit.hsva = themes::to_hsva(color);
            }
            self.set_token(i, color, cx);
        }
    }

    /// Every text token under 4.5:1 raised to it.
    pub(super) fn raise_all(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = &self.theme_edit else {
            return;
        };
        let low: Vec<(usize, u32)> = edit
            .theme
            .low_contrast()
            .into_iter()
            .map(|(t, _, _)| (t as usize, themes::raise(&edit.theme, edit.theme.get(t))))
            .collect();
        for (i, color) in low {
            self.set_token(i, color, cx);
        }
        self.show_value();
    }

    /// Writes the theme to its file in the themes folder and draws with it (Save, ⌘S).
    pub(super) fn save_theme(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = &mut self.theme_edit else {
            return;
        };
        let Some(dir) = themes::folder() else {
            return;
        };
        let file = edit
            .theme
            .file
            .clone()
            .unwrap_or_else(|| dir.join(format!("{}.json", edit.theme.id)));
        let json = edit.theme.to_json(false);
        // A snapshot run draws a state; it never writes the user's files.
        let snapshot =
            cfg!(feature = "snapshot") && std::env::var_os("GITLANCE_SNAPSHOT").is_some();
        let result = if snapshot {
            Ok(())
        } else {
            std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, json))
        };
        match result {
            Ok(()) => {
                edit.theme.file = Some(file.clone());
                edit.start = edit.theme.clone();
                edit.from = None;
                edit.said = Some(Ok(format!("Saved {}", super::format::home_relative(&file))));
                let id = edit.theme.id.clone();
                theme::reload();
                // Drawn with where it is offered: a theme of code only is the Code choice.
                let mut choice = theme::choice();
                if edit.theme.window {
                    choice.window = id;
                } else {
                    choice.code = Some(id);
                }
                theme::choose(choice);
            }
            Err(e) => edit.said = Some(Err(format!("Not saved: {e}"))),
        }
        cx.refresh_windows();
        cx.notify();
    }

    fn revert_theme(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = &mut self.theme_edit else {
            return;
        };
        edit.theme = edit.start.clone();
        edit.said = None;
        self.show_value();
        cx.notify();
    }

    fn render_theme_editor(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(edit) = &self.theme_edit else {
            return div().into_any_element();
        };
        let t = &edit.theme;
        let low = t.low_contrast();
        let worst = |i: usize| {
            themes::SURFACES
                .iter()
                .map(|&s| themes::contrast(value(t, i), t.get(s)))
                .fold(f32::MAX, f32::min)
        };
        let mut list: Vec<AnyElement> = Vec::new();
        let mut group = "";
        for i in 0..ROWS {
            let g = TOKENS.get(i).map_or("Code roles", |(_, g, _)| g);
            let (name, what) = row_name(i);
            if g != group {
                group = g;
                list.push(
                    div()
                        .pt(px(12.))
                        .pb(px(4.))
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::muted())
                        .child(g.to_uppercase())
                        .into_any_element(),
                );
            }
            let own = own(t, i);
            let selected = edit.selected == Some(i);
            let ratio = is_text(i).then(|| worst(i));
            list.push(
                div()
                    .id(("theme-token", i))
                    .h(px(30.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(ROW_RADIUS))
                    .text_size(px(12.))
                    .cursor_pointer()
                    // The token being edited is framed in the focus colour, its panel under it.
                    .border_1()
                    .border_color(if selected {
                        theme::focus()
                    } else {
                        gpui::transparent_black().into()
                    })
                    .when(selected, |s| s.bg(theme::selected()))
                    .when(!selected, |s| s.hover(|s| s.bg(theme::hover())))
                    .child(
                        div()
                            .size(px(18.))
                            .flex_none()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(theme::island_border())
                            .bg(rgba(value(t, i))),
                    )
                    .child(
                        div()
                            .w(px(150.))
                            .flex_none()
                            .font_family(theme::CODE_FONT)
                            .text_color(if own { theme::text() } else { theme::faint() })
                            .child(name),
                    )
                    .child(
                        div()
                            .w(px(90.))
                            .flex_none()
                            .font_family(theme::CODE_FONT)
                            .text_color(if own { theme::text() } else { theme::faint() })
                            .child(themes::hex(value(t, i))),
                    )
                    .child(
                        super::hover::cut(("token-what", i), what)
                            .flex_1()
                            .text_color(theme::muted()),
                    )
                    .children(ratio.map(|r| {
                        let ok = r >= 4.5;
                        div()
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(if ok { theme::muted() } else { theme::removed() })
                            .when(!ok, |s| s.font_weight(FontWeight::SEMIBOLD))
                            .child(if ok {
                                format!("✓ {r:.1}:1")
                            } else {
                                format!("{r:.1}:1")
                            })
                    }))
                    .on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| this.pick_token(i, cx)),
                    )
                    .into_any_element(),
            );
            if selected {
                // The token's panel opens under it (#56, 14). Not shrunk: in the scrolling
                // column it was squeezed to nothing, and the token looked as if nothing opened.
                list.push(
                    super::motion::appear(
                        div()
                            .flex_none()
                            .overflow_hidden()
                            .child(self.render_token_open(i, cx)),
                        ("token-open", i),
                        super::motion::BASE,
                        |panel, v| panel.max_h(px(260. * v)).opacity(v),
                    )
                    .into_any_element(),
                );
            }
        }
        let set = t.own.len() + t.own_roles.len();
        let from_label = edit.base.label.clone();
        let file = t
            .file
            .clone()
            .or_else(|| themes::folder().map(|d| d.join(format!("{}.json", t.id))))
            .map(|f| super::format::home_relative(&f))
            .unwrap_or_default();
        let open_json = t.file.clone();
        // The tokens scroll on their own, the preview stays beside them (Design: sticky).
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(t.label.clone()))
                    .child(
                        div()
                            .id("theme-extends")
                            .px(px(8.))
                            .h(px(22.))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .rounded(px(ROW_RADIUS))
                            .bg(theme::hover())
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::selected()))
                            .child(format!("extends {from_label}"))
                            .child(icons::icon("chevron").size(px(12.)).text_color(theme::muted()))
                            .on_click(cx.listener(|this, ev: &ClickEvent, _, cx| {
                                this.open_theme_bases(ev.position(), cx)
                            })),
                    )
                    .child(div().flex_1())
                    .children(open_json.map(|f| {
                        small_button("theme-open-json", "Open JSON")
                            .on_click(move |_: &ClickEvent, _, cx| cx.reveal_path(&f))
                    }))
                    .child(
                        small_button("theme-revert", "Revert")
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.revert_theme(cx))),
                    )
                    .child(
                        div()
                            .id("theme-save")
                            .px(px(12.))
                            .h(px(26.))
                            .flex()
                            .items_center()
                            .rounded(px(ROW_RADIUS))
                            .bg(theme::accent())
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::on_accent())
                            .cursor_pointer()
                            .gap(px(6.))
                            .child("Save")
                            .child(
                                div()
                                    .font_weight(FontWeight::NORMAL)
                                    .child(super::keys::text("⌘S").into_owned()),
                            )
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.save_theme(cx))),
                    )
                    .child(
                        small_button("theme-done", "Done")
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_theme_edit(cx))),
                    ),
            )
            .children(edit.from.as_ref().map(|from| {
                div()
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child(format!(
                        "Made from {from} when you opened it — a built-in theme is never edited; your changes are a theme of their own, saved to the themes folder."
                    ))
            }))
            .children(edit.said.as_ref().map(|said| {
                let (text, bad) = match said {
                    Ok(t) => (t.clone(), false),
                    Err(e) => (e.clone(), true),
                };
                div()
                    .text_size(px(12.))
                    .text_color(if bad { theme::removed() } else { theme::added() })
                    .child(text)
            }))
            .children((!low.is_empty()).then(|| {
                let (tok, ratio, on) = low[0];
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(ROW_RADIUS))
                    .bg(theme::danger_bg())
                    .text_size(px(12.))
                    .child(
                        div().flex_1().min_w_0().child(format!(
                            "{} — {} is {ratio:.1}:1 on {}; text needs 4.5:1.",
                            match low.len() {
                                1 => "1 problem".to_owned(),
                                n => format!("{n} problems"),
                            },
                            TOKENS[tok as usize].0,
                            TOKENS[on as usize].0
                        )),
                    )
                    .child(
                        small_button("theme-raise-all", "Raise to 4.5:1")
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.raise_all(cx))),
                    )
            }))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .gap(px(20.))
                    .child(
                        div()
                            .id("theme-tokens")
                            .flex_1()
                            .min_w(px(340.))
                            .max_w(px(470.))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .children(list),
                    )
                    .child(
                        div()
                            .id("theme-preview")
                            .flex_1()
                            .min_w(px(300.))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(theme::faint())
                                    .child(format!("Preview — the app in {}", t.label)),
                            )
                            .child(theme_preview(t)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .text_size(px(11.))
                    .text_color(theme::faint())
                    .child(format!("{set} set · {} from {from_label}", ROWS - set))
                    .child(div().flex_1())
                    .child(div().font_family(theme::CODE_FONT).child(file)),
            )
            .into_any_element()
    }

    /// Under the selected token: its value to type, its contrast on each surface, the fixes.
    fn render_token_open(&self, i: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(edit) = &self.theme_edit else {
            return div().into_any_element();
        };
        let t = &edit.theme;
        let text = is_text(i);
        let active = self.field == Some(Field::ThemeValue);
        let low = text
            && themes::SURFACES
                .iter()
                .any(|&s| themes::contrast(value(t, i), t.get(s)) < 4.5);
        let base_label = edit.base.label.clone();
        div()
            .ml(px(36.))
            .mb(px(6.))
            .p(px(10.))
            .rounded(px(ROW_RADIUS))
            .border_1()
            .border_color(theme::island_border())
            .bg(theme::base())
            .flex()
            .flex_col()
            .gap(px(8.))
            .text_size(px(12.))
            .child(self.color_picker(edit.hsva, cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("theme-value")
                            .w(px(130.))
                            .h(px(26.))
                            .px_2()
                            .flex()
                            .items_center()
                            .rounded(px(ROW_RADIUS))
                            .border_2()
                            .border_color(if active {
                                theme::focus()
                            } else {
                                theme::island_border()
                            })
                            .font_family(theme::CODE_FONT)
                            .cursor_text()
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.start_field(Field::ThemeValue, cx)
                            }))
                            .child(self.field_look(
                                &self.theme_value,
                                active,
                                "#rrggbb",
                                Default::default(),
                                cx.entity().downgrade(),
                            )),
                    )
                    .child(
                        div()
                            .text_color(theme::faint())
                            .child("#rgb, #rrggbb or #rrggbbaa — applied as you type"),
                    ),
            )
            .children(text.then(|| {
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(14.))
                    .children(themes::SURFACES.iter().map(|&s| {
                        let r = themes::contrast(value(t, i), t.get(s));
                        div()
                            .flex()
                            .gap(px(4.))
                            .text_color(theme::muted())
                            .child(format!("on {}", TOKENS[s as usize].0))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(if r >= 4.5 {
                                        theme::text()
                                    } else {
                                        theme::removed()
                                    })
                                    .child(format!("{r:.1}:1")),
                            )
                    }))
            }))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .children(low.then(|| {
                        small_button("theme-raise", "Raise to 4.5:1").on_click(cx.listener(
                            move |this, _: &ClickEvent, _, cx| {
                                let Some(edit) = &this.theme_edit else {
                                    return;
                                };
                                let c = themes::raise(&edit.theme, value(&edit.theme, i));
                                this.set_token(i, c, cx);
                                this.show_value();
                            },
                        ))
                    }))
                    .child(
                        small_button("theme-base", format!("Use {base_label}'s")).on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.reset_token(i, cx);
                            }),
                        ),
                    ),
            )
            .into_any_element()
    }

    /// The picker over the hex field: a square of the hue, saturation across and value down, and
    /// the hue and the alpha as strips beside it. A press sets it, a drag follows the pointer
    /// even past the edge.
    fn color_picker(&self, [h, s, v, a]: [f32; 4], cx: &mut Context<Self>) -> AnyElement {
        use gpui::{linear_color_stop as stop, linear_gradient};
        let solid = |hsva| rgba(themes::from_hsva(hsva));
        let white = rgba(0xffffffff);
        // A ring or a bar over a strip, where it stands.
        let bar = |y: f32| {
            div()
                .absolute()
                .left(px(-2.))
                .top(px(y * PICK_H - 3.))
                .w(px(STRIP + 4.))
                .h(px(6.))
                .rounded(px(3.))
                .border_2()
                .border_color(white)
                .shadow_sm()
        };
        let square = self
            .picker_area(Pick::Square, SQUARE, cx)
            .rounded(px(4.))
            .bg(solid([h, 1., 1., 1.]))
            .child(
                div()
                    .absolute()
                    .size_full()
                    .rounded(px(4.))
                    .bg(linear_gradient(
                        90.,
                        stop(white, 0.),
                        stop(rgba(0xffffff00), 1.),
                    )),
            )
            .child(
                div()
                    .absolute()
                    .size_full()
                    .rounded(px(4.))
                    .bg(linear_gradient(
                        180.,
                        stop(rgba(0x00000000), 0.),
                        stop(rgba(0x000000ff), 1.),
                    )),
            )
            .child(
                div()
                    .absolute()
                    .left(px(s * SQUARE - 6.))
                    .top(px((1. - v) * PICK_H - 6.))
                    .size(px(12.))
                    .rounded_full()
                    .border_2()
                    .border_color(white)
                    .shadow_sm(),
            );
        let hue = self
            .picker_area(Pick::Hue, STRIP, cx)
            .flex()
            .flex_col()
            .children((0..6).map(|k| {
                let k = k as f32;
                div()
                    .flex_1()
                    .w_full()
                    .when(k == 0., |d| d.rounded_t(px(3.)))
                    .when(k == 5., |d| d.rounded_b(px(3.)))
                    .bg(linear_gradient(
                        180.,
                        stop(solid([k * 60., 1., 1., 1.]), 0.),
                        stop(solid([(k + 1.) * 60., 1., 1., 1.]), 1.),
                    ))
            }))
            .child(bar(h / 360.));
        // Over squares, so what shows through is seen.
        let alpha = self
            .picker_area(Pick::Alpha, STRIP, cx)
            .rounded(px(3.))
            .bg(white)
            .child(
                div()
                    .absolute()
                    .size_full()
                    .rounded(px(3.))
                    .bg(gpui::checkerboard(rgba(0xc8c8c8ff), 6.)),
            )
            .child(
                div()
                    .absolute()
                    .size_full()
                    .rounded(px(3.))
                    .bg(linear_gradient(
                        180.,
                        stop(solid([h, s, v, 1.]), 0.),
                        stop(solid([h, s, v, 0.]), 1.),
                    )),
            )
            .child(bar(1. - a));
        div()
            .flex()
            .gap(px(10.))
            .child(square)
            .child(hue)
            .child(alpha)
            .into_any_element()
    }

    /// One of the picker's areas: a press there, or a drag from it, sets what it holds.
    fn picker_area(&self, which: Pick, w: f32, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        use gpui::{Bounds, DragMoveEvent, MouseButton, MouseDownEvent, Pixels};
        let area: std::rc::Rc<std::cell::Cell<Option<Bounds<Pixels>>>> = Default::default();
        let found = area.clone();
        div()
            .id(("color-pick", which as usize))
            .relative()
            .flex_none()
            .w(px(w))
            .h(px(PICK_H))
            .cursor_crosshair()
            .child(
                gpui::canvas(move |b, _, _| found.set(Some(b)), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    if let Some(b) = area.get() {
                        this.pick_at(which, fraction(b, e.position), cx);
                    }
                }),
            )
            .on_drag(PickDrag(which), |_, _, _, cx| {
                cx.new(|_| super::hover::NoTip)
            })
            .on_drag_move(
                cx.listener(move |this, e: &DragMoveEvent<PickDrag>, _, cx| {
                    if e.drag(cx).0 == which {
                        this.pick_at(which, fraction(e.bounds, e.event.position), cx);
                    }
                }),
            )
    }

    /// The pointer at `(x, y)` (0–1 across and down) in area `which`.
    fn pick_at(&mut self, which: Pick, (x, y): (f32, f32), cx: &mut Context<Self>) {
        let Some(edit) = &self.theme_edit else {
            return;
        };
        let [h, s, v, a] = edit.hsva;
        let hsva = match which {
            Pick::Square => [h, x, 1. - y, a],
            Pick::Hue => [y * 360., s, v, a],
            Pick::Alpha => [h, s, v, 1. - y],
        };
        self.pick_color(hsva, cx);
    }
}

/// The colour picker's square, its height, and its strips' width.
const SQUARE: f32 = 200.;
const PICK_H: f32 = 120.;
const STRIP: f32 = 14.;

/// The picker's areas.
#[derive(Clone, Copy, PartialEq)]
enum Pick {
    Square,
    Hue,
    Alpha,
}

/// A drag in the picker, by the area it started in.
struct PickDrag(Pick);

/// Where `p` is in `b`: 0–1 across and down, held at the edges.
fn fraction(b: gpui::Bounds<gpui::Pixels>, p: gpui::Point<gpui::Pixels>) -> (f32, f32) {
    let f = |p: gpui::Pixels, o: gpui::Pixels, l: gpui::Pixels| {
        if l > px(0.) {
            ((p - o) / l).clamp(0., 1.)
        } else {
            0.
        }
    };
    (
        f(p.x, b.origin.x, b.size.width),
        f(p.y, b.origin.y, b.size.height),
    )
}
