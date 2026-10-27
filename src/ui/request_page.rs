//! A merge request's page (#73): the section tabs of its bar — Overview, Commits, Changes — and
//! what the first two show; Changes is the diff, with one line of identity over it. Overview holds
//! what the Details tool window held (#8), as a page: the identity and its facts, the description,
//! and a column of who reviews (#137: the discussions are the Discussions window's). A request you
//! have never been inside opens on it, later ones on Changes (`nav::first_section`). Commits lists
//! the request's commits with the version each came with; one opens as a level under it.

use super::nav::{Item, Page, Section};
use super::{Workspace, format, icons, island, markdown, plural, px, theme, words};
use crate::mr::{Details, Mr, MrState};
use gpui::{AnyElement, ClickEvent, Context, Div, FontWeight, Rgba, SharedString, div, prelude::*};

/// Overview's column beside the description, and the least height it keeps when the
/// description is short.
const SIDE: f32 = 300.;
const SIDE_MIN: f32 = 320.;

impl Workspace {
    /// The request of the page on screen, once it is the one loaded.
    fn page_request(&self) -> Option<&Mr> {
        let Some(Item::Request(iid)) = self.nav_page()?.item() else {
            return None;
        };
        self.requests
            .find(iid)
            .filter(|_| self.requests.current == Some(iid))
    }

