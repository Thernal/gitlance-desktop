//! The side panel of the redesign (Design `design/shell/`, #49): one island of sections one under
//! the other — Versions, Commits, Changes — each folding to a header that still says what is in it
//! (a count, the first item, how far the review is). An open Commits or Changes takes the room left;
//! Versions takes what its rows need. Folds are kept in `layout.txt`; ⌥⌘2 folds Changes.

use super::nav::Page;
use super::zones::Zone;
use super::{Selection, Tip, Workspace, chip, commit_chip, icons, island, plural, px, theme};
use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, MouseButton, SharedString, div, prelude::*,
    uniform_list,
};

/// The height of a section's header, folded or open.
const HEADER: f32 = 34.;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Versions,
    Commits,
    Changes,
}

impl Workspace {
    fn section_folded(&self, section: Section) -> bool {
        match section {
            Section::Versions => self.layout.fold_versions,
            Section::Commits => self.layout.fold_commits,
            Section::Changes => !self.layout.show_files,
        }
    }

    pub(super) fn fold_section(&mut self, section: u8, cx: &mut Context<Self>) {
        // Its rows stay drawn while it folds (#56, 1).
        self.mark_motion(&format!("fold-{section}"), cx);
        let fold = match section {
            0 => &mut self.layout.fold_versions,
            1 => &mut self.layout.fold_commits,
            _ => {
                self.layout.show_files = !self.layout.show_files;
                return self.after_fold(cx);
            }
        };
        *fold = !*fold;
        self.after_fold(cx);
    }

    fn after_fold(&mut self, cx: &mut Context<Self>) {
        self.save_layout();
        // A folded section gives the keyboard back to the diff.
        let gone = match self.zone {
            Zone::Commits => self.layout.fold_commits,
            Zone::Files => !self.files_zone(),
            _ => false,
        };
        if gone && self.diff.is_some() {
            self.set_zone(Zone::Diff, cx);
        }
        cx.notify();
    }

    /// A section's header: the fold chevron, its title, what it holds when folded, and on the
    /// right its count and controls. `fold` is None for a section that cannot open.
    fn section_header(
        &self,
        section: Section,
        title: String,
        summary: Option<String>,
        end: Vec<AnyElement>,
        fold: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let folded = self.section_folded(section) || !fold;
        let id = match section {
            Section::Versions => "fold-versions",
            Section::Commits => "fold-commits",
            Section::Changes => "fold-changes",
        };
        let which = section as u8;
        div()
            .id(id)
            .flex_none()
            .h(px(HEADER))
            .pl(px(8.))
            .pr(px(10.))
            .flex()
            .items_center()
            .gap(px(6.))
            .when(fold, |s| {
                s.cursor_pointer().on_click(
                    // A double click folds or opens it once, as one click does.
                    cx.listener(move |this, e: &ClickEvent, _, cx| {
                        if e.click_count() < 2 {
                            this.fold_section(which, cx)
                        }
                    }),
                )
            })
            // The chevron turns as it folds (#56, 1).
            .child(super::motion::toward(
                icons::icon("chevron").size(px(14.)).text_color(if fold {
                    theme::faint()
                } else {
                    theme::panel()
                }),
                (id, 1usize),
                !folded,
                false,
                super::motion::FAST,
                |chevron, v| {
                    chevron.with_transformation(gpui::Transformation::rotate(gpui::radians(
                        -std::f32::consts::FRAC_PI_2 * (1. - v),
                    )))
                },
            ))
            .child(
                div()
                    .flex_none()
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::muted())
                    .child(title.to_uppercase()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .children(summary.filter(|_| folded)),
            )
            .children(end)
    }

    /// The panel shows the versions: a request's page with more than one, not on its commits nor
    /// a branch's.
    fn side_versions(&self) -> bool {
        let page = self.nav_page();
        let tabbed = !self.item_sections().is_empty();
        let commit_level = matches!(
            page,
            Some(
                Page::RequestCommit(..)
                    | Page::BranchCommit(..)
                    | Page::RequestCommits(..)
                    | Page::BranchCommits(..)
            )
        );
        let branch = tabbed
            && matches!(
                page,
                Some(Page::Branch(_) | Page::BranchCommit(..) | Page::BranchCommits(..))
            );
        self.versions.len() > 1 && !commit_level && !branch
    }

