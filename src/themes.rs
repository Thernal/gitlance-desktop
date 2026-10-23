//! Themes as data (#12, Design `docs/specs/themes.md`): a theme is the colours of the token layer of
//! `tokens.css` by name, read from JSON — the five built-in themes ship inside the app
//! (`assets/themes/`), a user's themes are the `.json` files in the data folder's `themes/`. A
//! theme `extends` another (One Dark by default) and sets only what differs; what cannot be read is
//! listed as its problems, and the rest still loads. `ui::theme` draws with the one in use.

use crate::highlight::role;
use serde_json::{Map, Value};
use std::path::PathBuf;

/// How many roles a theme may colour by name (`code.parameter` …, #13).
pub const ROLES: usize = role::ROLES.len();

/// The tokens a theme sets, in the editor's order: (name, group, what it is for).
pub const TOKENS: [(&str, &str, &str); 41] = [
    ("background-01", "Surfaces", "the window"),
    ("background-02", "Surfaces", "lists, panels"),
    ("background-03", "Surfaces", "the diff"),
    ("background-04", "Surfaces", "a hovered row"),
    ("content-01", "Text", "text, code"),
    ("content-02", "Text", "secondary text"),
    ("content-03", "Text", "labels, line numbers"),
    ("content-05", "Text", "on the accent"),
    ("interactive-01", "Accent", "links, the accent"),
    (
        "interactive-01-hover",
        "Accent",
        "the accent under the pointer",
    ),
    ("interactive-01-press", "Accent", "the accent pressed"),
    ("interactive-03", "Accent", "a quiet fill"),
    ("interactive-03-hover", "Accent", "a selected row"),
    ("interactive-disabled", "Accent", "a control that is off"),
    ("border-01", "Borders", "a pane's edge"),
    ("border-02", "Borders", "between the diff's halves"),
    ("border-03", "Borders", "an accent edge"),
    ("success", "Status", "added, passed"),
    ("danger", "Status", "removed, failed"),
    ("attention", "Status", "running, a match"),
    ("info", "Status", "information"),
    ("success-bg", "Status", "behind success"),
    ("danger-bg", "Status", "behind danger"),
    ("attention-bg", "Status", "behind attention"),
    ("info-bg", "Status", "behind information"),
    ("focus-ring", "Status", "the focused pane and field"),
    ("agent", "Status", "comments for an agent"),
    ("diff-added-01", "Diff", "an added line"),
    ("diff-added-02", "Diff", "an added line's gutter"),
    ("diff-added-03", "Diff", "an added word"),
    ("diff-removed-01", "Diff", "a removed line"),
    ("diff-removed-02", "Diff", "a removed line's gutter"),
    ("diff-removed-03", "Diff", "a removed word"),
    ("code-01", "Code", "keyword"),
    ("code-02", "Code", "function"),
    ("code-03", "Code", "string"),
    ("code-04", "Code", "number, constant"),
    ("code-05", "Code", "type"),
    ("code-06", "Code", "comment"),
    ("code-07", "Code", "member, tag"),
    ("code-08", "Code", "operator, escape"),
];

/// A token's place in `TOKENS`. Every token of the layer has one, drawn with or not yet.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum T {
    Bg1,
    Bg2,
    Bg3,
    Bg4,
    Text1,
    Text2,
    Text3,
    Text5,
    Accent,
    AccentHover,
    AccentPress,
    Fill,
    FillHover,
    Disabled,
    Border1,
    Border2,
    Border3,
    Success,
    Danger,
    Attention,
    Info,
    SuccessBg,
    DangerBg,
    AttentionBg,
    InfoBg,
    Focus,
    Agent,
    Added1,
    Added2,
    Added3,
    Removed1,
    Removed2,
    Removed3,
    Code1,
    Code2,
    Code3,
    Code4,
    Code5,
    Code6,
    Code7,
    Code8,
}

