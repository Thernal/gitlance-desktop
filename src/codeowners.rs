//! CODEOWNERS, read from the target branch's tree, to suggest a new request's reviewers (#47:
//! "suggested: code owners of the changed files"). The same rules on GitLab and GitHub: a pattern
//! as in `.gitignore`, the last matching line wins, owners are `@user` (teams and e-mails are not
//! people to ask here).

/// Where each host looks, in order; the first file found is the one.
pub const PLACES_GITLAB: [&str; 3] = ["CODEOWNERS", "docs/CODEOWNERS", ".gitlab/CODEOWNERS"];
pub const PLACES_GITHUB: [&str; 3] = [".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"];

/// One rule: a pattern and its owners' usernames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pattern: String,
    owners: Vec<String>,
}

pub fn parse(text: &str) -> Vec<Rule> {
    text.lines()
        .map(|l| l.split(" #").next().unwrap_or_default().trim())
        // Comments, and GitLab's section headings (`[Docs]`, `^[Optional]`).
        .filter(|l| {
            !l.is_empty() && !l.starts_with('#') && !l.starts_with('[') && !l.starts_with("^[")
        })
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            let pattern = parts.next()?.to_owned();
            let owners = parts
                .filter_map(|o| o.strip_prefix('@'))
                .filter(|o| !o.contains('/'))
                .map(str::to_owned)
                .collect();
            Some(Rule { pattern, owners })
        })
        .collect()
}

/// The owners of `path`: those of the last rule matching it.
pub fn owners<'a>(rules: &'a [Rule], path: &str) -> &'a [String] {
    rules
        .iter()
        .rev()
        .find(|r| matches(&r.pattern, path))
        .map_or(&[], |r| r.owners.as_slice())
}

/// The owners of all `paths`, most files first, without `me`.
pub fn suggest(rules: &[Rule], paths: &[String], me: Option<&str>) -> Vec<String> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for path in paths {
        for owner in owners(rules, path) {
            if Some(owner.as_str()) == me {
                continue;
            }
            match counts.iter_mut().find(|(o, _)| o == owner) {
                Some((_, n)) => *n += 1,
                None => counts.push((owner.clone(), 1)),
            }
        }
    }
    counts.sort_by_key(|c| std::cmp::Reverse(c.1));
    counts.into_iter().map(|(o, _)| o).collect()
}

/// Whether a CODEOWNERS pattern takes `path` (relative, `/`-separated), as `.gitignore` reads it.
pub fn matches(pattern: &str, path: &str) -> bool {
    let anchored = pattern.starts_with('/');
    let p = pattern.trim_start_matches('/');
    let dir = p.ends_with('/');
    let p = p.trim_end_matches('/');
    if p.is_empty() || p == "*" && !dir {
        return true;
    }
    // A pattern with no slash inside names a file or folder at any depth.
    if !anchored && !p.contains('/') {
        let segments: Vec<&str> = path.split('/').collect();
        return segments
            .iter()
            .enumerate()
            .any(|(i, seg)| glob(p, seg) && (!dir || i + 1 < segments.len()));
    }
    // Otherwise from the root: the path itself, or a folder holding it.
    let mut prefix = String::new();
    for (i, seg) in path.split('/').enumerate() {
        if i > 0 {
            prefix.push('/');
        }
        prefix.push_str(seg);
        let whole = prefix.len() == path.len();
        if glob(p, &prefix) && (!dir || !whole) {
            return true;
        }
    }
    false
}

/// `*` within a segment, `**` across them, `?` one character.
fn glob(pattern: &str, text: &str) -> bool {
    fn go(p: &[u8], t: &[u8]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some(b'*') if p.get(1) == Some(&b'*') => {
                let rest = &p[2..];
                let rest = rest.strip_prefix(b"/").unwrap_or(rest);
                (0..=t.len()).any(|i| go(rest, &t[i..]))
            }
            Some(b'*') => (0..=t.len())
                .take_while(|&i| i == 0 || t[i - 1] != b'/')
                .any(|i| go(&p[1..], &t[i..])),
            Some(b'?') => !t.is_empty() && t[0] != b'/' && go(&p[1..], &t[1..]),
            Some(&c) => t.first() == Some(&c) && go(&p[1..], &t[1..]),
        }
    }
    go(pattern.as_bytes(), text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_read_as_gitignore_does() {
        assert!(matches("*", "a/b.kt"));
        assert!(matches("*.kt", "core/network/AuthApi.kt"));
        assert!(!matches("*.kt", "core/build.gradle"));
        assert!(matches("/core/", "core/network/AuthApi.kt"));
        assert!(!matches("/core/", "feature/core/x.kt"));
        assert!(matches("network/", "core/network/AuthApi.kt"));
        assert!(!matches("network/", "core/network"));
        assert!(matches(
            "/feature/login/**/test/",
            "feature/login/a/test/x.kt"
        ));
        assert!(matches("docs/*.md", "docs/a.md"));
        assert!(!matches("docs/*.md", "docs/sub/a.md"));
        assert!(matches("/README.md", "README.md"));
    }

    #[test]
    fn the_last_matching_line_owns_the_file_and_people_are_suggested_by_count() {
        let rules = parse(
            "# owners\n* @lead\n[Core]\n/core/ @ember @org/team mail@x.io\n*.kt @juniper # kotlin\n",
        );
        assert_eq!(owners(&rules, "core/network/AuthApi.kt"), ["juniper"]);
        assert_eq!(owners(&rules, "core/build.gradle"), ["ember"]);
        assert_eq!(owners(&rules, "README.md"), ["lead"]);
        let paths = ["core/a.kt", "core/b.gradle", "x.kt"].map(String::from);
        assert_eq!(suggest(&rules, &paths, Some("ember")), ["juniper"]);
        assert_eq!(suggest(&rules, &paths, None), ["juniper", "ember"]);
    }
}
