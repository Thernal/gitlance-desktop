//! The text of a field being edited: a caret, a selection, word and line movement, undo. No GPUI
//! here — the UI maps keys and clicks onto these calls and draws `text`, `caret` and `selection`.
//! Positions are byte offsets on char boundaries.

/// One edit field's state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextEdit {
    text: String,
    caret: usize,
    /// The other end of the selection, when there is one.
    anchor: Option<usize>,
    undo: Vec<(String, usize)>,
    redo: Vec<(String, usize)>,
    /// The column ↑ and ↓ try to keep, in chars.
    column: Option<usize>,
    /// Where the last typed char ended: typing on from there stays one undo step until a new word.
    run: Option<usize>,
}

/// Where a movement goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
    Up,
    Down,
    Start,
    End,
}

impl TextEdit {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let caret = text.len();
        Self {
            text,
            caret,
            ..Self::default()
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    /// The selected range, start before end; `None` when nothing is selected.
    pub fn selection(&self) -> Option<std::ops::Range<usize>> {
        let anchor = self.anchor.filter(|&a| a != self.caret)?;
        Some(anchor.min(self.caret)..anchor.max(self.caret))
    }

    pub fn selected_text(&self) -> Option<&str> {
        self.selection().map(|r| &self.text[r])
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.caret = self.text.len();
        self.column = None;
    }

    /// Puts the caret at `at` (a click); with `extend` (⇧-click, a drag) the selection grows to it.
    pub fn place(&mut self, at: usize, extend: bool) {
        let at = self.floor(at.min(self.text.len()));
        self.run = None;
        if extend {
            self.anchor.get_or_insert(self.caret);
        } else {
            self.anchor = None;
        }
        self.caret = at;
        self.column = None;
    }

    /// Selects the word around `at` (a double click).
    pub fn select_word(&mut self, at: usize) {
        let at = self.floor(at.min(self.text.len()));
        let start = self.text[..at]
            .char_indices()
            .rev()
            .take_while(|(_, c)| is_word(*c))
            .last()
            .map_or(at, |(i, _)| i);
        let end = self.text[at..]
            .char_indices()
            .find(|(_, c)| !is_word(*c))
            .map_or(self.text.len(), |(i, _)| at + i);
        self.anchor = Some(start);
        self.caret = end;
    }

    /// Moves the caret; with `select` the selection follows it. A move without `select` from a
    /// selection lands on the selection's edge, as text fields do.
    pub fn move_to(&mut self, motion: Move, select: bool) {
        if !select && let Some(sel) = self.selection() {
            match motion {
                Move::Left | Move::WordLeft => {
                    self.caret = sel.start;
                    self.anchor = None;
                    return;
                }
                Move::Right | Move::WordRight => {
                    self.caret = sel.end;
                    self.anchor = None;
                    return;
                }
                _ => {}
            }
        }
        if select {
            self.anchor.get_or_insert(self.caret);
        } else {
            self.anchor = None;
        }
        self.run = None;
        if !matches!(motion, Move::Up | Move::Down) {
            self.column = None;
        }
        self.caret = match motion {
            Move::Left => self.prev_char(self.caret),
            Move::Right => self.next_char(self.caret),
            Move::WordLeft => self.word_left(),
            Move::WordRight => self.word_right(),
            Move::LineStart => self.line_start(self.caret),
            Move::LineEnd => self.line_end(self.caret),
            Move::Start => 0,
            Move::End => self.text.len(),
            Move::Up | Move::Down => self.vertical(motion == Move::Up),
        };
    }

    /// Types `s` over the selection, or at the caret.
    pub fn insert(&mut self, s: &str) {
        let one_char = s.chars().count() == 1;
        let continues = !self.text[..self.caret].ends_with(char::is_whitespace)
            || s.starts_with(char::is_whitespace);
        let typing_on =
            one_char && continues && self.selection().is_none() && self.run == Some(self.caret);
        if !typing_on {
            self.remember();
        }
        let at = match self.selection() {
            Some(sel) => {
                self.text.replace_range(sel.clone(), "");
                sel.start
            }
            None => self.caret,
        };
        self.text.insert_str(at, s);
        self.caret = at + s.len();
        self.anchor = None;
        self.column = None;
        self.run = one_char.then_some(self.caret);
    }

    /// Backspace: the selection, or the char (with `word`, the word; with `line`, to the line's
    /// start) before the caret.
    pub fn delete_back(&mut self, word: bool, line: bool) {
        if self.delete_selection() {
            return;
        }
        let from = if line {
            let start = self.line_start(self.caret);
            if start == self.caret {
                self.prev_char(self.caret)
            } else {
                start
            }
        } else if word {
            self.word_left()
        } else {
            self.prev_char(self.caret)
        };
        self.delete_range(from..self.caret);
    }

    /// Forward delete: the selection, or the char (with `word`, the word) after the caret.
    pub fn delete_forward(&mut self, word: bool) {
        if self.delete_selection() {
            return;
        }
        let to = if word {
            self.word_right()
        } else {
            self.next_char(self.caret)
        };
        self.delete_range(self.caret..to);
    }

    /// Replaces `range` with `with` (one undo step) and selects `select`, a range of the new text.
    pub fn replace(
        &mut self,
        range: std::ops::Range<usize>,
        with: &str,
        select: std::ops::Range<usize>,
    ) {
        self.remember();
        self.run = None;
        self.text.replace_range(range, with);
        let clamp = |at: usize| self.floor(at.min(self.text.len()));
        let (start, end) = (clamp(select.start), clamp(select.end));
        self.anchor = (start != end).then_some(start);
        self.caret = end;
        self.column = None;
    }

    /// Takes the selection out and returns it (⌘X).
    pub fn cut(&mut self) -> Option<String> {
        let taken = self.selected_text()?.to_owned();
        self.delete_selection();
        Some(taken)
    }

    pub fn undo(&mut self) {
        if let Some((text, caret)) = self.undo.pop() {
            self.redo
                .push((std::mem::replace(&mut self.text, text), self.caret));
            self.caret = caret.min(self.text.len());
            self.anchor = None;
        }
    }

    pub fn redo(&mut self) {
        if let Some((text, caret)) = self.redo.pop() {
            self.undo
                .push((std::mem::replace(&mut self.text, text), self.caret));
            self.caret = caret.min(self.text.len());
            self.anchor = None;
        }
    }

    // ---- inside -------------------------------------------------------------------------------

    fn remember(&mut self) {
        self.undo.push((self.text.clone(), self.caret));
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn delete_selection(&mut self) -> bool {
        match self.selection() {
            Some(sel) => {
                self.delete_range(sel);
                true
            }
            None => false,
        }
    }

    fn delete_range(&mut self, range: std::ops::Range<usize>) {
        if range.is_empty() {
            return;
        }
        self.remember();
        self.run = None;
        self.caret = range.start;
        self.text.replace_range(range, "");
        self.anchor = None;
        self.column = None;
    }

    fn floor(&self, mut at: usize) -> usize {
        while !self.text.is_char_boundary(at) {
            at -= 1;
        }
        at
    }

    fn prev_char(&self, at: usize) -> usize {
        self.text[..at]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    fn next_char(&self, at: usize) -> usize {
        self.text[at..]
            .chars()
            .next()
            .map_or(at, |c| at + c.len_utf8())
    }

    fn word_left(&self) -> usize {
        let before = &self.text[..self.caret];
        let mut chars = before.char_indices().rev().peekable();
        while chars.next_if(|(_, c)| !is_word(*c)).is_some() {}
        let mut at = chars.peek().map_or(0, |(i, c)| i + c.len_utf8());
        for (i, c) in chars {
            if !is_word(c) {
                break;
            }
            at = i;
        }
        at
    }

    fn word_right(&self) -> usize {
        let after = &self.text[self.caret..];
        let mut chars = after.char_indices().peekable();
        while chars.next_if(|(_, c)| !is_word(*c)).is_some() {}
        while chars.next_if(|(_, c)| is_word(*c)).is_some() {}
        chars
            .peek()
            .map_or(self.text.len(), |(i, _)| self.caret + i)
    }

    fn line_start(&self, at: usize) -> usize {
        self.text[..at].rfind('\n').map_or(0, |i| i + 1)
    }

    fn line_end(&self, at: usize) -> usize {
        self.text[at..]
            .find('\n')
            .map_or(self.text.len(), |i| at + i)
    }

    /// ↑ ↓ by logical line, keeping the column the first press started from.
    fn vertical(&mut self, up: bool) -> usize {
        let start = self.line_start(self.caret);
        let column = *self
            .column
            .get_or_insert_with(|| self.text[start..self.caret].chars().count());
        let target = if up {
            if start == 0 {
                return 0;
            }
            self.line_start(start - 1)
        } else {
            let end = self.line_end(self.caret);
            if end == self.text.len() {
                return self.text.len();
            }
            end + 1
        };
        let line_end = self.line_end(target);
        self.text[target..line_end]
            .char_indices()
            .nth(column)
            .map_or(line_end, |(i, _)| target + i)
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str, caret: usize) -> TextEdit {
        let mut e = TextEdit::new(text);
        e.caret = caret;
        e
    }

    #[test]
    fn typing_goes_in_at_the_caret_and_over_a_selection() {
        let mut e = at("hello world", 5);
        e.insert(",");
        assert_eq!((e.text(), e.caret()), ("hello, world", 6));
        e.move_to(Move::WordRight, true);
        assert_eq!(e.selected_text(), Some(" world"));
        e.insert(" there");
        assert_eq!(e.text(), "hello, there");
    }

    #[test]
    fn words_skip_punctuation_and_stop_at_word_edges() {
        let mut e = at("let v = value.max(0);", 21);
        e.move_to(Move::WordLeft, false);
        assert_eq!(e.caret(), 18); // before `0`
        e.move_to(Move::WordLeft, false);
        assert_eq!(e.caret(), 14); // before `max`
        e.move_to(Move::WordRight, false);
        assert_eq!(e.caret(), 17); // after `max`
    }

    #[test]
    fn non_ascii_text_moves_by_chars() {
        let mut e = TextEdit::new("şəkil");
        e.move_to(Move::Left, false);
        e.move_to(Move::Left, false);
        assert_eq!(&e.text()[e.caret()..], "il");
        e.delete_back(false, false);
        assert_eq!(e.text(), "şəil");
    }

    #[test]
    fn up_and_down_keep_the_column() {
        let mut e = at("abcdef\nab\nabcdef", 5);
        e.move_to(Move::Down, false);
        assert_eq!(e.caret(), 9); // end of the short line
        e.move_to(Move::Down, false);
        assert_eq!(e.caret(), 15); // column 5 again
        e.move_to(Move::Up, true);
        assert_eq!(e.selected_text(), Some("\nabcde"));
    }

    #[test]
    fn a_move_without_shift_collapses_a_selection_to_its_edge() {
        let mut e = TextEdit::new("one two");
        e.select_all();
        e.move_to(Move::Left, false);
        assert_eq!((e.caret(), e.selection()), (0, None));
    }

    #[test]
    fn deleting_words_and_lines() {
        let mut e = TextEdit::new("first line\nsecond word");
        e.delete_back(true, false);
        assert_eq!(e.text(), "first line\nsecond ");
        e.delete_back(false, true);
        assert_eq!(e.text(), "first line\n");
        e.move_to(Move::Start, false);
        e.delete_forward(true);
        assert_eq!(e.text(), " line\n");
    }

    #[test]
    fn undo_takes_back_a_typed_word_at_a_time() {
        let mut e = TextEdit::new("");
        for c in "one two".chars() {
            e.insert(&c.to_string());
        }
        e.undo();
        assert_eq!(e.text(), "one ");
        e.undo();
        assert_eq!(e.text(), "");
        e.redo();
        assert_eq!(e.text(), "one ");
    }

    #[test]
    fn a_double_click_selects_the_word() {
        let mut e = TextEdit::new("call coerce_in(0)");
        e.select_word(8);
        assert_eq!(e.selected_text(), Some("coerce_in"));
    }
}
