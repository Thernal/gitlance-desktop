//! Which input has the keyboard (#168). An input can be open and keep its text without having
//! the keyboard: one at a time has it, and the key context and the key router follow that one, not
//! which inputs are open. It loses the keyboard to a press outside its box, to another page (a
//! composer also to another file), to another input, to esc while it holds text; it gets it back
//! from a press in its box, from its page coming back, from the action that opened it. A card over
//! everything (⌘K, Go to file, the list beside a folder …) holds the keyboard while it is open and gives it back on closing.

use super::Workspace;
use super::lists::Field;
use super::nav::Page;
use super::spaces::Space;
use gpui::{Context, MouseButton, MouseDownEvent, WeakEntity, prelude::*};

/// Every input that takes typed keys.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Input {
    /// ⌘K and the palettes sharing its box.
    Palette,
    /// Go to file, and find in files (`findfiles.rs`).
    FindFiles,
    /// Which app opens a file, asked (`opening.rs`).
    OpenAsk,
    /// Which chat a note goes to (`agent.rs`).
    AgentPick,
    /// The reactions' grid and its search.
    Reactions,
    /// The new version's card (#135).
    UpdateCard,
    /// What is beside a folder of Code's path (`beside.rs`).
    Beside,
    /// The card before a merge goes, its message (`merging.rs`, #164).
    Merge,
    /// A one-line field: a list's filter, the token, the host, a name being given … —
    /// `Workspace::field` says which, and is set only while it has the keyboard.
    Field,
    /// A comment, a reply or a note being written.
    Compose,
    /// The number of a line to write a note on.
    GoLine,
    /// The find bar over the diff.
    DiffFind,
    /// The commits' search.
    CommitSearch,
    /// The New merge request form.
    NewRequest,
    /// Submit review's summary.
    Submit,
    /// Report a problem's title and description.
    Report,
}

impl Input {
    const ALL: [Input; 16] = [
        Input::Palette,
        Input::FindFiles,
        Input::OpenAsk,
        Input::AgentPick,
        Input::Reactions,
        Input::UpdateCard,
        Input::Beside,
        Input::Merge,
        Input::Field,
        Input::Compose,
        Input::GoLine,
        Input::DiffFind,
        Input::CommitSearch,
        Input::NewRequest,
        Input::Submit,
        Input::Report,
    ];

    /// Over everything until it closes: it is never open without the keyboard, and closing it
    /// gives the keyboard back to the input that had it before.
    fn modal(self) -> bool {
        matches!(
            self,
            Input::Palette
                | Input::FindFiles
                | Input::OpenAsk
                | Input::AgentPick
                | Input::Reactions
                | Input::UpdateCard
                | Input::Beside
                | Input::Merge
        )
    }

    /// Of the page it was opened on: elsewhere it keeps its text, not the keyboard. Submit and
    /// Report are in the tool window, which stays from page to page.
    fn local(self) -> bool {
        !self.modal() && !matches!(self, Input::Submit | Input::Report)
    }
}

/// Where the keyboard was taken: Settings or a space's page, and the diff's file.
#[derive(Clone, PartialEq, Debug)]
struct Spot {
    settings: bool,
    space: Option<Space>,
    page: Option<Page>,
    file: Option<String>,
}

#[derive(Clone, Debug)]
struct Holder {
    input: Input,
    /// None until the next look: the action that opened it may still go to its page.
    at: Option<Spot>,
}

/// The input with the keyboard, the one a card over everything took it from, and the last one to
/// let it go.
#[derive(Default)]
pub(super) struct Keyboard {
    holder: Option<Holder>,
    before: Option<Holder>,
    left: Option<Holder>,
    /// `left` lost it to another page or file: it is given back there.
    left_away: bool,
    /// The inputs open at the last look, a bit each: one opened since takes the keyboard.
    open: u16,
}

impl Workspace {
    /// Whether `input` is open, with the keyboard or not.
    fn input_open(&self, input: Input) -> bool {
        match input {
            Input::Palette => self.palette.is_some(),
            Input::FindFiles => self.ffind.is_some(),
            Input::OpenAsk => self.open_ask.is_some(),
            Input::AgentPick => self.agent_pick.is_some(),
            Input::Reactions => self.reaction_grid.is_some(),
            Input::UpdateCard => self.update_card.is_some(),
            Input::Beside => self.beside.is_some(),
            Input::Merge => self.merging.is_some(),
            Input::Field => self.field.is_some(),
            Input::Compose => self.compose.is_some(),
            Input::GoLine => self.goline.is_some(),
            Input::DiffFind => self.dfind.is_some(),
            Input::CommitSearch => self.find.text.is_some(),
            Input::NewRequest => self.newreq.is_some(),
            Input::Submit => self.submit.is_some(),
            Input::Report => self.report_typing(),
        }
    }

