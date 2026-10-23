//! The Settings page: how changed words are marked, which mode diffs open in, and whether the
//! window refreshes itself. Designed in `../Design/mockups/settings/a-general.html`.

use super::diff_view::{self, chip};
use super::px;
use super::{GAP, ISLAND_RADIUS, Workspace, button, island, theme};
use crate::editor::{self, Editor};
use crate::storage::{DiffMode, MarkStyle};
use gpui::{ClickEvent, Context, FontWeight, SharedString, div, prelude::*};

impl Workspace {
    pub(super) fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.find.text = None;
        self.refilter();
        self.settings_open = true;
        cx.notify();
    }

    /// Pastes a token from the clipboard for this repository's GitLab host and tries it.
    fn use_clipboard_token(&mut self, cx: &mut Context<Self>) {
        let Some(remote) = self.web.clone() else {
            return;
        };
        let text = cx
            .read_from_clipboard()
            .and_then(|c| c.text())
            .unwrap_or_default();
        let token = text.trim();
        if token.len() < 8 || token.contains(char::is_whitespace) {
            self.gitlab_check = Some(Err(
                super::words::say("The clipboard does not hold a token — copy one from GitLab → Preferences → Access tokens (scope: api).").into_owned(),
            ));
            return cx.notify();
        }
        match crate::mr::save_token(&remote, token) {
            Ok(()) => {
                self.load_requests(cx);
                self.test_gitlab(cx);
            }
            Err(e) => {
                self.gitlab_check = Some(Err(format!("{e:#}")));
                cx.notify();
            }
        }
    }

    /// Asks GitLab who the token belongs to.
    fn test_gitlab(&mut self, cx: &mut Context<Self>) {
        let Some(remote) = self.web.clone() else {
            return;
        };
        self.gitlab_check = None;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { crate::mr::me(&remote) })
                .await;
            this.update(cx, |this, cx| {
                this.gitlab_check = Some(result.map_err(|e| format!("{e:#}")));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The GitLab block: which host, where the token comes from, a way to give one and to test it.
    fn render_gitlab_settings(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let remote = self.web.as_ref()?;
        let host = crate::mr::host(remote);
        let source = crate::mr::token_source(remote).map(|(_, from)| from);
        let status = match (&self.gitlab_check, source) {
            (Some(Ok(who)), _) => (format!("Connected as {who}"), theme::added()),
            (Some(Err(e)), _) => (e.clone(), theme::removed()),
            (None, Some(from)) => (format!("Token from {from}"), theme::muted()),
            (None, None) => (
                "No token for this host yet — merge requests stay hidden.".to_owned(),
                theme::warning(),
            ),
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap_5()
                .child(heading(if remote.github { "GitHub" } else { "GitLab" }))
                .child(item(
                    "Merge requests",
                    if remote.github {
                        "The host is read from this repository's origin remote — github.com or GitHub Enterprise. A classic token with the repo scope reads and reviews pull requests (private ones too); without one, GitLance uses the GitHub CLI's (gh auth login). It is kept per host in ~/.config/gitlance/tokens, for you only, and sent to curl on its stdin."
                    } else {
                        "The host is read from this repository's origin remote, so any GitLab works — gitlab.com or your own. The token needs the api scope (to comment, reply and resolve); read_api is enough to only read. It is kept per host in ~/.config/gitlance/tokens, for you only, and sent to curl on its stdin."
                    },
                    div()
                        .flex()
                        .flex_col()
                        .items_end()
                        .gap_2()
                        .child(div().text_size(px(12.)).child(host))
                        .child(
                            diff_view::group()
                                .child(
                                    chip("gitlab-paste", "Use token from clipboard", false)
                                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                            this.use_clipboard_token(cx)
                                        })),
                                )
                                .child(chip("gitlab-test", "Test", false).on_click(cx.listener(
                                    |this, _: &ClickEvent, _, cx| this.test_gitlab(cx),
                                ))),
                        )
                        .child(
                            div()
                                .max_w(px(320.))
                                .text_size(px(12.))
                                .text_color(status.1)
                                .child(status.0),
                        ),
                ))
                .into_any_element(),
        )
    }

    fn change_settings(&mut self, change: impl FnOnce(&mut Self), cx: &mut Context<Self>) {
        change(self);
        self.settings.save();
        super::share_settings(&self.settings);
        cx.notify();
    }

    pub(super) fn render_settings(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let s = self.settings;
        let marks = diff_view::group().children(MarkStyle::ALL.map(|style| {
            chip(style_id(style), style.label(), s.mark_style == style).on_click(cx.listener(
                move |this, _: &ClickEvent, _, cx| {
                    this.change_settings(|this| this.settings.mark_style = style, cx)
                },
            ))
        }));
        let modes = diff_view::group().children(DiffMode::ALL.map(|mode| {
            chip(mode_id(mode), mode.label(), s.default_mode == mode).on_click(cx.listener(
                move |this, _: &ClickEvent, _, cx| {
                    this.change_settings(|this| this.settings.default_mode = mode, cx);
                    // The page's choice is what the open diff shows too.
                    this.set_mode(mode, cx);
                },
            ))
        }));
        let installed: Vec<Editor> = Editor::ALL.into_iter().filter(|e| e.installed()).collect();
        let current = editor::pick(s.editor);
        let editors = diff_view::group().children(installed.iter().map(|&e| {
            chip(e.key(), e.label(), current == Some(e)).on_click(cx.listener(
                move |this, _: &ClickEvent, _, cx| {
                    this.change_settings(|this| this.settings.editor = Some(e), cx)
                },
            ))
        }));
        let open_in = diff_view::group().children(crate::storage::OpenIn::ALL.map(|o| {
            chip(o.label(), o.label(), s.open_in == o).on_click(cx.listener(
                move |this, _: &ClickEvent, _, cx| {
                    this.change_settings(|this| this.settings.open_in = o, cx)
                },
            ))
        }));
        let projects = diff_view::group().children(crate::storage::Projects::ALL.map(|p| {
            chip(p.label(), p.label(), super::projects() == p).on_click(cx.listener(
                move |this, _: &ClickEvent, _, cx| {
                    this.change_settings(|this| this.settings.projects = p, cx)
                },
            ))
        }));
        let panes = diff_view::group()
            .child(
                chip("panes-islands", "Islands", !super::flat()).on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        this.change_settings(|this| this.settings.flat = false, cx)
                    },
                )),
            )
            .child(
                chip("panes-flat", "Flat", super::flat()).on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        this.change_settings(|this| this.settings.flat = true, cx)
                    },
                )),
            );
        let animations = switch("animations", s.animations).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                this.change_settings(
                    |this| this.settings.animations = !this.settings.animations,
                    cx,
                );
                super::motion::set(this.settings.animations, cx);
            },
        ));
        let refresh = switch("refresh", s.auto_refresh).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                this.change_settings(
                    |this| this.settings.auto_refresh = !this.settings.auto_refresh,
                    cx,
                )
            },
        ));

        let ui_zoom = diff_view::group()
            .child(chip("ui-zoom-out", "−", false).on_click(
                cx.listener(|this, _: &ClickEvent, window, cx| this.ui_zoom_by(-1, window, cx)),
            ))
            .child(
                chip("ui-zoom-reset", format!("{}%", s.ui_zoom), s.ui_zoom != 100).on_click(
                    cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.set_ui_zoom(100, window, cx)
                    }),
                ),
            )
            .child(chip("ui-zoom-in", "+", false).on_click(
                cx.listener(|this, _: &ClickEvent, window, cx| this.ui_zoom_by(1, window, cx)),
            ));
        let zoom = diff_view::group()
            .child(
                chip("zoom-out", "−", false)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.zoom_by(-1, cx))),
            )
            .child(
                chip("zoom-reset", format!("{}%", s.zoom), s.zoom != 100)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.set_zoom(100, cx))),
            )
            .child(
                chip("zoom-in", "+", false)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.zoom_by(1, cx))),
            );
        let fold = switch("fold-generated", s.fold_generated).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                this.change_settings(
                    |this| {
                        this.settings.fold_generated = !this.settings.fold_generated;
                        this.unfolded.clear();
                    },
                    cx,
                );
                this.refresh_rows();
                this.build_stack(cx);
            },
        ));
        let scrolled = switch("auto-reviewed", s.auto_reviewed).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                this.change_settings(
                    |this| this.settings.auto_reviewed = !this.settings.auto_reviewed,
                    cx,
                );
            },
        ));

        let rich = switch("rich-comments", s.rich_comments)
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_rich_comments(cx)));

        let fetch = switch("fetch", self.fetch.enabled).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                let on = !this.fetch.enabled;
                this.set_fetch_enabled(on, cx)
            },
        ));

        let gitlab = self.render_gitlab_settings(cx);
        let section = self.settings_section;
        // The theme editor takes the page's height: its tokens scroll beside a preview that stays.
        let editing = section == Section::Appearance && self.theme_edit.is_some();
        let column = || div().flex().flex_col().gap_5();
        let content: gpui::AnyElement = match section {
            Section::Diff => column()
                .child(item(
                    "Changed words",
                    "How the words that changed inside a changed line are marked, in Words and Structural modes.",
                    marks,
                ))
                .child(
                    div()
                        .rounded(px(ISLAND_RADIUS))
                        .overflow_hidden()
                        .bg(theme::editor())
                        .border_1()
                        .border_color(theme::island_border())
                        .child(diff_view::mark_preview(s.mark_style)),
                )
                .child(item(
                    "Default mode",
                    "What a diff opens in. Lines, Words and Structural are also in the toolbar and the View menu.",
                    modes,
                ))
                .child(item(
                    "Interface size",
                    "How big everything in the window is, 60 % to 250 %. ⌘+ and ⌘− change it anywhere, ⌘0 puts it back; a click on the percentage does the same.",
                    ui_zoom,
                ))
                .child(item(
                    "Code size",
                    "The code in a diff, as a share of the interface size, 60 % to 250 %. It follows the interface size too, so this is only for making code larger or smaller than the rest.",
                    zoom,
                ))
                .child(item(
                    "Fold generated files",
                    "Lock files, minified and generated code (Cargo.lock, package-lock.json, *.min.js, *.pb.go …) start folded when they change by more than 40 lines. They stay in the file list, tagged “generated”; one click shows them.",
                    fold,
                ))
                .child(item(
                    "Mark reviewed as I scroll past",
                    "With all files in one scroll (⌥A switches to one file at a time), a file whose last line scrolls off the top while you scroll down is marked reviewed. Off: only V, ⇧V and the box mark a file — a file that flew by under a fast scroll was not read.",
                    scrolled,
                ))
                .child(item(
                    "Format comments as markdown",
                    "Draws headings, lists, code, tables and links in comments — GitLab discussions are written in markdown. Off shows the text as written; ⌥M or Raw / Rich beside a comment switches it anywhere.",
                    rich,
                ))
                .into_any_element(),
            Section::Appearance => self.render_appearance(
                item(
                    "Several projects",
                    "How the open repositories are shown: a rail of their marks on the left (⌘1…9 in its order, a right-click moves or closes one, a drag reorders), tabs in the title bar, or listed in the menu with the spaces nested under the one open.",
                    projects,
                )
                .into_any_element(),
                item(
                    "Panes",
                    "Islands: rounded panes on the window's ground with gaps between them. Flat: square panes side by side, split by hairlines.",
                    panes,
                )
                .into_any_element(),
                item(
                    "Animations",
                    "Panels, menus and folds move instead of jumping, in 120 or 200 ms; nothing waits for them. Off, or with Reduce motion on in System Settings → Accessibility → Display, everything appears at once.",
                    animations,
                )
                .into_any_element(),
                cx,
            ),
            Section::General => column()
                .child(item(
                    "Open files in",
                    "The editor behind “Open in …” on a file or a line. It opens the file in the working tree.",
                    if installed.is_empty() {
                        div()
                            .text_size(px(12.))
                            .text_color(theme::faint())
                            .child("Android Studio, Zed or VS Code not found")
                            .into_any_element()
                    } else {
                        editors.into_any_element()
                    },
                ))
                .child(item(
                    "Open a repository in",
                    "Where a repository goes when another one is open: ask each time, in place of the open one, or in a new tab beside it.",
                    open_in,
                ))
                .into_any_element(),
            Section::Repository => column()
                .child(item(
                    "Refresh when the repository changes",
                    "Re-reads refs and history when something outside the app commits, amends or rebases. Never writes to the repository.",
                    refresh,
                ))
                .child(item(
                    "Fetch in the background",
                    "Runs git fetch --all --prune for this repository every 5 minutes and when the window comes back to the front, so other people’s force pushes show up as versions. It writes remote-tracking refs and objects — the one exception to the read-only rule — and uses your own git, SSH agent and VPN. Off until you turn it on, per repository.",
                    fetch,
                ))
                .into_any_element(),
            Section::Requests => gitlab.unwrap_or_else(|| {
                div()
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child("This repository has no GitLab or GitHub remote, so there are no merge requests to set up.")
                    .into_any_element()
            }),
            Section::Agents => self.render_agents_settings(cx).into_any_element(),
            Section::Keyboard => self.render_keyboard_settings(cx).into_any_element(),
        };
        let nav = div()
            .w(px(200.))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(2.))
            .p(px(12.))
            .border_r_1()
            .border_color(theme::island_border())
            .child(
                div()
                    .px(px(10.))
                    .pt(px(8.))
                    .pb(px(12.))
                    .text_size(px(17.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Settings"),
            )
            .children(Section::ALL.map(|sec| {
                div()
                    .id(sec.label())
                    .h(px(30.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(super::ROW_RADIUS))
                    .cursor_pointer()
                    .when(sec == section, |s| {
                        s.bg(theme::selected()).text_color(theme::text())
                    })
                    .when(sec != section, |s| s.text_color(theme::muted()))
                    .hover(|s| s.bg(theme::hover()))
                    .child(super::words::say(sec.label()).into_owned())
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.settings_section = sec;
                        cx.notify();
                    }))
            }));
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .px(px(GAP))
            .pb(px(GAP))
            .child(
                island()
                    .flex_1()
                    .flex_row()
                    .bg(theme::editor())
                    .child(nav)
                    .child(
                        div()
                            .id("settings")
                            .flex_1()
                            .min_w_0()
                            .when(!editing, |s| s.overflow_y_scroll())
                            .when(editing, |s| s.flex().flex_col())
                            .child(
                                div()
                                    .w_full()
                                    .max_w(px(if editing { 1180. } else { 760. }))
                                    .when(editing, |s| s.flex_1().min_h_0())
                                    .mx_auto()
                                    .flex()
                                    .flex_col()
                                    .gap_5()
                                    .p(px(24.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .child(div().flex_1().child(heading(
                                                super::words::say(section.label()).into_owned(),
                                            )))
                                            .child(
                                                button("settings-done", "Done", "esc").on_click(
                                                    cx.listener(|this, _: &ClickEvent, _, cx| {
                                                        this.settings_open = false;
                                                        cx.notify();
                                                    }),
                                                ),
                                            ),
                                    )
                                    // Another section fades in (#56, 14).
                                    .child(super::motion::appear(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .when(editing, |s| s.flex_1().min_h_0())
                                            .child(content),
                                        ("settings-section", section as usize),
                                        super::motion::FAST,
                                        |content, v| content.opacity(v),
                                    )),
                            ),
                    ),
            )
    }
}

