//! A host without a token, connected without a paste (#142, `crate::connect`): what git keeps is
//! looked for once a run, in the background, and used when the host takes it; else the connect
//! panel offers once to make a token with the SSH key — Not now, Read only, Connect. A token
//! GitLance made is renewed a few days before its end. Settings → Merge requests makes one later
//! (Use SSH key).

use super::discussion::{self, Kind};
use super::{Workspace, px, theme};
use crate::connect::{self, Known, Refused, Ssh};
use gpui::{ClickEvent, Context, Div, FontWeight, SharedString, Stateful, div, prelude::*};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Where connecting a host without a token stands, for the tab's host.
#[derive(Default)]
pub struct Connecting {
    /// Looking for a token in git and asking SSH, now.
    pub looking: bool,
    /// The scope a token is being made with over SSH, now.
    pub making: Option<&'static str>,
    /// Why the last token was not made.
    pub error: Option<String>,
}

/// What looking found, by host, for this run: another tab of the host, or the project opened
/// again, does not ask again.
fn looked() -> &'static Mutex<HashMap<String, connect::Looked>> {
    static LOOKED: OnceLock<Mutex<HashMap<String, connect::Looked>>> = OnceLock::new();
    LOOKED.get_or_init(Default::default)
}

fn looked_for(host: &str) -> Option<connect::Looked> {
    looked().lock().ok()?.get(host).cloned()
}

/// Hosts whose token GitLance made was renewed (or tried) this run.
fn renewed() -> &'static Mutex<Vec<String>> {
    static RENEWED: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    RENEWED.get_or_init(Default::default)
}

/// What a token made over SSH is named on the host.
fn token_name() -> String {
    format!("GitLance ({})", crate::platform::computer_name())
}

impl Workspace {
    /// The remote's URL as git has it, and where ssh reaches its host.
    fn ssh_of(&self) -> Option<(String, Ssh)> {
        let remote = self.web.as_ref()?;
        let repo = crate::git::Repo::open(self.root.as_ref()?).ok()?;
        let url = repo.web_remote_url()?;
        let ssh = Ssh::of(&url, remote);
        Some((url, ssh))
    }

    /// No token for the host (`load_requests`): what git keeps, and who the SSH key is, looked for
    /// once a run; a token the host takes connects at once.
    pub(super) fn look_for_token(&mut self, cx: &mut Context<Self>) {
        // A project opening reads its requests before the tab holds its root: looked for after.
        cx.spawn(async move |this, cx| this.update(cx, |this, cx| this.look_now(cx)).ok())
            .detach();
    }

