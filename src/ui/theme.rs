//! The colours the window draws with, read from the theme in use (#12, `crate::themes`): each
//! function names the token it reads from `../Design/tokens.css`'s theme layer. The window theme
//! gives the chrome and the diff's surface; the code theme (the window's, unless Settings →
//! Appearance → Code picks another) gives the diff's tints and the syntax colours (D16).

use crate::themes::{self, T, TOKENS, Theme};
use gpui::{Hsla, Rgba, SharedString, rgba};
use std::sync::RwLock;

/// The interface's face unless another is chosen: the system's.
pub const UI_FONT: &str = ".SystemUIFont";
/// The code's face unless another is chosen.
pub const CODE_FONT: &str = "Menlo";

// ---- the faces chosen (#24) -----------------------------------------------------------------

/// The faces chosen in Settings → Appearance → Fonts and size (`fonts.txt`: `ui=` and `code=`
/// lines, a face's family name each; none is the default).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Faces {
    pub ui: Option<String>,
    pub code: Option<String>,
}

impl Faces {
    fn parse(text: &str) -> Self {
        let mut faces = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = Some(value.trim().to_owned()).filter(|v| !v.is_empty());
            match key.trim() {
                "ui" => faces.ui = value,
                "code" => faces.code = value,
                _ => {}
            }
        }
        faces
    }

    fn text(&self) -> String {
        let line =
            |key: &str, face: &Option<String>| format!("{key}={}\n", face.as_deref().unwrap_or(""));
        line("ui", &self.ui) + &line("code", &self.code)
    }
}

/// The interface's face and the code's, as drawn.
static FACES: RwLock<(SharedString, SharedString)> = RwLock::new((
    SharedString::new_static(UI_FONT),
    SharedString::new_static(CODE_FONT),
));

fn draw_faces(faces: &Faces) {
    let face = |chosen: &Option<String>, default: &'static str| {
        chosen
            .clone()
            .map_or(SharedString::new_static(default), SharedString::from)
    };
    if let Ok(mut drawn) = FACES.write() {
        *drawn = (face(&faces.ui, UI_FONT), face(&faces.code, CODE_FONT));
    }
}

/// The face the interface is drawn in.
pub fn ui_font() -> SharedString {
    FACES.read().map_or(UI_FONT.into(), |f| f.0.clone())
}

/// The face the code is drawn in.
pub fn code_font() -> SharedString {
    FACES.read().map_or(CODE_FONT.into(), |f| f.1.clone())
}

/// The faces chosen.
pub fn faces() -> Faces {
    let chosen = |face: SharedString, default: &str| (face != default).then(|| face.to_string());
    Faces {
        ui: chosen(ui_font(), UI_FONT),
        code: chosen(code_font(), CODE_FONT),
    }
}

/// Draws with `faces` and keeps them.
pub fn choose_faces(faces: &Faces) {
    crate::storage::save("fonts.txt", faces.text());
    draw_faces(faces);
}

/// The tokens in use.
type Colors = [u32; TOKENS.len()];

static CURRENT: RwLock<Option<Colors>> = RwLock::new(None);

/// Draws with `window`, its code layer from `code` when given.
pub fn set(window: &Theme, code: Option<&Theme>) {
    let code = code.unwrap_or(window);
    let mut colors = [0u32; TOKENS.len()];
    for (i, c) in colors.iter_mut().enumerate() {
        *c = if themes::is_code(i) {
            code.colors[i]
        } else {
            window.colors[i]
        };
    }
    if let Ok(mut current) = CURRENT.write() {
        *current = Some(colors);
    }
    if let Ok(mut roles) = ROLE_COLORS.write() {
        *roles = code.roles;
    }
}

fn raw(i: usize) -> u32 {
    if let Ok(current) = CURRENT.read()
        && let Some(colors) = current.as_ref()
    {
        return colors[i];
    }
    // Before a theme is set (tests): One Dark.
    static ONE_DARK: std::sync::OnceLock<Colors> = std::sync::OnceLock::new();
    ONE_DARK.get_or_init(|| themes::resolve(&[])[0].colors)[i]
}

fn get(t: T) -> Rgba {
    rgba(raw(t as usize))
}

/// Whether the window's theme is light: its window lighter than its text.
pub fn light() -> bool {
    themes::luminance(raw(T::Bg1 as usize)) > themes::luminance(raw(T::Text1 as usize))
}

/// Token `t` at `alpha` (0–255) over whatever is under it.
fn with_alpha(t: T, alpha: u32) -> Rgba {
    rgba(raw(t as usize) & 0xffffff00 | alpha)
}

/// The code's own text: the window's (D16). `--content-01`
pub fn code_text() -> Rgba {
    get(T::Text1)
}

