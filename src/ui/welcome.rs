//! The window with no repository open (#54): open one (⌘O), connect GitLab or GitHub with a token,
//! clone one from a URL; a folder dropped anywhere opens too. The repositories opened lately are
//! listed under them.

use super::input::Look;
use super::lists::Field;
use super::{ISLAND_RADIUS, Workspace, icons, keys, px, theme};
use crate::git::WebRemote;
use gpui::{ClickEvent, Context, Div, FontWeight, PathPromptOptions, div, prelude::*};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The card the welcome shows in place of its choices.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Card {
    Connect,
    Clone,
}

#[derive(Default)]
pub struct Welcome {
    pub card: Option<Card>,
    /// Connect: the host typed; Clone: the URL.
    pub host: String,
    pub url: String,
    /// What the last check or clone said: who the token is, or why not.
    pub said: Option<Result<String, String>>,
    pub busy: bool,
}

/// `host` as typed — `gitlab.example.com`, `https://gitlab.example.com/` — as a remote to check a
/// token against: GitHub's for `github.com`, else a GitLab host's.
fn remote_of(host: &str) -> Option<WebRemote> {
    let host = host.trim();
    let host = host
        .split_once("://")
        .map_or(host, |(_, rest)| rest)
        .trim_end_matches('/');
    let host = if host.is_empty() { "gitlab.com" } else { host };
    (!host.contains(char::is_whitespace)).then(|| WebRemote {
        base: format!("https://{host}/-/-"),
        github: host == "github.com",
    })
}

/// The folder a clone of `url` goes into: its last part without `.git`.
fn clone_name(url: &str) -> Option<String> {
    let last = url.trim().trim_end_matches('/').rsplit(['/', ':']).next()?;
    let name = last.strip_suffix(".git").unwrap_or(last);
    (!name.is_empty() && name != "." && name != "..").then(|| name.to_owned())
}

/// `git clone url` into `into`, with the user's own git (SSH agent, credential helpers); the
/// error in a sentence.
fn clone(url: &str, into: &Path) -> Result<(), String> {
    let out = Command::new("git")
        .arg("clone")
        .arg("--quiet")
        .arg("--")
        .arg(url)
        .arg(into)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|_| "git is not installed".to_owned())?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    Err(err
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("git clone failed")
        .trim()
        .trim_start_matches("fatal: ")
        .to_owned())
}

impl Workspace {
    pub(super) fn open_welcome_card(&mut self, card: Card, cx: &mut Context<Self>) {
        self.welcome.card = Some(card);
        self.welcome.said = None;
        self.start_field(
            match card {
                Card::Connect => Field::Host,
                Card::Clone => Field::CloneUrl,
            },
            cx,
        );
    }

    fn close_welcome_card(&mut self, cx: &mut Context<Self>) {
        self.welcome.card = None;
        self.welcome.said = None;
        self.field = None;
        cx.notify();
    }

