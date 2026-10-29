//! Set up GitLance (#143 B): on the first start, a page of tabs — Agents, Keys, Look — whose title,
//! tabs and foot stay put while only the column between them scrolls. Each part is its Settings
//! section at its shortest, drawn by the same code (`key_presets`, `window_cards`, `panes_chips`,
//! `font_rows`, the connect panel's `way_row`): what is set here is set there. Later and Done bring
//! back what was under it — the welcome, with no repository open. A part is offered once
//! (`setup.txt`); a release that adds one offers that part alone; Help → Set Up GitLance… opens
//! every part again.

use super::discussion::{self, Kind};
use super::keymap::Keymap;
use super::keys::{self, On};
use super::menu::{Act, Entry};
use super::settings::item;
use super::{ROW_RADIUS, Workspace, icons, px, theme};
use crate::storage;
use gpui::{AnyElement, ClickEvent, Context, FontWeight, Pixels, Point, div, prelude::*};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Agents,
    Keys,
    Look,
}

impl Part {
    pub const ALL: [Part; 3] = [Part::Agents, Part::Keys, Part::Look];

    fn name(self) -> &'static str {
        match self {
            Part::Agents => "Agents",
            Part::Keys => "Keys",
            Part::Look => "Look",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Part::Agents => "agents",
            Part::Keys => "keys",
            Part::Look => "look",
        }
    }
}

/// The keys that matter, by group, as the Keys part lists them.
const GROUPS: [(&str, &[&str]); 4] = [
    (
        "Move",
        &["NextFile", "PreviousFile", "NextChange", "SearchFiles"],
    ),
    ("Find", &["SearchEverywhere", "FindInFiles", "OpenRecent"]),
    (
        "Review",
        &["CommentLine", "MarkReviewed", "SendToAgent", "SubmitReview"],
    ),
    ("Window", &["FocusDiff", "ToggleAgents", "ShowShortcuts"]),
];

/// The page while it is open.
pub struct Setup {
    pub parts: Vec<Part>,
    pub at: usize,
    /// Each Claude Code profile (`~/.claude`) and whether GitLance is in it; whether Codex has it —
    /// looked at as the page opens, a tab is shown and a connect ends.
    profiles: Vec<(String, bool)>,
    codex: bool,
}

impl Setup {
    fn look(&mut self) {
        self.profiles = crate::agents::claude_profiles()
            .iter()
            .map(|p| {
                let name = p
                    .file_name()
                    .map(|n| format!("~/{}", n.to_string_lossy()))
                    .unwrap_or_default();
                (name, crate::agents::claude_connected(p))
            })
            .collect();
        self.codex = crate::codex::connected();
    }

    fn part(&self) -> Part {
        self.parts[self.at.min(self.parts.len() - 1)]
    }
}

/// The parts in `text` (`setup.txt`: a part's key a line).
fn parse_seen(text: &str) -> Vec<Part> {
    text.lines()
        .filter_map(|l| Part::ALL.into_iter().find(|p| p.key() == l.trim()))
        .collect()
}

/// The parts to offer: those not seen — none when GitLance was used before Set up came (its
/// settings kept, no `setup.txt`): what the parts set is in Settings already.
fn unseen(seen: &str, used_before: bool) -> Vec<Part> {
    if seen.is_empty() && used_before {
        return Vec::new();
    }
    let seen = parse_seen(seen);
    Part::ALL
        .into_iter()
        .filter(|p| !seen.contains(p))
        .collect()
}

fn keep_seen(parts: &[Part]) {
    let seen = parse_seen(&storage::read("setup.txt"));
    let text: String = Part::ALL
        .into_iter()
        .filter(|p| seen.contains(p) || parts.contains(p))
        .map(|p| format!("{}\n", p.key()))
        .collect();
    storage::save("setup.txt", text);
}

/// What the first window offers as it opens; a snapshot draws Set up only when asked.
pub(super) fn due() -> Vec<Part> {
    if cfg!(feature = "snapshot") && std::env::var_os("GITLANCE_SNAPSHOT").is_some() {
        return Vec::new();
    }
    let seen = storage::read("setup.txt");
    let parts = unseen(&seen, !storage::read("settings.txt").is_empty());
    if seen.is_empty() && parts.is_empty() {
        keep_seen(&Part::ALL);
    }
    parts
}

/// The page's column: the top, the part and the foot line up in it.
fn column() -> gpui::Div {
    div().w_full().max_w(px(760.)).mx_auto().px(px(24.))
}

