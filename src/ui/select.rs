//! Selecting text in the diff like in an editor: a click puts the caret, a drag selects characters,
//! a double click a word, a triple click the line, ⇧-click extends. A selection lives on one side of
//! the diff and runs between two positions (line and byte offset into that line's text).

use super::{CopySelection, SelectAll, Workspace};
use crate::worddiff;
use gpui::{ClipboardItem, Context, Window};
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    /// 1-based.
    pub line: u32,
    /// Byte offset into the line's text.
    pub byte: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sel {
    /// On the removed side of the diff.
    pub old: bool,
    pub anchor: Pos,
    pub head: Pos,
}

impl Sel {
    /// The two ends, first then last.
    pub fn ends(&self) -> (Pos, Pos) {
        (self.anchor.min(self.head), self.anchor.max(self.head))
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// What of a line of `len` bytes is selected; `None` when none of it is.
    pub fn in_line(&self, line: u32, len: usize) -> Option<Range<usize>> {
        let (start, end) = self.ends();
        if self.is_empty() || line < start.line || line > end.line {
            return None;
        }
        let from = if line == start.line {
            start.byte.min(len)
        } else {
            0
        };
        let to = if line == end.line {
            end.byte.min(len)
        } else {
            len
        };
        (from < to).then_some(from..to)
    }
}

/// The word, run of spaces or single symbol at `byte` of `text`.
pub fn word_at(text: &str, byte: usize) -> Range<usize> {
    let words = worddiff::words(text);
    words
        .iter()
        .find(|w| w.start <= byte && byte < w.end)
        .or_else(|| words.last().filter(|w| w.end == byte))
        .cloned()
        .unwrap_or(byte..byte)
}

impl Workspace {
    fn line_text(&self, old: bool, line: u32) -> String {
        self.data
            .as_ref()
            .map(|d| d.side(old).line(line).0.to_owned())
            .unwrap_or_default()
    }

    /// A press on a line at byte `byte`: the caret, a word or the line, by click count; ⇧ extends.
    pub(super) fn select_press(
        &mut self,
        old: bool,
        line: u32,
        byte: usize,
        clicks: usize,
        shift: bool,
        cx: &mut Context<Self>,
    ) {
        self.ctx_menu = None;
        self.note_sel = None;
        let at = Pos { line, byte };
        let text = self.line_text(old, line);
        self.selecting = true;
        self.sel = match (self.sel, clicks, shift) {
            (Some(sel), 1, true) if sel.old == old => Some(Sel { head: at, ..sel }),
            (_, 2, _) => {
                let w = word_at(&text, byte);
                Some(Sel {
                    old,
                    anchor: Pos {
                        line,
                        byte: w.start,
                    },
                    head: Pos { line, byte: w.end },
                })
            }
            (_, n, _) if n >= 3 => Some(Sel {
                old,
                anchor: Pos { line, byte: 0 },
                head: Pos {
                    line,
                    byte: text.len(),
                },
            }),
            _ => Some(Sel {
                old,
                anchor: at,
                head: at,
            }),
        };
        cx.notify();
    }

    /// The pointer moved over a line with the button down: the selection reaches it.
    pub(super) fn select_drag(
        &mut self,
        old: bool,
        line: u32,
        byte: usize,
        cx: &mut Context<Self>,
    ) {
        if !self.selecting {
            return;
        }
        if let Some(sel) = &mut self.sel
            && sel.old == old
        {
            let head = Pos { line, byte };
            if sel.head != head {
                sel.head = head;
                cx.notify();
            }
        }
    }

    /// The selected text, `None` when nothing is selected.
    pub(super) fn selected_text(&self) -> Option<String> {
        let sel = self.sel.filter(|s| !s.is_empty())?;
        let data = self.data.as_ref()?;
        let side = data.side(sel.old);
        let (start, end) = sel.ends();
        let lines: Vec<&str> = (start.line..=end.line)
            .map(|n| {
                let text = side.line(n).0;
                let from = if n == start.line {
                    start.byte.min(text.len())
                } else {
                    0
                };
                let to = if n == end.line {
                    end.byte.min(text.len())
                } else {
                    text.len()
                };
                text.get(from..to).unwrap_or_default()
            })
            .collect();
        Some(lines.join("\n"))
    }

