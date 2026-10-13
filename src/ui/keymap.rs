//! Rebindable keys. The defaults are the bindings written in `ui::run`; `keymap.txt` in the app
//! support folder replaces the keys of an action, one line each: `NextFile = ] | l` (keys split by
//! ` | `, nothing after `=` leaves the action without a key). Settings → Keyboard writes it.

use crate::storage;
use gpui::{App, DummyKeyboardMapper, Global, KeyBinding};
use std::collections::{BTreeMap, HashSet};

/// The keys as written in code, and the user's changes.
pub struct Keymap {
    defaults: Vec<KeyBinding>,
    /// Action name (`NextFile`) → its keys (`cmd-]`, `l`).
    overrides: BTreeMap<String, Vec<String>>,
}

impl Global for Keymap {}

/// One action as Settings → Keyboard lists it.
pub struct Entry {
    pub name: &'static str,
    /// `Next file`.
    pub label: String,
    /// Its keys, as GPUI writes them (`cmd-shift-enter`).
    pub keys: Vec<String>,
    pub changed: bool,
    /// Another action on one of its keys where both apply: (key, the other action's label).
    pub clash: Option<(String, String)>,
}

/// Binds the defaults with the user's changes on top.
pub fn install(defaults: Vec<KeyBinding>, cx: &mut App) {
    let overrides = parse(&storage::read("keymap.txt"));
    cx.set_global(Keymap {
        defaults,
        overrides,
    });
    apply(cx);
}

/// Gives action `name` the keys `keys` (`None`: back to its defaults) and binds again.
pub fn set(name: &str, keys: Option<Vec<String>>, cx: &mut App) {
    let map = cx.global_mut::<Keymap>();
    match keys {
        Some(keys) => {
            map.overrides.insert(name.to_owned(), keys);
        }
        None => {
            map.overrides.remove(name);
        }
    }
    storage::save("keymap.txt", render(&map.overrides));
    apply(cx);
}

fn apply(cx: &mut App) {
    let bindings = cx.global::<Keymap>().effective();
    cx.clear_key_bindings();
    cx.bind_keys(bindings);
}