/// The tokens the Code choice brings (#12 D, D16): the diff's tints and the syntax colours. The
/// surface they are drawn on and the code's plain text stay the window's.
pub fn is_code(i: usize) -> bool {
    i >= T::Added1 as usize
}

/// The syntax colours.
pub const SYNTAX: [T; 8] = [
    T::Code1,
    T::Code2,
    T::Code3,
    T::Code4,
    T::Code5,
    T::Code6,
    T::Code7,
    T::Code8,
];

/// The tokens that are text, each read on every surface: they need 4.5:1.
pub const TEXT: [T; 16] = [
    T::Text1,
    T::Text2,
    T::Text3,
    T::Accent,
    T::Success,
    T::Danger,
    T::Attention,
    T::Agent,
    T::Code1,
    T::Code2,
    T::Code3,
    T::Code4,
    T::Code5,
    T::Code6,
    T::Code7,
    T::Code8,
];

/// The surfaces text is read on.
pub const SURFACES: [T; 4] = [T::Bg1, T::Bg2, T::Bg3, T::Bg4];

pub fn index(name: &str) -> Option<usize> {
    TOKENS.iter().position(|(n, _, _)| *n == name)
}

/// The built-in themes: (id, file).
const BUILT_IN: [(&str, &str); 5] = [
    ("one-dark", include_str!("../assets/themes/one-dark.json")),
    ("graphite", include_str!("../assets/themes/graphite.json")),
    ("darcula", include_str!("../assets/themes/darcula.json")),
    ("night", include_str!("../assets/themes/night.json")),
    ("paper", include_str!("../assets/themes/paper.json")),
];

/// A theme ready to draw with: every token, 0xRRGGBBAA.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// Its id: the built-in's, or the file's `name`.
    pub id: String,
    /// As shown: "One Dark", or the user's name.
    pub label: String,
    pub colors: [u32; TOKENS.len()],
    /// The parse tree's roles it gives a colour of their own (`code.parameter`); the rest are drawn
    /// with their code token (#13, X45).
    pub roles: [Option<u32>; ROLES],
    /// Offered as a window theme: its file sets `colors` or `window`, or neither part apart.
    pub window: bool,
    /// Offered as a code theme: its file sets `colors` or `code`, or neither part apart.
    pub code: bool,
    /// Its file sets `window` and `code` apart, and is written back so.
    pub split: bool,
    /// Shipped with the app.
    pub built_in: bool,
    /// The theme it extends.
    pub extends: Option<String>,
    /// The tokens it sets itself (the rest come from `extends`).
    pub own: Vec<usize>,
    /// The roles it colours itself.
    pub own_roles: Vec<usize>,
    /// What could not be read, and text under 4.5:1.
    pub problems: Vec<String>,
    /// The file it was read from (a user theme).
    pub file: Option<PathBuf>,
}

impl Theme {
    pub fn get(&self, t: T) -> u32 {
        self.colors[t as usize]
    }

    /// Light: its window is lighter than its text.
    pub fn light(&self) -> bool {
        luminance(self.get(T::Bg1)) > luminance(self.get(T::Text1))
    }