    fn look_now(&mut self, cx: &mut Context<Self>) {
        let (Some(remote), Some(root)) = (self.web.clone(), self.root.clone()) else {
            return;
        };
        let host = crate::mr::api_host(&remote);
        if looked_for(&host).is_some() || self.connecting.looking {
            return;
        }
        let Some((url, _)) = self.ssh_of() else {
            return;
        };
        self.connecting.looking = true;
        cx.spawn(async move |this, cx| {
            let check = remote.clone();
            let found = cx
                .background_executor()
                .spawn(async move { connect::look(&root, &url, &check) })
                .await;
            if let Ok(mut all) = looked().lock() {
                all.insert(host.clone(), found.clone());
            }
            this.update(cx, |this, cx| {
                this.connecting.looking = false;
                if let Some((who, from)) = found.connected {
                    this.gitlab_check = Some(Ok(who.clone()));
                    this.load_requests(cx);
                    let host = crate::mr::host(&remote);
                    this.toast_info(
                        format!("Connected to {host} as {who}, with the token {from} keeps").into(),
                        cx,
                    );
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Make the token (`api`) or Read only (`read_api`): made over SSH, kept as the host's token,
    /// and the requests read; refused, the panel says why and remembers what the host allows.
    pub(super) fn make_ssh_token(&mut self, scope: &'static str, cx: &mut Context<Self>) {
        let Some(remote) = self.web.clone() else {
            return;
        };
        let Some((_, ssh)) = self.ssh_of() else {
            return;
        };
        if self.connecting.making.is_some() {
            return;
        }
        self.connecting.making = Some(scope);
        self.connecting.error = None;
        self.gitlab_check = None;
        cx.notify();
        let host = crate::mr::api_host(&remote);
        cx.spawn(async move |this, cx| {
            let check = remote.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let name = token_name();
                    let before = connect::note(&host).made;
                    let made = connect::make(&ssh, &name, scope)?;
                    // One GitLance made before goes, as a renewed one's does; one pasted is yours.
                    if before.is_some_and(|m| crate::mr::token(&check).is_some_and(|t| m.is(&t))) {
                        let _ = crate::mr::revoke_token(&check);
                    }
                    crate::mr::save_token(&check, &made.token)
                        .map_err(|e| Refused::Other(format!("{e:#}")))?;
                    connect::set_note(&host, |n| {
                        n.declined = false;
                        n.made = Some(connect::MadeNote::of(&made, &name, scope));
                    });
                    Ok(crate::mr::me(&check).map_err(|e| format!("{e:#}")))
                })
                .await;
            this.update(cx, |this, cx| {
                this.connecting.making = None;
                let host = crate::mr::api_host(&remote);
                match result {
                    Ok(who) => {
                        this.gitlab_check = Some(who);
                        this.load_requests(cx);
                    }
                    Err(Refused::Off) => {
                        connect::set_note(&host, |n| n.ssh_off = true);
                        this.connecting.error = Some(format!(
                            "{} does not make tokens over SSH: paste one instead.",
                            crate::mr::host(&remote)
                        ));
                    }
                    Err(Refused::Scopes(allowed)) => {
                        let read = allowed.iter().any(|s| s == "read_api");
                        connect::set_note(&host, |n| n.allowed = allowed);
                        this.connecting.error = Some(if read {
                            format!(
                                "{} makes only read-only tokens over SSH.",
                                crate::mr::host(&remote)
                            )
                        } else {
                            format!(
                                "{} makes no token GitLance can use over SSH: paste one instead.",
                                crate::mr::host(&remote)
                            )
                        });
                    }
                    Err(Refused::Other(why)) => {
                        this.connecting.error = Some(format!("No token made: {why}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Not now: not offered again for this host; Settings → Merge requests makes one later.
    fn decline_ssh_token(&mut self, cx: &mut Context<Self>) {
        if let Some(remote) = &self.web {
            connect::set_note(&crate::mr::api_host(remote), |n| n.declined = true);
        }
        cx.notify();
    }

    /// The token GitLance made, read with the requests (`load_requests`): a few days before its end,
    /// a new one over SSH with the same scope, the old one revoked — once a run, quietly; refused
    /// or unreachable, the next run tries again while the old one still works.
    pub(super) fn renew_ssh_token(&mut self, cx: &mut Context<Self>) {
        let Some(remote) = self.web.clone() else {
            return;
        };
        let host = crate::mr::api_host(&remote);
        let Some(made) = connect::note(&host).made else {
            return;
        };
        let in_use = crate::mr::token(&remote).is_some_and(|t| made.is(&t));
        if !in_use || !made.due(connect::today()) {
            return;
        }
        let Some((_, ssh)) = self.ssh_of() else {
            return;
        };
        match renewed().lock() {
            Ok(mut done) if !done.contains(&host) => done.push(host.clone()),
            _ => return,
        }
        cx.spawn(async move |this, cx| {
            let renewed = cx
                .background_executor()
                .spawn(async move {
                    let fresh = connect::make(&ssh, &made.name, &made.scope).ok()?;
                    // The old one goes with its own call, before the new one takes its place.
                    let _ = crate::mr::revoke_token(&remote);
                    crate::mr::save_token(&remote, &fresh.token).ok()?;
                    connect::set_note(&host, |n| {
                        n.made = Some(connect::MadeNote::of(&fresh, &made.name, &made.scope));
                    });
                    Some(fresh.expires)
                })
                .await;
            if renewed.is_some() {
                this.update(cx, |_, cx| cx.notify()).ok();
            }
        })
        .detach();
    }

    /// The connect panel offers a token made over SSH (in the paste steps' place): GitLab knows the
    /// key, and it was neither declined nor refused.
    pub(super) fn ssh_offering(&self) -> bool {
        let Some(remote) = &self.web else {
            return false;
        };
        let host = crate::mr::api_host(remote);
        let note = connect::note(&host);
        let may = |scope: &str| note.allowed.is_empty() || note.allowed.iter().any(|s| s == scope);
        matches!(looked_for(&host).map(|l| l.ssh), Some(Ok(Known::GitLab(_))))
            && !note.declined
            && !note.ssh_off
            && (may("api") || may("read_api"))
    }

    /// Over the paste steps of the connect panel: a line while looking; the card offering a token
    /// made over SSH — what it is on the left, its actions on the right, what each makes in its
    /// tooltip; or a line on why SSH cannot. `None` when there is nothing to say.
    pub(super) fn render_ssh_offer(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let remote = self.web.as_ref()?;
        let host = crate::mr::host(remote);
        let line = |text: String| {
            div()
                .max_w(px(420.))
                .text_size(px(12.))
                .text_center()
                .text_color(theme::faint())
                .child(text)
        };
        if self.connecting.looking {
            return Some(line("Checking git and your SSH key…".to_owned()).into_any_element());
        }
        let looked = looked_for(&crate::mr::api_host(remote))?;
        let note = connect::note(&crate::mr::api_host(remote));
        let refused = looked
            .refused
            .map(|from| line(format!("{host} refused the token {from} keeps.")));
        // What the host lets SSH make: everything, or what it said.
        let may = |scope: &str| note.allowed.is_empty() || note.allowed.iter().any(|s| s == scope);
        let (write, read) = (may("api"), may("read_api"));
        let user = match &looked.ssh {
            Ok(Known::GitLab(user)) if !note.ssh_off && !note.declined && (write || read) => {
                user.clone()
            }
            // Not now, or GitHub (its text says gh auth login): nothing more to say.
            other => {
                let why = match other {
                    Ok(Known::GitLab(_)) if note.ssh_off => {
                        Some(format!("{host} does not make tokens over SSH."))
                    }
                    Err(why) if !why.is_empty() => Some(format!("{why}.")),
                    _ => None,
                };
                let lines: Vec<_> = refused.into_iter().chain(why.map(line)).collect();
                return (!lines.is_empty()).then(|| {
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(2.))
                        .children(lines)
                        .into_any_element()
                });
            }
        };
        let making = self.connecting.making;
        let (name, days) = (token_name(), connect::DAYS);
        let connect = discussion::button(
            "ssh-make",
            match (making, write) {
                (Some(_), _) => "Connecting…",
                (None, true) => "Connect",
                (None, false) => "Connect read-only",
            },
            None,
            Kind::Primary(theme::accent()),
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.make_ssh_token(if write { "api" } else { "read_api" }, cx)
        }));
        let connect = tipped(
            connect,
            if write {
                format!(
                    "Makes an api token “{name}” with your SSH key, for {days} days; GitLance renews it."
                )
            } else {
                format!(
                    "Makes a read_api token “{name}” with your SSH key, for {days} days; {host} makes only read-only ones over SSH."
                )
            },
        );
        let buttons = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(tipped(
                discussion::button("ssh-decline", "Not now", None, Kind::Quiet).on_click(
                    cx.listener(|this, _: &ClickEvent, _, cx| this.decline_ssh_token(cx)),
                ),
                "Paste a token instead; Settings → Merge requests can still use your SSH key."
                    .to_owned(),
            ))
            .when(write && read, |row| {
                row.child(tipped(
                    discussion::button("ssh-make-read", "Read only", None, Kind::Plain).on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.make_ssh_token("read_api", cx)
                        }),
                    ),
                    format!(
                        "Makes a read_api token “{name}” instead: GitLance reads, never writes."
                    ),
                ))
            })
            .child(connect);
        let card = div()
            .w_full()
            .max_w(px(600.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .py(px(12.))
            .pl(px(16.))
            .pr(px(12.))
            .rounded(px(10.))
            .bg(theme::panel())
            .border_1()
            .border_color(theme::island_border())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        super::icons::icon("key")
                            .flex_none()
                            .size(px(18.))
                            .text_color(theme::muted()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Connect with your SSH key"),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.))
                                    .text_color(theme::muted())
                                    .child(format!("GitLab knows you as @{user}")),
                            ),
                    )
                    .child(buttons),
            )
            .children(self.connecting.error.clone().map(|e| {
                div()
                    .text_size(px(12.))
                    .text_color(theme::removed())
                    .child(e)
            }));
        Some(
            div()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(8.))
                .children(refused)
                .child(card)
                .into_any_element(),
        )
    }

    /// The token GitLance made, while it is the one in use.
    fn made_in_use(&self) -> Option<connect::MadeNote> {
        let remote = self.web.as_ref()?;
        let made = connect::note(&crate::mr::api_host(remote)).made?;
        crate::mr::token(remote)
            .is_some_and(|t| made.is(&t))
            .then_some(made)
    }

    /// Settings → Merge requests (GitLab): Use SSH key beside Use token from clipboard, while the
    /// token in use is not one GitLance made to write and the host makes one over SSH.
    pub(super) fn ssh_settings_chip(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let remote = self.web.as_ref().filter(|r| !r.github)?;
        let note = connect::note(&crate::mr::api_host(remote));
        let may_write = note.allowed.is_empty() || note.allowed.iter().any(|s| s == "api");
        let made = self.made_in_use();
        if note.ssh_off || !may_write || made.is_some_and(|m| m.scope == "api") {
            return None;
        }
        let chip = super::diff_view::chip("gitlab-ssh", "Use SSH key", false)
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.make_ssh_token("api", cx)));
        Some(tipped(
            chip,
            format!(
                "Makes an api token “{}” with your SSH key, for {} days; GitLance renews it.",
                token_name(),
                connect::DAYS
            ),
        ))
    }

    /// What Settings says while a token is made over SSH, or why it was not.
    pub(super) fn ssh_settings_busy(&self) -> Option<(String, gpui::Rgba)> {
        if self.connecting.making.is_some() {
            return Some((
                "Making a token with your SSH key…".to_owned(),
                theme::muted(),
            ));
        }
        self.connecting.error.clone().map(|e| (e, theme::removed()))
    }

    /// What Settings says of the token in use when GitLance made it.
    pub(super) fn ssh_settings_made(&self) -> Option<String> {
        let made = self.made_in_use()?;
        Some(match made.expires {
            Some(day) => format!("SSH token · {} · expires {day}", made.scope),
            None => format!("SSH token · {}", made.scope),
        })
    }
}

/// `button` with `tip` shown when hovered.
fn tipped(button: Stateful<Div>, tip: String) -> Stateful<Div> {
    let tip = SharedString::from(tip);
    button.tooltip(move |_, cx| cx.new(|_| super::TipOwned(tip.clone())).into())
}
