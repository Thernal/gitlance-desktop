//! JetBrains IDEs signed in to a host (#156): IntelliJ IDEA, Android Studio, PyCharm and the rest
//! keep their GitLab and GitHub accounts in their settings folder (`options/gitlab.xml`,
//! `options/github.xml`) and each account's token in the system's password store, under
//! `IntelliJ Platform GitLab — <account id>`. An account is offered only while its token is there
//! (looked for without reading it); the token is read once, when the developer asks, and macOS asks
//! them first. Only a personal access token is taken: a GitLab sign-in's OAuth token lasts two
//! hours and refreshing it would sign the IDE out.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// An IDE's account on a host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    /// The IDE, as its maker names it: "IntelliJ IDEA".
    pub product: String,
    pub id: String,
    pub user: String,
    pub github: bool,
}

impl Account {
    /// Where its token is kept in the password store.
    pub fn service(&self) -> String {
        let subsystem = if self.github { "GitHub" } else { "GitLab" };
        format!("IntelliJ Platform {subsystem} — {}", self.id)
    }
}

/// A settings folder's name without its version, and the product it is.
const PRODUCTS: &[(&str, &str)] = &[
    ("IntelliJIdea", "IntelliJ IDEA"),
    ("IdeaIC", "IntelliJ IDEA CE"),
    ("AndroidStudio", "Android Studio"),
    ("AndroidStudioPreview", "Android Studio Preview"),
    ("PyCharm", "PyCharm"),
    ("PyCharmCE", "PyCharm CE"),
    ("WebStorm", "WebStorm"),
    ("GoLand", "GoLand"),
    ("RustRover", "RustRover"),
    ("CLion", "CLion"),
    ("PhpStorm", "PhpStorm"),
    ("Rider", "Rider"),
    ("RubyMine", "RubyMine"),
    ("DataGrip", "DataGrip"),
    ("DataSpell", "DataSpell"),
    ("Aqua", "Aqua"),
];

/// "IntelliJIdea2026.2" → "IntelliJ IDEA"; a folder without a version is not an IDE's.
fn product(folder: &str) -> Option<String> {
    let at = folder.find(|c: char| c.is_ascii_digit())?;
    let name = &folder[..at];
    (!name.is_empty()).then(|| {
        PRODUCTS
            .iter()
            .find(|(f, _)| *f == name)
            .map_or(name, |(_, p)| p)
            .to_owned()
    })
}

/// The accounts of `gitlab.xml`: a JSON list in the component's text, `{"accounts":[{"id",
/// "name", "server":{"uri"}}]}`, as (id, user, host).
fn gitlab_accounts(xml: &str) -> Vec<(String, String, String)> {
    let text = match xml.split_once("<![CDATA[") {
        Some((_, rest)) => rest.split_once("]]>").map_or(rest, |(t, _)| t).to_owned(),
        None => xml
            .split_once("name=\"GitLabAccounts\"")
            .and_then(|(_, rest)| rest.split_once('>'))
            .and_then(|(_, rest)| rest.split_once("</component>"))
            .map(|(t, _)| unescape(t))
            .unwrap_or_default(),
    };
    let Ok(json) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    json["accounts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            Some((
                a["id"].as_str()?.to_owned(),
                a["name"].as_str()?.to_owned(),
                bare_host(a["server"]["uri"].as_str()?),
            ))
        })
        .collect()
}

/// The accounts of `github.xml`: `<account name id><server host/></account>`, as (id, user, host).
fn github_accounts(xml: &str) -> Vec<(String, String, String)> {
    xml.split("<account ")
        .skip(1)
        .filter_map(|part| {
            let (tag, rest) = part.split_once('>')?;
            let body = rest.split("</account>").next().unwrap_or(rest);
            let server = body.split_once("<server ")?.1.split('>').next()?;
            Some((
                attribute(tag, "id")?,
                attribute(tag, "name")?,
                bare_host(&attribute(server, "host")?),
            ))
        })
        .collect()
}

/// `name="value"` in a tag's text.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let start = tag
        .match_indices(&format!("{name}=\""))
        .find(|(at, _)| *at == 0 || tag[..*at].ends_with(char::is_whitespace))?
        .0
        + name.len()
        + 2;
    let value = tag[start..].split('"').next()?;
    Some(unescape(value))
}

