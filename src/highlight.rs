//! Whole-file syntax highlighting, split into per-line spans. A span says which of the theme's code
//! tokens it is drawn in (`code-01` … `code-08`), not a colour: a theme changes without highlighting
//! again.

use std::ops::Range;
use std::path::Path;
use std::sync::OnceLock;
use syntect::highlighting::{
    Color, FontStyle, HighlightIterator, HighlightState, Highlighter, ScopeSelectors,
    StyleModifier, Theme, ThemeItem, ThemeSettings,
};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};

/// Files with a line longer than this are shown unhighlighted: syntect is slow on minified code.
const MAX_LINE_BYTES: usize = 4096;
/// Files larger than this are shown unhighlighted: syntect takes seconds on them.
const MAX_FILE_BYTES: usize = 1 << 20;

/// The code tokens a span is drawn in (`--code-0N`); 0 is the code's text.
pub mod palette {
    pub const FOREGROUND: u32 = 0;
    /// Keywords, storage.
    pub const KEYWORD: u32 = 1;
    /// Functions, links.
    pub const BLUE: u32 = 2;
    /// Strings.
    pub const GREEN: u32 = 3;
    /// Numbers, constants, attributes.
    pub const ORANGE: u32 = 4;
    /// Types, modules.
    pub const YELLOW: u32 = 5;
    /// Comments.
    pub const COMMENT: u32 = 6;
    /// Members, properties, tags, headings.
    pub const MEMBER: u32 = 7;
    /// Operators, escapes.
    pub const CYAN: u32 = 8;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    /// Byte range within the line, without its line ending.
    pub range: Range<usize>,
    /// The code token: `palette::KEYWORD` … `palette::CYAN`.
    pub color: u32,
    pub italic: bool,
}

/// The spans of every line of `text`; a line without spans is plain foreground.
pub fn highlight(path: &str, text: &str) -> Vec<Vec<Span>> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    if text.len() > MAX_FILE_BYTES || lines.iter().any(|l| l.len() > MAX_LINE_BYTES) {
        return vec![Vec::new(); lines.len()];
    }
    let syntaxes = syntaxes();
    let syntax = syntax_for(syntaxes, path, lines.first().copied().unwrap_or_default());
    spans_of(syntax, lines)
}

/// The spans of a code block in a comment, by its fence's language (`rust`, `kt`, `Kotlin`).
pub fn highlight_lang(lang: &str, text: &str) -> Vec<Vec<Span>> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let syntaxes = syntaxes();
    match syntaxes.find_syntax_by_token(lang) {
        Some(syntax) if lines.iter().all(|l| l.len() <= MAX_LINE_BYTES) => spans_of(syntax, lines),
        _ => vec![Vec::new(); lines.len()],
    }
}

fn spans_of(syntax: &SyntaxReference, lines: Vec<&str>) -> Vec<Vec<Span>> {
    let syntaxes = syntaxes();
    let highlighter = Highlighter::new(theme());
    let mut parse = ParseState::new(syntax);
    let mut state = HighlightState::new(&highlighter, ScopeStack::new());

    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        let Ok(ops) = parse.parse_line(line, syntaxes) else {
            out.push(Vec::new());
            continue;
        };
        let content_len = line.trim_end_matches(['\n', '\r']).len();
        let mut spans = Vec::new();
        let mut start = 0;
        for (style, piece) in HighlightIterator::new(&mut state, &ops, line, &highlighter) {
            let end = (start + piece.len()).min(content_len);
            let color = rgb(style.foreground);
            let italic = style.font_style.contains(FontStyle::ITALIC);
            if start < end && (color != palette::FOREGROUND || italic) {
                match spans.last_mut() {
                    Some(Span {
                        range,
                        color: c,
                        italic: i,
                    }) if range.end == start && *c == color && *i == italic => range.end = end,
                    _ => spans.push(Span {
                        range: start..end,
                        color,
                        italic,
                    }),
                }
            }
            start += piece.len();
        }
        out.push(spans);
    }
    out
}

fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

fn syntax_for<'a>(set: &'a SyntaxSet, path: &str, first_line: &str) -> &'a SyntaxReference {
    let path = Path::new(path);
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    set.find_syntax_by_extension(name)
        .or_else(|| set.find_syntax_by_extension(ext))
        .or_else(|| set.find_syntax_by_first_line(first_line))
        .unwrap_or_else(|| set.find_syntax_plain_text())
}

/// The code token a style carries in its red channel (see `color`).
fn rgb(c: Color) -> u32 {
    c.r as u32
}

fn theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(|| {
        use palette::*;
        let rules: &[(&str, u32, bool)] = &[
            ("comment, punctuation.definition.comment", COMMENT, true),
            ("string, punctuation.definition.string", GREEN, false),
            ("constant.character.escape, string.regexp", CYAN, false),
            (
                "constant.numeric, constant.language, constant.character, support.constant",
                ORANGE,
                false,
            ),
            ("keyword, storage, storage.type, storage.modifier", KEYWORD, false),
            ("keyword.operator", CYAN, false),
            (
                "entity.name.function, support.function, meta.function-call variable.function, variable.function",
                BLUE,
                false,
            ),
            (
                "entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, entity.name.trait, entity.name.impl, entity.other.inherited-class, support.type, support.class, storage.type.primitive",
                YELLOW,
                false,
            ),
            (
                "variable.other.member, variable.other.property, support.variable.property, meta.property-name, entity.name.tag, variable.language",
                MEMBER,
                false,
            ),
            (
                "entity.other.attribute-name, meta.attribute, constant.other.symbol, variable.parameter.function.language.special",
                ORANGE,
                false,
            ),
            ("entity.name.namespace, entity.name.module", YELLOW, false),
            ("markup.heading, entity.name.section", MEMBER, false),
            ("markup.bold", ORANGE, false),
            ("markup.italic", KEYWORD, true),
            ("markup.inline.raw, markup.raw", GREEN, false),
            ("markup.underline.link, string.other.link", BLUE, false),
            ("meta.diff.header, meta.diff.range", BLUE, false),
            ("markup.inserted", GREEN, false),
            ("markup.deleted", MEMBER, false),
            ("invalid", MEMBER, false),
        ];
        Theme {
            name: Some("GitLance roles".into()),
            author: None,
            settings: ThemeSettings {
                foreground: Some(color(FOREGROUND)),
                ..ThemeSettings::default()
            },
            scopes: rules
                .iter()
                .map(|(scope, fg, italic)| ThemeItem {
                    scope: scope.parse::<ScopeSelectors>().expect("valid selector"),
                    style: StyleModifier {
                        foreground: Some(color(*fg)),
                        background: None,
                        font_style: italic.then_some(FontStyle::ITALIC),
                    },
                })
                .collect(),
        }
    })
}

/// A code token passed through syntect as a colour: its number in the red channel.
fn color(role: u32) -> Color {
    Color {
        r: role as u8,
        g: 0,
        b: 0,
        a: 0xff,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_tokens_get_their_code_tokens() {
        let lines = highlight("main.rs", "// hi\nfn main() { let s = \"x\"; }\n");
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0],
            [Span {
                range: 0..5,
                color: palette::COMMENT,
                italic: true
            }]
        );
        let colored = |text: &str, color: u32| {
            let line = "fn main() { let s = \"x\"; }";
            let start = line.find(text).unwrap();
            lines[1]
                .iter()
                .any(|s| s.color == color && s.range.start <= start && start < s.range.end)
        };
        assert!(colored("fn", palette::KEYWORD));
        assert!(colored("main", palette::BLUE));
        assert!(colored("\"x\"", palette::GREEN));
    }

    #[test]
    fn unknown_files_are_plain() {
        let lines = highlight("notes.unknownext", "just text\n");
        assert_eq!(lines, [Vec::<Span>::new()]);
    }
}