    /// An item's Overview or Commits under its bar, the tool window beside it: a request's, or a
    /// branch's Commits (#74).
    pub(super) fn render_tab_page(&self, tab: Section, cx: &mut Context<Self>) -> Div {
        let body = match (self.page_request(), tab) {
            _ if matches!(self.nav_page(), Some(Page::Branch(_))) => {
                let line = self
                    .render_branch_line()
                    .unwrap_or_else(|| div().into_any_element());
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(super::gap()))
                    .child(line)
                    .child(self.commits_list(None, cx))
                    .into_any_element()
            }
            (None, _) => island()
                .flex_1()
                .bg(theme::editor())
                .px(px(28.))
                .py(px(26.))
                .child(super::roots::skeleton("request-loading", 4))
                .into_any_element(),
            (Some(m), Section::Overview) => self.render_overview(m, cx),
            (Some(m), _) => self.render_commits_tab(m, cx),
        };
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .child(body)
            .children(self.render_tool(cx))
    }

    /// Overview: the identity and its facts, then the description beside the facts column.
    fn render_overview(&self, m: &Mr, cx: &mut Context<Self>) -> AnyElement {
        let d = self.requests.details.clone().unwrap_or_default();
        // The list's own copy until the request's details are in.
        let text = if d.description.trim().is_empty() {
            &m.description
        } else {
            &d.description
        };
        let description = if text.trim().is_empty() {
            div()
                .text_color(theme::muted())
                .child("No description.")
                .into_any_element()
        } else {
            markdown::body(
                text,
                true,
                format!("overview-{}", m.iid),
                self.web.as_ref().map(|w| w.base.as_str()),
                Some(self.selecting(cx.entity().downgrade())),
            )
        };
        island()
            .flex_1()
            .min_w_0()
            .bg(theme::editor())
            .child(
                div()
                    .id("overview")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(self.overview_identity(m, cx))
                    .child(div().h(px(1.)).bg(theme::island_border()))
                    // The description sets the height; the facts and discussions beside it are no
                    // taller and scroll in their own column (the developer, 2026-10-23).
                    .child(
                        div()
                            .relative()
                            .px(px(24.))
                            .py(px(20.))
                            .child(
                                div()
                                    .mr(px(SIDE + 32.))
                                    .min_h(px(SIDE_MIN))
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .text_size(px(15.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child("Description"),
                                    )
                                    .child(div().text_size(px(13.)).child(description)),
                            )
                            .child(
                                div()
                                    .id("overview-side")
                                    .absolute()
                                    .top(px(20.))
                                    .bottom(px(20.))
                                    .right(px(24.))
                                    .w(px(SIDE))
                                    .overflow_y_scroll()
                                    .flex()
                                    .flex_col()
                                    .gap(px(20.))
                                    .text_size(px(12.))
                                    .children(self.overview_facts(&d)),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// Overview's head: the title and its state, who wants to merge what into what, and the facts
    /// in a row — author, branches, pipeline, approvals, how far your review is, the versions.
    fn overview_identity(&self, m: &Mr, cx: &mut Context<Self>) -> Div {
        let url = m.url.clone();
        let mine = self
            .requests
            .login
            .as_deref()
            .is_some_and(|me| me == m.author_login);
        let pipeline = self.requests.pipeline.as_ref().map_or_else(
            || faint("—").into_any_element(),
            |p| {
                let (glyph, word, color) = super::requests::pipeline_word(p);
                div()
                    .text_color(color)
                    .child(format!("{glyph} {word}"))
                    .into_any_element()
            },
        );
        let approvals = match &self.requests.approvals {
            Some(a) if a.required > 0 => {
                format!("{} of {}", a.required - a.left.min(a.required), a.required)
            }
            Some(a) if !a.by.is_empty() => format!("{} approved", a.by.len()),
            Some(_) => "none yet".to_owned(),
            None => "—".to_owned(),
        };
        let head = git2::Oid::from_str(&m.head).ok();
        let reviewed = self
            .review_progress()
            .map(|(done, total)| (done as u32, total as u32))
            .or_else(|| head.and_then(|h| self.head_part(h)))
            .map_or_else(
                || "—".to_owned(),
                |(done, total)| format!("{done} of {}", plural(total as usize, "file")),
            );
        let latest = self.versions.last().map(|v| v.number);
        let versions = match (self.marked_version(m.iid), latest) {
            (Some(at), Some(latest)) if at < latest => format!("v{at} → v{latest}"),
            (_, Some(latest)) => format!("v{latest}"),
            _ => "—".to_owned(),
        };
        div()
            .px(px(24.))
            .pt(px(20.))
            .pb(px(18.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(17.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.selectable(
                                format!("title-{}", m.iid),
                                &m.title,
                                cx.entity().downgrade(),
                            )),
                    )
                    .child(state_chip(m))
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("overview-web")
                            .flex_none()
                            .text_size(px(12.))
                            .text_color(theme::faint())
                            .cursor_pointer()
                            .hover(|s| s.text_color(theme::accent()))
                            .child(words::say("Open in GitLab ↗").into_owned())
                            .on_click(
                                cx.listener(move |_, _: &ClickEvent, _, cx| cx.open_url(&url)),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_x(px(5.))
                    .text_size(px(13.))
                    .text_color(theme::muted())
                    .child(div().text_color(theme::text()).child(if mine {
                        "You".to_owned()
                    } else {
                        m.author.clone()
                    }))
                    .child(match (m.state, mine) {
                        (MrState::Merged, _) => "merged",
                        (MrState::Closed, _) => "wanted to merge",
                        (MrState::Opened, true) => "want to merge",
                        (MrState::Opened, false) => "wants to merge",
                    })
                    .child(
                        div()
                            .text_color(theme::text())
                            .child(plural(self.commits.len(), "commit")),
                    )
                    .child("from")
                    .child(mono(&m.source).text_color(theme::accent()))
                    .child("into")
                    .child(mono(&m.target)),
            )
            .child(
                div()
                    .mt(px(12.))
                    .flex()
                    .flex_wrap()
                    .gap_x(px(32.))
                    .gap_y(px(12.))
                    .child(fact(
                        "Author",
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(super::discussion::avatar(&m.author, false, 16.))
                            .child(m.author.clone())
                            .into_any_element(),
                    ))
                    .child(fact(
                        "Branches",
                        div()
                            .flex()
                            .gap(px(5.))
                            .child(mono(&m.source).text_color(theme::accent()))
                            .child(faint("→"))
                            .child(mono(&m.target))
                            .into_any_element(),
                    ))
                    .child(fact("Pipeline", pipeline))
                    .child(fact(
                        "Approvals",
                        div()
                            .when(self.approval_waits(), |s| s.opacity(super::waiting::FADED))
                            .child(approvals)
                            .into_any_element(),
                    ))
                    .child(fact("Reviewed", div().child(reviewed).into_any_element()))
                    .child(fact("Versions", div().child(versions).into_any_element())),
            )
    }

    /// Overview's column: who reviews and how far (#137; the discussions are the Discussions
    /// window's).
    fn overview_facts(&self, d: &Details) -> Vec<AnyElement> {
        let approved = self
            .requests
            .approvals
            .as_ref()
            .map(|a| a.by.clone())
            .unwrap_or_default();
        // Who reviews: those asked, then whoever approved without being asked (GitHub drops an
        // approving reviewer from the asked ones).
        let mut reviewers = d.reviewers.clone();
        for name in approved.iter().chain(&d.changes_requested) {
            if !reviewers.contains(name) {
                reviewers.push(name.clone());
            }
        }
        if reviewers.is_empty() {
            return Vec::new();
        }
        vec![
            block(
                "Reviewers",
                reviewers.into_iter().map(|r| {
                    let state = if approved.contains(&r) {
                        ("✓ approved", theme::added())
                    } else if d.changes_requested.contains(&r) {
                        ("requested changes", theme::warning())
                    } else {
                        ("reviewing", theme::muted())
                    };
                    let login = self.login_of(&r);
                    person(r, login, Some(state)).into_any_element()
                }),
            )
            .into_any_element(),
        ]
    }

    /// A request's Commits: one line of identity, then its commits, each with the version it came
    /// with.
    fn render_commits_tab(&self, m: &Mr, cx: &mut Context<Self>) -> AnyElement {
        let tips: Vec<(usize, git2::Oid)> =
            self.versions.iter().map(|v| (v.number, v.tip)).collect();
        let ids: Vec<git2::Oid> = self.commits.iter().map(|c| c.id).collect();
        let versions = commit_versions(&ids, &tips);
        let line = self.request_line(m, cx);
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(super::gap()))
            .child(line)
            .child(self.commits_list(Some(versions), cx))
            .into_any_element()
    }

    /// An item's commits, newest first, with the version each came with when it has versions;
    /// ↑ ↓ pick one, ↵ or a second click opens it as a level.
    fn commits_list(
        &self,
        versions: Option<Vec<Option<usize>>>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use super::hover::Hover;
        let versions = versions.unwrap_or_else(|| vec![None; self.commits.len()]);
        let picked = self.picked_commit();
        // Shift picks a run of them, to review as one diff.
        let run = self.picked_run();
        // A commit's anatomy (#79): its sha and title always; when, and who when there are
        // several, while the list has 260 px; the icon and its version under 360 px leave first.
        // The tooltip holds them all.
        let room = self.widths.room("commits");
        let several = self
            .commits
            .first()
            .is_some_and(|first| self.commits.iter().any(|c| c.author != first.author));
        let rows: Vec<_> = self
            .commits
            .iter()
            .enumerate()
            .map(|(ix, c)| {
                let selected = match run {
                    Some((newest, oldest)) => (newest..=oldest).contains(&ix),
                    None => picked == Some(ix),
                };
                let mut tip = vec![
                    c.summary.clone(),
                    format!(
                        "{} · {} · {}",
                        format::short(c.id),
                        c.author,
                        super::hover::full_date(c.time)
                    ),
                ];
                if let Some(v) = versions[ix] {
                    tip.push(format!("came with v{v}"));
                }
                super::row(("request-commit", ix), selected)
                    .h(px(36.))
                    .gap(px(10.))
                    .px(px(10.))
                    .when(room.third(), |s| {
                        s.child(
                            icons::icon("commit")
                                .size(px(14.))
                                .text_color(theme::faint()),
                        )
                    })
                    .child(
                        div()
                            .flex_none()
                            .font_family(theme::CODE_FONT)
                            .text_size(px(12.))
                            .text_color(theme::faint())
                            .child(format::short(c.id)),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(c.summary.clone()))
                    .children((several && room.second()).then(|| {
                        div()
                            .flex_none()
                            .max_w(px(140.))
                            .truncate()
                            .text_size(px(11.))
                            .text_color(theme::muted())
                            .child(c.author.clone())
                    }))
                    .children(versions[ix].filter(|_| room.third()).map(|v| {
                        div()
                            .flex_none()
                            .px(px(6.))
                            .rounded_full()
                            .border_1()
                            .border_color(theme::accent())
                            .text_size(px(11.))
                            .text_color(theme::accent())
                            .child(format!("v{v}"))
                    }))
                    .when(room.second(), |s| {
                        s.child(
                            div()
                                .flex_none()
                                .text_size(px(11.))
                                .text_color(theme::faint())
                                .child(format::ago(c.time)),
                        )
                    })
                    .tip(tip.join("\n"))
                    .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                        let id = this.commits.get(ix).map(|c| c.id);
                        if event.modifiers().shift {
                            // The run from the one picked to this one.
                            if this.nav.pick_commit.is_none() {
                                this.nav.pick_commit = this
                                    .picked_commit()
                                    .and_then(|at| this.commits.get(at).map(|c| c.id));
                            }
                            this.nav.pick_to = id;
                            return cx.notify();
                        }
                        if this.picked_commit() == Some(ix) && event.click_count() > 1 {
                            return this.select_commit(ix, cx);
                        }
                        this.nav.pick_commit = id;
                        this.nav.pick_to = None;
                        cx.notify();
                    }))
            })
            .collect();
        island()
            .flex_1()
            .min_h_0()
            .bg(theme::editor())
            .child(
                div()
                    .id("request-commits")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(8.))
                    .py(px(8.))
                    .child(self.widths.measure("commits"))
                    .child(
                        div()
                            .id("commits-count")
                            .px(px(10.))
                            .pb(px(6.))
                            .text_size(px(11.))
                            .text_color(theme::faint())
                            .child(match run {
                                Some((newest, oldest)) => format!(
                                    "{} of {} picked · ↵ reviews them as one",
                                    oldest - newest + 1,
                                    plural(self.commits.len(), "commit")
                                ),
                                None => plural(self.commits.len(), "commit"),
                            })
                            .tip("Newest first · ↵ opens one, esc comes back here · shift and a click picks a run of them to review as one"),
                    )
                    .children(rows),
            )
            .into_any_element()
    }

    /// The section tabs in an item's bar with their counts — a request's Overview, Commits and
    /// Changes, a branch's Commits and Changes (#74); ⌃⇥ walks them.
    pub(super) fn section_tabs(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let at = self.page_section()?;
        let (loaded, files) = match self.nav_page()?.item()? {
            Item::Request(iid) => (
                self.requests.current == Some(iid),
                self.requests
                    .files
                    .filter(|(of, _)| *of == iid)
                    .map(|(_, n)| n),
            ),
            // Tabbed only while it is the branch loaded.
            Item::Branch(_) => (true, self.branch_page.as_ref().and_then(|f| f.files)),
            Item::Compare(..) | Item::WorkingTree => return None,
        };
        let count = |section: Section| match section {
            Section::Overview => None,
            Section::Commits => loaded.then_some(self.commits.len()),
            Section::Changes => files,
        };
        Some(self.item_sections().iter().copied().fold(
            super::diff_view::group(),
            |group, section| {
                let label = match count(section) {
                    Some(n) => format!("{} {n}", section.label()),
                    None => section.label().to_owned(),
                };
                group.child(
                    super::diff_view::chip(
                        SharedString::from(format!("section-{}", section.label())),
                        label,
                        section == at,
                    )
                    .on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| this.show_section(section, cx),
                    )),
                )
            },
        ))
    }

    /// The commit Commits has picked: the one esc came back from, else the newest.
    fn picked_commit(&self) -> Option<usize> {
        let at = self
            .nav
            .pick_commit
            .and_then(|id| self.commits.iter().position(|c| c.id == id));
        at.or((!self.commits.is_empty()).then_some(0))
    }

    /// The run of commits picked with shift: the newest's index and the oldest's.
    fn picked_run(&self) -> Option<(usize, usize)> {
        let at = |id: git2::Oid| self.commits.iter().position(|c| c.id == id);
        let a = at(self.nav.pick_commit?)?;
        let b = at(self.nav.pick_to?)?;
        (a != b).then_some((a.min(b), a.max(b)))
    }

    /// ↑ ↓ in a request's Commits: another commit picked, nothing opened.
    pub(super) fn step_commit_pick(&mut self, down: bool, cx: &mut Context<Self>) {
        let n = self.commits.len();
        let to = match (self.picked_commit(), down) {
            (Some(at), true) => (at + 1).min(n.saturating_sub(1)),
            (Some(at), false) => at.saturating_sub(1),
            (None, _) => return,
        };
        self.nav.pick_commit = self.commits.get(to).map(|c| c.id);
        self.nav.pick_to = None;
        cx.notify();
    }

    /// ↵ in a request's Commits: the commit picked opens as a level under it, or the run picked
    /// as one diff.
    pub(super) fn open_commit_pick(&mut self, cx: &mut Context<Self>) {
        if let Some((newest, oldest)) = self.picked_run() {
            return self.select_commits(newest, oldest, cx);
        }
        if let Some(ix) = self.picked_commit() {
            self.select_commit(ix, cx);
        }
    }

    /// [ and ] on a request's or a branch's commit: the commit before or after it in time (the list is newest
    /// first). Whether the page was one.
    pub(super) fn step_item_commit(&mut self, forward: bool, cx: &mut Context<Self>) -> bool {
        let (Some(Page::RequestCommit(_, id)) | Some(Page::BranchCommit(_, id))) =
            self.nav_page().cloned()
        else {
            return false;
        };
        let Some(at) = self.commits.iter().position(|c| c.id == id) else {
            return false;
        };
        let to = if forward {
            at.checked_sub(1)
        } else {
            (at + 1 < self.commits.len()).then_some(at + 1)
        };
        if let Some(to) = to {
            self.select_commit(to, cx);
        }
        true
    }

    /// One line of a request's identity, over its Commits and its Changes: the title, its state,
    /// the pipeline (#73).
    pub(super) fn render_request_line(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let m = self
            .requests
            .current
            .filter(|_| self.showing_request())
            .and_then(|iid| self.requests.find(iid))?;
        Some(self.request_line(m, cx).into_any_element())
    }

    fn request_line(&self, m: &Mr, cx: &mut Context<Self>) -> Div {
        island()
            .flex_none()
            .flex_row()
            .items_center()
            .gap(px(10.))
            .h(px(40.))
            .px(px(14.))
            // Its whole identity in a long hover: the line names no author (the developer,
            // 2026-10-23).
            .child({
                use super::hover::Hover;
                let title = div()
                    .id("request-line-title")
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.selectable(
                        format!("line-title-{}", m.iid),
                        &m.title,
                        cx.entity().downgrade(),
                    ));
                match self.request_tip(m.iid) {
                    Some(tip) => title.tip(tip),
                    None => title,
                }
            })
            .child(state_chip(m))
            .child(div().flex_1())
            .children(self.render_pipeline_pill(cx))
    }
}

