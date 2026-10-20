//! What hover shows (#53, Design `docs/specs/hover.md`): tooltips on a long hover (`LONG`, never on a
//! pointer passing over) — the whole of cut text, the date behind a relative time, what a short form
//! stands for — and cards for a commit, a person (#51) and a branch, which stay while the pointer is
//! on them and hold their actions (copy, open, mention).

use super::{ROW_RADIUS, TipOwned, Workspace, format, px, theme};
use crate::git::{CommitInfo, Deco, DecoKind};
use gpui::{
    AnyView, App, ClickEvent, Context, ElementId, FontWeight, IntoElement, Render, SharedString,
    StatefulInteractiveElement, WeakEntity, Window, canvas, div, prelude::*,
};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

/// How long the pointer rests before a tooltip or a card shows.
pub const LONG: Duration = Duration::from_millis(800);

/// A tooltip or a card on a long hover.
pub trait Hover: StatefulInteractiveElement + Sized {
    /// One line of text.
    fn tip(self, text: impl Into<SharedString>) -> Self {
        let text = text.into();
        self.tooltip_show_delay(LONG)
            .tooltip(move |_, cx| cx.new(|_| TipOwned(text.clone())).into())
    }

    /// A card that stays while the pointer is on it.
    fn card(self, build: impl Fn(&mut Window, &mut App) -> AnyView + 'static) -> Self {
        self.tooltip_show_delay(LONG).hoverable_tooltip(build)
    }
}

impl<E: StatefulInteractiveElement> Hover for E {}

/// A tooltip with nothing to say draws nothing.
pub struct NoTip;

impl Render for NoTip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// Text cut to its room with an ellipsis, whose long hover gives the whole of it — only when it is
/// cut this time (X60): a canvas over it measures the text against the room it has.
pub fn cut(id: impl Into<ElementId>, text: impl Into<SharedString>) -> gpui::Stateful<gpui::Div> {
    cut_to(id, text, 1)
}

