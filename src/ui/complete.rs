//! Completion while writing a comment (#51 mentions, #9 `:name` emoji): `@` and a letter lists
//! people — the request's first (its author, reviewers, who wrote in it), then the project's
//! members; `:` and two letters lists emoji by name. ↑ ↓ move, ↵ or ⇥ puts it in, esc closes it.
//! No agents: they are messaged apart.

use super::{ROW_RADIUS, Workspace, px, theme};
use gpui::{FontWeight, div, prelude::*};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    Person,
    Emoji,
}

/// One row of the list: what it shows, beside it, and what it puts in the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub label: String,
    pub detail: String,
    pub insert: String,
}

/// The token being completed before `caret`: its trigger, where the trigger is, what follows it.
pub fn token(text: &str, caret: usize) -> Option<(Trigger, usize, String)> {
    let before = text.get(..caret)?;
    let word_start = before
        .char_indices()
        .rev()
        .find(|(_, c)| !(c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '+')))
        .map_or(0, |(i, c)| i + c.len_utf8());
    let at = word_start.checked_sub(1)?;
    let trigger = match before.as_bytes().get(at)? {
        b'@' => Trigger::Person,
        b':' => Trigger::Emoji,
        _ => return None,
    };
    // The trigger starts a word: at the start, or after a space or an opening bracket.
    if at > 0
        && !matches!(
            before.as_bytes()[at - 1],
            b' ' | b'\n' | b'\t' | b'(' | b'['
        )
    {
        return None;
    }
    let query = before[word_start..].to_owned();
    if trigger == Trigger::Emoji && query.chars().count() < 2 {
        return None;
    }
    Some((trigger, at, query))
}

impl Workspace {
    /// Who can be mentioned, the request's people first: (username, name).
    fn people(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        let mut add = |login: &str, name: &str| {
            if !login.is_empty() && !out.iter().any(|(l, _)| l == login) {
                out.push((login.to_owned(), name.to_owned()));
            }
        };
        if let Some(m) = self
            .requests
            .current
            .and_then(|iid| self.requests.list.iter().find(|m| m.iid == iid))
        {
            add(&m.author_login, &m.author);
            for r in &m.reviewers {
                add(r, "");
            }
        }
        for t in &self.requests.threads {
            for n in &t.notes {
                add(&n.login, &n.author);
            }
        }
        for (login, name) in &self.requests.members {
            add(login, name);
        }
        // A name learnt later (a member) fills in one known only by login (a reviewer).
        let members = &self.requests.members;
        out.into_iter()
            .map(|(l, n)| {
                let name = if n.is_empty() {
                    members
                        .iter()
                        .find(|(m, _)| *m == l)
                        .map(|(_, n)| n.clone())
                        .unwrap_or_default()
                } else {
                    n
                };
                (l, name)
            })
            .collect()
    }

    /// The list for the token before `caret` in `text`, unless it was closed with esc at `closed`.
    pub(super) fn completion(
        &self,
        text: &str,
        caret: usize,
        closed: Option<usize>,
    ) -> Option<(usize, Vec<Choice>)> {
        let (trigger, at, query) = token(text, caret)?;
        if closed == Some(at) {
            return None;
        }
        let q = query.to_lowercase();
        let choices: Vec<Choice> = match trigger {
            Trigger::Person => {
                let mut people: Vec<(String, String)> = self
                    .people()
                    .into_iter()
                    .filter(|(l, n)| l.to_lowercase().contains(&q) || n.to_lowercase().contains(&q))
                    .collect();
                // Who starts with what is typed comes first; otherwise the request's order stays.
                people.sort_by_key(|(l, n)| {
                    !(l.to_lowercase().starts_with(&q) || n.to_lowercase().starts_with(&q))
                });
                people
            }
            .into_iter()
            .take(8)
            .map(|(login, name)| Choice {
                label: if name.is_empty() { login.clone() } else { name },
                detail: format!("@{login}"),
                insert: format!("@{login} "),
            })
            .collect(),
            Trigger::Emoji => crate::emoji::find(&q, 8)
                .into_iter()
                .map(|(name, emoji)| Choice {
                    label: format!("{emoji}  {name}"),
                    detail: String::new(),
                    insert: emoji.to_owned(),
                })
                .collect(),
        };
        (!choices.is_empty()).then_some((at, choices))
    }

    /// The list under the field, `pick` selected.
    pub(super) fn render_completion(&self, choices: &[Choice], pick: usize) -> gpui::Div {
        div()
            .mx(px(8.))
            .mb(px(6.))
            .p(px(4.))
            .rounded(px(ROW_RADIUS))
            .border_1()
            .border_color(theme::island_border())
            .bg(theme::panel())
            .flex()
            .flex_col()
            .children(choices.iter().enumerate().map(|(i, c)| {
                div()
                    .h(px(26.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(ROW_RADIUS))
                    .when(i == pick, |s| s.bg(theme::selected()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(c.label.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(theme::faint())
                            .child(c.detail.clone()),
                    )
            }))
            .child(
                div()
                    .px(px(8.))
                    .pt(px(4.))
                    .text_size(px(11.))
                    .text_color(theme::faint())
                    .child("↑↓ move · ↵ or ⇥ puts it in · esc"),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_read_back_from_the_caret() {
        let t = "thanks @jun";
        assert_eq!(token(t, t.len()), Some((Trigger::Person, 7, "jun".into())));
        let t = "nice :ta";
        assert_eq!(token(t, t.len()), Some((Trigger::Emoji, 5, "ta".into())));
        assert_eq!(token("nice :t", 7), None);
        assert_eq!(token("mail a@b", 8), None);
        assert_eq!(token("time 10:30", 10), None);
        assert_eq!(token("@", 1), Some((Trigger::Person, 0, String::new())));
    }
}