/// Each commit's version, the commits newest first: the version whose tip it is, else the one of
/// the commit above it — it came with that push. None above the first tip found (a push rewrote
/// what the versions knew).
fn commit_versions(commits: &[git2::Oid], tips: &[(usize, git2::Oid)]) -> Vec<Option<usize>> {
    let mut at = None;
    commits
        .iter()
        .map(|id| {
            // The first push it was the tip of: when it came.
            if let Some(&(number, _)) = tips.iter().find(|(_, tip)| tip == id) {
                at = Some(number);
            }
            at
        })
        .collect()
}

/// The request's state as a chip: open, a draft, merged or closed (#114).
fn state_chip(m: &Mr) -> Div {
    let (word, color) = match m.state {
        MrState::Merged => ("Merged", theme::accent()),
        MrState::Closed => ("Closed", theme::removed()),
        MrState::Opened if m.draft => ("Draft", theme::warning()),
        MrState::Opened => ("Open", theme::added()),
    };
    div()
        .flex_none()
        .px(px(6.))
        .rounded(px(4.))
        .bg(Rgba { a: 0.15, ..color })
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(color)
        .child(word)
}

/// A fact of Overview's head: its label over its value.
fn fact(label: &'static str, value: AnyElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::faint())
                .child(label.to_uppercase()),
        )
        .child(div().text_size(px(13.)).child(value))
}

