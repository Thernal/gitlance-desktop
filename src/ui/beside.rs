//! What is beside a folder of Code's path (#132, B): a click on a folder's name lists, under it,
//! what is in the folder above — folders first, then files, the one on the path saying "here".
//! Its first row opens the folder's own page. Typing filters it, ↑ ↓ pick, ↵ opens, esc closes; a
//! folder chosen opens its page, a file its tab. It holds the keyboard while it is open, and a
//! press anywhere else only closes it.

use super::input::{self, Edit};
use super::{ISLAND_RADIUS, Workspace, icons, px, theme};
use gpui::{
    AnyElement, ClickEvent, Context, KeyDownEvent, MouseButton, SharedString, div, prelude::*,
};

/// The list, open under a folder of the path.
pub struct Beside {
    /// The folder whose name opened it.
    pub folder: String,
    query: String,
    /// The row picked: 0 the folder's own page, then the entries shown; None the folder among
    /// them until a key moves it (the files may still be on their way).
    at: Option<usize>,
    scroll: gpui::ScrollHandle,
}

/// What is in the folder above `folder`, from `files` (the project's, sorted): its folders, then
/// its files, each by its path and whether it is a folder.
pub(super) fn beside(files: &[String], folder: &str) -> Vec<(String, bool)> {
    let parent = folder.rsplit_once('/').map_or("", |(p, _)| p);
    let prefix = if parent.is_empty() {
        String::new()
    } else {
        format!("{parent}/")
    };
    let mut folders = std::collections::BTreeSet::new();
    let mut here = Vec::new();
    for f in files.iter().filter_map(|f| f.strip_prefix(&prefix)) {
        match f.split_once('/') {
            Some((dir, _)) => {
                folders.insert(format!("{prefix}{dir}"));
            }
            None => here.push(format!("{prefix}{f}")),
        }
    }
    let folders = folders.into_iter().map(|d| (d, true));
    folders
        .chain(here.into_iter().map(|f| (f, false)))
        .collect()
}

/// The entries whose names hold `query`, in any case.
fn shown(entries: Vec<(String, bool)>, query: &str) -> Vec<(String, bool)> {
    let query = query.trim().to_lowercase();
    entries
        .into_iter()
        .filter(|(path, _)| name(path).to_lowercase().contains(&query))
        .collect()
}

fn name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, n)| n)
}

impl Workspace {
    /// The list under folder `folder`'s name, the folder itself picked.
    pub(super) fn open_beside(&mut self, folder: String, cx: &mut Context<Self>) {
        self.reset_field_edit();
        self.beside = Some(Beside {
            folder,
            query: String::new(),
            at: None,
            scroll: gpui::ScrollHandle::new(),
        });
        cx.notify();
    }

    /// The row picked.
    fn beside_at(&self, rows: &[(String, bool)]) -> usize {
        let Some(b) = &self.beside else {
            return 0;
        };
        b.at.unwrap_or_else(|| {
            rows.iter()
                .position(|(path, _)| *path == b.folder)
                .map_or(0, |i| i + 1)
        })
    }

    /// Row `at` picked, scrolled to: a line parts the folder's own row from the entries.
    fn pick_row(&mut self, at: usize) {
        if let Some(b) = self.beside.as_mut() {
            b.at = Some(at);
            b.scroll.scroll_to_item(if at == 0 { 0 } else { at + 1 });
        }
    }

    fn close_beside(&mut self, cx: &mut Context<Self>) {
        self.beside = None;
        cx.notify();
    }

    /// The entries shown, as filtered.
    fn beside_rows(&self) -> Vec<(String, bool)> {
        let Some(b) = &self.beside else {
            return Vec::new();
        };
        shown(beside(self.code.files(), &b.folder), &b.query)
    }