    /// The panel shows the commits: on a page without a Commits tab (the main line).
    fn side_commits(&self) -> bool {
        self.item_sections().is_empty()
    }

    /// The panel's sections on this page as icons, where it folds into the menu's column (#80):
    /// its icon, its name, and the zone it gives the keyboard to.
    pub(super) fn side_icons(&self) -> Vec<(&'static str, &'static str, Option<Zone>)> {
        if self.settings_open || self.zen.is_some() {
            return Vec::new();
        }
        match self.nav.space {
            super::spaces::Space::Code if self.root.is_some() => {
                vec![("folder", "Files", None)]
            }
            super::spaces::Space::Branches | super::spaces::Space::Requests
                if self.nav_page().is_some()
                    && !matches!(
                        self.page_tab(),
                        Some(super::nav::Section::Overview | super::nav::Section::Commits)
                    ) =>
            {
                let mut icons = Vec::new();
                if self.side_versions() {
                    icons.push(("layers", "Versions", None));
                }
                if self.side_commits() && !self.commits.is_empty() {
                    icons.push(("commit", "Commits", Some(Zone::Commits)));
                }
                if self.diff.as_ref().is_some_and(|d| !d.files.is_empty()) {
                    icons.push(("files", "Changes", Some(Zone::Files)));
                }
                icons
            }
            _ => Vec::new(),
        }
    }

