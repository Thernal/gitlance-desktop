//! Highlighting from a parse tree (#13, Design `docs/specs/highlighting.md`): tree-sitter for the
//! languages Design lists (D23). Each token gets a role — a declared function from a call, a
//! parameter from a local, a field, a constant — rather than only a colour. Every language's own
//! query (`assets/highlight/`) comes after the grammar's bundled one: its patterns name the roles
//! the bundled ones do not tell apart, and a later pattern capturing the same node wins. A locals query
//! gives a parameter's or a local's uses the role of its definition. Markdown injects its inline
//! grammar and the languages of its fenced code.

use super::Span;
use super::role::{self, *};
use std::sync::OnceLock;
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

/// The capture names recognised and the role each is drawn as; a capture takes the longest of
/// these it starts with (`keyword.function` is a keyword), one starting with none is not drawn.
const CAPTURES: [(&str, usize); 41] = [
    ("role.function", FUNCTION),
    ("role.function-call", FUNCTION_CALL),
    ("role.type", TYPE),
    ("role.parameter", PARAMETER),
    ("role.local", LOCAL),
    ("role.field", FIELD),
    ("role.constant", CONSTANT),
    ("role.annotation", ANNOTATION),
    ("role.named-argument", NAMED_ARGUMENT),
    ("keyword", KEYWORD),
    ("variable.builtin", KEYWORD),
    ("constant.builtin", KEYWORD),
    ("boolean", KEYWORD),
    ("function", FUNCTION_CALL),
    ("function.call", FUNCTION_CALL),
    ("function.method", FUNCTION_CALL),
    ("function.builtin", FUNCTION_CALL),
    ("function.macro", FUNCTION_CALL),
    ("constructor", TYPE),
    ("type", TYPE),
    ("module", TYPE),
    ("namespace", TYPE),
    ("variable.parameter", PARAMETER),
    ("variable.member", FIELD),
    ("property", FIELD),
    ("tag", FIELD),
    ("string.special.key", FIELD),
    ("text.title", FIELD),
    ("variable", LOCAL),
    ("constant", CONSTANT),
    ("number", NUMBER),
    ("string", STRING),
    ("character", STRING),
    ("text.literal", STRING),
    ("escape", OPERATOR),
    ("string.escape", OPERATOR),
    ("operator", OPERATOR),
    ("punctuation.special", OPERATOR),
    ("attribute", ANNOTATION),
    ("label", ANNOTATION),
    ("comment", COMMENT),
];

/// Links in Markdown: drawn as a call is.
const LINKS: [&str; 2] = ["text.uri", "text.reference"];

/// A language parsed with tree-sitter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Kotlin,
    Java,
    Rust,
    Swift,
    TypeScript,
    Tsx,
    JavaScript,
    Go,
    Python,
    Json,
    Yaml,
    Markdown,
    MarkdownInline,
}

const ALL: [Lang; 13] = [
    Lang::Kotlin,
    Lang::Java,
    Lang::Rust,
    Lang::Swift,
    Lang::TypeScript,
    Lang::Tsx,
    Lang::JavaScript,
    Lang::Go,
    Lang::Python,
    Lang::Json,
    Lang::Yaml,
    Lang::Markdown,
    Lang::MarkdownInline,
];