    pub(super) fn beside_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(b) = self.beside.as_mut() else {
            return;
        };
        match input::edit(&mut b.query, &mut self.field_edit, &event.keystroke, cx) {
            Edit::Changed => {
                // The first entry that holds what is typed, else the folder's own row.
                let any = !self.beside_rows().is_empty();
                self.pick_row(usize::from(any));
            }
            Edit::Enter { .. } => {
                let at = self.beside_at(&self.beside_rows());
                return self.pick_beside(at, cx);
            }
            Edit::Escape => self.beside = None,
            Edit::Selected | Edit::Ignored => {}
        }
        cx.notify();
    }

    /// ↑ ↓ : the row before or after, the folder's own row first.
    pub(super) fn step_beside(&mut self, down: bool, cx: &mut Context<Self>) {
        let rows = self.beside_rows();
        let at = self.beside_at(&rows);
        self.pick_row(if down {
            (at + 1).min(rows.len())
        } else {
            at.saturating_sub(1)
        });
        cx.notify();
    }

    /// Row `at` chosen: the folder's page, or the entry's — a folder's page, a file's tab.
    fn pick_beside(&mut self, at: usize, cx: &mut Context<Self>) {
        let rows = self.beside_rows();
        let Some(b) = self.beside.take() else {
            return;
        };
        self.nav.from = None;
        match at.checked_sub(1).and_then(|i| rows.get(i)) {
            None => self.open_folder_page(b.folder, cx),
            Some((path, true)) => self.open_folder_page(path.clone(), cx),
            Some((path, false)) => self.open_code_file(path.clone(), true, cx),
        }
        cx.notify();
    }

    /// Behind the list, over everything else: a press there only closes it.
    pub(super) fn render_beside_backdrop(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.beside.as_ref()?;
        Some(
            div()
                .id("beside-backdrop")
                .absolute()
                .size_full()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.close_beside(cx)),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|this, _, _, cx| this.close_beside(cx)),
                )
                .into_any_element(),
        )
    }

    /// The list under `folder`'s name, while it is the one open.
    pub(super) fn render_beside(&self, folder: &str, cx: &Context<Self>) -> Option<AnyElement> {
        let b = self.beside.as_ref().filter(|b| b.folder == folder)?;
        let rows = self.beside_rows();
        let at = self.beside_at(&rows);
        let up = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                icons::icon("corner-left-up")
                    .size(px(14.))
                    .text_color(theme::muted()),
            )
            .child(
                super::hover::cut("beside-open", format!("Open {}", name(folder)))
                    .flex_1()
                    .min_w_0(),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(11.))
                    .text_color(theme::faint())
                    .child("the folder\u{2019}s page"),
            );
        let row = |i: usize, body: gpui::Div| {
            super::row(SharedString::from(format!("beside-{i}")), i == at)
                .h(px(26.))
                .flex_none()
                .child(body.flex_1().min_w_0())
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.pick_beside(i, cx)))
        };
        let entries = rows.iter().enumerate().map(|(i, (path, folder_here))| {
            let icon = if *folder_here {
                icons::folder_icon(path, 14.)
            } else {
                icons::file_icon(path, 14.)
            };
            row(
                i + 1,
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icon)
                    .child(
                        super::hover::cut(
                            SharedString::from(format!("beside-name-{i}")),
                            name(path).to_owned(),
                        )
                        .flex_1()
                        .min_w_0(),
                    )
                    .children((path == folder).then(|| {
                        div()
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(theme::faint())
                            .child("here")
                    })),
            )
        });
        let line = || div().my(px(4.)).h(px(1.)).bg(theme::island_border());
        let list = div()
            .occlude()
            .w(px(260.))
            .flex()
            .flex_col()
            .p(px(4.))
            .rounded(px(ISLAND_RADIUS))
            .border_1()
            .border_color(theme::island_border())
            .bg(theme::panel())
            .shadow_lg()
            .text_size(px(13.))
            .text_color(theme::text())
            .child(
                div()
                    .h(px(28.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        icons::icon("search")
                            .size(px(14.))
                            .text_color(theme::faint()),
                    )
                    .child(self.field_text(&b.query, true, "type to filter", cx)),
            )
            .child(line())
            .child(
                div()
                    .id("beside-rows")
                    .max_h(px(320.))
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .track_scroll(&b.scroll)
                    .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                    .child(row(0, up))
                    .child(line().flex_none())
                    .children(entries),
            )
            .child(line())
            .child(
                div()
                    .px(px(8.))
                    .py(px(2.))
                    .text_size(px(11.))
                    .text_color(theme::faint())
                    .child("↑↓ ↵ · esc closes"),
            );
        Some(
            div()
                .absolute()
                .left_0()
                .bottom_0()
                .size_0()
                .child(
                    gpui::deferred(
                        gpui::anchored()
                            .offset(gpui::point(px(-8.), px(12.)))
                            .snap_to_window_with_margin(px(8.))
                            .child(list),
                    )
                    .with_priority(1),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> Vec<String> {
        [
            "Cargo.toml",
            "README.md",
            "src/main.rs",
            "src/ui/bar.rs",
            "src/ui/code/tabs.rs",
            "tests/smoke.rs",
        ]
        .map(String::from)
        .to_vec()
    }

    #[test]
    fn a_folder_lists_what_is_in_the_folder_above_folders_first() {
        let files = files();
        assert_eq!(
            beside(&files, "src/ui"),
            vec![("src/ui".into(), true), ("src/main.rs".into(), false)]
        );
        assert_eq!(
            beside(&files, "src"),
            vec![
                ("src".into(), true),
                ("tests".into(), true),
                ("Cargo.toml".into(), false),
                ("README.md".into(), false),
            ]
        );
        assert_eq!(
            shown(beside(&files, "src"), "RE"),
            vec![("README.md".into(), false)]
        );
    }
}