    /// The theme as a JSON file: every token when `full`, else the ones it sets itself — in
    /// `window` and `code` when its file had them apart, else in `colors`.
    pub fn to_json(&self, full: bool) -> String {
        // The tokens written: every one, or the code part's (`Some(true)`), or the window's.
        let colors = |part: Option<bool>| {
            let mut colors = Map::new();
            for (i, (name, _, _)) in TOKENS.iter().enumerate() {
                if (full || self.own.contains(&i)) && part.is_none_or(|code| code == is_code(i)) {
                    colors.insert((*name).to_owned(), Value::String(hex(self.colors[i])));
                }
            }
            // Roles are code: in `colors` or `code`.
            if part != Some(false) {
                for (r, look) in role::ROLES.iter().enumerate() {
                    if let Some(c) = self.roles[r].filter(|_| full || self.own_roles.contains(&r)) {
                        colors.insert(format!("code.{}", look.name), Value::String(hex(c)));
                    }
                }
            }
            Value::Object(colors)
        };
        let mut root = Map::new();
        root.insert("name".into(), Value::String(self.id.clone()));
        if !full && let Some(e) = &self.extends {
            root.insert("extends".into(), Value::String(e.clone()));
        }
        if self.split && !full {
            let sets = |code: bool| {
                self.own.iter().any(|&i| is_code(i) == code) || (code && !self.own_roles.is_empty())
            };
            if self.window || sets(false) {
                root.insert("window".into(), colors(Some(false)));
            }
            if self.code || sets(true) {
                root.insert("code".into(), colors(Some(true)));
            }
        } else {
            root.insert("colors".into(), colors(None));
        }
        serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default() + "\n"
    }

    /// The text tokens under 4.5:1 on some surface: (token, the lowest ratio, on which surface).
    pub fn low_contrast(&self) -> Vec<(T, f32, T)> {
        TEXT.iter()
            .filter_map(|&t| {
                let (ratio, on) = SURFACES
                    .iter()
                    .map(|&s| (contrast(self.get(t), self.get(s)), s))
                    .min_by(|a, b| a.0.total_cmp(&b.0))?;
                (ratio < 4.5).then_some((t, ratio, on))
            })
            .collect()
    }
}

/// A theme file as read: its fields, before `extends` is followed.
#[derive(Clone, Debug, Default)]
struct Raw {
    id: String,
    label: String,
    extends: Option<String>,
    colors: Vec<(usize, u32)>,
    roles: Vec<(usize, u32)>,
    problems: Vec<String>,
    built_in: bool,
    file: Option<PathBuf>,
    window: bool,
    code: bool,
    split: bool,
}

fn label_of(id: &str) -> String {
    match id {
        "one-dark" => "One Dark".into(),
        "graphite" => "Graphite".into(),
        "darcula" => "Darcula".into(),
        "night" => "Night".into(),
        "paper" => "Paper".into(),
        other => other.to_owned(),
    }
}

/// Reads one theme file; `fallback` names it when it has no `name`.
fn read_raw(text: &str, fallback: &str) -> Raw {
    let mut raw = Raw {
        id: fallback.to_owned(),
        label: label_of(fallback),
        window: true,
        code: true,
        ..Raw::default()
    };
    let value: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            raw.problems.push(format!("Not JSON: {e}"));
            return raw;
        }
    };
    let Some(obj) = value.as_object() else {
        raw.problems.push("Not a JSON object".into());
        return raw;
    };
    if let Some(name) = obj
        .get("name")
        .and_then(Value::as_str)
        .filter(|n| !n.is_empty())
    {
        raw.id = name.to_owned();
        raw.label = label_of(name);
    }
    raw.extends = obj
        .get("extends")
        .and_then(Value::as_str)
        .map(str::to_owned);
    for key in obj.keys() {
        match key.as_str() {
            "name" | "extends" | "colors" | "window" | "code" => {}
            // Islands | Flat is the setting alone (D12).
            "islands" => raw
                .problems
                .push("“islands” is a setting: Settings → Appearance → Panes".into()),
            _ => raw.problems.push(format!("Unknown field “{key}”")),
        }
    }
    // A file may set the window and the code apart; a theme with one of them is offered there only.
    let (window, code) = (obj.contains_key("window"), obj.contains_key("code"));
    if window || code {
        let both = obj.contains_key("colors");
        raw.window = window || both;
        raw.code = code || both;
        raw.split = true;
    }
    // Each field and the part it may set: any token, the window's, the code's.
    for (field, part) in [
        ("colors", None),
        ("window", Some(false)),
        ("code", Some(true)),
    ] {
        let Some(colors) = obj.get(field) else {
            continue;
        };
        let Some(colors) = colors.as_object() else {
            raw.problems.push(format!("“{field}” is not an object"));
            continue;
        };
        for (name, value) in colors {
            // A role by name (`code.parameter`): its own colour (#13, X45).
            if let Some(r) = name.strip_prefix("code.") {
                if part == Some(false) {
                    raw.problems.push(format!("“{name}” belongs in “code”"));
                    continue;
                }
                let Some(r) = role::named(r) else {
                    raw.problems.push(format!("No role “{name}”"));
                    continue;
                };
                match value.as_str().and_then(parse_hex) {
                    Some(c) => raw.roles.push((r, c)),
                    None => raw.problems.push(format!(
                        "{name}: “{value}” is not #rgb, #rrggbb or #rrggbbaa"
                    )),
                }
                continue;
            }
            let Some(i) = index(name) else {
                raw.problems.push(format!("No token “{name}”"));
                continue;
            };
            if let Some(code) = part
                && code != is_code(i)
            {
                let home = if is_code(i) { "code" } else { "window" };
                raw.problems.push(format!("“{name}” belongs in “{home}”"));
                continue;
            }
            match value.as_str().and_then(parse_hex) {
                Some(c) => raw.colors.push((i, c)),
                None => raw.problems.push(format!(
                    "{name}: “{value}” is not #rgb, #rrggbb or #rrggbbaa"
                )),
            }
        }
    }
    raw
}

