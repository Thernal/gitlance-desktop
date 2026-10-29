//! Lucide icons (ISC licence, https://lucide.dev), embedded: the app is offline and uses one icon
//! set, 1.75 px strokes with round caps, drawn in the text colour.

use super::px;
use gpui::{AssetSource, Result, SharedString, Svg, prelude::*, svg};
use std::borrow::Cow;

macro_rules! lucide {
    ($($body:literal)+) => {
        concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" "#,
            r#"stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">"#,
            $($body,)+
            "</svg>"
        )
    };
}

const ICONS: &[(&str, &str)] = &[
    (
        "update",
        lucide!(
            r#"<circle cx="12" cy="12" r="10"/>"#
            r#"<path d="m16 12-4-4-4 4"/>"#
            r#"<path d="M12 16V8"/>"#
        ),
    ),
    (
        "flag",
        lucide!(
            r#"<path d="M4 15s1-1 4-1 5 2 8 2 4-1 4-1V3s-1 1-4 1-5-2-8-2-4 1-4 1z"/>"#
            r#"<line x1="4" x2="4" y1="22" y2="15"/>"#
        ),
    ),
    (
        "refresh",
        lucide!(
            r#"<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/>"#
            r#"<path d="M21 3v5h-5"/>"#
            r#"<path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/>"#
            r#"<path d="M8 16H3v5"/>"#
        ),
    ),
    (
        "open",
        lucide!(
            r#"<path d="m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2"/>"#
        ),
    ),
    (
        "settings",
        lucide!(
            r#"<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/>"#
            r#"<circle cx="12" cy="12" r="3"/>"#
        ),
    ),
    (
        "search",
        lucide!(r#"<circle cx="11" cy="11" r="8"/>"# r#"<path d="m21 21-4.3-4.3"/>"#),
    ),
    (
        "eye",
        lucide!(
            r#"<path d="M2.06 12.35a1 1 0 0 1 0-.7 10.75 10.75 0 0 1 19.88 0 1 1 0 0 1 0 .7 10.75 10.75 0 0 1-19.88 0"/>"#
            r#"<circle cx="12" cy="12" r="3"/>"#
        ),
    ),
    ("check", lucide!(r#"<path d="M20 6 9 17l-5-5"/>"#)),
    (
        "x",
        lucide!(r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#),
    ),
    (
        "bell",
        lucide!(
            r#"<path d="M10.268 21a2 2 0 0 0 3.464 0"/><path d="M3.262 15.326A1 1 0 0 0 4 17h16a1 1 0 0 0 .74-1.673C19.41 13.956 18 12.499 18 8A6 6 0 0 0 6 8c0 4.499-1.411 5.956-2.738 7.326"/>"#
        ),
    ),
    (
        "alert",
        lucide!(
            r#"<circle cx="12" cy="12" r="10"/><line x1="12" x2="12" y1="8" y2="12"/><line x1="12" x2="12.01" y1="16" y2="16"/>"#
        ),
    ),
    // The composer's markdown helpers, and T that holds them when its box is narrow (#129).
    (
        "type",
        lucide!(
            r#"<polyline points="4 7 4 4 20 4 20 7"/>"#
            r#"<line x1="9" x2="15" y1="20" y2="20"/>"#
            r#"<line x1="12" x2="12" y1="4" y2="20"/>"#
        ),
    ),
    (
        "bold",
        lucide!(
            r#"<path d="M6 12h9a4 4 0 0 1 0 8H7a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h7a4 4 0 0 1 0 8"/>"#
        ),
    ),
    (
        "italic",
        lucide!(
            r#"<line x1="19" x2="10" y1="4" y2="4"/>"#
            r#"<line x1="14" x2="5" y1="20" y2="20"/>"#
            r#"<line x1="15" x2="9" y1="4" y2="20"/>"#
        ),
    ),
    (
        "strikethrough",
        lucide!(
            r#"<path d="M16 4H9a3 3 0 0 0-2.83 4"/>"#
            r#"<path d="M14 12a4 4 0 0 1 0 8H6"/>"#
            r#"<line x1="4" x2="20" y1="12" y2="12"/>"#
        ),
    ),
    (
        "code",
        lucide!(r#"<polyline points="16 18 22 12 16 6"/>"# r#"<polyline points="8 6 2 12 8 18"/>"#),
    ),
    (
        "link",
        lucide!(
            r#"<path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/>"#
            r#"<path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/>"#
        ),
    ),
    (
        "heading",
        lucide!(r#"<path d="M6 12h12"/>"# r#"<path d="M6 20V4"/>"# r#"<path d="M18 20V4"/>"#),
    ),
    (
        "quote",
        lucide!(
            r#"<path d="M17 6H3"/>"#
            r#"<path d="M21 12H8"/>"#
            r#"<path d="M21 18H8"/>"#
            r#"<path d="M3 12v6"/>"#
        ),
    ),
    (
        "list",
        lucide!(
            r#"<path d="M3 12h.01"/>"#
            r#"<path d="M3 18h.01"/>"#
            r#"<path d="M3 6h.01"/>"#
            r#"<path d="M8 12h13"/>"#
            r#"<path d="M8 18h13"/>"#
            r#"<path d="M8 6h13"/>"#
        ),
    ),
    (
        "list-ordered",
        lucide!(
            r#"<path d="M10 12h11"/>"#
            r#"<path d="M10 18h11"/>"#
            r#"<path d="M10 6h11"/>"#
            r#"<path d="M4 10h2"/>"#
            r#"<path d="M4 6h1v4"/>"#
            r#"<path d="M6 18H4c0-1 2-2 2-3s-1-1.5-2-1"/>"#
        ),
    ),
    (
        "list-checks",
        lucide!(
            r#"<path d="m3 17 2 2 4-4"/>"#
            r#"<path d="m3 7 2 2 4-4"/>"#
            r#"<path d="M13 6h8"/>"#
            r#"<path d="M13 12h8"/>"#
            r#"<path d="M13 18h8"/>"#
        ),
    ),
    (
        "code-block",
        lucide!(
            r#"<path d="m10 9-3 3 3 3"/>"#
            r#"<path d="m14 15 3-3-3-3"/>"#
            r#"<rect x="3" y="3" width="18" height="18" rx="2"/>"#
        ),
    ),
    ("back", lucide!(r#"<path d="m15 18-6-6 6-6"/>"#)),
    (
        "plus",
        lucide!(r#"<path d="M5 12h14"/>"# r#"<path d="M12 5v14"/>"#),
    ),
    (
        "external",
        lucide!(
            r#"<path d="M15 3h6v6"/>"#
            r#"<path d="M10 14 21 3"/>"#
            r#"<path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>"#
        ),
    ),
    ("forward", lucide!(r#"<path d="m9 18 6-6-6-6"/>"#)),
    ("chevron", lucide!(r#"<path d="m6 9 6 6 6-6"/>"#)),
    ("chevron-right", lucide!(r#"<path d="m9 18 6-6-6-6"/>"#)),
    (
        "sidebar",
        lucide!(r#"<rect width="18" height="18" x="3" y="3" rx="2"/>"# r#"<path d="M9 3v18"/>"#),
    ),
    (
        "files",
        lucide!(
            r#"<path d="M20 7h-3a2 2 0 0 1-2-2V2"/>"#
            r#"<path d="M9 18a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h7l4 4v10a2 2 0 0 1-2 2Z"/>"#
            r#"<path d="M3 7.6v12.8A1.6 1.6 0 0 0 4.6 22h9.8"/>"#
        ),
    ),
    (
        "pull-request",
        lucide!(
            r#"<circle cx="18" cy="18" r="3"/>"#
            r#"<circle cx="6" cy="6" r="3"/>"#
            r#"<path d="M13 6h3a2 2 0 0 1 2 2v7"/>"#
            r#"<line x1="6" x2="6" y1="9" y2="21"/>"#
        ),
    ),
    (
        "user-check",
        lucide!(
            r#"<path d="m16 11 2 2 4-4"/>"#
            r#"<path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/>"#
            r#"<circle cx="9" cy="7" r="4"/>"#
        ),
    ),
    (
        "bookmark",
        lucide!(r#"<path d="m19 21-7-4-7 4V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v16z"/>"#),
    ),
    (
        "pin",
        lucide!(
            r#"<path d="M12 17v5"/>"#
            r#"<path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z"/>"#
        ),
    ),
    (
        "message",
        lucide!(r#"<path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>"#),
    ),
    (
        "info",
        lucide!(r#"<circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/>"#),
    ),
    (
        "key",
        lucide!(
            r#"<path d="M2.586 17.414A2 2 0 0 0 2 18.828V21a1 1 0 0 0 1 1h3a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h1a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h.172a2 2 0 0 0 1.414-.586l.814-.814a6.5 6.5 0 1 0-4-4z"/>"#
            r#"<circle cx="16.5" cy="7.5" r=".5" fill="black"/>"#
        ),
    ),
    (
        "terminal",
        lucide!(
            r#"<polyline points="4 17 10 11 4 5"/>"#
            r#"<line x1="12" x2="20" y1="19" y2="19"/>"#
        ),
    ),
    (
        "app",
        lucide!(
            r#"<rect x="2" y="4" width="20" height="16" rx="2"/>"#
            r#"<path d="M10 4v4"/>"#
            r#"<path d="M2 8h20"/>"#
            r#"<path d="M6 4v4"/>"#
        ),
    ),
    (
        "inbox",
        lucide!(
            r#"<polyline points="22 12 16 12 14 15 10 15 8 12 2 12"/>"#
            r#"<path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/>"#
        ),
    ),
    (
        "commit",
        lucide!(
            r#"<circle cx="12" cy="12" r="3"/>"#
            r#"<line x1="3" x2="9" y1="12" y2="12"/><line x1="15" x2="21" y1="12" y2="12"/>"#
        ),
    ),
    (
        "bot",
        lucide!(
            r#"<path d="M12 8V4H8"/>"#
            r#"<rect width="16" height="12" x="4" y="8" rx="2"/>"#
            r#"<path d="M2 14h2"/><path d="M20 14h2"/><path d="M15 13v2"/><path d="M9 13v2"/>"#
        ),
    ),
    (
        "branch",
        lucide!(
            r#"<line x1="6" x2="6" y1="3" y2="15"/>"#
            r#"<circle cx="18" cy="6" r="3"/>"#
            r#"<circle cx="6" cy="18" r="3"/>"#
            r#"<path d="M18 9a9 9 0 0 1-9 9"/>"#
        ),
    ),
    // A comment's secondary buttons keep only these when the diff is narrow (#69).
    (
        "pencil",
        lucide!(
            r#"<path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"/>"#
            r#"<path d="m15 5 4 4"/>"#
        ),
    ),
    ("check", lucide!(r#"<path d="M20 6 9 17l-5-5"/>"#)),
    (
        "rotate-ccw",
        lucide!(
            r#"<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/>"#
            r#"<path d="M3 3v5h5"/>"#
        ),
    ),
    (
        "trash",
        lucide!(
            r#"<path d="M10 11v6"/><path d="M14 11v6"/>"#
            r#"<path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/>"#
            r#"<path d="M3 6h18"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>"#
        ),
    ),
    (
        "send",
        lucide!(
            r#"<path d="M14.536 21.686a.5.5 0 0 0 .937-.024l6.5-19a.496.496 0 0 0-.635-.635l-19 6.5a.5.5 0 0 0-.024.937l7.93 3.18a2 2 0 0 1 1.112 1.11z"/>"#
            r#"<path d="m21.854 2.147-10.94 10.939"/>"#
        ),
    ),
    (
        "layers",
        lucide!(
            r#"<path d="m12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83Z"/>"#
            r#"<path d="m22 17.65-9.17 4.16a2 2 0 0 1-1.66 0L2 17.65"/>"#
            r#"<path d="m22 12.65-9.17 4.16a2 2 0 0 1-1.66 0L2 12.65"/>"#
        ),
    ),
    // The Code space and a file open in it (the shell, #70).
    (
        "folder",
        lucide!(
            r#"<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>"#
        ),
    ),
    (
        "file",
        lucide!(
            r#"<path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/>"#
            r#"<path d="M14 2v4a2 2 0 0 0 2 2h4"/>"#
        ),
    ),
    (
        "user",
        lucide!(
            r#"<path d="M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2"/>"#
            r#"<circle cx="12" cy="7" r="4"/>"#
        ),
    ),
];

/// Serves the embedded icons to GPUI's SVG renderer.
pub struct Icons;

impl AssetSource for Icons {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(name, _)| path == format!("icons/{name}.svg"))
            .map(|(_, body)| Cow::Borrowed(body.as_bytes())))
    }

    fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .map(|(name, _)| format!("icons/{name}.svg").into())
            .collect())
    }
}

/// An 18 px icon; set its colour on the icon itself (an svg takes none from its parent).
pub fn icon(name: &'static str) -> Svg {
    debug_assert!(ICONS.iter().any(|(n, _)| *n == name), "unknown icon {name}");
    svg()
        .path(format!("icons/{name}.svg"))
        .size(px(18.))
        .flex_none()
}

/// The icon of the file at `path` (from the repository's root), in colour, `size` square
/// (#95, `crate::file_icons`).
pub fn file_icon(path: &str, size: f32) -> gpui::AnyElement {
    typed(crate::file_icons::file(path, !super::theme::light()), size)
}

/// The icon of the folder at `path`.
pub fn folder_icon(path: &str, size: f32) -> gpui::AnyElement {
    typed(
        crate::file_icons::folder_icon(path, !super::theme::light()),
        size,
    )
}

fn typed(image: Option<std::sync::Arc<gpui::Image>>, size: f32) -> gpui::AnyElement {
    match image {
        Some(image) => gpui::img(image)
            .size(px(size))
            .flex_none()
            .into_any_element(),
        None => icon("file").size(px(size)).into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_served_as_an_svg() {
        for (name, _) in ICONS {
            let body = Icons.load(&format!("icons/{name}.svg")).unwrap().unwrap();
            let text = std::str::from_utf8(&body).unwrap();
            assert!(
                text.starts_with("<svg") && text.ends_with("</svg>"),
                "{name}"
            );
        }
        assert!(Icons.load("icons/nope.svg").unwrap().is_none());
    }
}