    /// The side panel; None when it is hidden (⌘J, zen).
    pub(super) fn render_side(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        // Hidden a moment ago (⌘J, zen), it is drawn on its way out (#56, 4).
        if !self.layout.show_sidebar && !self.motion.recent("side") && !self.side_drawer() {
            return None;
        }
        let keyboard = matches!(self.zone, Zone::Commits | Zone::Files);
        let mut open_lists = 0;
        let mut sections: Vec<AnyElement> = Vec::new();
        let rule = || div().flex_none().h(px(1.)).bg(theme::island_border());

        // A request's or a branch's commits are its Commits tab, a commit of it a level there: on
        // its pages the panel holds the changes — a request's with its versions — and on a commit
        // only the commit's (#73, #74).
        if self.side_versions() {
            let folded = self.section_folded(Section::Versions);
            let summary = match self.selection {
                Selection::Versions { from, to } => format!(
                    "v{} → v{}",
                    self.versions[from].number, self.versions[to].number
                ),
                _ => format!("latest v{}", self.versions[self.versions.len() - 1].number),
            };
            sections.push(
                div()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .child(self.section_header(
                        Section::Versions,
                        "Versions".into(),
                        Some(summary),
                        vec![count(self.versions.len())],
                        true,
                        cx,
                    ))
                    // Opening and folding alike, to its own height and back (#110); the padding
                    // inside, so nothing is left of it at the end.
                    .children(
                        self.folding(
                            "versions",
                            "fold-0",
                            !folded,
                            div()
                                .id("versions-list")
                                .max_h(px(240.))
                                .overflow_y_scroll()
                                .child(div().pb(px(6.)).child(self.version_rows(cx))),
                        ),
                    )
                    .into_any_element(),
            );
        }

        if self.side_commits() {
            let commits_folded = self.section_folded(Section::Commits);
            let first = match self.selection {
                Selection::Commit(ix) => self.commits.get(ix),
                _ => self.commits.first(),
            };
            let commit_summary = first.map(|c| {
                if self.commits.len() > 1 {
                    format!("{} +{}", c.summary, self.commits.len() - 1)
                } else {
                    c.summary.clone()
                }
            });
            let mut commit_end: Vec<AnyElement> = Vec::new();
            commit_end.extend(
                self.render_working_chip(cx)
                    .map(IntoElement::into_any_element),
            );
            commit_end.push(
                commit_chip("commit-search", self.find.text.is_some())
                    .px(px(5.))
                    .child(icons::icon("search").size(px(15.)).text_color(
                        if self.find.text.is_some() {
                            theme::text()
                        } else {
                            theme::muted()
                        },
                    ))
                    .tooltip_show_delay(crate::ui::hover::LONG)
                    .tooltip(|_, cx| cx.new(|_| Tip("Search commits  ⌘⇧F")).into())
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        if this.layout.fold_commits {
                            this.fold_section(1, cx);
                        }
                        this.start_find(cx)
                    }))
                    .into_any_element(),
            );
            commit_end.push(count(self.commits.len()));
            if !sections.is_empty() {
                sections.push(rule().into_any_element());
            }
            if !commits_folded {
                open_lists += 1;
            }
            let commits_rows = !commits_folded || self.motion.recent("fold-1");
            let commits = div()
                .relative()
                .flex()
                .flex_col()
                .overflow_hidden()
                .child(self.widths.measure("side-commits"))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.set_zone(Zone::Commits, cx)),
                )
                .child(self.section_header(
                    Section::Commits,
                    format!("Commits{}", self.zone_tag(Zone::Commits)),
                    commit_summary,
                    commit_end,
                    true,
                    cx,
                ))
                .when(commits_rows, |s| {
                    s.children(self.find.text.is_some().then(|| self.render_find(cx)))
                        .children(self.render_pill(cx))
                        .child(if self.find.shown.as_ref().is_some_and(Vec::is_empty) {
                            self.render_no_match().into_any_element()
                        } else {
                            uniform_list(
                                "commits",
                                self.commit_rows(),
                                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                    range
                                        .map(|row| this.render_commit_row(this.commit_at(row), cx))
                                        .collect::<Vec<_>>()
                                }),
                            )
                            .track_scroll(&self.commit_scroll)
                            .flex_1()
                            .pb(px(6.))
                            .into_any_element()
                        })
                });
            sections.push(open_share(commits, "commits-open", !commits_folded).into_any_element());
        }

        if let Some(diff) = self.diff.as_ref().filter(|d| !d.files.is_empty()) {
            let one = diff.files.len() == 1;
            let folded = one || self.section_folded(Section::Changes);
            let summary = if one {
                Some(diff.files[0].path().to_owned())
            } else {
                Some(match self.review_progress() {
                    Some((done, total)) => format!("{done} / {total} reviewed"),
                    None => plural(diff.files.len(), "file"),
                })
            };
            let list = self.options.list_files;
            let mut end: Vec<AnyElement> = Vec::new();
            if !folded {
                end.push(
                    super::diff_view::group()
                        // A chip is not the heading: a click on it does not fold the section.
                        .child(chip("files-tree", "Tree", !list).on_click(cx.listener(
                            |this, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                this.set_file_view(false, cx)
                            },
                        )))
                        .child(chip("files-list", "List", list).on_click(cx.listener(
                            |this, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                this.set_file_view(true, cx)
                            },
                        )))
                        .into_any_element(),
                );
            }
            end.push(count(diff.files.len()));
            let done = self.all_reviewed();
            if !folded {
                open_lists += 1;
            }
            sections.push(rule().into_any_element());
            let rows = !folded || (!one && self.motion.recent("fold-2"));
            let changes = div()
                .flex()
                .flex_col()
                .overflow_hidden()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        if this.files_zone() {
                            this.set_zone(Zone::Files, cx)
                        }
                    }),
                )
                .child(self.section_header(
                    Section::Changes,
                    format!(
                        "Changes{}{}",
                        if done { " ✓" } else { "" },
                        self.zone_tag(Zone::Files)
                    ),
                    summary,
                    end,
                    !one,
                    cx,
                ))
                .when(rows, |s| s.child(self.render_changes(cx)));
            sections.push(open_share(changes, "changes-open", !folded).into_any_element());
        }

        Some(
            island()
                .w(px(self.layout.sidebar))
                .flex_none()
                .h_full()
                .border_2()
                .border_color(if keyboard {
                    theme::splitter()
                } else {
                    theme::island_border()
                })
                .children(sections)
                // Everything folded: the sections sit at the top.
                .when(open_lists == 0, |s| s.child(div().flex_1())),
        )
    }
}

/// A section that shares the panel's height with the other open one: its share grows from nothing
/// (its header alone) as it opens, and back as it folds (#56, 1).
fn open_share(section: gpui::Div, id: &'static str, open: bool) -> impl IntoElement {
    super::motion::toward(section, id, open, false, super::motion::BASE, |s, v| {
        s.flex_grow(v)
            .flex_shrink(1.)
            .flex_basis(px(0.))
            .min_h(px(super::motion::mix(HEADER, 120., v)))
    })
}

/// A section's count, at the right of its header.
fn count(n: usize) -> AnyElement {
    div()
        .flex_none()
        .text_size(px(11.))
        .text_color(theme::faint())
        .child(SharedString::from(n.to_string()))
        .into_any_element()
}