/// The built-in themes and the user's (`user`: (file, text)), each with `extends` followed.
pub fn resolve(user: &[(PathBuf, String)]) -> Vec<Theme> {
    let mut raws: Vec<Raw> = BUILT_IN
        .iter()
        .map(|(id, text)| Raw {
            built_in: true,
            ..read_raw(text, id)
        })
        .collect();
    for (file, text) in user {
        let stem = file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("theme")
            .to_owned();
        let mut raw = read_raw(text, &stem);
        raw.file = Some(file.clone());
        // A user theme may not take a built-in's name: it would hide it.
        if BUILT_IN.iter().any(|(id, _)| *id == raw.id) {
            raw.problems
                .push(format!("“{}” is a built-in theme's name", raw.id));
            raw.id = stem.clone();
            raw.label = stem;
        }
        if raws.iter().any(|r| r.id == raw.id) {
            raw.problems
                .push(format!("Another theme is called “{}”", raw.id));
            continue;
        }
        raws.push(raw);
    }
    let base = |raws: &[Raw]| raws[0].colors.clone();
    let mut out = Vec::new();
    for (k, raw) in raws.iter().enumerate() {
        let mut problems = raw.problems.clone();
        // The chain of themes it extends, nearest first; One Dark under them all.
        let mut chain: Vec<&Raw> = vec![raw];
        let mut next = if k == 0 {
            None
        } else {
            Some(raw.extends.clone().unwrap_or_else(|| "one-dark".into()))
        };
        while let Some(id) = next {
            match raws.iter().find(|r| r.id == id) {
                Some(r) if chain.iter().any(|c| c.id == r.id) => {
                    problems.push(format!("“extends” goes round in a circle through “{id}”"));
                    break;
                }
                Some(r) => {
                    chain.push(r);
                    next = (r.id != "one-dark")
                        .then(|| r.extends.clone().unwrap_or_else(|| "one-dark".into()));
                }
                None => {
                    problems.push(format!("“extends”: no theme “{id}”"));
                    chain.push(&raws[0]);
                    break;
                }
            }
        }
        let mut colors = [0u32; TOKENS.len()];
        for (i, c) in base(&raws) {
            colors[i] = c;
        }
        let mut roles = [None; ROLES];
        for r in chain.iter().rev() {
            for &(i, c) in &r.colors {
                colors[i] = c;
            }
            for &(i, c) in &r.roles {
                roles[i] = Some(c);
            }
        }
        let mut theme = Theme {
            id: raw.id.clone(),
            label: raw.label.clone(),
            colors,
            roles,
            window: raw.window,
            code: raw.code,
            split: raw.split,
            built_in: raw.built_in,
            extends: raw.extends.clone(),
            own: raw.colors.iter().map(|(i, _)| *i).collect(),
            own_roles: raw.roles.iter().map(|(i, _)| *i).collect(),
            problems: Vec::new(),
            file: raw.file.clone(),
        };
        for (t, ratio, on) in theme.low_contrast() {
            problems.push(format!(
                "{} is {ratio:.1}:1 on {}; text needs 4.5:1",
                TOKENS[t as usize].0, TOKENS[on as usize].0
            ));
        }
        theme.problems = problems;
        out.push(theme);
    }
    out
}

