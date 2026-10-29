//! People's pictures (the developer, 2026-10-23): the `avatar_url` GitLab and GitHub give with a
//! user is fetched once onto the `avatars/` shelf of `crate::pictures`, and an avatar shows it;
//! until it is in, and for one without, the initial on its colour. Users are learnt from every answer the hosts give (`harvest`), by username and name.
//! Gravatar is asked for a real picture only (`d=404`): its made-up pattern for one who has none
//! is no picture of them. A host that keeps its uploads behind sign-in (a GitLab whose Public
//! visibility is restricted answers 401 to an API token too) leaves the initial; a picture that
//! could not be had is not asked for again for a week (`<name>.none`).

use serde_json::Value;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// Who has which picture: username and name, lower case → its address.
static WHO: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);
/// Each person's username by their name (and by the username itself), lower case: what an
/// initial's colour is made from, so one looks the same wherever they are named (#112).
static LOGINS: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);

/// Learns the users in an answer: any object with a username (`login` on GitHub) and an
/// `avatar_url`, however deep.
pub fn harvest(value: &Value) {
    match value {
        Value::Object(map) => {
            let url = map.get("avatar_url").and_then(Value::as_str).unwrap_or("");
            let login = ["username", "login"]
                .iter()
                .find_map(|k| map.get(*k).and_then(Value::as_str))
                .unwrap_or("");
            let name = map.get("name").and_then(Value::as_str).unwrap_or("");
            if !login.is_empty() {
                let mut logins = LOGINS.lock().unwrap_or_else(|e| e.into_inner());
                for key in [login, name].into_iter().filter(|k| !k.is_empty()) {
                    logins.insert(key.to_lowercase(), login.to_lowercase());
                }
            }
            if !url.is_empty() && !login.is_empty() {
                learn(login, name, url);
            }
            map.values().for_each(harvest);
        }
        Value::Array(items) => items.iter().for_each(harvest),
        _ => {}
    }
}

fn learn(login: &str, name: &str, url: &str) {
    let url = &real_picture(url);
    {
        let mut who = WHO.lock().unwrap_or_else(|e| e.into_inner());
        for key in [login, name].into_iter().filter(|k| !k.is_empty()) {
            who.insert(key.to_lowercase(), url.to_owned());
        }
    }
    crate::pictures::want(url, crate::pictures::Shelf::Avatars, None);
}

/// `url` as it is asked for: Gravatar's for a real picture only — its `d=` (the pattern made up
/// for one who has none) becomes `d=404`.
fn real_picture(url: &str) -> String {
    if !url.contains("gravatar.com/avatar/") {
        return url.to_owned();
    }
    let (base, query) = url.split_once('?').unwrap_or((url, ""));
    let mut params: Vec<String> = query
        .split('&')
        .filter(|p| !p.is_empty() && !p.starts_with("d="))
        .map(str::to_owned)
        .collect();
    params.push("d=404".to_owned());
    format!("{base}?{}", params.join("&"))
}

/// The picture of `who` (a username or a name), once it is in.
pub fn picture(who: &str) -> Option<std::sync::Arc<gpui::Image>> {
    let url = WHO
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&who.to_lowercase())
        .cloned()?;
    crate::pictures::get(&url).map(|(image, _)| image)
}

/// The username of `who` (a name or a username), lower case, once a host named them; else `who`
/// itself, lower case.
pub fn login_of(who: &str) -> String {
    let key = who.to_lowercase();
    LOGINS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
        .cloned()
        .unwrap_or(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gravatar_is_asked_for_a_real_picture_only() {
        assert_eq!(
            real_picture("https://secure.gravatar.com/avatar/b8e3?s=80&d=identicon"),
            "https://secure.gravatar.com/avatar/b8e3?s=80&d=404"
        );
        assert_eq!(
            real_picture("https://www.gravatar.com/avatar/b8e3"),
            "https://www.gravatar.com/avatar/b8e3?d=404"
        );
        let upload = "https://gitlab.example/uploads/-/system/user/avatar/1/avatar.png";
        assert_eq!(real_picture(upload), upload);
    }

    #[test]
    fn users_are_learnt_however_deep() {
        let answer = serde_json::json!([{
            "author": { "username": "ember-avatars-test", "name": "Ember Avatars", "avatar_url": "" },
            "reviewers": [{ "login": "wren-avatars-test", "avatar_url": "x:not-a-url" }]
        }]);
        harvest(&answer);
        // An initial's colour comes from the username, by name or by username alike.
        assert_eq!(login_of("Ember Avatars"), "ember-avatars-test");
        assert_eq!(login_of("EMBER-AVATARS-TEST"), "ember-avatars-test");
        assert_eq!(login_of("Nobody Known"), "nobody known");
        let who = WHO.lock().unwrap();
        assert!(!who.contains_key("ember-avatars-test"));
        assert_eq!(
            who.get("wren-avatars-test").map(String::as_str),
            Some("x:not-a-url")
        );
    }
}
