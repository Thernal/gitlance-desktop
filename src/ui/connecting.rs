//! A host without a token (#142, #156, `crate::connect`): the ways in this computer offers, easiest
//! first, each with the account it signs in as and a button named for what it does — Use (the
//! CLI's token, git's), Make a token (the SSH key; read-only under ▾), Copy… (a JetBrains IDE's:
//! macOS asks first), Open ↗ (made by hand, then pasted). ↑↓ move the focus between them, ↵
//! presses the focused one's button, ⌘V pastes a token. The way picked is remembered per host and
//! the next run goes straight to it; Settings → Merge requests lists the same ways to change it.
//! A token GitLance made is renewed a few days before its end.

use super::discussion::{self, Kind};
use super::{Workspace, px, theme};
use crate::connect::{self, Known, NotHere, Refused, Via, Way};
use crate::git::WebRemote;
use gpui::{AnyElement, ClickEvent, Context, FontWeight, SharedString, Stateful, div, prelude::*};
use gpui::{Div, Rgba};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Where connecting the tab's host stands.
#[derive(Default)]
pub struct Connecting {
    /// Looking on this computer — the CLI, git, the IDEs — now.
    pub looking: bool,
    /// Asking the host who the SSH key is, now.
    pub asking: bool,
    /// Reading the token of the way picked before the requests (`borrow_token`), now.
    pub borrowing: bool,
    /// The scope a token is being made with over SSH, now.
    pub making: Option<&'static str>,
    /// The IDE account whose token is being read — macOS asking — now.
    pub copying: Option<String>,
    /// Why the last way did not connect.
    pub error: Option<String>,
    /// The connect panel's focused row (↑↓).
    pub focus: usize,
    /// The SSH key's ▾ is open: Read only under it.
    pub read_menu: bool,
    /// Settings → Merge requests: the row picked (`row_key`), and the SSH key's scope picked there.
    pub pick: Option<String>,
    pub pick_scope: Option<&'static str>,
    /// The SSH row was shown asking when the rest came in: a no keeps it in place, dimmed, so
    /// nothing moves under the pointer.
    ssh_in_place: bool,
}

/// What looking found for a host this run: another tab of the host, or the project opened again,
/// does not look again.
#[derive(Clone, Default)]
struct Found {
    here: Option<connect::Looked>,
    ssh: Option<Result<Known, String>>,
}

fn found() -> &'static Mutex<HashMap<String, Found>> {
    static FOUND: OnceLock<Mutex<HashMap<String, Found>>> = OnceLock::new();
    FOUND.get_or_init(Default::default)
}

fn found_for(host: &str) -> Found {
    found()
        .lock()
        .ok()
        .and_then(|all| all.get(host).cloned())
        .unwrap_or_default()
}

fn keep(host: &str, change: impl FnOnce(&mut Found)) {
    if let Ok(mut all) = found().lock() {
        change(all.entry(host.to_owned()).or_default());
    }
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

/// One row of the ways: a way, or the SSH key while it asks the host, or its no kept in place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Row {
    Way(Way),
    Asking,
    Failed(String),
}

/// Made by hand on the host's page, then pasted: always there, last.
fn by_hand() -> Way {
    Way {
        via: Via::Hand,
        user: None,
        scopes: Vec::new(),
        write: true,
        expires: None,
    }
}

/// The scope a way is said by: `api` when it has it, else its first.
fn main_scope(scopes: &[String]) -> Option<&str> {
    scopes
        .iter()
        .find(|s| *s == "api")
        .or_else(|| scopes.first())
        .map(String::as_str)
}

/// What Settings remembers a row by while it is picked.
fn row_key(way: &Way) -> String {
    match &way.via {
        Via::Ide(account) => format!("ide:{}", account.id),
        via => via.key().to_owned(),
    }
}

/// Why a token was not copied from an IDE.
enum Uncopied {
    Unread(crate::ide::Unread),
    Refused(String),
}

impl Workspace {
    /// The remote's URL as git has it, and where ssh reaches its host.
    fn ssh_of(&self) -> Option<(String, connect::Ssh)> {
        let remote = self.web.as_ref()?;
        let repo = crate::git::Repo::open(self.root.as_ref()?).ok()?;
        let url = repo.web_remote_url()?;
        let ssh = connect::Ssh::of(&url, remote);
        Some((url, ssh))
    }

    /// No token for the host (`load_requests`), or Settings → Merge requests opened: the ways in
    /// are looked for, once a run.
    pub(super) fn look_for_token(&mut self, cx: &mut Context<Self>) {
        // A project opening reads its requests before the tab holds its root: looked for after.
        cx.spawn(async move |this, cx| this.update(cx, |this, cx| this.look_now(cx)).ok())
            .detach();
    }