/// The window behind the islands. `--background-01`
pub fn base() -> Rgba {
    get(T::Bg1)
}

/// List islands; the diff island uses `editor`. `--background-02`
pub fn panel() -> Rgba {
    get(T::Bg2)
}

/// `--border-01`
pub fn island_border() -> Rgba {
    get(T::Border1)
}

/// `--background-03`
pub fn editor() -> Rgba {
    get(T::Bg3)
}

/// `--border-02`
pub fn border() -> Rgba {
    get(T::Border2)
}

/// `--background-04`
pub fn hover() -> Rgba {
    get(T::Bg4)
}

/// `--interactive-03-hover`
pub fn selected() -> Rgba {
    get(T::FillHover)
}

/// `--content-01`
pub fn text() -> Rgba {
    get(T::Text1)
}

/// `--content-02`
pub fn muted() -> Rgba {
    get(T::Text2)
}

/// `--content-03`, 4.5:1 on every surface.
pub fn faint() -> Rgba {
    get(T::Text3)
}

/// `--interactive-01`
pub fn accent() -> Rgba {
    get(T::Accent)
}

/// `--success`
pub fn added() -> Rgba {
    get(T::Success)
}

/// Red ink — deletions, errors, Request changes. `--danger`: One Dark's red raised to 4.5:1; the
/// diff's red tints stay One Dark's (`removed_line`, `removed_word`, `removed_gutter`).
pub fn removed() -> Rgba {
    get(T::Danger)
}

/// `--code-05`
pub fn renamed() -> Rgba {
    get(T::Code5)
}

/// `--attention`
pub fn warning() -> Rgba {
    get(T::Attention)
}

/// The tint behind a search match. `--attention-bg`
pub fn warning_bg() -> Rgba {
    get(T::AttentionBg)
}

/// The focus ring of a text field. `--focus-ring`
pub fn focus() -> Rgba {
    get(T::Focus)
}

/// Comments for a coding agent, apart from the focus ring and the code's keyword purple. `--agent`
pub fn agent() -> Rgba {
    get(T::Agent)
}

/// The tint behind an agent note's pill.
pub fn agent_bg() -> Rgba {
    with_alpha(T::Agent, 0x2e)
}

/// The tint behind a "Resolved" chip. `--success-bg`
pub fn added_bg() -> Rgba {
    get(T::SuccessBg)
}

/// The tint behind the new-commits pill. `--info-bg`
pub fn info_bg() -> Rgba {
    get(T::InfoBg)
}

/// The tint behind selected lines of a diff.
pub fn selection() -> Rgba {
    with_alpha(T::Accent, 0x38)
}

/// A pane splitter under the pointer or being dragged.
pub fn splitter() -> Rgba {
    with_alpha(T::Accent, 0x99)
}

/// `--diff-added-01`
pub fn added_line() -> Rgba {
    get(T::Added1)
}

/// `--diff-removed-01`
pub fn removed_line() -> Rgba {
    get(T::Removed1)
}

pub fn scrollbar() -> Rgba {
    with_alpha(T::Text1, 0x40)
}

/// A changed token inside an added line. `--diff-added-03`
pub fn added_word() -> Rgba {
    get(T::Added3)
}

/// A changed token inside a removed line. `--diff-removed-03`
pub fn removed_word() -> Rgba {
    get(T::Removed3)
}

/// `--diff-added-02`
pub fn added_gutter() -> Rgba {
    get(T::Added2)
}

/// `--diff-removed-02`
pub fn removed_gutter() -> Rgba {
    get(T::Removed2)
}

/// The colour of code drawn in `role` (`crate::highlight::Span::color`): `code-01` … `code-08`,
/// 0 the code's text; a parse tree's role (`highlight::role::BASE` + n) in the code theme's own
/// colour for it (`code.parameter` …, X45), else in the code token it falls back to.
pub fn code(role: u32) -> Hsla {
    use crate::highlight::role as r;
    let token = |n: u32| match n {
        1..=8 => rgba(raw(T::Code1 as usize + n as usize - 1)).into(),
        r::MUTED => muted().into(),
        _ => code_text().into(),
    };
    if role < r::BASE {
        return token(role);
    }
    let ix = (role - r::BASE) as usize;
    match ROLE_COLORS
        .read()
        .ok()
        .and_then(|c| c.get(ix).copied().flatten())
    {
        Some(c) => rgba(c).into(),
        None => token(r::ROLES.get(ix).map_or(0, |l| l.token)),
    }
}

/// The code theme's own colours for the parse tree's roles.
static ROLE_COLORS: RwLock<[Option<u32>; crate::highlight::role::ROLES.len()]> =
    RwLock::new([None; crate::highlight::role::ROLES.len()]);

