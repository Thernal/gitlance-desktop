//! The side panel of the redesign (Design `design/shell/`, #49): one island of sections one under
//! the other — Versions, Commits, Changes — each folding to a header that still says what is in it
//! (a count, the first item, how far the review is). An open Commits or Changes takes the room left;
//! Versions takes what its rows need. Folds are kept in `layout.txt`; ⌥⌘2 folds Changes.

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
        self.layout.save();
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
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.fold_section(which, cx)),
                )
            })
            .child(
                icons::icon(if folded { "chevron-right" } else { "chevron" })
                    .size(px(14.))
                    .text_color(if fold { theme::faint() } else { theme::panel() }),
            )
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

    /// The side panel; None when it is hidden (⌥⌘1, zen).
    pub(super) fn render_side(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        if !self.layout.show_sidebar {
            return None;
        }
        let keyboard = matches!(self.zone, Zone::Commits | Zone::Files);
        let mut open_lists = 0;
        let mut sections: Vec<AnyElement> = Vec::new();
        let rule = || div().flex_none().h(px(1.)).bg(theme::island_border());

        if self.versions.len() > 1 {
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
                    .children((!folded).then(|| {
                        div()
                            .id("versions-list")
                            .max_h(px(240.))
                            .overflow_y_scroll()
                            .pb(px(6.))
                            .child(self.version_rows(cx))
                    }))
                    .into_any_element(),
            );
        }

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
            self.render_request_chip(cx)
                .map(IntoElement::into_any_element),
        );
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
                .tooltip(|_, cx| cx.new(|_| Tip("Search commits  ⌘⇧F")).into())
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
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
        sections.push(
            div()
                .when(!commits_folded, |s| s.flex_1().min_h(px(120.)))
                .when(commits_folded, |s| s.flex_none())
                .flex()
                .flex_col()
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
                .when(!commits_folded, |s| {
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
                })
                .into_any_element(),
        );

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
                        .child(chip("files-tree", "Tree", !list).on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.set_file_view(false, cx)
                            }),
                        ))
                        .child(chip("files-list", "List", list).on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| this.set_file_view(true, cx)),
                        ))
                        .into_any_element(),
                );
            }
            end.push(count(diff.files.len()));
            let done = self.all_reviewed();
            if !folded {
                open_lists += 1;
            }
            sections.push(rule().into_any_element());
            sections.push(
                div()
                    .when(!folded, |s| s.flex_1().min_h(px(120.)))
                    .when(folded, |s| s.flex_none())
                    .flex()
                    .flex_col()
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
                    .when(!folded, |s| s.child(self.render_changes(cx)))
                    .into_any_element(),
            );
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

/// A section's count, at the right of its header.
fn count(n: usize) -> AnyElement {
    div()
        .flex_none()
        .text_size(px(11.))
        .text_color(theme::faint())
        .child(SharedString::from(n.to_string()))
        .into_any_element()
}
