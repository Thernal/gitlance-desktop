//! The functions and types a file's diff touches: a declaration is found by its keyword, not by a
//! grammar, so it works for the common languages and says nothing for the rest. Pure text in, a
//! list out.

use crate::git::{FileDiff, LineKind};

/// One declaration of a file and what the diff did to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    /// `+` the declaration is new, `~` its body changed, `−` it is gone.
    pub mark: char,
    /// `fn`, `struct`, `class`, `def` …
    pub kind: &'static str,
    pub name: String,
    /// 1-based, on the old side when `old`.
    pub line: u32,
    pub old: bool,
}

const KINDS: &[&str] = &[
    "fn",
    "struct",
    "enum",
    "trait",
    "impl",
    "mod",
    "class",
    "interface",
    "object",
    "def",
    "func",
    "function",
    "fun",
    "type",
    "protocol",
    "extension",
    "record",
];

/// Words that may stand before the keyword.
const MODIFIERS: &[&str] = &[
    "pub",
    "pub(crate)",
    "pub(super)",
    "async",
    "unsafe",
    "const",
    "static",
    "public",
    "private",
    "protected",
    "internal",
    "export",
    "default",
    "final",
    "abstract",
    "open",
    "override",
    "suspend",
    "data",
    "sealed",
    "inline",
    "extern",
    "@objc",
    "mutating",
];

/// The declaration on a line: its kind and name.
pub fn declaration(line: &str) -> Option<(&'static str, String)> {
    let mut words = line.split_whitespace().peekable();
    while words.peek().is_some_and(|w| MODIFIERS.contains(w)) {
        words.next();
    }
    let kind = *KINDS.iter().find(|k| Some(**k) == words.peek().copied())?;
    words.next();
    let name: String = words
        .next()?
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '<' && kind == "impl")
        .collect();
    let name = name.trim_end_matches('<').to_owned();
    (!name.is_empty()).then_some((kind, name))
}

/// The function (or other declaration) around 1-based line `line` of a file whose lines are
/// `line_of(1..=count)`: from its declaration to the line that closes it — the first line after it
/// indented no deeper, taken in when it only closes (`}`, `)`, `end`) at the same depth. `None` outside any, or for
/// one longer than `MOST` lines (a class, not a function).
pub fn enclosing<'a>(
    count: u32,
    line: u32,
    line_of: impl Fn(u32) -> &'a str,
) -> Option<(u32, u32)> {
    const MOST: u32 = 400;
    let indent = |text: &str| text.len() - text.trim_start().len();
    let blank = |text: &str| text.trim().is_empty();
    // The nearest declaration above that reaches the line: a line between two functions belongs
    // to what holds them.
    let reach = |start: u32| {
        let depth = indent(line_of(start));
        let mut end = start;
        for n in start + 1..=count.min(start + MOST) {
            let text = line_of(n);
            if blank(text) {
                continue;
            }
            if indent(text) <= depth {
                let t = text.trim_start();
                let closes =
                    t.starts_with('}') || t.starts_with(')') || t.starts_with(']') || t == "end";
                // Its own closing line, not the one of what holds it.
                if closes && indent(text) == depth {
                    end = n;
                }
                break;
            }
            end = n;
        }
        end
    };
    (line.saturating_sub(MOST).max(1)..=line.min(count))
        .rev()
        .filter(|&n| declaration(line_of(n)).is_some())
        .map(|start| (start, reach(start)))
        .find(|&(_, end)| end >= line)
}