/// The folder of the user's themes.
pub fn folder() -> Option<PathBuf> {
    Some(crate::platform::data_dir()?.join("themes"))
}

/// Every `.json` file of the themes folder, by name.
pub fn read_folder() -> Vec<(PathBuf, String)> {
    let Some(dir) = folder() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(PathBuf, String)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter_map(|p| std::fs::read_to_string(&p).ok().map(|t| (p, t)))
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

/// The syntax colours of `code` under 4.5:1 where the Code choice draws them (D16): on `window`'s
/// diff surface. (token, its ratio)
pub fn code_on(code: &Theme, window: &Theme) -> Vec<(T, f32)> {
    SYNTAX
        .iter()
        .map(|&t| (t, contrast(code.get(t), window.get(T::Bg3))))
        .filter(|(_, ratio)| *ratio < 4.5)
        .collect()
}

/// When the themes folder last changed (its files' times, summed), to notice an edit.
pub fn folder_stamp() -> u128 {
    let Some(Ok(entries)) = folder().map(std::fs::read_dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .filter_map(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .fold(1, |a, b| a.wrapping_add(b))
}

/// `#rgb`, `#rrggbb` or `#rrggbbaa` as 0xRRGGBBAA.
pub fn parse_hex(s: &str) -> Option<u32> {
    let h = s.trim().strip_prefix('#')?;
    if !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    match h.len() {
        3 => {
            let (r, g, b) = ((v >> 8) & 0xf, (v >> 4) & 0xf, v & 0xf);
            Some((r * 0x11) << 24 | (g * 0x11) << 16 | (b * 0x11) << 8 | 0xff)
        }
        6 => Some(v << 8 | 0xff),
        8 => Some(v),
        _ => None,
    }
}

/// 0xRRGGBBAA as `#rrggbb`, or `#rrggbbaa` when not opaque.
pub fn hex(c: u32) -> String {
    if c & 0xff == 0xff {
        format!("#{:06x}", c >> 8)
    } else {
        format!("#{c:08x}")
    }
}

/// `c` as hue (0–360), saturation, value and alpha (0–1): the colour picker's terms.
pub fn to_hsva(c: u32) -> [f32; 4] {
    let ch = |shift: u32| ((c >> shift) & 0xff) as f32 / 255.;
    let (r, g, b, a) = (ch(24), ch(16), ch(8), ch(0));
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    let h = if d == 0. {
        0.
    } else if max == r {
        60. * ((g - b) / d).rem_euclid(6.)
    } else if max == g {
        60. * ((b - r) / d + 2.)
    } else {
        60. * ((r - g) / d + 4.)
    };
    let s = if max == 0. { 0. } else { d / max };
    [h, s, max, a]
}

/// The colour of hue, saturation, value and alpha.
pub fn from_hsva([h, s, v, a]: [f32; 4]) -> u32 {
    let c = v * s;
    let at = h.rem_euclid(360.) / 60.;
    let x = c * (1. - (at % 2. - 1.).abs());
    let (r, g, b) = match at as u32 {
        0 => (c, x, 0.),
        1 => (x, c, 0.),
        2 => (0., c, x),
        3 => (0., x, c),
        4 => (x, 0., c),
        _ => (c, 0., x),
    };
    let byte = |f: f32| ((f + v - c).clamp(0., 1.) * 255.).round() as u32;
    byte(r) << 24 | byte(g) << 16 | byte(b) << 8 | (a.clamp(0., 1.) * 255.).round() as u32
}

pub(crate) fn luminance(c: u32) -> f32 {
    let ch = |shift: u32| {
        let v = ((c >> shift) & 0xff) as f32 / 255.;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(24) + 0.7152 * ch(16) + 0.0722 * ch(8)
}

/// `top` laid over `under` (opaque).
fn over(top: u32, under: u32) -> u32 {
    let a = (top & 0xff) as f32 / 255.;
    let mix = |shift: u32| {
        let t = ((top >> shift) & 0xff) as f32;
        let u = ((under >> shift) & 0xff) as f32;
        ((t * a + u * (1. - a)).round() as u32) << shift
    };
    mix(24) | mix(16) | mix(8) | 0xff
}

/// The WCAG contrast of text `ink` on `ground`.
pub fn contrast(ink: u32, ground: u32) -> f32 {
    let ink = over(ink, ground);
    let (a, b) = (luminance(ink), luminance(ground));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// `ink` moved toward the text colour of `ground`'s opposite until it reads at 4.5:1 on every
/// surface of `theme` — the editor's "Raise to 4.5:1".
pub fn raise(theme: &Theme, ink: u32) -> u32 {
    let worst = |c: u32| {
        SURFACES
            .iter()
            .map(|&s| contrast(c, theme.get(s)))
            .fold(f32::MAX, f32::min)
    };
    let target: u32 = if theme.light() {
        0x000000ff
    } else {
        0xffffffff
    };
    let mut c = ink | 0xff;
    for step in 1..=20 {
        if worst(c) >= 4.5 {
            break;
        }
        let k = step as f32 / 20.;
        let mix = |shift: u32| {
            let a = ((ink >> shift) & 0xff) as f32;
            let b = ((target >> shift) & 0xff) as f32;
            ((a + (b - a) * k).round() as u32) << shift
        };
        c = mix(24) | mix(16) | mix(8) | 0xff;
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_go_to_the_pickers_terms_and_back() {
        for c in [
            0xff0000ff, 0x61afefff, 0x282c34ff, 0x00000000, 0xffffff80, 0x98c37940,
        ] {
            assert_eq!(from_hsva(to_hsva(c)), c, "{c:08x}");
        }
        let [h, s, v, a] = to_hsva(0x00ff00ff);
        assert_eq!((h, s, v, a), (120., 1., 1., 1.));
        // A grey has no hue: any gives it back.
        assert_eq!(from_hsva([200., 0., 0.5, 1.]), 0x808080ff);
    }

    #[test]
    fn every_built_in_reads_at_four_and_a_half_to_one() {
        for t in resolve(&[]) {
            assert!(t.problems.is_empty(), "{}: {:?}", t.id, t.problems);
        }
    }

    #[test]
    fn a_user_theme_extends_and_lists_its_problems() {
        let text = r##"{ "name": "mine", "extends": "night", "islands": false,
            "colors": { "interactive-01": "#e5c07b", "nope": "#fff", "code-01": "blue", "code.parameter": "#d19a66" } }"##;
        let themes = resolve(&[(PathBuf::from("/t/mine.json"), text.into())]);
        let night = themes.iter().find(|t| t.id == "night").unwrap();
        let mine = themes.iter().find(|t| t.id == "mine").unwrap();
        assert_eq!(mine.get(T::Accent), 0xe5c07bff);
        assert_eq!(mine.get(T::Bg1), night.get(T::Bg1));
        assert!(mine.window && mine.code);
        // A role by name keeps its colour (#13).
        let parameter = role::named("parameter").unwrap();
        assert_eq!(mine.roles[parameter], Some(0xd19a66ff));
        assert!(
            mine.to_json(false)
                .contains("\"code.parameter\": \"#d19a66\"")
        );
        // `islands` is a setting now (D12): the file is told so.
        assert_eq!(mine.problems.len(), 3, "{:?}", mine.problems);
        assert!(mine.problems.iter().any(|p| p.contains("Settings")));
        let json = mine.to_json(false);
        assert!(json.contains("\"extends\": \"night\""));
        assert!(!json.contains("islands"));
    }

    #[test]
    fn window_and_code_set_their_parts_apart() {
        let text =
            r##"{ "name": "ink", "code": { "code-01": "#ff9e64", "background-01": "#000" } }"##;
        let themes = resolve(&[(PathBuf::from("/t/ink.json"), text.into())]);
        let ink = themes.iter().find(|t| t.id == "ink").unwrap();
        assert!(ink.code && !ink.window);
        assert_eq!(ink.get(T::Code1), 0xff9e64ff);
        assert_eq!(ink.get(T::Bg1), themes[0].get(T::Bg1));
        assert_eq!(ink.problems, ["“background-01” belongs in “window”"]);
        let json = ink.to_json(false);
        assert!(
            json.contains("\"code\"")
                && !json.contains("\"colors\"")
                && !json.contains("\"window\"")
        );
        // Read back, it is the same theme.
        let again = resolve(&[(PathBuf::from("/t/ink.json"), json)]);
        let again = again.iter().find(|t| t.id == "ink").unwrap();
        assert_eq!(
            (again.colors, again.window, again.code),
            (ink.colors, false, true)
        );
        let both = resolve(&[(
            PathBuf::from("/t/both.json"),
            r##"{ "name": "both", "window": { "interactive-01": "#e5c07b" }, "code": {} }"##.into(),
        )]);
        let both = both.iter().find(|t| t.id == "both").unwrap();
        assert!(both.window && both.code && both.problems.is_empty());
    }

    #[test]
    fn light_code_on_a_dark_window_is_said() {
        let themes = resolve(&[]);
        let find = |id: &str| themes.iter().find(|t| t.id == id).unwrap();
        assert!(code_on(find("one-dark"), find("one-dark")).is_empty());
        assert!(!code_on(find("paper"), find("graphite")).is_empty());
    }

    #[test]
    fn a_broken_file_or_a_circle_is_a_problem_not_a_failure() {
        let themes = resolve(&[
            (
                PathBuf::from("/t/a.json"),
                r#"{ "name": "a", "extends": "b" }"#.into(),
            ),
            (
                PathBuf::from("/t/b.json"),
                r#"{ "name": "b", "extends": "a" }"#.into(),
            ),
            (PathBuf::from("/t/c.json"), "{ not json".into()),
        ]);
        let a = themes.iter().find(|t| t.id == "a").unwrap();
        assert!(a.problems.iter().any(|p| p.contains("circle")));
        let c = themes.iter().find(|t| t.id == "c").unwrap();
        assert!(c.problems[0].starts_with("Not JSON"));
        assert_eq!(c.colors, themes[0].colors);
    }

    #[test]
    fn hex_reads_three_six_and_eight_digits() {
        assert_eq!(parse_hex("#fff"), Some(0xffffffff));
        assert_eq!(parse_hex("#61afef"), Some(0x61afefff));
        assert_eq!(parse_hex("#98c3791f"), Some(0x98c3791f));
        assert_eq!(parse_hex("61afef"), None);
        assert_eq!(hex(0x98c3791f), "#98c3791f");
    }

    #[test]
    fn raising_a_dim_label_reaches_the_bar() {
        let one = &resolve(&[])[0];
        let raised = raise(one, 0x5c6370ff);
        assert!(
            SURFACES
                .iter()
                .all(|&s| contrast(raised, one.get(s)) >= 4.5)
        );
    }
}
