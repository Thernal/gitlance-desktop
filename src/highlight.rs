//! Whole-file syntax highlighting, split into per-line spans. A span says which of the theme's code
//! tokens it is drawn in (`code-01` … `code-08`), or which role (`role::BASE` + n), not a colour: a
//! theme changes without highlighting again. The languages Design lists are parsed with tree-sitter
//! and get roles (`tree.rs`, #13); every other file goes through syntect's grammars.

mod tree;

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

/// The roles a parse tree gives tokens (#13, Design `design/next/highlighting/a-roles.html`): a
/// theme's JSON sets one by its name (`code.parameter`); otherwise it is drawn with its code token
/// and a slant or weight, so every theme covers them.
pub mod role {
    /// A span's `color` from here on is a role: `BASE` + its index in `ROLES`.
    pub const BASE: u32 = 16;
    /// The secondary text colour, for the one role drawn without a code token.
    pub const MUTED: u32 = 9;

    pub struct Look {
        /// `code.<name>` in a theme's JSON.
        pub name: &'static str,
        /// What it is drawn with unless the theme gives it a colour: a code token, 0 the code's
        /// text, `MUTED` the secondary text.
        pub token: u32,
        pub italic: bool,
        pub bold: bool,
    }

    const fn look(name: &'static str, token: u32, italic: bool, bold: bool) -> Look {
        Look {
            name,
            token,
            italic,
            bold,
        }
    }

    pub const KEYWORD: usize = 0;
    pub const FUNCTION: usize = 1;
    pub const FUNCTION_CALL: usize = 2;
    pub const TYPE: usize = 3;
    pub const PARAMETER: usize = 4;
    pub const LOCAL: usize = 5;
    pub const FIELD: usize = 6;
    pub const CONSTANT: usize = 7;
    pub const NUMBER: usize = 8;
    pub const STRING: usize = 9;
    pub const ANNOTATION: usize = 10;
    pub const NAMED_ARGUMENT: usize = 11;
    pub const OPERATOR: usize = 12;
    pub const COMMENT: usize = 13;

    pub const ROLES: [Look; 14] = [
        look("keyword", 1, false, false),
        look("function", 2, false, true),
        look("function-call", 2, false, false),
        look("type", 5, false, false),
        look("parameter", 0, true, false),
        look("local-variable", 0, false, false),
        look("field", 7, false, false),
        look("constant", 4, true, false),
        look("number", 4, false, false),
        look("string", 3, false, false),
        look("annotation", 8, false, false),
        look("named-argument", MUTED, false, false),
        look("operator", 8, false, false),
        look("comment", 6, true, false),
    ];

