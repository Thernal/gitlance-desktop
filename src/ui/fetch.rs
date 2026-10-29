//! Background fetch: on for every repository until turned off for it. It runs `git fetch` through
//! `git::fetch` when the window gains focus and every few minutes; the auto-refresh then sees the
//! moved refs and marks new versions.

use super::hover::CutInTip;
use super::px;
use super::{Workspace, format, theme};
use crate::storage;
use gpui::{ClickEvent, Context, Task, div, prelude::*};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const INTERVAL: Duration = Duration::from_secs(300);
/// A focus gain fetches only when the last fetch is older than this.
const FOCUS_AFTER: i64 = 60;

#[derive(Default)]
pub struct Fetch {
    pub enabled: bool,
    running: bool,
    /// Seconds since the epoch of the last successful fetch.
    last: Option<i64>,
    error: Option<String>,
    task: Option<Task<()>>,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl Workspace {
    /// Called when a repository opens: reads whether it is turned off and starts the timer.
    pub(super) fn start_fetching(&mut self, cx: &mut Context<Self>) {
        // A plain folder, or a repository with no remote, has nothing to fetch.
        let enabled = !self.plain
            && self.root.as_deref().is_some_and(|root| {
                storage::fetch_enabled(root)
                    && crate::git::Repo::open(root)
                        .ok()
                        .and_then(|r| r.web_remote_name())
                        .is_some()
            });
        self.fetch = Fetch {
            enabled,
            ..Fetch::default()
        };
        if enabled {
            self.fetch.task = Some(cx.spawn(async move |this, cx| {
                loop {
                    if this.update(cx, |this, cx| this.fetch_now(cx)).is_err() {
                        return;
                    }
                    cx.background_executor().timer(INTERVAL).await;
                }
            }));
        }
        cx.notify();
    }

    pub(super) fn set_fetch_enabled(&mut self, on: bool, cx: &mut Context<Self>) {
        if let Some(root) = &self.root {
            storage::set_fetch_enabled(root, on);
        }
        self.start_fetching(cx);
    }

    /// The window came back to the front.
    pub(super) fn window_activated(&mut self, cx: &mut Context<Self>) {
        self.load_notices(cx);
        if self.fetch.enabled && self.fetch.last.is_none_or(|t| now() - t > FOCUS_AFTER) {
            self.fetch_now(cx);
        }
    }

    pub(super) fn fetch_now(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        if !self.fetch.enabled || self.fetch.running {
            return;
        }
        self.fetch.running = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { crate::git::fetch::all(&root) })
                .await;
            this.update(cx, |this, cx| {
                this.fetch.running = false;
                match result {
                    Ok(()) => {
                        this.fetch.last = Some(now());
                        this.fetch.error = None;
                    }
                    Err(e) => this.fetch.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The title bar's word on the fetch: when it last ran, or why it failed.
    pub(super) fn render_fetch(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        if !self.fetch.enabled {
            return None;
        }
        let (text, color) = match (&self.fetch.error, self.fetch.running, self.fetch.last) {
            (Some(e), _, _) => (format!("Fetch failed: {e}  ·  Retry"), theme::removed()),
            (None, true, _) => ("Fetching…".to_owned(), theme::muted()),
            (None, false, Some(t)) => (format!("Fetched {}", format::ago(t)), theme::muted()),
            (None, false, None) => ("Fetch waiting".to_owned(), theme::muted()),
        };
        // A long hover: the whole message, when it last fetched and from where (X59).
        let mut lines: Vec<String> = Vec::new();
        if let Some(e) = &self.fetch.error {
            lines.push(format!("Fetch failed: {e}"));
        }
        lines.push(match self.fetch.last {
            Some(t) => format!("Last fetched {}", super::hover::full_date(t)),
            None => "Not fetched yet".to_owned(),
        });
        lines.push(match &self.web {
            Some(w) => format!(
                "git fetch --all --prune: every remote, origin at {}",
                w.base
            ),
            None => "git fetch --all --prune: every remote".to_owned(),
        });
        lines.push("Every 5 minutes and when the window comes back · a click fetches now".into());
        let tip: gpui::SharedString = lines.join("\n").into();
        Some(
            div()
                .id("fetch-status")
                .flex_none()
                .max_w(px(420.))
                .cut_in_tip()
                .h(px(24.))
                .flex()
                .items_center()
                .px_2()
                .rounded(px(super::ROW_RADIUS))
                .text_size(px(11.))
                .text_color(color)
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .tooltip_show_delay(super::hover::LONG)
                .tooltip(move |_, cx| cx.new(|_| super::TipOwned(tip.clone())).into())
                .child(text)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.fetch_now(cx))),
        )
    }
}