impl Lang {
    /// The file extensions it is chosen by.
    fn extensions(self) -> &'static [&'static str] {
        match self {
            Lang::Kotlin => &["kt", "kts"],
            Lang::Java => &["java"],
            Lang::Rust => &["rs"],
            Lang::Swift => &["swift"],
            Lang::TypeScript => &["ts", "mts", "cts"],
            Lang::Tsx => &["tsx"],
            Lang::JavaScript => &["js", "mjs", "cjs", "jsx"],
            Lang::Go => &["go"],
            Lang::Python => &["py", "pyi"],
            Lang::Json => &["json"],
            Lang::Yaml => &["yaml", "yml"],
            Lang::Markdown => &["md", "markdown"],
            Lang::MarkdownInline => &[],
        }
    }

    /// The names a code fence or an injection calls it by.
    fn names(self) -> &'static [&'static str] {
        match self {
            Lang::Kotlin => &["kotlin", "kt", "kts"],
            Lang::Java => &["java"],
            Lang::Rust => &["rust", "rs"],
            Lang::Swift => &["swift"],
            Lang::TypeScript => &["typescript", "ts"],
            Lang::Tsx => &["tsx"],
            Lang::JavaScript => &["javascript", "js", "jsx", "node"],
            Lang::Go => &["go", "golang"],
            Lang::Python => &["python", "py"],
            Lang::Json => &["json", "jsonc"],
            Lang::Yaml => &["yaml", "yml"],
            Lang::Markdown => &["markdown", "md"],
            Lang::MarkdownInline => &["markdown_inline"],
        }
    }

    /// Its language and queries: highlights (GitLance's last), injections, locals.
    fn parts(self) -> (tree_sitter::Language, String, &'static str, String) {
        use tree_sitter_javascript as js;
        use tree_sitter_typescript as ts;
        let own = |own: &str, bundled: &str| format!("{bundled}\n{own}");
        match self {
            Lang::Kotlin => (
                tree_sitter_kotlin_ng::LANGUAGE.into(),
                include_str!("../../assets/highlight/kotlin.scm").to_owned(),
                "",
                include_str!("../../assets/highlight/kotlin-locals.scm").to_owned(),
            ),
            Lang::Java => (
                tree_sitter_java::LANGUAGE.into(),
                own(
                    include_str!("../../assets/highlight/java.scm"),
                    tree_sitter_java::HIGHLIGHTS_QUERY,
                ),
                "",
                include_str!("../../assets/highlight/java-locals.scm").to_owned(),
            ),
            Lang::Rust => (
                tree_sitter_rust::LANGUAGE.into(),
                own(
                    include_str!("../../assets/highlight/rust.scm"),
                    tree_sitter_rust::HIGHLIGHTS_QUERY,
                ),
                tree_sitter_rust::INJECTIONS_QUERY,
                include_str!("../../assets/highlight/rust-locals.scm").to_owned(),
            ),
            Lang::Swift => (
                tree_sitter_swift::LANGUAGE.into(),
                own(
                    include_str!("../../assets/highlight/swift.scm"),
                    tree_sitter_swift::HIGHLIGHTS_QUERY,
                ),
                tree_sitter_swift::INJECTIONS_QUERY,
                tree_sitter_swift::LOCALS_QUERY.to_owned(),
            ),
            // TypeScript's query adds to JavaScript's, so its patterns come after.
            Lang::TypeScript | Lang::Tsx => {
                let jsx = if self == Lang::Tsx {
                    js::JSX_HIGHLIGHT_QUERY
                } else {
                    ""
                };
                (
                    if self == Lang::Tsx {
                        ts::LANGUAGE_TSX.into()
                    } else {
                        ts::LANGUAGE_TYPESCRIPT.into()
                    },
                    format!(
                        "{}\n{jsx}\n{}\n{}",
                        js::HIGHLIGHT_QUERY,
                        ts::HIGHLIGHTS_QUERY,
                        include_str!("../../assets/highlight/ecma.scm"),
                    ),
                    js::INJECTIONS_QUERY,
                    format!("{}\n{}", ts::LOCALS_QUERY, js::LOCALS_QUERY),
                )
            }
            Lang::JavaScript => (
                js::LANGUAGE.into(),
                format!(
                    "{}\n{}\n{}",
                    js::HIGHLIGHT_QUERY,
                    js::JSX_HIGHLIGHT_QUERY,
                    include_str!("../../assets/highlight/ecma.scm"),
                ),
                js::INJECTIONS_QUERY,
                js::LOCALS_QUERY.to_owned(),
            ),
            Lang::Go => (
                tree_sitter_go::LANGUAGE.into(),
                own(
                    include_str!("../../assets/highlight/go.scm"),
                    tree_sitter_go::HIGHLIGHTS_QUERY,
                ),
                "",
                include_str!("../../assets/highlight/go-locals.scm").to_owned(),
            ),
            Lang::Python => (
                tree_sitter_python::LANGUAGE.into(),
                own(
                    include_str!("../../assets/highlight/python.scm"),
                    tree_sitter_python::HIGHLIGHTS_QUERY,
                ),
                "",
                include_str!("../../assets/highlight/python-locals.scm").to_owned(),
            ),
            Lang::Json => (
                tree_sitter_json::LANGUAGE.into(),
                own(
                    include_str!("../../assets/highlight/json.scm"),
                    tree_sitter_json::HIGHLIGHTS_QUERY,
                ),
                "",
                String::new(),
            ),
            Lang::Yaml => (
                tree_sitter_yaml::LANGUAGE.into(),
                tree_sitter_yaml::HIGHLIGHTS_QUERY.to_owned(),
                "",
                String::new(),
            ),
            Lang::Markdown => (
                tree_sitter_md::LANGUAGE.into(),
                tree_sitter_md::HIGHLIGHT_QUERY_BLOCK.to_owned(),
                include_str!("../../assets/highlight/markdown-injections.scm"),
                String::new(),
            ),
            Lang::MarkdownInline => (
                tree_sitter_md::INLINE_LANGUAGE.into(),
                tree_sitter_md::HIGHLIGHT_QUERY_INLINE.to_owned(),
                tree_sitter_md::INJECTION_QUERY_INLINE,
                String::new(),
            ),
        }
    }

    /// Its highlighting, built the first time it is asked for; None when a query does not build.
    fn config(self) -> Option<&'static HighlightConfiguration> {
        static CONFIGS: [OnceLock<Option<HighlightConfiguration>>; ALL.len()] =
            [const { OnceLock::new() }; ALL.len()];
        let ix = ALL.iter().position(|&l| l == self)?;
        CONFIGS[ix]
            .get_or_init(|| {
                let (language, highlights, injections, locals) = self.parts();
                let name = self.names()[0];
                let mut config =
                    HighlightConfiguration::new(language, name, &highlights, injections, &locals)
                        .ok()?;
                config.configure(&recognised());
                Some(config)
            })
            .as_ref()
    }
}