    /// The CLI, git and the IDEs in one look; the SSH key apart, as it takes longer.
    fn look_now(&mut self, cx: &mut Context<Self>) {
        let (Some(remote), Some(root)) = (self.web.clone(), self.root.clone()) else {
            return;
        };
        let Some((url, _)) = self.ssh_of() else {
            return;
        };
        let host = crate::mr::api_host(&remote);
        let known = found_for(&host);
        if known.here.is_none() && !self.connecting.looking {
            self.connecting.looking = true;
            let (remote, url, host) = (remote.clone(), url.clone(), host.clone());
            cx.spawn(async move |this, cx| {
                let github = remote.github;
                let looked = cx
                    .background_executor()
                    .spawn(async move { connect::look_here(&root, &url, &remote) })
                    .await;
                keep(&host, |f| f.here = Some(looked));
                this.update(cx, |this, cx| {
                    this.connecting.looking = false;
                    this.connecting.ssh_in_place = !github && found_for(&host).ssh.is_none();
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        if known.ssh.is_none() && !self.connecting.asking {
            self.connecting.asking = true;
            cx.spawn(async move |this, cx| {
                let said = cx
                    .background_executor()
                    .spawn(async move { connect::look_ssh(&url, &remote) })
                    .await;
                keep(&host, |f| f.ssh = Some(said));
                this.update(cx, |this, cx| {
                    this.connecting.asking = false;
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    /// The way picked for the host borrows a token not read yet this run (`load_requests`): it is
    /// read, then the requests are.
    pub(super) fn borrow_token(&mut self, cx: &mut Context<Self>) {
        // As `look_for_token`: the tab holds its root after the first read.
        cx.spawn(async move |this, cx| {
            this.update(cx, |this, cx| {
                let Some(remote) = this.web.clone() else {
                    return;
                };
                if this.connecting.borrowing {
                    return;
                }
                let root = this.root.clone().unwrap_or_else(|| ".".into());
                let url = this.ssh_of().map(|(url, _)| url).unwrap_or_default();
                this.connecting.borrowing = true;
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .spawn(async move { connect::borrow(&root, &url, &remote) })
                        .await;
                    this.update(cx, |this, cx| {
                        this.connecting.borrowing = false;
                        this.load_requests(cx);
                        cx.notify();
                    })
                    .ok();
                })
                .detach();
            })
            .ok()
        })
        .detach();
    }

    /// The ways in to the tab's host, easiest first and by hand last, and those found that cannot
    /// be used; while the CLI, git and the IDEs are being looked in, by hand alone.
    pub(super) fn ways(&self) -> Option<(Vec<Row>, Vec<NotHere>)> {
        let remote = self.web.as_ref()?;
        let host = crate::mr::host(remote);
        let found = found_for(&crate::mr::api_host(remote));
        let note = connect::note(&crate::mr::api_host(remote));
        let (mut ways, mut not) = match &found.here {
            Some(here) => (here.ways.clone(), here.not_here.clone()),
            None => return Some((vec![Row::Way(by_hand())], Vec::new())),
        };
        // What the host lets SSH make: everything, or what it said.
        let may = |scope: &str| note.allowed.is_empty() || note.allowed.iter().any(|s| s == scope);
        let not_ssh = |why: String| NotHere { via: Via::Ssh, why };
        let mut in_place = None;
        match &found.ssh {
            None if !remote.github => in_place = Some(Row::Asking),
            Some(Ok(Known::GitLab(_))) if note.ssh_off => {
                not.push(not_ssh(format!("{host} does not make tokens over SSH")))
            }
            Some(Ok(Known::GitLab(_))) if !may("api") && !may("read_api") => not.push(not_ssh(
                format!("{host} makes no token GitLance can use over SSH"),
            )),
            Some(Ok(Known::GitLab(user))) => ways.push(Way {
                via: Via::Ssh,
                user: Some(user.clone()),
                scopes: vec![if may("api") { "api" } else { "read_api" }.to_owned()],
                write: may("api"),
                expires: None,
            }),
            Some(Err(why)) if self.connecting.ssh_in_place => {
                in_place = Some(Row::Failed(why.clone()))
            }
            Some(Err(why)) if !why.is_empty() => not.push(not_ssh(why.clone())),
            _ => {}
        }
        connect::order(&mut ways);
        let mut rows: Vec<Row> = ways.into_iter().map(Row::Way).collect();
        if let Some(row) = in_place {
            // The SSH key's place: after those there that write, before an IDE's.
            let at = rows
                .iter()
                .position(|r| matches!(r, Row::Way(w) if !w.write || matches!(w.via, Via::Ide(_))))
                .unwrap_or(rows.len());
            rows.insert(at, row);
        }
        rows.push(Row::Way(by_hand()));
        Some((rows, not))
    }

    /// The focused row, on a way.
    fn focused(&self, rows: &[Row]) -> usize {
        self.connecting.focus.min(rows.len().saturating_sub(1))
    }

    /// ↑ ↓ on the connect panel: the focus to the way before or after.
    pub(super) fn step_way(&mut self, down: bool, cx: &mut Context<Self>) {
        let Some((rows, _)) = self.ways() else {
            return;
        };
        let at = self.focused(&rows);
        let next = if down {
            (at + 1..rows.len()).find(|&i| matches!(rows[i], Row::Way(_)))
        } else {
            (0..at).rev().find(|&i| matches!(rows[i], Row::Way(_)))
        };
        if let Some(next) = next {
            self.connecting.focus = next;
            self.connecting.read_menu = false;
        }
        cx.notify();
    }

    /// ↵ on the connect panel with nothing pasted: the focused way's button.
    pub(super) fn press_way(&mut self, cx: &mut Context<Self>) {
        let Some((rows, _)) = self.ways() else {
            return;
        };
        if let Some(Row::Way(way)) = rows.get(self.focused(&rows)).cloned() {
            self.take_way(way, None, cx);
        }
    }

    /// What a way's button does; `scope`, the SSH key's picked in Settings.
    fn take_way(&mut self, way: Way, scope: Option<&'static str>, cx: &mut Context<Self>) {
        self.connecting.read_menu = false;
        match way.via {
            Via::Cli | Via::Git(_) => self.use_borrowed(way, cx),
            Via::Ssh => {
                let scope = scope.unwrap_or(if way.write { "api" } else { "read_api" });
                self.make_ssh_token(scope, cx)
            }
            Via::Ide(account) => self.copy_ide_token(account, cx),
            Via::Hand if !self.token_input.trim().is_empty() => self.submit_token(cx),
            Via::Hand => {
                if let Some(remote) = &self.web {
                    cx.open_url(&crate::mr::token_page(remote));
                }
                self.start_field(super::lists::Field::Token, cx);
            }
        }
    }

    /// Use: the CLI's token or git's, where it is. The one saved for the host stops being used —
    /// GitLance's own revoked, another forgotten.
    fn use_borrowed(&mut self, way: Way, cx: &mut Context<Self>) {
        let Some(remote) = self.web.clone() else {
            return;
        };
        let host = crate::mr::api_host(&remote);
        self.connecting.error = None;
        let made = self.made_in_use().is_some();
        let saved = crate::mr::saved_token(&host).is_some();
        let key = way.via.key();
        let said = match &way.via {
            Via::Git(from) => format!("with the token {from} keeps"),
            _ => format!("with {}’s token", connect::cli_name(remote.github).1),
        };
        cx.spawn(async move |this, cx| {
            let check = remote.clone();
            cx.background_executor()
                .spawn(async move {
                    if made {
                        let _ = crate::mr::revoke_token(&check);
                    }
                    if saved {
                        crate::mr::forget_token(&check);
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                connect::set_note(&host, |n| {
                    n.way = Some(key.to_owned());
                    n.ide = None;
                    if made {
                        n.made = None;
                    }
                });
                this.connected(way.user, said, cx);
            })
            .ok();
        })
        .detach();
    }

    /// A way connected: the requests read, a toast saying with what (its button opens Settings).
    fn connected(&mut self, user: Option<String>, said: String, cx: &mut Context<Self>) {
        self.connecting.error = None;
        self.connecting.focus = 0;
        self.connecting.pick = None;
        self.connecting.pick_scope = None;
        self.gitlab_check = user.as_ref().map(|u| Ok(format!("@{u}")));
        self.load_requests(cx);
        let text = match user {
            Some(user) => format!("Connected as @{user} · {said}"),
            None => format!("Connected · {said}"),
        };
        self.toast_act(super::toast::Act::Connected, 1, move |_| text.clone(), cx);
        cx.notify();
    }

    /// Copy…: the IDE's token read — macOS asks — checked with the host and kept as a pasted one.
    fn copy_ide_token(&mut self, account: crate::ide::Account, cx: &mut Context<Self>) {
        let Some(remote) = self.web.clone() else {
            return;
        };
        if self.connecting.copying.is_some() {
            return;
        }
        self.connecting.copying = Some(account.id.clone());
        self.connecting.error = None;
        cx.notify();
        let host = crate::mr::api_host(&remote);
        let made = self.made_in_use().is_some();
        cx.spawn(async move |this, cx| {
            let (check, read) = (remote.clone(), account.clone());
            let result = cx
                .background_executor()
                .spawn(async move {
                    let token = crate::ide::read(&read).map_err(Uncopied::Unread)?;
                    let access = connect::check(&check, &token).map_err(Uncopied::Refused)?;
                    if made {
                        let _ = crate::mr::revoke_token(&check);
                    }
                    crate::mr::save_token(&check, &token)
                        .map_err(|e| Uncopied::Refused(format!("{e:#}")))?;
                    Ok(access)
                })
                .await;
            this.update(cx, |this, cx| {
                this.connecting.copying = None;
                let product = account.product.clone();
                match result {
                    Ok(access) => {
                        connect::set_note(&host, |n| {
                            n.way = Some("ide".to_owned());
                            n.ide = Some(product.clone());
                            if made {
                                n.made = None;
                            }
                        });
                        let scope = main_scope(&access.scopes)
                            .map(|s| format!(" · {s}"))
                            .unwrap_or_default();
                        let until = access
                            .expires
                            .map(|d| format!(" · until {d}"))
                            .unwrap_or_default();
                        this.connected(
                            Some(access.user),
                            format!("copied {product}’s token{scope}{until}"),
                            cx,
                        );
                    }
                    Err(Uncopied::Unread(crate::ide::Unread::OAuth)) => {
                        connect::set_note(&host, |n| {
                            if !n.oauth.contains(&account.id) {
                                n.oauth.push(account.id.clone());
                            }
                        });
                        // Under the rest from now on.
                        keep(&host, |f| {
                            if let Some(here) = &mut f.here {
                                here.ways.retain(
                                    |w| !matches!(&w.via, Via::Ide(a) if a.id == account.id),
                                );
                                here.not_here.push(NotHere {
                                    via: Via::Ide(account.clone()),
                                    why: connect::OAUTH.to_owned(),
                                });
                            }
                        });
                        this.connecting.error = Some(format!(
                            "{product} is signed in with OAuth, whose token lasts two hours: GitLance does not copy it."
                        ));
                    }
                    Err(Uncopied::Unread(crate::ide::Unread::Denied)) => {
                        this.connecting.error =
                            Some(format!("macOS did not let GitLance read {product}’s token."));
                    }
                    Err(Uncopied::Refused(why)) => {
                        this.connecting.error = Some(format!("{product}’s token: {why}."));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Make a token (`api`) or Read only (`read_api`): made over SSH, kept as the host's token,
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
        let user = match found_for(&host).ssh {
            Some(Ok(Known::GitLab(user))) => Some(user),
            _ => None,
        };
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
                        n.way = Some("ssh".to_owned());
                        n.ide = None;
                        n.made = Some(connect::MadeNote::of(&made, &name, scope));
                    });
                    Ok(name)
                })
                .await;
            this.update(cx, |this, cx| {
                this.connecting.making = None;
                let host = crate::mr::api_host(&remote);
                let at = crate::mr::host(&remote);
                match result {
                    Ok(name) => this.connected(
                        user,
                        format!("made “{name}” on {at}, {scope}, {} days", connect::DAYS),
                        cx,
                    ),
                    Err(Refused::Off) => {
                        connect::set_note(&host, |n| n.ssh_off = true);
                        this.connecting.error = Some(format!(
                            "{at} does not make tokens over SSH: another way, or one made by hand."
                        ));
                    }
                    Err(Refused::Scopes(allowed)) => {
                        let read = allowed.iter().any(|s| s == "read_api");
                        connect::set_note(&host, |n| n.allowed = allowed);
                        this.connecting.error = Some(if read {
                            format!("{at} makes only read-only tokens over SSH.")
                        } else {
                            format!("{at} makes no token GitLance can use over SSH.")
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

    /// The token GitLance made, read with the requests (`load_requests`): a few days before its end,
    /// a new one over SSH with the same scope, the old one revoked — once a run, quietly; refused
    /// or unreachable, the next run tries again while the old one still works.
    pub(super) fn renew_ssh_token(&mut self, cx: &mut Context<Self>) {
        let Some(remote) = self.web.clone() else {
            return;
        };
        let host = crate::mr::api_host(&remote);
        let Some(made) = self.made_in_use() else {
            return;
        };
        if !made.due(connect::today()) {
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

    /// The token GitLance made, while it is the one in use.
    fn made_in_use(&self) -> Option<connect::MadeNote> {
        let remote = self.web.as_ref()?;
        let made = connect::note(&crate::mr::api_host(remote)).made?;
        crate::mr::token_now(remote)
            .is_some_and(|t| made.is(&t))
            .then_some(made)
    }

    /// A row's icon, name, account and what it does, as the panel and Settings say them.
    fn describe(
        &self,
        row: &Row,
        remote: &WebRemote,
    ) -> (&'static str, String, Option<String>, String) {
        let host = crate::mr::host(remote);
        let (cli, program) = connect::cli_name(remote.github);
        let scope_until = |way: &Way| {
            let mut out = main_scope(&way.scopes)
                .map(|s| format!(" · {s}"))
                .unwrap_or_default();
            if let Some(day) = &way.expires {
                out.push_str(&format!(" · until {day}"));
            }
            out
        };
        let way = match row {
            Row::Way(way) => way,
            Row::Asking => {
                return (
                    "key",
                    "Your SSH key".to_owned(),
                    None,
                    format!("asking {host}…"),
                );
            }
            Row::Failed(why) => return ("key", "Your SSH key".to_owned(), None, why.clone()),
        };
        let who = way.user.as_ref().map(|u| format!("@{u}"));
        match &way.via {
            Via::Cli => (
                "terminal",
                cli.to_owned(),
                who,
                format!(
                    "{program} is signed in{} · nothing is asked",
                    scope_until(way)
                ),
            ),
            Via::Git(from) => (
                "branch",
                capital(from),
                who,
                format!("git keeps it{} · nothing is asked", scope_until(way)),
            ),
            Via::Ssh => {
                let what = match self.connecting.making {
                    Some(scope) => format!("making “{}” · {scope}…", token_name()),
                    None => format!(
                        "makes “{}” · {} · {} days, GitLance renews it",
                        token_name(),
                        main_scope(&way.scopes).unwrap_or("api"),
                        connect::DAYS
                    ),
                };
                ("key", "Your SSH key".to_owned(), who, what)
            }
            Via::Ide(account) => {
                let what = if self.connecting.copying.as_deref() == Some(account.id.as_str()) {
                    "macOS is asking for your password — Allow is enough, it is read once"
                } else {
                    "copies its token once · asks for your Mac’s password"
                };
                ("app", account.product.clone(), who, what.to_owned())
            }
            Via::Hand => (
                "external",
                format!("Make one on {host}"),
                None,
                if remote.github {
                    "opens the page with repo ticked; paste the token below".to_owned()
                } else {
                    "opens the page with the name and api filled in; paste the token below"
                        .to_owned()
                },
            ),
        }
    }

    /// What a way sets off, said before it happens (Settings, a row picked).
    fn more(way: &Way, remote: &WebRemote) -> String {
        let host = crate::mr::host(remote);
        let (_, program) = connect::cli_name(remote.github);
        match &way.via {
            Via::Cli => format!(
                "GitLance uses {program}’s token where it is and never copies it; signing out of {program} brings this back."
            ),
            Via::Git(from) => {
                format!("GitLance uses the token {from} keeps where it is and never copies it.")
            }
            Via::Ssh => format!(
                "A token of GitLance’s own, made by {host} over SSH: nothing to copy, and renewed before it ends."
            ),
            Via::Ide(account) => format!(
                "macOS asks once whether GitLance may read {0}’s token; it is kept as a pasted one. Changing it in {0} later does not change GitLance’s.",
                account.product
            ),
            Via::Hand => "The token is kept in a file only you can read.".to_owned(),
        }
    }

    /// A way's button: named for what it does, filled on the focused one — with ↵ where ↵
    /// presses it (`keyed`); nothing to press while it is busy.
    fn way_button(
        &self,
        id: impl Into<gpui::ElementId>,
        way: &Way,
        on: bool,
        keyed: bool,
        scope: Option<&'static str>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (label, busy) = match &way.via {
            Via::Cli | Via::Git(_) => ("Use", false),
            Via::Ssh => (
                if self.connecting.making.is_some() {
                    "Making…"
                } else {
                    "Make a token"
                },
                self.connecting.making.is_some(),
            ),
            Via::Ide(a) => (
                "Copy…",
                self.connecting.copying.as_deref() == Some(a.id.as_str()),
            ),
            Via::Hand => ("Open ↗", false),
        };
        let kind = if on && !busy {
            Kind::Primary(theme::accent())
        } else {
            Kind::Plain
        };
        let button = discussion::button(id, label, (on && keyed && !busy).then_some("↵"), kind);
        if busy {
            return button.opacity(0.5);
        }
        let way = way.clone();
        button.on_click(
            cx.listener(move |this, _: &ClickEvent, _, cx| this.take_way(way.clone(), scope, cx)),
        )
    }

    /// The connect panel of Merge requests and the Inbox (`render_connect`): the ways in as rows,
    /// easiest first, each with its button; the keys; those found that cannot be used, with why.
    pub(super) fn render_ways(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(remote) = self.web.clone() else {
            return div().into_any_element();
        };
        let Some((rows, not)) = self.ways() else {
            return div().into_any_element();
        };
        let host = crate::mr::host(&remote);
        let looking =
            self.connecting.looking || found_for(&crate::mr::api_host(&remote)).here.is_none();
        let line = if looking {
            format!("{host} · looking for the ways in…")
        } else if rows.len() == 1 {
            format!("{host} · nothing on this computer connects it yet")
        } else {
            format!("{host} · the easiest way first")
        };
        let focus = self.focused(&rows);
        let can_read = {
            let note = connect::note(&crate::mr::api_host(&remote));
            note.allowed.is_empty() || note.allowed.iter().any(|s| s == "read_api")
        };
        let note = match (&self.gitlab_check, self.testing) {
            (_, true) => Some(("Checking…".to_owned(), theme::muted())),
            (Some(Err(e)), _) => Some((e.clone(), theme::removed())),
            (Some(Ok(who)), _) => Some((format!("Connected as {who}"), theme::added())),
            _ => None,
        };
        let mut card = div()
            .w_full()
            .max_w(px(640.))
            .flex()
            .flex_col()
            .rounded(px(10.))
            .bg(theme::panel())
            .border_1()
            .border_color(theme::island_border())
            .overflow_hidden();
        for (i, row) in rows.iter().enumerate() {
            let on = i == focus && matches!(row, Row::Way(_));
            let (icon, name, who, what) = self.describe(row, &remote);
            let dim = !matches!(row, Row::Way(_));
            let button: Option<AnyElement> = match row {
                Row::Way(way) if way.via == Via::Ssh && way.write && can_read => Some(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(2.))
                        .child(self.way_button(("way", i), way, on, true, Some("api"), cx))
                        .child(tipped(
                            discussion::button(
                                "way-read-menu",
                                "▾",
                                None,
                                if on {
                                    Kind::Primary(theme::accent())
                                } else {
                                    Kind::Plain
                                },
                            )
                            .on_click(cx.listener(
                                |this, _: &ClickEvent, _, cx| {
                                    this.connecting.read_menu = !this.connecting.read_menu;
                                    cx.notify();
                                },
                            )),
                            "Read only — a read_api token: GitLance reads, never writes".to_owned(),
                        ))
                        .into_any_element(),
                ),
                Row::Way(way) => Some(
                    self.way_button(("way", i), way, on, true, None, cx)
                        .into_any_element(),
                ),
                Row::Asking => Some(
                    discussion::button(("way", i), "Make a token", None, Kind::Plain)
                        .opacity(0.5)
                        .into_any_element(),
                ),
                Row::Failed(_) => None,
            };
            card = card.when(i > 0, |c| {
                c.child(div().h(px(1.)).bg(theme::island_border()))
            });
            card = card.child(
                way_row(("way-row", i), None, icon, name, who, what, on, dim)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.connecting.focus = i;
                        cx.notify();
                    }))
                    .children(button),
            );
            if let Row::Way(way) = row {
                if way.via == Via::Ssh && self.connecting.read_menu {
                    card = card.child(
                        div().flex().justify_end().px(px(14.)).pb(px(10.)).child(
                            discussion::button(
                                "way-read",
                                "Make a read-only token (read_api)",
                                None,
                                Kind::Plain,
                            )
                            .on_click(cx.listener(
                                |this, _: &ClickEvent, _, cx| {
                                    this.connecting.read_menu = false;
                                    this.make_ssh_token("read_api", cx)
                                },
                            )),
                        ),
                    );
                }
                if way.via == Via::Hand {
                    let field = self.field_box(
                        "token-field",
                        super::lists::Field::Token,
                        "Paste the token here",
                        cx,
                    );
                    card = card.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .pl(px(44.))
                            .pr(px(14.))
                            .pb(px(12.))
                            .when(on, |d| d.bg(theme::hover()))
                            .child(field)
                            .children(note.clone().map(|(text, color)| {
                                div().text_size(px(12.)).text_color(color).child(text)
                            })),
                    );
                }
            }
        }
        let hint = |keys: Vec<&str>, say: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .children(
                    keys.into_iter()
                        .map(|k| super::keys::caps(k, super::keys::On::Plain)),
                )
                .child(say)
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(10.))
            .px(px(24.))
            .child(
                super::icons::icon("pull-request")
                    .size(px(24.))
                    .text_color(theme::muted()),
            )
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(super::words::say("Connect GitLab to see merge requests").into_owned()),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child(line),
            )
            .child(card)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .text_size(px(11.))
                    .text_color(theme::faint())
                    .child(hint(vec!["↑", "↓"], "choose"))
                    .child(hint(vec!["↵"], "connect"))
                    .child(hint(vec!["⌘V"], "pastes a token")),
            )
            .children(self.connecting.error.clone().map(|e| {
                div()
                    .max_w(px(640.))
                    .text_size(px(12.))
                    .text_color(theme::removed())
                    .child(e)
            }))
            .children(not_here(&not, &remote))
            .into_any_element()
    }

    /// What is in use for the host, when it is none of the ways listed: its name and what it is.
    fn own_in_use(&self, remote: &WebRemote, rows: &[Row]) -> Option<(String, String)> {
        let (_, from) = crate::mr::token_source(remote)?;
        let listed = rows
            .iter()
            .any(|r| matches!(r, Row::Way(w) if self.in_use(w, remote)));
        if listed {
            return None;
        }
        let note = connect::note(&crate::mr::api_host(remote));
        let kept = "kept in a file only you can read";
        Some(match from {
            "Settings" => match (self.made_in_use(), note.way.as_deref(), note.ide) {
                (Some(made), _, _) => (
                    "Your SSH key’s token".to_owned(),
                    match made.expires {
                        Some(day) => format!("“{}” · {} · until {day}", made.name, made.scope),
                        None => format!("“{}” · {}", made.name, made.scope),
                    },
                ),
                (None, Some("ide"), Some(product)) => {
                    (format!("Copied from {product}"), kept.to_owned())
                }
                _ => ("A token you pasted".to_owned(), kept.to_owned()),
            },
            from if from.ends_with("variable") => (
                capital(from),
                "set where GitLance started: it is used whatever is picked here".to_owned(),
            ),
            from => (capital(from), "read where it is".to_owned()),
        })
    }

    /// Whether `way` is the one the host's token comes from now.
    fn in_use(&self, way: &Way, remote: &WebRemote) -> bool {
        let Some((_, from)) = crate::mr::token_source(remote) else {
            return false;
        };
        let note = connect::note(&crate::mr::api_host(remote));
        match &way.via {
            Via::Cli => from.contains("CLI"),
            Via::Git(at) => from == *at,
            Via::Ssh => self.made_in_use().is_some(),
            Via::Ide(account) => {
                from == "Settings"
                    && self.made_in_use().is_none()
                    && note.way.as_deref() == Some("ide")
                    && note.ide.as_deref() == Some(account.product.as_str())
            }
            Via::Hand => false,
        }
    }

    /// Settings → Merge requests (#156): the host's ways as a list to pick from, the one in use
    /// tagged with Test; the picked one open with what it does, its button and Cancel.
    pub(super) fn render_ways_settings(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let remote = self.web.clone()?;
        let (rows, not) = self.ways()?;
        let host = crate::mr::host(&remote);
        let env = crate::mr::token_source(&remote).is_some_and(|(_, f)| f.ends_with("variable"));
        let own = self.own_in_use(&remote, &rows);
        let note = connect::note(&crate::mr::api_host(&remote));
        let can_read = note.allowed.is_empty() || note.allowed.iter().any(|s| s == "read_api");
        let can_write = note.allowed.is_empty() || note.allowed.iter().any(|s| s == "api");
        let test = || {
            div()
                .id("ways-test")
                .flex_none()
                .text_size(px(12.))
                .text_color(theme::accent())
                .cursor_pointer()
                .child("Test")
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.test_gitlab(cx)))
        };
        let used_tag = || {
            div()
                .flex_none()
                .px(px(6.))
                .rounded(px(4.))
                .bg(theme::added_bg())
                .text_size(px(11.))
                .text_color(theme::added())
                .child("In use")
        };
        let mut card = div()
            .w_full()
            .flex()
            .flex_col()
            .rounded(px(10.))
            .bg(theme::panel())
            .border_1()
            .border_color(theme::island_border())
            .overflow_hidden();
        let has_own = own.is_some();
        if let Some((name, what)) = own {
            card = card.child(
                way_row(
                    "ways-own",
                    Some(false),
                    "key",
                    name,
                    None,
                    what,
                    false,
                    false,
                )
                .child(used_tag())
                .child(test()),
            );
        }
        let picked = self.connecting.pick.clone();
        let mut chosen: Option<Way> = None;
        let mut first = !has_own;
        for (i, row) in rows.iter().enumerate() {
            let (icon, name, who, what) = self.describe(row, &remote);
            if !std::mem::take(&mut first) {
                card = card.child(div().h(px(1.)).bg(theme::island_border()));
            }
            let Row::Way(way) = row else {
                let row = way_row(
                    ("ways-row", i),
                    Some(false),
                    icon,
                    name,
                    who,
                    what,
                    false,
                    true,
                );
                card = card.child(row);
                continue;
            };
            let key = row_key(way);
            let on = picked.as_deref() == Some(key.as_str());
            let used = self.in_use(way, &remote);
            if on && !used {
                chosen = Some(way.clone());
            }
            let pick = key.clone();
            card = card.child(
                way_row(("ways-row", i), Some(on), icon, name, who, what, on, false)
                    .when(!env, |r| {
                        r.cursor_pointer().on_click(cx.listener(
                            move |this, _: &ClickEvent, _, cx| {
                                this.connecting.pick = Some(pick.clone());
                                this.connecting.pick_scope = None;
                                cx.notify();
                            },
                        ))
                    })
                    .when(used, |r| r.child(used_tag()).child(test())),
            );
            if on && !used {
                let scopes = (way.via == Via::Ssh && can_write && can_read).then(|| {
                    let scope = self.connecting.pick_scope.unwrap_or("api");
                    div().flex().child(
                        super::diff_view::group()
                            .child(
                                super::diff_view::chip(
                                    "ways-api",
                                    "api · read and write",
                                    scope == "api",
                                )
                                .on_click(cx.listener(
                                    |this, _: &ClickEvent, _, cx| {
                                        this.connecting.pick_scope = Some("api");
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                super::diff_view::chip(
                                    "ways-read",
                                    "read_api · read only",
                                    scope == "read_api",
                                )
                                .on_click(cx.listener(
                                    |this, _: &ClickEvent, _, cx| {
                                        this.connecting.pick_scope = Some("read_api");
                                        cx.notify();
                                    },
                                )),
                            ),
                    )
                });
                card = card.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .pl(px(70.))
                        .pr(px(14.))
                        .pb(px(12.))
                        .bg(theme::hover())
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme::muted())
                                .child(Self::more(way, &remote)),
                        )
                        .children(scopes)
                        .when(way.via == Via::Hand, |d| {
                            d.child(self.field_box(
                                "token-field",
                                super::lists::Field::Token,
                                "Paste the token here",
                                cx,
                            ))
                        }),
                );
            }
        }
        let go = chosen.map(|way| {
            let leaves = match crate::mr::token_source(&remote).map(|(_, f)| f) {
                Some(f) if f.contains("CLI") => {
                    let (_, program) = connect::cli_name(remote.github);
                    format!("{program} keeps its token; GitLance stops using it.")
                }
                Some("Settings") if self.made_in_use().is_some() => {
                    "The token GitLance made is revoked.".to_owned()
                }
                Some("Settings") => "The token kept now is forgotten.".to_owned(),
                _ => String::new(),
            };
            let scope = (way.via == Via::Ssh).then(|| {
                self.connecting
                    .pick_scope
                    .unwrap_or(if can_write { "api" } else { "read_api" })
            });
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(12.))
                        .text_color(theme::faint())
                        .child(leaves),
                )
                .child(
                    discussion::button("ways-cancel", "Cancel", None, Kind::Quiet).on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.connecting.pick = None;
                            this.connecting.pick_scope = None;
                            cx.notify();
                        }),
                    ),
                )
                .child(self.way_button("ways-go", &way, true, false, scope, cx))
        });
        let status = self.ways_status(&remote);
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(
                    div()
                        .text_size(px(15.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(host),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child(if env {
                            "How GitLance signs in to this host. A token set where GitLance started is used: the ways below wait until it is not."
                        } else {
                            "How GitLance signs in to this host. The way in use is tagged; pick another to change it."
                        }),
                )
                .child(card)
                .children(go)
                .children(status.map(|(text, color)| {
                    div().text_size(px(12.)).text_color(color).child(text)
                }))
                .children(not_here(&not, &remote))
                .into_any_element(),
        )
    }

    /// The line under Settings' ways: being made or copied, why not, or what a test said.
    fn ways_status(&self, remote: &WebRemote) -> Option<(String, Rgba)> {
        if self.connecting.making.is_some() {
            return Some((
                "Making a token with your SSH key…".to_owned(),
                theme::muted(),
            ));
        }
        if let Some(e) = &self.connecting.error {
            return Some((e.clone(), theme::removed()));
        }
        match (&self.gitlab_check, crate::mr::token_source(remote)) {
            (Some(Ok(who)), _) => Some((format!("Connected as {who}"), theme::added())),
            (Some(Err(e)), _) => Some((e.clone(), theme::removed())),
            (None, None) => Some((
                super::words::say("No token for this host yet — merge requests stay hidden.")
                    .into_owned(),
                theme::warning(),
            )),
            (None, Some(_)) => None,
        }
    }
}

/// A row of the ways: Settings' pick mark (`picked`), its icon, the name with the account, what it
/// does; the caller adds what goes on its right.
#[allow(clippy::too_many_arguments)]
fn way_row(
    id: impl Into<gpui::ElementId>,
    picked: Option<bool>,
    icon: &'static str,
    name: String,
    who: Option<String>,
    what: String,
    on: bool,
    dim: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(12.))
        .px(px(14.))
        .py(px(10.))
        .when(on, |r| r.bg(theme::hover()))
        .when(dim, |r| r.opacity(0.6))
        .children(picked.map(radio))
        .child(
            super::icons::icon(icon)
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
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .min_w_0()
                        .child(
                            div()
                                .truncate()
                                .text_size(px(13.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(name),
                        )
                        .children(who.map(|w| {
                            div()
                                .flex_none()
                                .text_size(px(12.))
                                .text_color(theme::muted())
                                .child(w)
                        })),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child(what),
                ),
        )
}

/// Settings' pick mark.
fn radio(on: bool) -> Div {
    div()
        .flex_none()
        .size(px(14.))
        .rounded_full()
        .border_1()
        .border_color(if on {
            theme::accent()
        } else {
            theme::selected()
        })
        .when(on, |d| d.border_4())
}

/// The ways found that cannot be used, each with why.
fn not_here(not: &[NotHere], remote: &WebRemote) -> Option<AnyElement> {
    (!not.is_empty()).then(|| {
        let (cli, _) = connect::cli_name(remote.github);
        div()
            .w_full()
            .max_w(px(640.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .children(not.iter().map(|n| {
                let (icon, name) = match &n.via {
                    Via::Cli => ("terminal", cli.to_owned()),
                    Via::Git(from) => ("branch", capital(from)),
                    Via::Ssh => ("key", "Your SSH key".to_owned()),
                    Via::Ide(account) => ("app", account.product.clone()),
                    Via::Hand => ("external", String::new()),
                };
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .text_color(theme::faint())
                    .child(
                        super::icons::icon(icon)
                            .size(px(14.))
                            .text_color(theme::faint()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(format!("{name} — {}", n.why)),
                    )
            }))
            .into_any_element()
    })
}

/// "git's credential helper" → "Git's credential helper", for a name that starts a row.
fn capital(text: &str) -> String {
    let text = text.strip_prefix("the ").unwrap_or(text);
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// `button` with `tip` shown when hovered.
fn tipped(button: Stateful<Div>, tip: String) -> Stateful<Div> {
    let tip = SharedString::from(tip);
    button.tooltip(move |_, cx| cx.new(|_| super::TipOwned(tip.clone())).into())
}