/// The same for text clamped to `lines` lines.
pub fn cut_to(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    lines: usize,
) -> gpui::Stateful<gpui::Div> {
    let text: SharedString = text.into();
    let is_cut = Rc::new(Cell::new(false));
    let measure = (is_cut.clone(), text.clone());
    let shown = div().child(text.clone());
    div()
        .id(id)
        .relative()
        .min_w_0()
        .overflow_hidden()
        .child(if lines > 1 {
            shown.line_clamp(lines)
        } else {
            shown.truncate()
        })
        .child(
            canvas(
                move |bounds, window, _| {
                    let (is_cut, text) = measure;
                    let style = window.text_style();
                    let size = style.font_size.to_pixels(window.rem_size());
                    let room = bounds.size.width + gpui::px(0.5);
                    let cut = if lines > 1 {
                        let run = style.to_run(text.len());
                        window
                            .text_system()
                            .shape_text(text, size, &[run], Some(room), None)
                            .map(|l| l.iter().map(|l| l.wrap_boundaries.len() + 1).sum::<usize>())
                            .is_ok_and(|n| n > lines)
                    } else {
                        let line = SharedString::from(text.replace('\n', " "));
                        let run = style.to_run(line.len());
                        window
                            .text_system()
                            .shape_line(line, size, &[run], None)
                            .width
                            > room
                    };
                    is_cut.set(cut);
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
        .tooltip_show_delay(LONG)
        .tooltip(move |_, cx| {
            if is_cut.get() {
                cx.new(|_| TipOwned(text.clone())).into()
            } else {
                cx.new(|_| NoTip).into()
            }
        })
}

/// "Thu 16 Oct 2026, 11:24": the date behind a relative time, in local time (X61).
pub fn full_date(time: i64) -> String {
    full_date_at(time + format::local_offset(time))
}

/// The date of `time` as it is, written out.
fn full_date_at(time: i64) -> String {
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days = time.div_euclid(86400);
    let secs = time.rem_euclid(86400);
    let date = format::date_at(time);
    let (y, m, d) = (&date[..4], &date[5..7], &date[8..10]);
    let month = m.parse::<usize>().unwrap_or(1).clamp(1, 12);
    format!(
        "{} {} {} {y}, {:02}:{:02}",
        DAYS[(days + 4).rem_euclid(7) as usize],
        d.trim_start_matches('0'),
        MONTHS[month - 1],
        secs / 3600,
        secs % 3600 / 60
    )
}

/// A relative time ("2 h ago") whose long hover gives its date.
pub fn ago(id: impl Into<gpui::ElementId>, time: i64) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .whitespace_nowrap()
        .child(format::ago(time))
        .tip(full_date(time))
}

/// What a status letter stands for.
pub fn status_word(change: crate::git::ChangeKind, old: Option<&str>) -> String {
    use crate::git::ChangeKind::*;
    match change {
        Added => "Added".into(),
        Deleted => "Deleted".into(),
        Renamed => format!("Renamed from {}", old.unwrap_or_default()),
        Copied => format!("Copied from {}", old.unwrap_or_default()),
        TypeChanged => "Type changed".into(),
        Modified => "Modified".into(),
    }
}

/// The frame every card is drawn in.
fn frame() -> gpui::Div {
    div()
        .mt(px(6.))
        .w(px(360.))
        .p(px(12.))
        .rounded(px(10.))
        .border_1()
        .border_color(theme::island_border())
        .bg(theme::panel())
        .shadow_lg()
        .flex()
        .flex_col()
        .gap(px(8.))
        .text_size(px(12.))
        .text_color(theme::text())
}

fn action(id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px(px(8.))
        .h(px(24.))
        .flex()
        .items_center()
        .rounded(px(ROW_RADIUS))
        .border_1()
        .border_color(theme::island_border())
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .child(label)
}

/// A commit: its whole message's start, who and when, the full id, every branch and tag on it.
pub struct CommitCard {
    pub info: CommitInfo,
    pub refs: Vec<Deco>,
    /// The commit on GitLab or GitHub.
    pub url: Option<String>,
    /// The repository, to read what it changed when the card shows.
    pub root: Option<std::path::PathBuf>,
    /// (files, lines added, lines removed), once read (X58).
    pub stat: Option<(usize, usize, usize)>,
}

impl CommitCard {
    pub fn clone_of(other: &CommitCard) -> CommitCard {
        CommitCard {
            info: other.info.clone(),
            refs: other.refs.clone(),
            url: other.url.clone(),
            root: other.root.clone(),
            stat: other.stat,
        }
    }

    /// The card, shown: what the commit changed is read in the background and drawn when known.
    pub fn show(other: &CommitCard, cx: &mut App) -> AnyView {
        let card = cx.new(|_| CommitCard::clone_of(other));
        if let Some(root) = other.root.clone() {
            let (id, view) = (other.info.id, card.downgrade());
            cx.spawn(async move |cx| {
                let stat = cx
                    .background_executor()
                    .spawn(async move { crate::git::Repo::open(&root)?.commit_stat(id) })
                    .await;
                view.update(cx, |card, cx| {
                    card.stat = stat.ok();
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        card.into()
    }
}

impl Render for CommitCard {
    // It fades in after the long rest (#56, 7).
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        super::motion::appear(self.body(), "card-in", super::motion::FAST, |c, v| {
            c.opacity(v)
        })
    }
}

impl CommitCard {
    fn body(&mut self) -> gpui::Div {
        let c = &self.info;
        let body: Vec<String> = c
            .message
            .lines()
            .skip(1)
            .skip_while(|l| l.trim().is_empty())
            .take(4)
            .map(str::to_owned)
            .collect();
        let sha = c.id.to_string();
        let url = self.url.clone();
        frame()
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(c.summary.clone()),
            )
            .children((!body.is_empty()).then(|| {
                div()
                    .text_color(theme::muted())
                    .children(body.into_iter().map(|l| div().child(l)))
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(super::discussion::avatar(&c.author, false, 20.))
                    .child(
                        div().flex().flex_col().child(c.author.clone()).child(
                            div()
                                .text_size(px(11.))
                                .text_color(theme::faint())
                                .child(format!("{} · {}", c.email, full_date(c.time))),
                        ),
                    ),
            )
            .children(self.stat.map(|(files, added, removed)| {
                div()
                    .flex()
                    .gap(px(6.))
                    .text_size(px(11.))
                    .child(
                        div()
                            .text_color(theme::muted())
                            .child(super::plural(files, "file")),
                    )
                    .child(div().text_color(theme::added()).child(format!("+{added}")))
                    .child(
                        div()
                            .text_color(theme::removed())
                            .child(format!("−{removed}")),
                    )
            }))
            .children((!self.refs.is_empty()).then(|| {
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(4.))
                    .children(self.refs.iter().map(|d| {
                        let (text, color) = match d.kind {
                            DecoKind::Head => (format!("HEAD → {}", d.name), theme::accent()),
                            DecoKind::Local => (d.name.clone(), theme::accent()),
                            DecoKind::Remote => (d.name.clone(), theme::faint()),
                            DecoKind::Tag => (d.name.clone(), theme::warning()),
                        };
                        div()
                            .px(px(6.))
                            .rounded_full()
                            .border_1()
                            .border_color(color)
                            .text_size(px(10.))
                            .text_color(color)
                            .child(text)
                    }))
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(theme::CODE_FONT)
                            .text_size(px(11.))
                            .text_color(theme::muted())
                            .child(sha.clone()),
                    )
                    .child(action("card-copy-sha", "Copy").on_click(
                        move |_: &ClickEvent, _, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(sha.clone()))
                        },
                    ))
                    .children(url.map(|url| {
                        action("card-open-commit", "Open ↗")
                            .on_click(move |_: &ClickEvent, _, cx| cx.open_url(&url))
                    })),
            )
    }
}

/// A person (#51): who they are here, and a way to mention them or open their page.
pub struct PersonCard {
    pub name: String,
    pub login: String,
    /// What they are in the open request: author, reviewer (approved), …
    pub role: Option<String>,
    /// What they did last in it: "Pushed v4 35 min ago · wrote 12 min ago" (X58).
    pub activity: Option<String>,
    pub url: Option<String>,
    pub ws: WeakEntity<Workspace>,
}

impl PersonCard {
    pub fn clone_of(other: &PersonCard) -> PersonCard {
        PersonCard {
            name: other.name.clone(),
            login: other.login.clone(),
            role: other.role.clone(),
            activity: other.activity.clone(),
            url: other.url.clone(),
            ws: other.ws.clone(),
        }
    }
}

impl Render for PersonCard {
    // It fades in after the long rest (#56, 7).
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        super::motion::appear(self.body(), "card-in", super::motion::FAST, |c, v| {
            c.opacity(v)
        })
    }
}

impl PersonCard {
    fn body(&mut self) -> gpui::Div {
        let login = self.login.clone();
        let ws = self.ws.clone();
        let url = self.url.clone();
        frame()
            .w(px(300.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(super::discussion::avatar(&self.name, false, 32.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(self.name.clone()),
                            )
                            .children((!self.login.is_empty()).then(|| {
                                div()
                                    .text_color(theme::muted())
                                    .child(format!("@{}", self.login))
                            })),
                    ),
            )
            .children(
                self.role
                    .clone()
                    .map(|r| div().text_color(theme::muted()).child(r)),
            )
            .children(
                self.activity
                    .clone()
                    .map(|a| div().text_size(px(11.)).text_color(theme::faint()).child(a)),
            )
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .children((!login.is_empty()).then(|| {
                        action("card-mention", "Mention").on_click(move |_: &ClickEvent, _, cx| {
                            ws.update(cx, |this, cx| this.mention(&login, cx)).ok();
                        })
                    }))
                    .children(url.map(|url| {
                        action("card-open-person", "Open ↗")
                            .on_click(move |_: &ClickEvent, _, cx| cx.open_url(&url))
                    })),
            )
    }
}

/// A mention under the pointer in comment text, and the person's card once the pointer rested on it
/// (#51, X62): the card stays while the pointer is on the mention or on the card.
pub struct MentionHover {
    login: String,
    /// Where the mention is drawn, in the window.
    spot: gpui::Bounds<gpui::Pixels>,
    card: Option<AnyView>,
    _wait: gpui::Task<()>,
}

impl MentionHover {
    /// Whether `at` is on the mention, or between it and its card.
    fn near(&self, at: gpui::Point<gpui::Pixels>) -> bool {
        self.spot.dilate(gpui::px(8.)).contains(&at)
    }
}

/// A branch: its upstream and how far apart, its last commit.
pub struct BranchCard {
    pub name: String,
    pub tracking: Option<(String, usize, usize)>,
    pub last: Option<CommitInfo>,
}

impl Render for BranchCard {
    // It fades in after the long rest (#56, 7).
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        super::motion::appear(self.body(), "card-in", super::motion::FAST, |c, v| {
            c.opacity(v)
        })
    }
}

impl BranchCard {
    fn body(&mut self) -> gpui::Div {
        frame()
            .child(
                div()
                    .font_family(theme::CODE_FONT)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.name.clone()),
            )
            .child(
                div()
                    .text_color(theme::muted())
                    .child(match &self.tracking {
                        Some((up, 0, 0)) => format!("Up to date with {up}"),
                        Some((up, ahead, behind)) => {
                            format!("{up} · ↑{ahead} ahead · ↓{behind} behind")
                        }
                        None => "No upstream".to_owned(),
                    }),
            )
            .children(self.last.as_ref().map(|c| {
                div().flex().flex_col().child(c.summary.clone()).child(
                    div()
                        .text_size(px(11.))
                        .text_color(theme::faint())
                        .child(format!(
                            "{} · {} · {}",
                            format::short(c.id),
                            c.author,
                            full_date(c.time)
                        )),
                )
            }))
    }
}

impl Workspace {
    /// The card of commit `ix` of the list.
    pub(super) fn commit_card(&self, ix: usize) -> Option<CommitCard> {
        let info = self.commits.get(ix)?.clone();
        Some(CommitCard {
            refs: self.decor.get(&info.id).cloned().unwrap_or_default(),
            url: self.web.as_ref().map(|w| w.commit(&info.id.to_string())),
            info,
            root: self.root.clone(),
            stat: None,
        })
    }

    /// The card of the person named `name` (`login` when known): their part in the open request.
    pub(super) fn person_card(
        &self,
        name: &str,
        login: &str,
        ws: WeakEntity<Workspace>,
    ) -> PersonCard {
        let login = if login.is_empty() {
            self.requests
                .members
                .iter()
                .find(|(_, n)| n == name)
                .map(|(l, _)| l.clone())
                .unwrap_or_default()
        } else {
            login.to_owned()
        };
        let m = self
            .requests
            .current
            .and_then(|iid| self.requests.list.iter().find(|m| m.iid == iid));
        let approved = self
            .requests
            .approvals
            .as_ref()
            .is_some_and(|a| a.by.iter().any(|b| b == name));
        let role = m.and_then(|m| {
            let mine = |l: &str| !login.is_empty() && l == login;
            if mine(&m.author_login) || m.author == name {
                Some("Author of this merge request".to_owned())
            } else if m.reviewers.iter().any(|r| mine(r) || r == name) || approved {
                Some(if approved {
                    "Reviewer · approved".to_owned()
                } else {
                    "Reviewer · reviewing".to_owned()
                })
            } else {
                None
            }
        });
        // What they did last here: the author's last push, anyone's last note.
        let theirs = |l: &str, n: &str| (!login.is_empty() && l == login) || n == name;
        let pushed = m
            .filter(|m| theirs(&m.author_login, &m.author))
            .and_then(|_| self.versions.last())
            .filter(|v| v.time > 0)
            .map(|v| format!("Pushed v{} {}", v.number, format::ago(v.time)));
        let wrote = m
            .and_then(|_| {
                self.requests
                    .threads
                    .iter()
                    .flat_map(|t| &t.notes)
                    .filter(|n| theirs(&n.login, &n.author))
                    .map(|n| n.created)
                    .max()
            })
            .map(|t| format!("wrote {}", format::ago(t)));
        let activity = match (pushed, wrote) {
            (Some(p), Some(w)) => Some(format!("{p} · {w}")),
            (Some(p), None) => Some(p),
            (None, Some(w)) => Some(format!("{}{}", w[..1].to_uppercase(), &w[1..])),
            (None, None) => None,
        };
        let url = self.web.as_ref().filter(|_| !login.is_empty()).map(|w| {
            let host_end = w
                .base
                .find("://")
                .and_then(|s| w.base[s + 3..].find('/').map(|i| s + 3 + i))
                .unwrap_or(w.base.len());
            format!("{}/{login}", &w.base[..host_end])
        });
        PersonCard {
            name: name.to_owned(),
            login,
            role,
            activity,
            url,
            ws,
        }
    }

    /// What builds the card of branch `ix` of the list when it shows: the repository is read then,
    /// not at every frame.
    pub(super) fn branch_card(
        &self,
        ix: usize,
    ) -> Option<impl Fn(&mut Window, &mut App) -> AnyView + 'static> {
        let b = self.branches.get(ix)?;
        let (root, name, tip) = (self.root.clone()?, b.name.clone(), b.tip);
        Some(move |_: &mut Window, cx: &mut App| {
            let repo = crate::git::Repo::open(&root).ok();
            let card = BranchCard {
                name: name.clone(),
                tracking: repo.as_ref().and_then(|r| r.tracking(&name)),
                last: repo.as_ref().and_then(|r| r.commit(tip).ok()),
            };
            cx.new(|_| card).into()
        })
    }

    /// The pointer is on mention `hit` (or on none): its card comes after a long rest there.
    pub(super) fn hover_mention(
        &mut self,
        hit: Option<(String, gpui::Bounds<gpui::Pixels>)>,
        cx: &mut Context<Self>,
    ) {
        match hit {
            Some((login, spot)) => {
                if self
                    .mention_hover
                    .as_ref()
                    .is_some_and(|h| h.login == login && h.spot == spot)
                {
                    return;
                }
                let wait = cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(LONG).await;
                    this.update(cx, |this, cx| this.show_mention_card(cx)).ok();
                });
                self.mention_hover = Some(MentionHover {
                    login,
                    spot,
                    card: None,
                    _wait: wait,
                });
            }
            // Off the mention before its card came: it does not come. A card shown waits for the
            // pointer to leave it.
            None => {
                if self
                    .mention_hover
                    .as_ref()
                    .is_some_and(|h| h.card.is_none())
                {
                    self.mention_hover = None;
                }
            }
        }
        cx.notify();
    }

    fn show_mention_card(&mut self, cx: &mut Context<Self>) {
        let Some(login) = self.mention_hover.as_ref().map(|h| h.login.clone()) else {
            return;
        };
        let name = self
            .requests
            .members
            .iter()
            .find(|(l, _)| *l == login)
            .map_or_else(|| login.clone(), |(_, n)| n.clone());
        let card = self.person_card(&name, &login, cx.entity().downgrade());
        let view: AnyView = cx.new(|_| card).into();
        if let Some(h) = &mut self.mention_hover {
            h.card = Some(view);
        }
        cx.notify();
    }

    /// The pointer moved: off the mention and off its card, the card goes.
    pub(super) fn mention_pointer(
        &mut self,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        if self
            .mention_hover
            .as_ref()
            .is_some_and(|h| !h.near(at) && h.card.is_some())
        {
            self.mention_hover = None;
            cx.notify();
        }
    }

    /// The card of the mention the pointer rests on, under it.
    pub(super) fn render_mention_card(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let h = self.mention_hover.as_ref()?;
        let card = h.card.clone()?;
        Some(
            gpui::deferred(
                gpui::anchored()
                    .position(gpui::point(h.spot.left(), h.spot.bottom()))
                    .snap_to_window_with_margin(gpui::px(8.))
                    .child(
                        div()
                            .id("mention-card")
                            .occlude()
                            .on_hover(cx.listener(|this, on: &bool, window, cx| {
                                let at = window.mouse_position();
                                if !*on && this.mention_hover.as_ref().is_some_and(|h| !h.near(at))
                                {
                                    this.mention_hover = None;
                                    cx.notify();
                                }
                            }))
                            .child(card),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// `@login ` into the comment being written, or onto the clipboard when none is.
    pub(super) fn mention(&mut self, login: &str, cx: &mut Context<Self>) {
        let tag = format!("@{login} ");
        match self.compose.as_mut() {
            Some(c) => c.edit.insert(&tag),
            None => cx.write_to_clipboard(gpui::ClipboardItem::new_string(tag)),
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_reads_as_a_date() {
        // 2026-10-16 11:24 UTC was a Friday; four hours on, 15:24.
        assert_eq!(full_date_at(1_792_149_840), "Fri 16 Oct 2026, 11:24");
        assert_eq!(
            full_date_at(1_792_149_840 + 4 * 3600),
            "Fri 16 Oct 2026, 15:24"
        );
    }
}
