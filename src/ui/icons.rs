//! Lucide icons (ISC licence, https://lucide.dev), embedded: the app is offline and uses one icon
//! set, 1.75 px strokes with round caps, drawn in the text colour.

use gpui::{AssetSource, Result, SharedString, Svg, prelude::*, px, svg};
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
    ("back", lucide!(r#"<path d="m15 18-6-6 6-6"/>"#)),
    ("forward", lucide!(r#"<path d="m9 18 6-6-6-6"/>"#)),
    ("chevron", lucide!(r#"<path d="m6 9 6 6 6-6"/>"#)),
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
        "branch",
        lucide!(
            r#"<line x1="6" x2="6" y1="3" y2="15"/>"#
            r#"<circle cx="18" cy="6" r="3"/>"#
            r#"<circle cx="6" cy="18" r="3"/>"#
            r#"<path d="M18 9a9 9 0 0 1-9 9"/>"#
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