/// The tint behind a removed line's pill or a failed check. `--danger-bg`
pub fn danger_bg() -> Rgba {
    get(T::DangerBg)
}

/// Ink on the accent (a filled button). `--content-05`
pub fn on_accent() -> Rgba {
    get(T::Text5)
}

// ---- the themes found and the ones chosen ---------------------------------------------------

/// Which themes are in use (`theme.txt`: the window's id, then the code's or nothing — the code
/// follows the window).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Choice {
    pub window: String,
    pub code: Option<String>,
}

impl Choice {
    fn read() -> Self {
        let text = crate::storage::read("theme.txt");
        let mut lines = text.lines().map(str::trim);
        let window = lines
            .next()
            .filter(|l| !l.is_empty())
            .unwrap_or("one-dark")
            .to_owned();
        let code = lines.next().filter(|l| !l.is_empty()).map(str::to_owned);
        Self { window, code }
    }

    fn save(&self) {
        crate::storage::save(
            "theme.txt",
            format!("{}\n{}\n", self.window, self.code.as_deref().unwrap_or("")),
        );
    }
}

struct Registry {
    themes: Vec<Theme>,
    choice: Choice,
    /// The themes folder as last read.
    stamp: u128,
}

static REGISTRY: RwLock<Option<Registry>> = RwLock::new(None);

/// Reads the themes (built in and the folder's) and the choice, and draws with it.
pub fn load() {
    draw_faces(&Faces::parse(&crate::storage::read("fonts.txt")));
    let registry = Registry {
        themes: themes::resolve(&themes::read_folder()),
        choice: Choice::read(),
        stamp: themes::folder_stamp(),
    };
    apply(&registry, &registry.choice);
    if let Ok(mut r) = REGISTRY.write() {
        *r = Some(registry);
    }
}

fn with<R>(f: impl FnOnce(&Registry) -> R) -> Option<R> {
    REGISTRY.read().ok()?.as_ref().map(f)
}

/// Every theme: the built-in ones, then the user's.
pub fn all() -> Vec<Theme> {
    with(|r| r.themes.clone()).unwrap_or_else(|| themes::resolve(&[]))
}

/// Theme `id`, if there is one.
pub fn find(id: &str) -> Option<Theme> {
    all().into_iter().find(|t| t.id == id)
}

/// The themes chosen (not one being tried).
pub fn choice() -> Choice {
    with(|r| r.choice.clone()).unwrap_or_else(|| Choice {
        window: "one-dark".into(),
        code: None,
    })
}

fn apply(registry: &Registry, choice: &Choice) {
    let find = |id: &str| registry.themes.iter().find(|t| t.id == id);
    let window = find(&choice.window).unwrap_or(&registry.themes[0]);
    let code = choice.code.as_deref().and_then(find);
    set(window, code);
}

/// Draws with `choice` and keeps it.
pub fn choose(choice: Choice) {
    choice.save();
    if let Ok(mut r) = REGISTRY.write()
        && let Some(r) = r.as_mut()
    {
        r.choice = choice;
        apply(r, &r.choice);
    }
}

/// Draws with `choice` for now (⌘K → Theme), without keeping it.
pub fn preview(choice: &Choice) {
    with(|r| apply(r, choice));
}

/// Draws with the chosen themes again, after a preview.
pub fn restore() {
    with(|r| apply(r, &r.choice));
}

/// Reads the themes folder again when it changed; true when it did.
pub fn reload_if_changed() -> bool {
    let stamp = themes::folder_stamp();
    if with(|r| r.stamp == stamp).unwrap_or(true) {
        return false;
    }
    reload();
    true
}

/// Reads the themes folder again (after the editor saved a theme).
pub fn reload() {
    let choice = choice();
    let registry = Registry {
        themes: themes::resolve(&themes::read_folder()),
        choice,
        stamp: themes::folder_stamp(),
    };
    apply(&registry, &registry.choice);
    if let Ok(mut r) = REGISTRY.write() {
        *r = Some(registry);
    }
}

#[cfg(test)]
mod tests {
    use super::Faces;

    #[test]
    fn the_faces_chosen_read_back_as_they_were_kept() {
        let faces = Faces {
            ui: Some("Avenir Next".into()),
            code: None,
        };
        assert_eq!(faces.text(), "ui=Avenir Next\ncode=\n");
        assert_eq!(Faces::parse(&faces.text()), faces);
        assert_eq!(Faces::parse(""), Faces::default());
        assert_eq!(
            Faces::parse("code= JetBrains Mono \nwhat=else"),
            Faces {
                ui: None,
                code: Some("JetBrains Mono".into())
            }
        );
    }
}