fn unescape(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// `https://gitlab.example.com/` → `gitlab.example.com`.
fn bare_host(uri: &str) -> String {
    let uri = uri.trim();
    uri.split_once("://")
        .map_or(uri, |(_, rest)| rest)
        .trim_end_matches('/')
        .to_owned()
}

/// The folders that hold the IDEs' settings folders; the checks' fixture has its own.
fn config_dirs() -> Vec<PathBuf> {
    match crate::connect::fixture() {
        Some(dir) => vec![dir.join("ide/JetBrains"), dir.join("ide/Google")],
        None => crate::platform::ide_config_dirs(),
    }
}

/// When a settings file was last written: the IDE used last wins an account two versions share.
fn written(file: &Path) -> SystemTime {
    std::fs::metadata(file)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// The IDEs' accounts on `host` (no scheme) whose token the password store holds, the IDE used
/// last first; one account in several versions of an IDE once.
pub fn accounts(host: &str, github: bool) -> Vec<Account> {
    let file = if github { "github.xml" } else { "gitlab.xml" };
    let mut found: Vec<(SystemTime, Account)> = Vec::new();
    for dir in config_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Some(product) = product(&entry.file_name().to_string_lossy()) else {
                continue;
            };
            let path = entry.path().join("options").join(file);
            let Ok(xml) = std::fs::read_to_string(&path) else {
                continue;
            };
            let listed = if github {
                github_accounts(&xml)
            } else {
                gitlab_accounts(&xml)
            };
            let when = written(&path);
            for (id, user, at) in listed {
                if !at.eq_ignore_ascii_case(host) {
                    continue;
                }
                let account = Account {
                    product: product.clone(),
                    id,
                    user,
                    github,
                };
                match found.iter_mut().find(|(_, a)| a.id == account.id) {
                    Some(kept) if kept.0 < when => *kept = (when, account),
                    Some(_) => {}
                    None => found.push((when, account)),
                }
            }
        }
    }
    found.sort_by_key(|(when, _)| std::cmp::Reverse(*when));
    found
        .into_iter()
        .map(|(_, a)| a)
        .filter(|a| has(&a.service()))
        .collect()
}

/// What an IDE keeps for an account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stored {
    /// A personal access token.
    Token(String),
    /// A sign-in's OAuth token: not taken.
    OAuth,
}

/// A bare token (GitHub, older IDEs), or GitLab's JSON: `{"type":"…GitLabCredentials.Token",
/// "accessToken":…}` or `…OAuth` with a `refreshToken`.
pub fn stored(value: &str) -> Option<Stored> {
    let value = value.trim();
    if !value.starts_with('{') {
        return (value.len() >= 8 && !value.contains(char::is_whitespace))
            .then(|| Stored::Token(value.to_owned()));
    }
    let json: Value = serde_json::from_str(value).ok()?;
    let token = json["accessToken"].as_str()?.trim();
    let oauth = json["type"].as_str().is_some_and(|t| t.ends_with("OAuth"))
        || json.get("refreshToken").is_some();
    Some(if oauth {
        Stored::OAuth
    } else {
        Stored::Token(token.to_owned())
    })
}

/// Why an IDE's token was not read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unread {
    /// The developer said no, or nothing is there now.
    Denied,
    /// It is a sign-in's OAuth token.
    OAuth,
}

/// The account's token, read once: macOS asks the developer first, so it waits for them — off the
/// window's thread.
pub fn read(account: &Account) -> Result<String, Unread> {
    let value = match crate::connect::fixture() {
        Some(dir) => std::fs::read_to_string(dir.join("keychain").join(&account.id)).ok(),
        None => crate::platform::keychain_read(&account.service()),
    };
    match value.as_deref().and_then(stored) {
        Some(Stored::Token(token)) => Ok(token),
        Some(Stored::OAuth) => Err(Unread::OAuth),
        None => Err(Unread::Denied),
    }
}