    /// The role a theme's `code.<name>` names.
    pub fn named(name: &str) -> Option<usize> {
        ROLES.iter().position(|r| r.name == name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    /// Byte range within the line, without its line ending.
    pub range: Range<usize>,
    /// The code token (`palette::KEYWORD` … `palette::CYAN`) or the role (`role::BASE` + n).
    pub color: u32,
    pub italic: bool,
    pub bold: bool,
}

/// The spans of every line of `text`; a line without spans is plain foreground.
pub fn highlight(path: &str, text: &str) -> Vec<Vec<Span>> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    if text.len() > MAX_FILE_BYTES || lines.iter().any(|l| l.len() > MAX_LINE_BYTES) {
        return vec![Vec::new(); lines.len()];
    }
    if let Some(spans) = tree::by_path(path).and_then(|lang| tree::spans(lang, text)) {
        return spans;
    }
    let syntaxes = syntaxes();
    let syntax = syntax_for(syntaxes, path, lines.first().copied().unwrap_or_default());
    spans_of(syntax, lines)
}

/// The spans of a code block in a comment, by its fence's language (`rust`, `kt`, `Kotlin`).
pub fn highlight_lang(lang: &str, text: &str) -> Vec<Vec<Span>> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    if lines.iter().all(|l| l.len() <= MAX_LINE_BYTES)
        && let Some(spans) = tree::by_name(lang).and_then(|lang| tree::spans(lang, text))
    {
        return spans;
    }
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
                        ..
                    }) if range.end == start && *c == color && *i == italic => range.end = end,
                    _ => spans.push(Span {
                        range: start..end,
                        color,
                        italic,
                        bold: false,
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

    /// The role of the span over `token` (its `n`th appearance) on line `line` of `text` as `path`.
    fn role_at(path: &str, text: &str, line: usize, token: &str, n: usize) -> Option<usize> {
        let lines = highlight(path, text);
        let source = text.lines().nth(line)?;
        let at = source.match_indices(token).nth(n)?.0;
        lines[line]
            .iter()
            .find(|s| s.range.start <= at && at < s.range.end)
            .map(|s| (s.color - role::BASE) as usize)
    }

    #[test]
    fn kotlin_reads_like_the_design() {
        // Design's sample (`a-roles.html`), line by line.
        let text = "@Singleton\n\
            class RefundCalculator(private val fx: Fx, private val clock: Clock) {\n\
            private val mode = RoundingMode.HALF_EVEN\n\
            companion object { const val MAX_PARTIAL = 12 }\n\
            fun refund(capture: Capture, amount: Money): Money {\n\
            // Round once, in the currency the customer paid in.\n\
            val exact = amount.round(mode)\n\
            require(exact <= capture.remaining) { \"Refund exceeds the capture\" }\n\
            return fx.convert(exact, to = capture.currency, at = clock.now())\n\
            }\n\
            val ok = x != null\n\
            }\n";
        let r = |line, token, n| role_at("RefundCalculator.kt", text, line, token, n);
        use role::*;
        assert_eq!(r(0, "Singleton", 0), Some(ANNOTATION));
        assert_eq!(r(1, "class", 0), Some(KEYWORD));
        assert_eq!(r(1, "RefundCalculator", 0), Some(TYPE));
        assert_eq!(r(1, "fx", 0), Some(FIELD));
        assert_eq!(r(1, "Fx", 0), Some(TYPE));
        assert_eq!(r(2, "mode", 0), Some(FIELD));
        assert_eq!(r(2, "RoundingMode", 0), Some(TYPE));
        assert_eq!(r(2, "HALF_EVEN", 0), Some(CONSTANT));
        assert_eq!(r(3, "MAX_PARTIAL", 0), Some(CONSTANT));
        assert_eq!(r(3, "12", 0), Some(NUMBER));
        assert_eq!(r(4, "refund", 0), Some(FUNCTION));
        assert_eq!(r(4, "capture", 0), Some(PARAMETER));
        assert_eq!(r(4, "Money", 0), Some(TYPE));
        assert_eq!(r(5, "Round", 0), Some(COMMENT));
        assert_eq!(r(6, "exact", 0), Some(LOCAL));
        // A parameter keeps its role where it is used; the called name is a call.
        assert_eq!(r(6, "amount", 0), Some(PARAMETER));
        assert_eq!(r(6, "round", 0), Some(FUNCTION_CALL));
        assert_eq!(r(6, "mode", 0), Some(FIELD));
        assert_eq!(r(7, "require", 0), Some(FUNCTION_CALL));
        assert_eq!(r(7, "<=", 0), Some(OPERATOR));
        assert_eq!(r(7, "remaining", 0), Some(FIELD));
        assert_eq!(r(7, "Refund", 0), Some(STRING));
        assert_eq!(r(8, "return", 0), Some(KEYWORD));
        assert_eq!(r(8, "convert", 0), Some(FUNCTION_CALL));
        assert_eq!(r(8, "to", 0), Some(NAMED_ARGUMENT));
        assert_eq!(r(8, "at", 0), Some(NAMED_ARGUMENT));
        assert_eq!(r(10, "null", 0), Some(KEYWORD));
        // A declared function is drawn semibold, a parameter in italic.
        let lines = highlight("RefundCalculator.kt", text);
        let at = |line: usize, token: &str| {
            let at = text.lines().nth(line).unwrap().find(token).unwrap();
            lines[line]
                .iter()
                .find(|s| s.range.start <= at && at < s.range.end)
                .cloned()
                .unwrap()
        };
        assert!(at(4, "refund").bold && at(4, "capture").italic);
    }

    #[test]
    fn rust_tokens_get_their_roles() {
        let text = "// hi\nfn main() { let s = \"x\"; run(s); }\n";
        let r = |line, token, n| role_at("main.rs", text, line, token, n);
        use role::*;
        assert_eq!(r(0, "hi", 0), Some(COMMENT));
        assert_eq!(r(1, "fn", 0), Some(KEYWORD));
        assert_eq!(r(1, "main", 0), Some(FUNCTION));
        assert_eq!(r(1, "\"x\"", 0), Some(STRING));
        assert_eq!(r(1, "run", 0), Some(FUNCTION_CALL));
    }

    #[test]
    fn every_language_tells_a_declared_function_from_a_call() {
        use role::*;
        let cases: [(&str, &str, &str, &str); 7] = [
            ("A.java", "class A { void run() { go(); } }\n", "run", "go"),
            ("a.go", "package a\nfunc run() { do() }\n", "run", "do"),
            ("a.py", "def run():\n    go()\n", "run", "go"),
            ("a.ts", "function run(): void { go(); }\n", "run", "go"),
            ("a.tsx", "function run() { return go(); }\n", "run", "go"),
            ("a.js", "function run() { go(); }\n", "run", "go"),
            ("a.swift", "func run() { go() }\n", "run", "go"),
        ];
        for (path, text, declared, called) in cases {
            let line = |token: &str| text.lines().position(|l| l.contains(token)).unwrap();
            assert_eq!(
                role_at(path, text, line(declared), declared, 0),
                Some(FUNCTION),
                "{path}: {declared}"
            );
            let n = usize::from(declared == called);
            assert_eq!(
                role_at(path, text, line(called), called, n),
                Some(FUNCTION_CALL),
                "{path}: {called}"
            );
        }
        // Data: keys as fields, values as strings and numbers.
        assert_eq!(
            role_at("a.json", "{\"a\": 1}\n", 0, "\"a\"", 0),
            Some(FIELD)
        );
        assert_eq!(role_at("a.yml", "a: 1\n", 0, "1", 0), Some(NUMBER));
        // Markdown: a heading, and a fenced Kotlin block parsed as Kotlin.
        let md = "# Title\n\n```kotlin\nfun go() = 1\n```\n";
        assert_eq!(role_at("a.md", md, 0, "Title", 0), Some(FIELD));
        assert_eq!(role_at("a.md", md, 3, "go", 0), Some(FUNCTION));
    }

    #[test]
    fn a_language_without_a_parser_keeps_syntect() {
        let lines = highlight("main.c", "// hi\nint main() {}\n");
        assert_eq!(lines[0][0].color, palette::COMMENT);
    }

    #[test]
    fn unknown_files_are_plain() {
        let lines = highlight("notes.unknownext", "just text\n");
        assert_eq!(lines, [Vec::<Span>::new()]);
    }
}