impl Workspace {
    /// Opens the page on `parts` (Help → Set Up GitLance…: all of them); they are offered now.
    pub(super) fn open_setup(&mut self, parts: Vec<Part>, cx: &mut Context<Self>) {
        if parts.is_empty() {
            return;
        }
        keep_seen(&parts);
        let mut setup = Setup {
            parts,
            at: 0,
            profiles: Vec::new(),
            codex: false,
        };
        setup.look();
        self.setup = Some(setup);
        self.settings_open = false;
        cx.notify();
    }

    /// Later, Done and esc: what was under it comes back. Whether it was open.
    pub(super) fn close_setup(&mut self, cx: &mut Context<Self>) -> bool {
        let open = self.setup.take().is_some();
        if open {
            cx.notify();
        }
        open
    }

    fn setup_go(&mut self, at: usize, cx: &mut Context<Self>) {
        if let Some(s) = &mut self.setup {
            s.at = at.min(s.parts.len() - 1);
            s.look();
            cx.notify();
        }
    }

    /// ↵: the next part, or Done on the last.
    pub(super) fn setup_next(&mut self, cx: &mut Context<Self>) {
        let Some(s) = &self.setup else {
            return;
        };
        if s.at + 1 < s.parts.len() {
            self.setup_go(s.at + 1, cx);
        } else {
            self.close_setup(cx);
        }
    }

    /// A connect ended: whether the profiles have GitLance now.
    pub(super) fn setup_look_again(&mut self) {
        if let Some(s) = &mut self.setup {
            s.look();
        }
    }

    /// The part's state as its Settings section says it: an agent connected, a keymap chosen, a
    /// look other than the first.
    fn part_done(&self, part: Part, setup: &Setup, cx: &Context<Self>) -> bool {
        match part {
            Part::Agents => setup.codex || setup.profiles.iter().any(|(_, on)| *on),
            Part::Keys => {
                let map = cx.global::<Keymap>();
                map.preset() != super::keymap::Preset::GitLance || map.changed_count() > 0
            }
            Part::Look => {
                let choice = theme::choice();
                choice.window != "one-dark"
                    || choice.code.is_some()
                    || theme::faces() != theme::Faces::default()
                    || self.settings.ui_zoom != 100
                    || self.settings.zoom != 100
                    || self.settings.flat
            }
        }
    }