/// Whether the password store holds the item, asked without reading it.
fn has(service: &str) -> bool {
    match crate::connect::fixture() {
        Some(dir) => service
            .rsplit_once(" — ")
            .is_some_and(|(_, id)| dir.join("keychain").join(id).is_file()),
        None => crate::platform::keychain_has(service),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_settings_folder_names_its_ide() {
        assert_eq!(
            product("IntelliJIdea2026.2").as_deref(),
            Some("IntelliJ IDEA")
        );
        assert_eq!(
            product("AndroidStudio2026.1.3").as_deref(),
            Some("Android Studio")
        );
        assert_eq!(product("PyCharmCE2025.3").as_deref(), Some("PyCharm CE"));
        assert_eq!(product("Fleet1.40").as_deref(), Some("Fleet"));
        assert_eq!(product("consentOptions"), None);
        assert_eq!(product("2026.1"), None);
    }

    #[test]
    fn the_accounts_are_read_from_both_files() {
        let gitlab = r#"<application>
  <component name="GitLabAccounts"><![CDATA[{
  "accounts": [
    { "id": "5f2c9a1e", "name": "ann", "server": { "uri": "https://gitlab.example.com/" } },
    { "id": "77aa", "name": "ann-home", "server": { "uri": "https://gitlab.com" } }
  ]
}]]></component>
</application>"#;
        assert_eq!(
            gitlab_accounts(gitlab),
            vec![
                ("5f2c9a1e".into(), "ann".into(), "gitlab.example.com".into()),
                ("77aa".into(), "ann-home".into(), "gitlab.com".into()),
            ]
        );
        assert!(
            gitlab_accounts(r#"<component name="GitLabAccounts"><![CDATA[{}]]></component>"#)
                .is_empty()
        );
        let escaped = r#"<component name="GitLabAccounts">{&quot;accounts&quot;:[{&quot;id&quot;:&quot;1&quot;,&quot;name&quot;:&quot;ann&quot;,&quot;server&quot;:{&quot;uri&quot;:&quot;https://h&quot;}}]}</component>"#;
        assert_eq!(gitlab_accounts(escaped).len(), 1);

        let github = r#"<application>
  <component name="GithubAccounts">
    <account name="ann" id="d6ef01">
      <server host="github.com" />
    </account>
    <account id="e1" name="ann-work">
      <server host="ghe.example.com" port="8443" />
    </account>
  </component>
</application>"#;
        assert_eq!(
            github_accounts(github),
            vec![
                ("d6ef01".into(), "ann".into(), "github.com".into()),
                ("e1".into(), "ann-work".into(), "ghe.example.com".into()),
            ]
        );
    }

    #[test]
    fn only_a_personal_token_is_taken_from_what_an_ide_keeps() {
        assert_eq!(
            stored("glpat-abcdef123\n"),
            Some(Stored::Token("glpat-abcdef123".into()))
        );
        assert_eq!(
            stored(
                r#"{"type":"org.jetbrains.plugins.gitlab.authentication.accounts.GitLabCredentials.Token","accessToken":"glpat-x1y2z3w4"}"#
            ),
            Some(Stored::Token("glpat-x1y2z3w4".into()))
        );
        assert_eq!(
            stored(
                r#"{"type":"org.jetbrains.plugins.gitlab.authentication.accounts.GitLabCredentials.OAuth","accessToken":"a","refreshToken":"r","clientId":"c","expiresIn":7200,"createdAt":1}"#
            ),
            Some(Stored::OAuth)
        );
        assert_eq!(stored(""), None);
        assert_eq!(stored("not a token"), None);
    }

    #[test]
    fn an_account_is_offered_once_and_only_with_its_token_there() {
        let dir = tempfile::TempDir::new().unwrap();
        crate::connect::TEST_FIXTURE.with(|f| *f.borrow_mut() = Some(dir.path().to_path_buf()));
        let gitlab = |id: &str| {
            format!(
                r#"<component name="GitLabAccounts"><![CDATA[{{"accounts":[{{"id":"{id}","name":"ann","server":{{"uri":"https://gitlab.example.com"}}}}]}}]]></component>"#
            )
        };
        for (folder, id) in [
            ("JetBrains/IntelliJIdea2026.2", "5f2c"),
            ("Google/AndroidStudio2026.1.3", "5f2c"),
            ("JetBrains/PyCharm2026.2", "no-token"),
        ] {
            let options = dir.path().join("ide").join(folder).join("options");
            std::fs::create_dir_all(&options).unwrap();
            std::fs::write(options.join("gitlab.xml"), gitlab(id)).unwrap();
        }
        std::fs::create_dir_all(dir.path().join("keychain")).unwrap();
        std::fs::write(dir.path().join("keychain/5f2c"), "glpat-ide12345").unwrap();
        let found = accounts("gitlab.example.com", false);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].user, "ann");
        assert_eq!(found[0].service(), "IntelliJ Platform GitLab — 5f2c");
        assert_eq!(read(&found[0]).as_deref(), Ok("glpat-ide12345"));
        assert!(accounts("gitlab.com", false).is_empty());
        assert!(accounts("gitlab.example.com", true).is_empty());
        crate::connect::TEST_FIXTURE.with(|f| *f.borrow_mut() = None);
    }
}