    /// ↵ in the connect card's token: kept for the host, then asked who it is; a token the host
    /// refuses is taken back.
    pub(super) fn welcome_connect(&mut self, cx: &mut Context<Self>) {
        let Some(remote) = remote_of(&self.welcome.host) else {
            self.welcome.said = Some(Err("That does not look like a host.".to_owned()));
            return cx.notify();
        };
        let token = self.token_input.trim().to_owned();
        if token.len() < 8 || token.contains(char::is_whitespace) {
            self.welcome.said = Some(Err("That does not look like a token.".to_owned()));
            return cx.notify();
        }
        if let Err(e) = crate::mr::save_token(&remote, &token) {
            self.welcome.said = Some(Err(format!("{e:#}")));
            return cx.notify();
        }
        self.welcome.said = None;
        self.welcome.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let check = remote.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let who = crate::mr::me(&check)?;
                    Ok::<_, anyhow::Error>(match crate::mr::token_info(&check) {
                        Some(info) => format!("{who} · {info}"),
                        None => who,
                    })
                })
                .await;
            this.update(cx, |this, cx| {
                this.welcome.busy = false;
                match result {
                    Ok(who) => {
                        this.welcome.said = Some(Ok(who));
                        this.token_input.clear();
                        this.field = None;
                    }
                    Err(e) => {
                        crate::mr::forget_token(&remote);
                        this.welcome.said = Some(Err(format!("{e:#}")));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// ↵ in the clone card: the folder it goes in is asked, then git clones it there and it opens.
    pub(super) fn welcome_clone(&mut self, cx: &mut Context<Self>) {
        let url = self.welcome.url.trim().to_owned();
        let Some(name) = clone_name(&url) else {
            self.welcome.said = Some(Err("That does not look like a repository's URL.".to_owned()));
            return cx.notify();
        };
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Clone Here".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(parent) = paths.into_iter().next() else {
                return;
            };
            let into: PathBuf = parent.join(&name);
            if into.exists() {
                this.update(cx, |this, cx| {
                    this.welcome.said = Some(Err(format!("{} is already there.", into.display())));
                    cx.notify();
                })
                .ok();
                return;
            }
            this.update(cx, |this, cx| {
                this.welcome.busy = true;
                this.welcome.said = Some(Ok(format!("Cloning into {}…", into.display())));
                this.field = None;
                cx.notify();
            })
            .ok();
            let target = into.clone();
            let result = cx
                .background_executor()
                .spawn(async move { clone(&url, &target) })
                .await;
            this.update(cx, |this, cx| {
                this.welcome.busy = false;
                match result {
                    Ok(()) => {
                        this.welcome = Default::default();
                        this.request_open(into, None, cx);
                    }
                    Err(e) => this.welcome.said = Some(Err(e)),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The welcome: GitLance, what it is, the three ways in and the drop target — or the card one
    /// of them opened —, and the repositories opened lately.
    pub(super) fn render_welcome(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match self.welcome.card {
            Some(card) => self.welcome_card(card, cx).into_any_element(),
            None => div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    choice(
                        "welcome-open",
                        "folder",
                        "Open a repository",
                        "a folder with a .git inside",
                    )
                    .child(keys::caps("⌘O", keys::On::Plain))
                    .on_click(cx.listener(
                        |this, _: &ClickEvent, window, cx| this.open(&super::Open, window, cx),
                    )),
                )
                .child(
                    choice(
                        "welcome-connect",
                        "pull-request",
                        "Connect GitLab or GitHub",
                        "your review requests and inbox, from a token",
                    )
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.open_welcome_card(Card::Connect, cx)
                    })),
                )
                .child(
                    choice(
                        "welcome-clone",
                        "files",
                        "Clone from a URL",
                        "git clone, then open it",
                    )
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.open_welcome_card(Card::Clone, cx)
                    })),
                )
                .child(
                    div()
                        .h(px(44.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(10.))
                        .border_1()
                        .border_dashed()
                        .border_color(theme::island_border())
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child("or drop a folder anywhere in the window"),
                )
                .into_any_element(),
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .m(px(super::GAP))
            .mt_0()
            .rounded(px(ISLAND_RADIUS))
            .bg(theme::editor())
            .child(
                div()
                    .w(px(560.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(22.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("GitLance"),
                            )
                            .child(div().text_color(theme::muted()).child(
                                "See what changed between the versions of a branch — and review it — on your own machine.",
                            )),
                    )
                    .child(body)
                    .children((!self.recent.is_empty() && self.welcome.card.is_none()).then(|| {
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .mt_2()
                            .child(super::section_label("Recent"))
                            .children(self.recent.iter().enumerate().map(|(ix, path)| {
                                let path = path.clone();
                                div()
                                    .id(("recent", ix))
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme::hover()))
                                    .text_color(theme::muted())
                                    .truncate()
                                    .child(path.display().to_string())
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.request_open(path.clone(), None, cx)
                                    }))
                            }))
                    })),
            )
    }

    /// The card a choice opened: connect (host, token, who it is) or clone (the URL).
    fn welcome_card(&self, card: Card, cx: &mut Context<Self>) -> Div {
        let field = |id: &'static str, field: Field, text: &str, placeholder: &'static str| {
            let active = self.field == Some(field);
            div()
                .id(id)
                .h(px(32.))
                .px(px(10.))
                .flex()
                .items_center()
                .rounded(px(super::ROW_RADIUS))
                .border_2()
                .border_color(if active {
                    theme::focus()
                } else {
                    theme::island_border()
                })
                .bg(theme::base())
                .cursor_text()
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.start_field(field, cx)),
                )
                .child(self.field_look(
                    text,
                    active,
                    placeholder,
                    Look {
                        mask: field == Field::Token,
                        ..Default::default()
                    },
                    cx.entity().downgrade(),
                ))
        };
        let label = |text: &'static str| {
            div()
                .text_size(px(12.))
                .text_color(theme::muted())
                .child(text)
        };
        let said = match (&self.welcome.said, self.welcome.busy) {
            (_, true) if card == Card::Connect => Some(("Checking…".to_owned(), theme::muted())),
            (Some(Ok(text)), _) if card == Card::Connect => {
                Some((format!("✓ Signed in as {text}"), theme::added()))
            }
            (Some(Ok(text)), _) => Some((text.clone(), theme::muted())),
            (Some(Err(e)), _) => Some((e.clone(), theme::removed())),
            _ => None,
        };
        let (title, text) = match card {
            Card::Connect => (
                "Connect GitLab or GitHub",
                "For review requests, discussions and approvals. The token stays on this Mac; GitLance writes nothing to the host unless you post a comment or a review.",
            ),
            Card::Clone => (
                "Clone from a URL",
                "git clone with your own git — its SSH keys and credentials —, into a folder you pick; then it opens.",
            ),
        };
        let fields = match card {
            Card::Connect => {
                let page = remote_of(&self.welcome.host).map(|r| crate::mr::token_page(&r));
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(label("Host"))
                    .child(field(
                        "welcome-host",
                        Field::Host,
                        &self.welcome.host,
                        "gitlab.com · github.com · gitlab.example.com",
                    ))
                    .child(
                        div()
                            .pt(px(4.))
                            .flex()
                            .items_center()
                            .child(div().flex_1().child(label("Personal access token")))
                            .children(page.map(|page| {
                                div()
                                    .id("welcome-token-page")
                                    .text_size(px(12.))
                                    .text_color(theme::accent())
                                    .cursor_pointer()
                                    .child("Create one ↗")
                                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                        cx.open_url(&page)
                                    }))
                            })),
                    )
                    .child(field(
                        "welcome-token",
                        Field::Token,
                        &self.token_input,
                        "Paste your token",
                    ))
            }
            Card::Clone => div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(label("URL"))
                .child(field(
                    "welcome-url",
                    Field::CloneUrl,
                    &self.welcome.url,
                    "git@gitlab.com:group/project.git · https://github.com/owner/repo",
                )),
        };
        let done = matches!(self.welcome.said, Some(Ok(_))) && card == Card::Connect;
        let (go, go_label) = match card {
            Card::Connect if done => ("welcome-done", "Done"),
            Card::Connect => ("welcome-check", "Connect"),
            Card::Clone => ("welcome-go", "Clone…"),
        };
        div()
            .p(px(20.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .rounded(px(ISLAND_RADIUS))
            .border_1()
            .border_color(theme::island_border())
            .bg(theme::panel())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child(text),
                    ),
            )
            .child(fields)
            .children(
                said.map(|(text, color)| div().text_size(px(12.)).text_color(color).child(text)),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        super::bar::bar_button("welcome-back", "Back", "esc", false).on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| this.close_welcome_card(cx)),
                        ),
                    )
                    .child(
                        super::bar::bar_button(go, go_label, "↵", true).on_click(cx.listener(
                            move |this, _: &ClickEvent, _, cx| match card {
                                Card::Connect if done => this.close_welcome_card(cx),
                                Card::Connect => this.welcome_connect(cx),
                                Card::Clone => this.welcome_clone(cx),
                            },
                        )),
                    ),
            )
    }
}