    /// The page: its title and tabs at the top, the part in the column between, the count and
    /// Back · Skip · Next at the foot.
    pub(super) fn render_setup(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(s) = &self.setup else {
            return div().into_any_element();
        };
        let part = s.part();
        let last = s.at + 1 == s.parts.len();
        let tabs = s
            .parts
            .iter()
            .enumerate()
            .fold(super::diff_view::group(), |group, (i, &p)| {
                let done = self.part_done(p, s, cx);
                group.child(
                    super::diff_view::chip(("setup-tab", i), p.name(), i == s.at)
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .children(done.then(|| {
                            icons::icon("check")
                                .size(px(12.))
                                .text_color(theme::added())
                        }))
                        .on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| this.setup_go(i, cx)),
                        ),
                )
            });
        let body = match part {
            Part::Agents => self.render_setup_agents(s, cx),
            Part::Keys => self.render_setup_keys(cx),
            Part::Look => self.render_setup_look(cx),
        };
        let back = (s.at > 0).then(|| {
            let to = s.at - 1;
            discussion::button("setup-back", "Back", None, Kind::Plain)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.setup_go(to, cx)))
        });
        let next = s.at + 1;
        let forward: Vec<AnyElement> = if last {
            vec![
                discussion::button(
                    "setup-done",
                    "Done",
                    Some("↵"),
                    Kind::Primary(theme::accent()),
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.close_setup(cx);
                }))
                .into_any_element(),
            ]
        } else {
            vec![
                discussion::button("setup-skip", "Skip", None, Kind::Plain)
                    .on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| this.setup_go(next, cx)),
                    )
                    .into_any_element(),
                discussion::button(
                    "setup-next",
                    "Next",
                    Some("↵"),
                    Kind::Primary(theme::accent()),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.setup_go(next, cx)))
                .into_any_element(),
            ]
        };
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .m(px(super::GAP))
            .mt_0()
            .rounded(px(super::ISLAND_RADIUS))
            .bg(theme::editor())
            .child(
                div().flex_none().pt(px(24.)).child(
                    column()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .mb(px(12.))
                                .child(
                                    div()
                                        .flex_1()
                                        .text_size(px(18.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("Set up GitLance"),
                                )
                                .child(
                                    discussion::button(
                                        "setup-later",
                                        "Later",
                                        Some("esc"),
                                        Kind::Plain,
                                    )
                                    .on_click(cx.listener(
                                        |this, _: &ClickEvent, _, cx| {
                                            this.close_setup(cx);
                                        },
                                    )),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .pb(px(10.))
                                .border_b_1()
                                .border_color(theme::island_border())
                                .child(tabs),
                        ),
                ),
            )
            .child(
                div()
                    .id("setup-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(column().pt(px(16.)).pb(px(24.)).child(body)),
            )
            .child(
                div()
                    .flex_none()
                    .py(px(12.))
                    .border_t_1()
                    .border_color(theme::island_border())
                    .child(
                        column()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(12.))
                                    .text_color(theme::faint())
                                    .child(format!("{} of {}", s.at + 1, s.parts.len())),
                            )
                            .children(back)
                            .children(forward),
                    ),
            )
            .into_any_element()
    }

    /// Agents: Claude Code with its profiles, Codex — the connect panel's rows.
    fn render_setup_agents(&self, s: &Setup, cx: &mut Context<Self>) -> AnyElement {
        let server = crate::agents::server_command();
        let codex_cmd = format!("codex mcp add gitlance -- {server}");
        let all_in = !s.profiles.is_empty() && s.profiles.iter().all(|(_, on)| *on);
        let what = if s.profiles.is_empty() {
            "No Claude Code profile found (~/.claude)".to_owned()
        } else {
            s.profiles
                .iter()
                .map(|(name, on)| format!("{name} {}", if *on { "connected" } else { "not yet" }))
                .collect::<Vec<_>>()
                .join(" · ")
        };
        let claude_end: Option<AnyElement> = if all_in {
            Some(super::tag("Connected", theme::added()).into_any_element())
        } else if s.profiles.is_empty() {
            None
        } else {
            Some(
                discussion::button(
                    "setup-connect-claude",
                    match (self.agent_setup_busy, s.profiles.len()) {
                        (true, _) => "Connecting…",
                        (false, 1) => "Connect",
                        (false, _) => "Connect all",
                    },
                    None,
                    Kind::Primary(theme::accent()),
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.connect_claude(cx)))
                .into_any_element(),
            )
        };
        let codex_end: AnyElement = if s.codex {
            super::tag("Ready", theme::added()).into_any_element()
        } else {
            discussion::button("setup-codex-copy", "Copy the command", None, Kind::Plain)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(codex_cmd.clone()));
                    this.toast_info("The command is copied: run it in a terminal".into(), cx);
                }))
                .into_any_element()
        };
        let row = |id: &'static str, icon, name: &str, what: String| {
            super::connecting::way_row(id, None, icon, name.to_owned(), None, what, false, false)
        };
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .rounded(px(10.))
                    .bg(theme::panel())
                    .border_1()
                    .border_color(theme::island_border())
                    .overflow_hidden()
                    .child(
                        row("setup-claude", "bot", "Claude Code", what).children(claude_end),
                    )
                    .child(div().h(px(1.)).bg(theme::island_border()))
                    .child(
                        row(
                            "setup-codex",
                            "terminal",
                            "Codex",
                            if s.codex {
                                "a Codex running in a terminal listens by itself".to_owned()
                            } else {
                                "codex mcp add, in a terminal".to_owned()
                            },
                        )
                        .child(codex_end),
                    ),
            )
            .children(self.agent_setup.clone().map(|said| {
                div()
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child(said)
            }))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child("A comment you write for an agent goes to its chat; its answers come back on the line."),
            )
            .into_any_element()
    }

    /// Keys: the keymaps to start from, then the keys that matter by group — a mark and
    /// GitLance's own where the one picked differs.
    fn render_setup_keys(&self, cx: &mut Context<Self>) -> AnyElement {
        let presets = self.key_presets(cx);
        let map = cx.global::<Keymap>();
        let entries = map.entries();
        let differ = |e: &super::keymap::Entry| e.keys != map.shipped(e.name);
        let differing = entries.iter().filter(|e| differ(e)).count();
        let caps = |keys: &[String]| {
            div().flex().gap(px(4.)).children(
                keys.first()
                    .map(|k| keys::caps(&super::keymap::symbols(k), On::Plain)),
            )
        };
        let group = |name: &'static str, actions: &[&str]| {
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .pt(px(12.))
                        .pb(px(4.))
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::muted())
                        .child(name.to_uppercase()),
                )
                .children(actions.iter().enumerate().filter_map(|(i, action)| {
                    let e = entries.iter().find(|e| e.name == *action)?;
                    let shipped = map.shipped(e.name);
                    let own = differ(e).then(|| {
                        shipped
                            .first()
                            .map(|k| super::keymap::symbols(k))
                            .unwrap_or_else(|| "no key".to_owned())
                    });
                    Some(
                        div()
                            .h(px(30.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .rounded(px(ROW_RADIUS))
                            .when(i % 2 == 1, |r| r.bg(theme::panel()))
                            .children(own.is_some().then(|| {
                                div()
                                    .flex_none()
                                    .size(px(6.))
                                    .rounded_full()
                                    .bg(theme::accent())
                            }))
                            .child(div().flex_1().min_w_0().child(e.label.clone()))
                            .children(own.map(|own| {
                                div()
                                    .text_size(px(11.))
                                    .text_color(theme::faint())
                                    .child(format!("GitLance {}", keys::text(&own)))
                            }))
                            .child(caps(&e.keys)),
                    )
                }))
        };
        let [(m, move_), (f, find), (r, review), (w, window)] = GROUPS;
        div()
            .flex()
            .flex_col()
            .child(presets)
            .child(
                div()
                    .flex()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(group(m, move_))
                            .child(group(f, find)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(group(r, review))
                            .child(group(w, window)),
                    ),
            )
            .child(
                div()
                    .pt(px(10.))
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child(format!(
                        "{differing} of {} keys differ from GitLance's · every key in Settings → Keyboard.",
                        entries.len()
                    )),
            )
            .into_any_element()
    }

    /// Look: the Window themes as Settings draws them, then Code colours, Panes, the faces and
    /// the sizes.
    fn render_setup_look(&self, cx: &mut Context<Self>) -> AnyElement {
        let choice = theme::choice();
        let code = choice
            .code
            .as_deref()
            .and_then(theme::find)
            .map_or("Follow the window".to_owned(), |t| t.label);
        let code_picker =
            div()
                .id("setup-code-theme")
                .min_w(px(190.))
                .h(px(28.))
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(px(ROW_RADIUS))
                .bg(theme::hover())
                .cursor_pointer()
                .hover(|s| s.bg(theme::selected()))
                .child(div().flex_1().text_size(px(13.)).child(code))
                .child(
                    icons::icon("chevron")
                        .size(px(12.))
                        .text_color(theme::muted()),
                )
                .on_click(cx.listener(|this, ev: &ClickEvent, _, cx| {
                    this.open_code_themes(ev.position(), cx)
                }));
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(super::appearance::rows_of(5, self.window_cards(cx)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(item(
                        "Code colours",
                        "Syntax and the diff's tints.",
                        code_picker,
                    ))
                    .child(item(
                        "Panes",
                        "Islands float with gaps; flat ones touch.",
                        self.panes_chips(cx),
                    ))
                    .children(self.font_rows(cx)),
            )
            .into_any_element()
    }

    /// Code colours' menu: Follow the window, then every code theme, the one in use checked.
    fn open_code_themes(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let current = theme::choice().code;
        let follow =
            Entry::new("Follow the window", Act::CodeTheme(None)).checked(current.is_none());
        let themes = theme::all()
            .into_iter()
            .filter(|t| t.code)
            .map(|t| {
                let on = current.as_deref() == Some(t.id.as_str());
                Entry::new(t.label, Act::CodeTheme(Some(t.id))).checked(on)
            })
            .collect();
        self.open_titled(at, vec![(None, vec![follow]), (Some("Code"), themes)], cx);
    }

    /// The code drawn in theme `id`, or the window's.
    pub(super) fn choose_code_theme(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        let mut choice = theme::choice();
        choice.code = id;
        self.choose_theme(choice, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_part_is_offered_once_and_not_to_who_used_gitlance_before() {
        // The first start: every part.
        assert_eq!(unseen("", false), Part::ALL);
        // GitLance used before Set up came: none.
        assert!(unseen("", true).is_empty());
        // A release that adds a part: that part alone.
        assert_eq!(unseen("agents\nkeys\n", true), [Part::Look]);
        assert_eq!(parse_seen("keys\nwhat\n look"), [Part::Keys, Part::Look]);
    }
}