/// The parts of Settings, in its left column.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Section {
    #[default]
    Diff,
    Appearance,
    General,
    Repository,
    Requests,
    Agents,
    Keyboard,
}

impl Section {
    const ALL: [Section; 7] = [
        Section::Diff,
        Section::Appearance,
        Section::General,
        Section::Repository,
        Section::Requests,
        Section::Agents,
        Section::Keyboard,
    ];

    fn label(self) -> &'static str {
        match self {
            Section::Diff => "Diff",
            Section::Appearance => "Appearance",
            Section::General => "General",
            Section::Repository => "Repository",
            Section::Requests => "Merge requests",
            Section::Agents => "Agents",
            Section::Keyboard => "Keyboard",
        }
    }
}

fn style_id(style: MarkStyle) -> &'static str {
    match style {
        MarkStyle::Tinted => "mark-tinted",
        MarkStyle::Underlined => "mark-underlined",
    }
}

fn mode_id(mode: DiffMode) -> &'static str {
    match mode {
        DiffMode::Lines => "default-lines",
        DiffMode::Words => "default-words",
        DiffMode::Structural => "default-structural",
    }
}

fn heading(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .text_size(px(15.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text.into())
}

/// A labelled row: the name and what it does on the left, the control on the right.
pub(super) fn item(
    title: &'static str,
    detail: &'static str,
    control: impl IntoElement,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_4()
        .py(px(10.))
        .border_b_1()
        .border_color(theme::island_border())
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(super::keys::text(title).into_owned())
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child(super::keys::text(detail).into_owned()),
                ),
        )
        .child(div().flex_none().child(control))
}