    /// The input with the keyboard: keys go to it, and single-key actions wait.
    pub(super) fn typing(&self) -> Option<Input> {
        // A card over everything, also one opened since the last look.
        if let Some(h) = &self.keyboard.holder
            && h.input.modal()
            && self.input_open(h.input)
        {
            return Some(h.input);
        }
        if let Some(card) = Input::ALL
            .into_iter()
            .find(|i| i.modal() && self.input_open(*i))
        {
            return Some(card);
        }
        let h = self.keyboard.holder.as_ref()?;
        (self.input_open(h.input) && self.here(h)).then_some(h.input)
    }

    pub(super) fn has_keyboard(&self, input: Input) -> bool {
        self.typing() == Some(input)
    }

    fn spot(&self) -> Spot {
        Spot {
            settings: self.settings_open,
            space: self.space(),
            page: self.nav_page().cloned(),
            file: self.current_path(),
        }
    }

    /// Whether `h` is on screen where it took the keyboard: a composer on its page and file.
    fn here(&self, h: &Holder) -> bool {
        let Some(at) = &h.at else {
            return true;
        };
        if !h.input.local() {
            return true;
        }
        let file = self
            .diff
            .as_ref()
            .and_then(|d| d.files.get(self.file))
            .map(|f| f.path());
        at.settings == self.settings_open
            && at.space == self.space()
            && at.page.as_ref() == self.nav_page()
            && (h.input != Input::Compose || at.file.as_deref() == file)
    }

    /// `input` takes the keyboard: from a press in its box, or the action that opens it.
    pub(super) fn take_keyboard(&mut self, input: Input) {
        if self
            .keyboard
            .holder
            .as_ref()
            .is_some_and(|h| h.input == input)
            && self.typing() == Some(input)
        {
            return;
        }
        let had = self.keyboard.holder.replace(Holder { input, at: None });
        if input.modal() {
            // A card over a card keeps the first one's: it is what comes back.
            if !had.as_ref().is_some_and(|h| h.input.modal()) {
                self.keyboard.before = had;
            }
        } else {
            if input != Input::Field {
                self.field = None;
            }
            self.keyboard.before = None;
            self.keyboard.left = None;
        }
    }

    /// The input with the keyboard lets it go, its text kept: nothing has it. One left empty
    /// closes — a find, a filter, the line number.
    pub(super) fn leave_input(&mut self, cx: &mut Context<Self>) {
        if self.typing().is_none_or(Input::modal) {
            return;
        }
        let Some(h) = self.keyboard.holder.take() else {
            return;
        };
        match h.input {
            Input::Field => self.field = None,
            Input::DiffFind if self.dfind.as_deref() == Some("") => self.close_dfind(cx),
            Input::CommitSearch if self.find.text.as_deref() == Some("") => self.close_find(cx),
            Input::GoLine => self.close_goline(),
            _ => {}
        }
        self.keyboard.left = Some(h);
        self.keyboard.left_away = false;
        cx.notify();
    }

    /// `input`, the last to let the keyboard go and on screen where it had it, takes it back.
    pub(super) fn give_back(&mut self, input: Input) -> bool {
        match self.keyboard.left.take() {
            Some(h) if h.input == input && self.input_open(input) && self.here(&h) => {
                self.keyboard.holder = Some(h);
                true
            }
            left => {
                self.keyboard.left = left;
                false
            }
        }
    }