/// One of the welcome's ways in: its icon, what it does and a word on it.
fn choice(
    id: &'static str,
    icon: &'static str,
    title: &'static str,
    text: &'static str,
) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .h(px(56.))
        .px(px(14.))
        .flex()
        .items_center()
        .gap(px(12.))
        .rounded(px(10.))
        .border_1()
        .border_color(theme::island_border())
        .bg(theme::panel())
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .child(icons::icon(icon).size(px(18.)).text_color(theme::accent()))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(title)
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child(text),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_is_read_however_it_is_typed() {
        let base = |h: &str| remote_of(h).map(|r| (r.base, r.github));
        assert_eq!(
            base("https://gitlab.example.com/"),
            Some(("https://gitlab.example.com/-/-".to_owned(), false))
        );
        assert_eq!(
            base("github.com"),
            Some(("https://github.com/-/-".to_owned(), true))
        );
        assert_eq!(base(""), Some(("https://gitlab.com/-/-".to_owned(), false)));
        assert_eq!(base("a b"), None);
    }

    #[test]
    fn a_clone_is_named_after_its_url() {
        assert_eq!(
            clone_name("git@gitlab.com:group/payments.git").as_deref(),
            Some("payments")
        );
        assert_eq!(
            clone_name("https://github.com/owner/repo/").as_deref(),
            Some("repo")
        );
        assert_eq!(clone_name("git@host:repo.git").as_deref(), Some("repo"));
        assert_eq!(clone_name(""), None);
    }

    #[test]
    fn a_clone_lands_where_asked_and_a_bad_url_says_why() {
        let dir = tempfile::TempDir::new().unwrap();
        let origin = dir.path().join("origin.git");
        git2::Repository::init_bare(&origin).unwrap();
        let into = dir.path().join("work");
        clone(origin.to_str().unwrap(), &into).unwrap();
        assert!(into.join(".git").is_dir());
        let err = clone(
            dir.path().join("none").to_str().unwrap(),
            &dir.path().join("x"),
        );
        assert!(err.is_err_and(|e| !e.is_empty() && !e.starts_with("fatal:")));
    }
}