/// A block of Overview's column: its heading over its rows.
fn block(title: &str, rows: impl IntoIterator<Item = AnyElement>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::faint())
                .child(title.to_uppercase()),
        )
        .children(rows)
}

/// Someone in a list: the face, the name with the username under it (the developer,
/// 2026-10-23), and where they stand.
fn person(name: String, login: Option<String>, state: Option<(&'static str, Rgba)>) -> Div {
    let login = login.filter(|l| *l != name);
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(super::discussion::avatar(
            &name,
            false,
            if login.is_some() { 24. } else { 18. },
        ))
        .child(div().flex().flex_col().child(name).children(login.map(|l| {
            div()
                .text_size(px(11.))
                .text_color(theme::faint())
                .child(format!("@{l}"))
        })))
        .children(state.map(|(word, color)| div().text_color(color).child(word)))
}

fn mono(text: &str) -> Div {
    div().font_family(theme::CODE_FONT).child(text.to_owned())
}

fn faint(text: &'static str) -> Div {
    div().text_color(theme::faint()).child(text)
}

#[cfg(test)]
mod tests {
    use super::commit_versions;
    use git2::Oid;

    fn id(n: u8) -> Oid {
        Oid::from_bytes(&[n; 20]).unwrap()
    }

    #[test]
    fn a_commit_is_of_the_push_it_came_with() {
        // Newest first: 3 is v3's tip, 2 v2's, 1 came with v2 too (v1's tip was 0).
        let commits = [id(3), id(2), id(1), id(0)];
        let tips = [(1, id(0)), (2, id(2)), (3, id(3))];
        assert_eq!(
            commit_versions(&commits, &tips),
            [Some(3), Some(2), Some(2), Some(1)]
        );
        // A push rewrote them all: none of the tips is among them but the last.
        let rewritten = [id(9), id(8), id(7)];
        assert_eq!(
            commit_versions(&rewritten, &[(1, id(0)), (2, id(9))]),
            [Some(2), Some(2), Some(2)]
        );
        assert_eq!(commit_versions(&[id(1)], &[]), [None]);
    }
}