fn switch(id: &'static str, on: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .w(px(34.))
        .h(px(20.))
        .rounded_full()
        .flex()
        .items_center()
        .px(px(2.))
        .cursor_pointer()
        .bg(if on { theme::accent() } else { theme::hover() })
        .when(on, |s| s.justify_end())
        .child(div().size(px(16.)).rounded_full().bg(if on {
            theme::base()
        } else {
            theme::muted()
        }))
}

impl Workspace {
    /// Settings → Keyboard: every action with its keys, filtered by name or key. Change waits for
    /// the next key, Reset takes the shipped ones back; a changed action has a dot, and a key two
    /// actions share is outlined in red, the other one named in its tooltip.
    fn render_keyboard_settings(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        use super::discussion::{Kind, button};
        use super::keymap::{self, Keymap};
        use super::keys::{self, On};
        let query = self.keys_filter.trim().to_lowercase();
        let changed_only = self.keys_changed_only;
        let map = cx.global::<Keymap>();
        let (preset, changed_count) = (map.preset(), map.changed_count());
        let entries: Vec<_> = map
            .entries()
            .into_iter()
            .filter(|e| matches_keys(e, &query, self.keys_exact))
            .filter(|e| !changed_only || e.changed)
            .collect();
        let presets = div().flex().flex_wrap().gap(px(6.)).pb(px(12.)).children(
            keymap::Preset::ALL.into_iter().enumerate().map(|(i, p)| {
                let on = p == preset;
                div()
                    .id(("key-preset", i))
                    .px(px(12.))
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .rounded_full()
                    .border_1()
                    .border_color(if on {
                        theme::accent()
                    } else {
                        theme::island_border()
                    })
                    .when(on, |s| s.bg(theme::info_bg()))
                    .text_size(px(12.))
                    .text_color(if on { theme::text() } else { theme::muted() })
                    .cursor_pointer()
                    .child(if on && changed_count > 0 {
                        format!("{} · {changed_count} changed", p.label())
                    } else {
                        p.label().to_owned()
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        keymap::set_preset(p, cx);
                        this.refresh_menus(cx);
                        cx.notify();
                    }))
            }),
        );
        let by_keys = self.recording == Some(BY_KEYS);
        let tools = div()
            .flex()
            .gap(px(6.))
            .pb(px(8.))
            // The next keys pressed filter the list (Design D, X55).
            .child(
                button(
                    "keys-by-keys",
                    if by_keys {
                        "Press the keys…"
                    } else {
                        "By keys"
                    },
                    None,
                    if by_keys {
                        Kind::Primary(theme::accent())
                    } else {
                        Kind::Plain
                    },
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.recording = if by_keys { None } else { Some(BY_KEYS) };
                    cx.notify();
                })),
            )
            .child(
                button(
                    "keys-changed-only",
                    "Changed only",
                    None,
                    if changed_only {
                        Kind::Primary(theme::accent())
                    } else {
                        Kind::Plain
                    },
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.keys_changed_only = !this.keys_changed_only;
                    cx.notify();
                })),
            )
            .child(div().flex_1())
            .child(
                button("keys-export", "Export", None, Kind::Plain).on_click(cx.listener(
                    |_, _: &ClickEvent, _, cx| {
                        let text = cx.global::<Keymap>().export();
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                    },
                )),
            )
            .children((changed_count > 0).then(|| {
                button(
                    "keys-reset-all",
                    format!("Reset to {}", preset.label()),
                    None,
                    Kind::Plain,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    keymap::reset(cx);
                    this.refresh_menus(cx);
                    cx.notify();
                }))
            }));
        let count = entries.len();
        let rows = entries.into_iter().enumerate().map(|(ix, e)| {
            let name = e.name;
            let was = e
                .was
                .iter()
                .map(|k| {
                    // A letter key as the caps write it: ⇧⌘O.
                    let mut s = keymap::symbols(k);
                    let last = s.chars().last().filter(char::is_ascii_lowercase);
                    let alone = s.chars().rev().nth(1).is_none_or(|c| !c.is_alphanumeric());
                    if let (Some(c), true) = (last, alone) {
                        s.pop();
                        s.push(c.to_ascii_uppercase());
                    }
                    s
                })
                .collect::<Vec<_>>()
                .join(" · ");
            let waiting = self.recording == Some(name);
            let clash = e.clash.clone();
            let keys = e.keys.iter().map(|k| {
                let mine = clash
                    .as_ref()
                    .filter(|(key, _)| key == k)
                    .map(|(_, other)| {
                        gpui::SharedString::from(format!("Also {other} — the one you set wins"))
                    });
                div()
                    .id(gpui::SharedString::from(format!("key-{name}-{k}")))
                    .p(px(2.))
                    .rounded(px(5.))
                    .when(mine.is_some(), |s| {
                        s.border_1().border_color(theme::removed())
                    })
                    .child(keys::caps(&keymap::symbols(k), On::Plain))
                    .when_some(mine, |s, tip| {
                        s.tooltip_show_delay(crate::ui::hover::LONG)
                            .tooltip(move |_, cx| cx.new(|_| super::TipOwned(tip.clone())).into())
                    })
            });
            div()
                .flex()
                .items_center()
                .gap_3()
                .min_h(px(44.))
                .border_b_1()
                .border_color(theme::island_border())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_none()
                                .size(px(6.))
                                .rounded_full()
                                .when(e.changed, |s| s.bg(theme::accent())),
                        )
                        .child(div().truncate().child(e.label)),
                )
                .child(
                    div()
                        .flex_none()
                        .w(px(200.))
                        .flex()
                        .justify_end()
                        .items_center()
                        .gap(px(6.))
                        .when(waiting, |s| {
                            s.child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme::accent())
                                    .child("Press a key…"),
                            )
                        })
                        .when(!waiting && e.changed && !was.is_empty(), |s| {
                            s.child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(theme::faint())
                                    .line_through()
                                    .child(keys::text(&was).into_owned()),
                            )
                        })
                        .when(!waiting, |s| s.children(keys))
                        .when(!waiting && e.keys.is_empty(), |s| {
                            s.child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme::faint())
                                    .child("no key"),
                            )
                        }),
                )
                .child(
                    div()
                        .flex_none()
                        .w(px(168.))
                        .flex()
                        .justify_end()
                        .gap(px(6.))
                        .children(e.changed.then(|| {
                            button(("key-reset", ix), "Reset", None, Kind::Quiet).on_click(
                                cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    keymap::set(name, None, cx);
                                    this.refresh_menus(cx);
                                    cx.notify();
                                }),
                            )
                        }))
                        .child(
                            button(
                                ("key-change", ix),
                                if waiting { "Cancel" } else { "Change" },
                                None,
                                Kind::Plain,
                            )
                            .on_click(cx.listener(
                                move |this, _: &ClickEvent, _, cx| {
                                    this.recording = if waiting { None } else { Some(name) };
                                    cx.notify();
                                },
                            )),
                        ),
                )
        });
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .pb(px(12.))
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child("Start from a preset — GitLance’s keys, Android Studio’s or VS Code’s — and change any key on top: Change gives an action one new key in place of its keys; esc cancels, ⌫ leaves it without one. A changed key shows what it was; the changes are kept in keymap.txt in GitLance’s data folder."),
            )
            .child(presets)
            .child(tools)
            .child(
                div()
                    .pb(px(8.))
                    .child(self.render_field(
                        "keys-filter",
                        super::lists::Field::Keys,
                        if crate::platform::MAC {
                            "Filter by name or key — mark, ⌘B, cmd-b"
                        } else {
                            "Filter by name or key — mark, Ctrl+B, ctrl-b"
                        },
                        cx,
                    )),
            )
            .children(rows)
            .children((count == 0).then(|| {
                div()
                    .py(px(12.))
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child("No action by that name or key.")
            }))
    }

    /// The key pressed while Settings → Keyboard waits for one.
    pub(super) fn record_key(&mut self, event: &gpui::KeyDownEvent, cx: &mut Context<Self>) {
        let Some(name) = self.recording.take() else {
            return;
        };
        let key = &event.keystroke;
        let plain = !(key.modifiers.platform
            || key.modifiers.alt
            || key.modifiers.control
            || key.modifiers.shift);
        // By keys: what was pressed becomes the filter, as the app writes keys.
        if name == BY_KEYS {
            if !(plain && key.key == "escape") {
                // A letter key as the caps write it: ⌘B.
                let mut spec = super::keymap::symbols(&key.unparse());
                if spec.chars().last().is_some_and(|c| c.is_ascii_lowercase())
                    && spec
                        .chars()
                        .rev()
                        .nth(1)
                        .is_none_or(|c| !c.is_alphanumeric())
                {
                    let last = spec.pop().map(|c| c.to_ascii_uppercase());
                    spec.extend(last);
                }
                self.keys_filter = super::keys::text(&spec).into_owned();
                self.keys_exact = true;
            }
            return cx.notify();
        }
        match key.key.as_str() {
            "escape" if plain => {}
            "backspace" if plain => super::keymap::set(name, Some(Vec::new()), cx),
            _ => super::keymap::set(name, Some(vec![key.unparse()]), cx),
        }
        self.refresh_menus(cx);
        cx.notify();
    }

    /// Rebuilds the menu bar once the current event is over: macOS matches a menu key equivalent
    /// against the new menus at once, and the action it fires must not find the workspace still
    /// being updated (a crash when a recorded key was also a new menu shortcut).
    pub(super) fn refresh_menus(&self, cx: &mut Context<Self>) {
        let menus = super::menus(&self.options);
        cx.spawn(async move |_, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(50))
                .await;
            cx.update(|cx| cx.set_menus(menus));
        })
        .detach();
    }
}