    /// Each frame and each key: an input opened since the last look takes the keyboard; a card
    /// closed gives it back; a page left takes it from its inputs, a page come back to returns it.
    pub(super) fn sync_keyboard(&mut self) {
        let mut open = 0u16;
        for (i, input) in Input::ALL.into_iter().enumerate() {
            if self.input_open(input) {
                open |= 1 << i;
            }
        }
        let opened = open & !self.keyboard.open;
        self.keyboard.open = open;
        // A card opened with a field is over it.
        if let Some(input) = Input::ALL
            .into_iter()
            .enumerate()
            .filter(|(i, _)| opened & (1 << i) != 0)
            .map(|(_, input)| input)
            .min_by_key(|input| !input.modal())
        {
            self.take_keyboard(input);
        }
        let spot = self.spot();
        if let Some(h) = self.keyboard.holder.as_mut()
            && h.at.is_none()
        {
            h.at = Some(spot);
        }
        if let Some(h) = self.keyboard.holder.clone() {
            if !self.input_open(h.input) {
                self.keyboard.holder = if h.input.modal() {
                    self.keyboard.before.take()
                } else {
                    None
                };
            } else if !self.here(&h) {
                self.keyboard.holder = None;
                if h.input == Input::Field {
                    self.field = None;
                } else {
                    self.keyboard.left = Some(h);
                    self.keyboard.left_away = true;
                }
            }
        }
        if self.keyboard.holder.is_none()
            && self.keyboard.left_away
            && let Some(input) = self.keyboard.left.as_ref().map(|h| h.input)
        {
            self.give_back(input);
        }
    }
}

/// The box of `input`: a press in it gives it the keyboard, a press outside takes it away.
pub(super) fn input_box<E: InteractiveElement>(el: E, input: Input, cx: &Context<Workspace>) -> E {
    input_box_of(el, input, cx.entity().downgrade())
}

/// `input_box` for a view drawn without its context (a row of a list).
pub(super) fn input_box_of<E: InteractiveElement>(
    el: E,
    input: Input,
    this: WeakEntity<Workspace>,
) -> E {
    let out = this.clone();
    el.on_mouse_down(MouseButton::Left, move |_, _, cx| {
        this.update(cx, |this, cx| {
            this.take_keyboard(input);
            cx.notify();
        })
        .ok();
    })
    .on_mouse_down_out(move |_, _, cx| {
        out.update(cx, |this, cx| {
            if this.has_keyboard(input) {
                this.leave_input(cx);
            }
        })
        .ok();
    })
}

/// The box of `field`: a press outside it takes the keyboard away; a click in it gives it back
/// (the box's own `start_field`).
pub(super) fn field_box_out<E: InteractiveElement>(
    el: E,
    field: Field,
    cx: &Context<Workspace>,
) -> E {
    el.on_mouse_down_out(
        cx.listener(move |this: &mut Workspace, _: &MouseDownEvent, _, cx| {
            if this.field == Some(field) && this.has_keyboard(Input::Field) {
                this.leave_input(cx);
            }
        }),
    )
}

#[cfg(test)]
mod tests {
    /// The key handlers `find_key` routes to by `Input`: the only places a key may edit a text.
    const ROUTED: [&str; 13] = [
        "ffind_key",
        "palette_key",
        "beside_key",
        "merge_key",
        "reaction_key",
        "submit_key",
        "newreq_key",
        "goline_key",
        "report_key",
        "compose_key",
        "filter_key",
        "dfind_key",
        "search_key",
    ];

    /// A text edited from keys outside the router would take keys whether or not it has the
    /// keyboard (#168): every function that edits one is a routed handler, and each is routed.
    #[test]
    fn every_text_is_edited_through_the_router() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui");
        let mut stray = Vec::new();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            // The editors themselves, and this test's own words.
            if !name.ends_with(".rs") || name == "input.rs" || name == "typing.rs" {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            let mut func = String::new();
            for line in source.lines() {
                let code = line.trim_start();
                if let Some(at) = code.find("fn ")
                    && (at == 0 || code[..at].ends_with(") ") || code.starts_with("pub"))
                {
                    func = code[at + 3..]
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                }
                let edits = [
                    "input::edit(",
                    "input::edit_area",
                    "input::edit_text(",
                    "input::edit\n",
                ]
                .iter()
                .any(|p| format!("{code}\n").contains(p));
                if edits && !ROUTED.contains(&func.as_str()) {
                    stray.push(format!("{name}: {func}"));
                }
            }
        }
        assert!(
            stray.is_empty(),
            "edits a text outside the key router: {stray:?}"
        );
        let router = std::fs::read_to_string(dir.join("find.rs")).unwrap();
        let router = &router[router.find("fn find_key(").unwrap()..];
        let router = &router[..router.find("None => {}").unwrap()];
        for handler in ROUTED {
            assert!(
                router.contains(&format!("self.{handler}(")),
                "{handler} is not routed"
            );
        }
    }
}
