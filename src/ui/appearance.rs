//! Settings → Appearance (Design #12 A + C, `docs/specs/themes.md`, flow 09): the Window themes as
//! cards three to a row — tabs down the left, the code as bars in the theme's syntax colours and
//! diff tints —, the Code themes as syntax samples with "Follow the window" first, several projects,
//! panes, and your themes (the folder, Copy One Dark as JSON, each file's problems). A theme opens in
//! the editor: its tokens by group, contrast per text token with a fix; editing a built-in theme
//! makes a new one that extends it. The whole window is the preview while editing.

use super::lists::Field;
use super::{ROW_RADIUS, Workspace, px, theme};
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
    /// The last save's outcome, said beside Save.
    said: Option<Result<String, String>>,
}

fn c(t: &Theme, tok: T) -> Rgba {
    rgba(t.get(tok))
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

/// A card's frame: the accent around the chosen one, its name and a word under it.
fn card(
    id: (&'static str, usize),
    face: Div,
    name: String,
    tag: Option<String>,
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
                .children(tag.map(|t| div().text_size(px(11.)).text_color(theme::faint()).child(t)))
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

/// A tag for a theme's card: default, light, yours, its problems.
fn tag_of(t: &Theme) -> Option<String> {
    if !t.problems.is_empty() {
        return Some(match t.problems.len() {
            1 => "1 problem".into(),
            n => format!("{n} problems"),
        });
    }
    if t.id == "one-dark" {
        Some("default".into())
    } else if !t.built_in {
        Some("JSON".into())
    } else if t.light() {
        Some("light".into())
    } else {
        None
    }
}

impl Workspace {
    /// The page: Window, Code, several projects, panes, your themes — or the editor.
    pub(super) fn render_appearance(
        &self,
        projects: AnyElement,
        panes: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.theme_edit.is_some() {
            return self.render_theme_editor(cx);
        }
        let all = theme::all();
        let choice = theme::choice();
        let windows: Vec<AnyElement> = all
            .iter()
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
        codes.extend(all.iter().enumerate().map(|(i, t)| {
            let id = t.id.clone();
            card(
                ("code-theme", i),
                code_face(t),
                t.label.clone(),
                None,
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
            .child(projects)
            .child(panes)
            .child(section(
                "Your themes",
                "A .json file in the themes folder shows up above; its colors set any token by name. A theme extends One Dark unless it says another.",
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
                            .child(format!("{} — {}", t.label, tag_of(t).unwrap_or_default())),
                    )
                    .children(t.problems.iter().map(|p| div().child(format!("· {p}"))))
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
        theme::preview_theme(&theme);
        self.theme_edit = Some(ThemeEdit {
            start: theme.clone(),
            theme,
            base,
            from,
            selected: None,
            said: None,
        });
        cx.refresh_windows();
        cx.notify();
    }

    /// Leaves the editor; the chosen themes draw again.
    pub(super) fn close_theme_edit(&mut self, cx: &mut Context<Self>) {
        self.theme_edit = None;
        if self.field == Some(Field::ThemeValue) {
            self.field = None;
        }
        theme::restore();
        cx.refresh_windows();
        cx.notify();
    }

    fn pick_token(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(edit) = &mut self.theme_edit else {
            return;
        };
        if edit.selected == Some(i) {
            edit.selected = None;
            self.field = None;
            return cx.notify();
        }
        edit.selected = Some(i);
        self.theme_value = themes::hex(edit.theme.colors[i]);
        self.start_field(Field::ThemeValue, cx);
    }

    /// Token `i` of the theme being edited becomes `color`, drawn at once.
    fn set_token(&mut self, i: usize, color: u32, cx: &mut Context<Self>) {
        let Some(edit) = &mut self.theme_edit else {
            return;
        };
        edit.theme.colors[i] = color;
        if !edit.theme.own.contains(&i) {
            edit.theme.own.push(i);
        }
        edit.said = None;
        theme::preview_theme(&edit.theme);
        cx.refresh_windows();
        cx.notify();
    }

    /// The hex field changed: a whole colour is applied.
    pub(super) fn theme_value_changed(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.theme_edit.as_ref().and_then(|e| e.selected) else {
            return;
        };
        if let Some(color) = themes::parse_hex(&self.theme_value) {
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
        if let Some(i) = self.theme_edit.as_ref().and_then(|e| e.selected) {
            self.theme_value = self
                .theme_edit
                .as_ref()
                .map(|e| themes::hex(e.theme.colors[i]))
                .unwrap_or_default();
        }
    }

    /// Writes the theme to its file in the themes folder and draws with it.
    fn save_theme(&mut self, cx: &mut Context<Self>) {
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
                let mut choice = theme::choice();
                choice.window = id;
                theme::choose(choice);
                if let Some(edit) = &self.theme_edit {
                    theme::preview_theme(&edit.theme);
                }
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
        if let Some(i) = edit.selected {
            self.theme_value = themes::hex(edit.theme.colors[i]);
        }
        if let Some(edit) = &self.theme_edit {
            theme::preview_theme(&edit.theme);
        }
        cx.refresh_windows();
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
                .map(|&s| themes::contrast(t.colors[i], t.get(s)))
                .fold(f32::MAX, f32::min)
        };
        let is_text = |i: usize| themes::TEXT.iter().any(|&x| x as usize == i);
        let mut list: Vec<AnyElement> = Vec::new();
        let mut group = "";
        for (i, (name, g, what)) in TOKENS.iter().enumerate() {
            if *g != group {
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
            let own = t.own.contains(&i);
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
                    .when(selected, |s| s.bg(theme::selected()))
                    .hover(|s| s.bg(theme::hover()))
                    .child(
                        div()
                            .size(px(18.))
                            .flex_none()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(theme::island_border())
                            .bg(rgba(t.colors[i])),
                    )
                    .child(
                        div()
                            .w(px(150.))
                            .flex_none()
                            .font_family(theme::CODE_FONT)
                            .text_color(if own { theme::text() } else { theme::faint() })
                            .child(*name),
                    )
                    .child(
                        div()
                            .w(px(90.))
                            .flex_none()
                            .font_family(theme::CODE_FONT)
                            .text_color(if own { theme::text() } else { theme::faint() })
                            .child(themes::hex(t.colors[i])),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(theme::muted())
                            .child(*what),
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
                list.push(self.render_token_open(i, cx));
            }
        }
        let set = t.own.len();
        let from_label = edit.base.label.clone();
        let file = t
            .file
            .clone()
            .or_else(|| themes::folder().map(|d| d.join(format!("{}.json", t.id))))
            .map(|f| super::format::home_relative(&f))
            .unwrap_or_default();
        let open_json = t.file.clone();
        div()
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
                            .px(px(8.))
                            .h(px(22.))
                            .flex()
                            .items_center()
                            .rounded(px(ROW_RADIUS))
                            .bg(theme::hover())
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child(format!("extends {from_label}")),
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
                            .child("Save")
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
            .child(div().flex().flex_col().children(list))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .text_size(px(11.))
                    .text_color(theme::faint())
                    .child(format!("{set} set · {} from {from_label}", TOKENS.len() - set))
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
        let text = themes::TEXT.iter().any(|&x| x as usize == i);
        let active = self.field == Some(Field::ThemeValue);
        let low = text
            && themes::SURFACES
                .iter()
                .any(|&s| themes::contrast(t.colors[i], t.get(s)) < 4.5);
        let base = edit.base.colors[i];
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
                        let r = themes::contrast(t.colors[i], t.get(s));
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
                                let c = themes::raise(&edit.theme, edit.theme.colors[i]);
                                this.theme_value = themes::hex(c);
                                this.set_token(i, c, cx);
                            },
                        ))
                    }))
                    .child(
                        small_button("theme-base", format!("Use {base_label}'s")).on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.theme_value = themes::hex(base);
                                this.set_token(i, base, cx);
                                if let Some(edit) = &mut this.theme_edit {
                                    edit.theme.own.retain(|&o| o != i);
                                }
                            }),
                        ),
                    ),
            )
            .into_any_element()
    }
}