/// The symbols of `file` the diff touches, in file order, removed ones last.
pub fn changed(file: &FileDiff) -> Vec<Symbol> {
    let new_text = file.new_text.as_deref().unwrap_or_default();
    let old_text = file.old_text.as_deref().unwrap_or_default();
    let added: std::collections::HashSet<u32> = file
        .hunks
        .iter()
        .flat_map(|h| &h.lines)
        .filter(|l| l.kind == LineKind::Added)
        .filter_map(|l| l.new_line)
        .collect();

    // The declarations of the new text, and for each the lines up to the next one.
    let decls: Vec<(u32, &'static str, String)> = new_text
        .lines()
        .enumerate()
        .filter_map(|(i, l)| declaration(l).map(|(k, n)| (i as u32 + 1, k, n)))
        .collect();
    let total = new_text.lines().count() as u32;
    let mut out = Vec::new();
    for (i, (line, kind, name)) in decls.iter().enumerate() {
        let end = decls.get(i + 1).map_or(total + 1, |d| d.0);
        let mark = if added.contains(line) {
            '+'
        } else if (*line..end).any(|n| added.contains(&n)) {
            '~'
        } else {
            continue;
        };
        out.push(Symbol {
            mark,
            kind,
            name: name.clone(),
            line: *line,
            old: false,
        });
    }

    // A declaration on a removed line.
    let new_names: std::collections::HashSet<&str> =
        decls.iter().map(|(_, _, n)| n.as_str()).collect();
    let removed: std::collections::HashSet<u32> = file
        .hunks
        .iter()
        .flat_map(|h| &h.lines)
        .filter(|l| l.kind == LineKind::Removed)
        .filter_map(|l| l.old_line)
        .collect();
    for (i, text) in old_text.lines().enumerate() {
        let line = i as u32 + 1;
        if !removed.contains(&line) {
            continue;
        }
        if let Some((kind, name)) = declaration(text)
            && !new_names.contains(name.as_str())
        {
            out.push(Symbol {
                mark: '−',
                kind,
                name,
                line,
                old: true,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_function_around_a_line_ends_where_it_closes() {
        let text = "class A {\n    fun one() {\n        a()\n\n        b()\n    }\n\n    fun two() = c()\n}\n";
        let lines: Vec<&str> = text.lines().collect();
        let at = |n: u32| lines[n as usize - 1];
        let count = lines.len() as u32;
        assert_eq!(enclosing(count, 5, at), Some((2, 6)));
        assert_eq!(enclosing(count, 2, at), Some((2, 6)));
        assert_eq!(enclosing(count, 8, at), Some((8, 8)));
        // Between the two, the class is what holds it.
        assert_eq!(enclosing(count, 7, at), Some((1, 9)));
        let py = ["def f():", "    x = 1", "    return x", "", "y = 2"];
        assert_eq!(enclosing(5, 3, |n| py[n as usize - 1]), Some((1, 3)));
        assert_eq!(enclosing(5, 5, |n| py[n as usize - 1]), None);
    }

    #[test]
    fn declarations_are_read_by_keyword() {
        assert_eq!(
            declaration("pub struct NewRequest {"),
            Some(("struct", "NewRequest".into()))
        );
        assert_eq!(
            declaration("    pub(super) async fn open_create(&mut self) {"),
            Some(("fn", "open_create".into()))
        );
        assert_eq!(
            declaration("class Foo : Bar() {"),
            Some(("class", "Foo".into()))
        );
        assert_eq!(declaration("def retry(n):"), Some(("def", "retry".into())));
        assert_eq!(declaration("let fn_count = 1;"), None);
        assert_eq!(declaration("// fn commented()"), None);
    }

    #[test]
    fn an_impl_is_named_by_its_type() {
        assert_eq!(
            declaration("impl Workspace {"),
            Some(("impl", "Workspace".into()))
        );
    }

    fn file(old: &str, new: &str, added: &[u32], removed: &[u32]) -> FileDiff {
        use crate::git::{ChangeKind, DiffLine, Hunk};
        let lines = added
            .iter()
            .map(|&n| DiffLine {
                kind: LineKind::Added,
                old_line: None,
                new_line: Some(n),
            })
            .chain(removed.iter().map(|&n| DiffLine {
                kind: LineKind::Removed,
                old_line: Some(n),
                new_line: None,
            }))
            .collect();
        FileDiff {
            old_path: Some("a.rs".into()),
            new_path: Some("a.rs".into()),
            change: ChangeKind::Modified,
            added: added.len(),
            removed: removed.len(),
            hunks: vec![Hunk { lines }],
            old_text: Some(old.into()),
            new_text: Some(new.into()),
            note: None,
            old_image: None,
            new_image: None,
            pictures: Vec::new(),
        }
    }

    #[test]
    fn changed_symbols_are_new_edited_or_gone() {
        let old = "fn keep() {
    1
}
fn edit() {
    1
}
fn gone() {
}
";
        let new = "fn keep() {
    1
}
fn edit() {
    2
}
fn fresh() {
}
";
        let symbols = changed(&file(old, new, &[5, 7, 8], &[5, 7, 8]));
        let marks: Vec<(char, &str)> = symbols.iter().map(|s| (s.mark, s.name.as_str())).collect();
        assert_eq!(marks, [('~', "edit"), ('+', "fresh"), ('−', "gone")]);
    }
}