/// `gitlance::NextFile` → `NextFile`.
fn short(name: &'static str) -> &'static str {
    name.rsplit("::").next().unwrap_or(name)
}

impl Keymap {
    /// The bindings in force: an action with changed keys gets them, in the context of its first
    /// default binding. The changed ones come last, so where a key is also shipped for another
    /// action (a clash the Keyboard page shows), the user's choice is the one that acts.
    fn effective(&self) -> Vec<KeyBinding> {
        let mut out = Vec::new();
        let mut changed = Vec::new();
        let mut replaced = HashSet::new();
        for b in &self.defaults {
            let name = short(b.action().name());
            let Some(keys) = self.overrides.get(name) else {
                out.push(b.clone());
                continue;
            };
            if !replaced.insert(name) {
                continue;
            }
            changed.extend(keys.iter().filter_map(|k| {
                KeyBinding::load(
                    k,
                    b.action().boxed_clone(),
                    b.predicate(),
                    false,
                    None,
                    &DummyKeyboardMapper,
                )
                .ok()
            }));
        }
        out.extend(changed);
        out
    }

    /// Every action with a key (or that had one), by label.
    pub fn entries(&self) -> Vec<Entry> {
        let effective = self.effective();
        let mut seen = HashSet::new();
        let mut entries: Vec<Entry> = self
            .defaults
            .iter()
            .filter_map(|b| {
                let name = short(b.action().name());
                seen.insert(name).then(|| {
                    let mine: Vec<&KeyBinding> = effective
                        .iter()
                        .filter(|e| short(e.action().name()) == name)
                        .collect();
                    let keys: Vec<String> = mine.iter().map(|e| keystrokes(e)).collect();
                    let clash = mine.iter().find_map(|e| {
                        let key = keystrokes(e);
                        effective
                            .iter()
                            .filter(|o| short(o.action().name()) != name)
                            .find(|o| keystrokes(o) == key && overlap(e, o))
                            .map(|o| (key.clone(), label(short(o.action().name()))))
                    });
                    Entry {
                        name,
                        label: label(name),
                        keys,
                        changed: self.overrides.contains_key(name),
                        clash,
                    }
                })
            })
            .collect();
        entries.sort_by(|a, b| a.label.cmp(&b.label));
        entries
    }

    /// The key to show for `action` when the user changed it (`""` when it has none now); `None`
    /// keeps the shipped one.
    pub fn shown(&self, action: &dyn gpui::Action) -> Option<String> {
        self.overrides
            .get(short(action.name()))
            .map(|keys| keys.first().map(|k| symbols(k)).unwrap_or_default())
    }

    /// The keys the user changed: (label, keys).
    pub fn changes(&self) -> Vec<(String, Vec<String>)> {
        self.overrides
            .iter()
            .map(|(name, keys)| (label(name), keys.clone()))
            .collect()
    }
}

fn keystrokes(b: &KeyBinding) -> String {
    b.keystrokes()
        .iter()
        .map(|k| k.inner().unparse())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether two bindings can be live at once. Contexts here are `Workspace` with `Typing`,
/// `Palette` and `Caret` refinements: the same context, or one of them plain `Workspace` (or
/// none), counts as overlapping; two different refinements are taken to be meant apart.
fn overlap(a: &KeyBinding, b: &KeyBinding) -> bool {
    let context = |k: &KeyBinding| k.predicate().map(|p| p.to_string());
    let (ca, cb) = (context(a), context(b));
    let plain = |c: &Option<String>| c.as_deref().is_none_or(|c| c == "Workspace");
    ca == cb || plain(&ca) || plain(&cb)
}

/// `NextFile` → `Next file`.
fn label(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push(' ');
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// `cmd-shift-enter` → `⇧⌘↵`, as the app writes keys (macOS order: ⌃ ⌥ ⇧ ⌘).
pub fn symbols(keys: &str) -> String {
    keys.split(' ')
        .map(|stroke| {
            let parts: Vec<&str> = stroke.split('-').collect();
            // `cmd--` is ⌘ and the minus key.
            let (mods, key) = match parts.as_slice() {
                [.., "", ""] => (&parts[..parts.len() - 2], "-"),
                [mods @ .., key] => (mods, *key),
                [] => (&parts[..0], ""),
            };
            let mut out = String::new();
            for (word, sign) in [("ctrl", "⌃"), ("alt", "⌥"), ("shift", "⇧"), ("cmd", "⌘")]
            {
                if mods.contains(&word) {
                    out.push_str(sign);
                }
            }
            out.push_str(match key {
                "enter" => "↵",
                "escape" => "esc",
                "backspace" => "⌫",
                "delete" => "⌦",
                "tab" => "⇥",
                "space" => "Space",
                "up" => "↑",
                "down" => "↓",
                "left" => "←",
                "right" => "→",
                "pageup" => "PgUp",
                "pagedown" => "PgDn",
                "home" => "Home",
                "end" => "End",
                other => other,
            });
            out
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse(text: &str) -> BTreeMap<String, Vec<String>> {
    text.lines()
        .filter_map(|line| {
            let (name, keys) = line.split_once('=')?;
            let name = name.trim();
            (!name.is_empty() && !name.starts_with('#')).then(|| {
                let keys = keys
                    .split(" | ")
                    .map(str::trim)
                    .filter(|k| !k.is_empty())
                    .map(str::to_owned)
                    .collect();
                (name.to_owned(), keys)
            })
        })
        .collect()
}

fn render(overrides: &BTreeMap<String, Vec<String>>) -> String {
    overrides
        .iter()
        .map(|(name, keys)| format!("{name} = {}\n", keys.join(" | ")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    gpui::actions!(test, [NextFile, PreviousFile, Comment]);

    fn map(overrides: &str) -> Keymap {
        Keymap {
            defaults: vec![
                KeyBinding::new("l", NextFile, Some("Workspace && !Typing")),
                KeyBinding::new("]", NextFile, Some("Workspace && !Typing")),
                KeyBinding::new("h", PreviousFile, Some("Workspace && !Typing")),
                KeyBinding::new("c", Comment, Some("Workspace && !Typing")),
            ],
            overrides: parse(overrides),
        }
    }

    #[test]
    fn a_change_replaces_every_key_of_its_action() {
        let entries = map("NextFile = cmd-down | j\n").entries();
        let next = entries.iter().find(|e| e.name == "NextFile").unwrap();
        assert_eq!(next.keys, vec!["cmd-down", "j"]);
        assert!(next.changed);
        assert_eq!(next.label, "Next file");
        assert!(next.clash.is_none());
    }

    #[test]
    fn a_changed_key_is_bound_last_so_it_wins() {
        let bound = map("NextFile = c\n").effective();
        assert_eq!(short(bound.last().unwrap().action().name()), "NextFile");
    }

    #[test]
    fn a_key_taken_twice_is_a_clash() {
        let entries = map("Comment = h\n").entries();
        let comment = entries.iter().find(|e| e.name == "Comment").unwrap();
        assert_eq!(comment.clash, Some(("h".into(), "Previous file".into())));
    }

    #[test]
    fn an_empty_change_unbinds() {
        let entries = map("Comment =\n").entries();
        assert!(
            entries
                .iter()
                .find(|e| e.name == "Comment")
                .unwrap()
                .keys
                .is_empty()
        );
    }

    #[test]
    fn the_file_reads_back_what_it_wrote() {
        let overrides = parse("NextFile = cmd-] | l\nComment =\n");
        assert_eq!(parse(&render(&overrides)), overrides);
    }

    #[test]
    fn the_shipped_keys_do_not_clash() {
        let shipped = Keymap {
            defaults: super::super::default_bindings(),
            overrides: BTreeMap::new(),
        };
        let clashes: Vec<_> = shipped
            .entries()
            .into_iter()
            .filter_map(|e| e.clash.map(|c| (e.label, c)))
            .collect();
        assert!(clashes.is_empty(), "{clashes:?}");
    }

    #[test]
    fn keys_are_written_as_the_app_writes_them() {
        assert_eq!(symbols("cmd-shift-enter"), "⇧⌘↵");
        assert_eq!(symbols("alt-down"), "⌥↓");
        assert_eq!(symbols("cmd--"), "⌘-");
        assert_eq!(symbols("ctrl-1"), "⌃1");
    }
}