/// Every capture name recognised, in the order `Highlight(n)` numbers them.
fn recognised() -> Vec<&'static str> {
    CAPTURES
        .iter()
        .map(|(name, _)| *name)
        .chain(LINKS)
        .collect()
}

/// The role `Highlight(n)` is drawn as.
fn role_of(n: usize) -> usize {
    CAPTURES.get(n).map_or(FUNCTION_CALL, |(_, r)| *r)
}

/// The language a file is parsed as, by its extension.
pub fn by_path(path: &str) -> Option<Lang> {
    let ext = std::path::Path::new(path)
        .extension()?
        .to_str()?
        .to_ascii_lowercase();
    ALL.into_iter()
        .find(|l| l.extensions().contains(&ext.as_str()))
}

/// The language a code fence or an injection names.
pub fn by_name(name: &str) -> Option<Lang> {
    let name = name.trim().to_ascii_lowercase();
    ALL.into_iter().find(|l| l.names().contains(&name.as_str()))
}

/// The spans of every line of `text` parsed as `lang`; None when it cannot be (a query that does
/// not build, a parse that fails): the caller falls back to syntect.
pub fn spans(lang: Lang, text: &str) -> Option<Vec<Vec<Span>>> {
    let config = lang.config()?;
    let starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .filter(|&i| i < text.len() || i == 0)
        .collect();
    let mut out: Vec<Vec<Span>> = vec![Vec::new(); text.split_inclusive('\n').count().max(1)];
    let mut highlighter = Highlighter::new();
    let events = highlighter
        .highlight(config, text.as_bytes(), None, None, |name| {
            by_name(name).and_then(Lang::config)
        })
        .ok()?;
    let mut stack: Vec<usize> = Vec::new();
    for event in events {
        match event.ok()? {
            HighlightEvent::HighlightStart(h) => stack.push(role_of(h.0)),
            HighlightEvent::HighlightEnd => {
                stack.pop();
            }
            HighlightEvent::Source { start, end } => {
                if let Some(&r) = stack.last() {
                    paint(&mut out, &starts, text, start..end, r);
                }
            }
        }
    }
    Some(out)
}

/// Bytes `range` of `text` drawn as role `r`, split at its line ends.
fn paint(
    out: &mut [Vec<Span>],
    starts: &[usize],
    text: &str,
    range: std::ops::Range<usize>,
    r: usize,
) {
    let mut at = range.start;
    while at < range.end {
        let line = starts.partition_point(|&s| s <= at).saturating_sub(1);
        let line_start = starts[line];
        let next = starts.get(line + 1).copied().unwrap_or(text.len());
        // Without the line's ending.
        let content = text[line_start..next].trim_end_matches(['\n', '\r']).len();
        let end = range.end.min(line_start + content);
        if at < end
            && let Some(spans) = out.get_mut(line)
        {
            let look = &role::ROLES[r];
            let span = Span {
                range: at - line_start..end - line_start,
                color: role::BASE + r as u32,
                italic: look.italic,
                bold: look.bold,
            };
            match spans.last_mut() {
                Some(last) if last.range.end == span.range.start && last.color == span.color => {
                    last.range.end = span.range.end
                }
                _ => spans.push(span),
            }
        }
        at = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_builds() {
        for lang in ALL {
            assert!(lang.config().is_some(), "{lang:?}: a query does not build");
        }
    }
}