/// What `Workspace::recording` holds while By keys waits for the keys to filter by.
pub(super) const BY_KEYS: &str = "\u{0}by keys";

/// Whether a keyboard row answers `query` (lower case): every word in its name, or a key written
/// as the app writes it (`⌘B`) or as GPUI does (`cmd-b`).
fn matches_keys(e: &super::keymap::Entry, query: &str, exact: bool) -> bool {
    if query.is_empty() {
        return true;
    }
    let written = |k: &String| super::keys::text(&super::keymap::symbols(k)).to_lowercase();
    if exact {
        return e.keys.iter().any(|k| written(k) == query);
    }
    let label = e.label.to_lowercase();
    query.split_whitespace().all(|w| label.contains(w))
        || e.keys
            .iter()
            .any(|k| k.contains(query) || written(k).contains(query))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_pressed_matches_only_that_key() {
        let entry = |keys: &[&str]| super::super::keymap::Entry {
            name: "X",
            label: "Switch branch".into(),
            keys: keys.iter().map(|k| (*k).to_owned()).collect(),
            changed: false,
            was: Vec::new(),
            clash: None,
        };
        let pressed = super::super::keys::text("⌘B").to_lowercase();
        assert!(matches_keys(&entry(&["cmd-b"]), &pressed, true));
        assert!(!matches_keys(&entry(&["cmd-shift-b"]), &pressed, true));
        // Typed, it is a part of a key or of the name: ⇧ on a Mac, shift on Windows and Linux.
        let typed = super::super::keys::text("⇧").to_lowercase();
        assert!(matches_keys(&entry(&["cmd-shift-b"]), &typed, false));
        assert!(!matches_keys(&entry(&["cmd-b"]), &typed, false));
        assert!(matches_keys(&entry(&[]), "branch", false));
    }
}