    /// ⌘C.
    pub(super) fn copy_selection(
        &mut self,
        _: &CopySelection,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A selection in the field being typed in wins; then one in a comment's text; then the
        // diff's.
        let text = self
            .field_selection()
            .or_else(|| self.note_sel.as_ref().and_then(|s| s.text()))
            .or_else(|| self.selected_text());
        if let Some(text) = text {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// The text selected in the field that has the keyboard (never in the token's).
    fn field_selection(&self) -> Option<String> {
        if let Some(compose) = &self.compose {
            return compose.edit.selected_text().map(str::to_owned);
        }
        let typing = self.find.text.is_some()
            || self.dfind.is_some()
            || self.palette.is_some()
            || self.ffind.is_some()
            || self.goline.is_some()
            || self.newreq.is_some()
            || self.submit.is_some()
            || self.field.is_some_and(|f| f != super::lists::Field::Token);
        typing
            .then(|| self.field_edit.selected_text().map(str::to_owned))
            .flatten()
    }

    /// ⌘A: the whole side of the diff that is being read (the new one, unless the old is selected).
    pub(super) fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        let Some(data) = &self.data else {
            return;
        };
        let old = self.sel.is_some_and(|s| s.old);
        let side = data.side(old);
        let last = side.count().max(1);
        self.sel = Some(Sel {
            old,
            anchor: Pos { line: 1, byte: 0 },
            head: Pos {
                line: last,
                byte: side.line(last).0.len(),
            },
        });
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(a: (u32, usize), b: (u32, usize)) -> Sel {
        Sel {
            old: false,
            anchor: Pos {
                line: a.0,
                byte: a.1,
            },
            head: Pos {
                line: b.0,
                byte: b.1,
            },
        }
    }

    #[test]
    fn a_selection_covers_part_of_its_first_and_last_lines() {
        let s = sel((2, 4), (4, 3));
        assert_eq!(s.in_line(1, 10), None);
        assert_eq!(s.in_line(2, 10), Some(4..10));
        assert_eq!(s.in_line(3, 10), Some(0..10));
        assert_eq!(s.in_line(4, 10), Some(0..3));
        assert_eq!(s.in_line(5, 10), None);
    }

    #[test]
    fn the_ends_are_the_same_whichever_way_it_was_dragged() {
        let a = sel((2, 4), (4, 3));
        let b = sel((4, 3), (2, 4));
        assert_eq!(a.ends(), b.ends());
        assert_eq!(b.in_line(2, 10), Some(4..10));
    }

    #[test]
    fn a_caret_selects_nothing() {
        let s = sel((2, 4), (2, 4));
        assert!(s.is_empty());
        assert_eq!(s.in_line(2, 10), None);
    }

    #[test]
    fn a_double_click_takes_the_word_under_it() {
        let text = "let max_len = 10;";
        assert_eq!(word_at(text, 5), 4..11);
        assert_eq!(word_at(text, 3), 3..4);
        assert_eq!(word_at(text, text.len()), text.len() - 1..text.len());
        assert_eq!(word_at("", 0), 0..0);
    }
}

impl Workspace {
    /// How the text of comments can be selected: the selection now, and presses back to here.
    pub(super) fn selecting(&self, this: gpui::WeakEntity<Self>) -> super::markdown::Selecting {
        super::markdown::Selecting {
            current: self.note_sel.clone(),
            on: std::rc::Rc::new(move |press, cx| {
                this.update(cx, |this, cx| this.note_press(press, cx)).ok();
            }),
        }
    }

    /// A press in a comment's text: the caret (nothing selected yet), a word, the block, or
    /// growing the selection with ⇧ or a drag. The diff's selection goes.
    fn note_press(&mut self, press: super::markdown::Press, cx: &mut Context<Self>) {
        use super::markdown::{Press, TextSel};
        match press {
            Press::Down {
                note,
                block,
                at,
                clicks,
                shift,
                texts,
            } => {
                self.sel = None;
                self.ctx_menu = None;
                let text = texts.borrow().get(block).cloned().unwrap_or_default();
                let same = self.note_sel.as_ref().is_some_and(|s| s.note == note);
                self.note_sel = Some(match (clicks, shift) {
                    (1, true) if same => {
                        let mut s = self.note_sel.clone().expect("checked");
                        s.head = (block, at);
                        s
                    }
                    (2, _) => {
                        let w = word_at(&text, at);
                        TextSel {
                            note,
                            anchor: (block, w.start),
                            head: (block, w.end),
                            texts,
                        }
                    }
                    (c, _) if c >= 3 => TextSel {
                        note,
                        anchor: (block, 0),
                        head: (block, text.len()),
                        texts,
                    },
                    _ => TextSel {
                        note,
                        anchor: (block, at),
                        head: (block, at),
                        texts,
                    },
                });
            }
            Press::Drag { note, block, at } => match self.note_sel.as_mut() {
                Some(s) if s.note == note => s.head = (block, at),
                _ => return,
            },
        }
        cx.notify();
    }
}
