//! Lucide icons (ISC, see `assets/icons/LICENSE-lucide.txt`), embedded as SVG, recoloured to
//! white, rasterised by egui_extras at the on-screen size and tinted per use. Same approach as
//! PhotoCraft: no Unicode glyphs, which half the time render as boxes.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use egui::{Color32, Rect, Response, Sense, Ui, Vec2, load::Bytes};

use crate::theme::Tokens;

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_str!(concat!("../assets/icons/", $name, ".svg")))),*]
    };
}

const SOURCES: &[(&str, &str)] = icons![
    "arrow-down",
    "arrow-up",
    "blocks",
    "book-open",
    "case-sensitive",
    "chevron-down",
    "chevron-right",
    "chevron-up",
    "chevrons-down-up",
    "code",
    "ellipsis",
    "file",
    "file-code",
    "file-text",
    "files",
    "folder",
    "folder-open",
    "folder-plus",
    "image",
    "list-x",
    "palette",
    "plus",
    "refresh-cw",
    "regex",
    "replace",
    "replace-all",
    "save",
    "search",
    "settings",
    "square-terminal",
    "whole-word",
    "x",
];

fn white_icons() -> &'static HashMap<&'static str, Arc<[u8]>> {
    static MAP: OnceLock<HashMap<&'static str, Arc<[u8]>>> = OnceLock::new();
    MAP.get_or_init(|| {
        SOURCES
            .iter()
            .map(|(name, svg)| {
                let white = svg.replace("currentColor", "#ffffff");
                (*name, Arc::from(white.into_bytes().into_boxed_slice()))
            })
            .collect()
    })
}

/// An egui image for an icon, tinted. Unknown names give an empty image.
pub fn image(name: &str, size: f32, tint: Color32) -> egui::Image<'static> {
    let bytes = white_icons()
        .get(name)
        .cloned()
        .unwrap_or_else(|| Arc::from(Vec::new().into_boxed_slice()));
    egui::Image::from_bytes(format!("bytes://icons/{name}.svg"), Bytes::Shared(bytes))
        .fit_to_exact_size(Vec2::splat(size))
        .tint(tint)
}

/// Paints an icon centred in `rect`.
pub fn paint(ui: &Ui, rect: Rect, name: &str, size: f32, tint: Color32) {
    let r = Rect::from_center_size(rect.center(), Vec2::splat(size));
    image(name, size, tint).paint_at(ui, r);
}

/// A small square icon-only button (rows, panel headers).
pub fn small_button(ui: &mut Ui, icon: &str, tooltip: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, t.radius_sm, t.pressed);
    }
    let tint = if !ui.is_enabled() {
        t.text_faint
    } else if resp.hovered() {
        t.text
    } else {
        t.icon
    };
    paint(ui, rect, icon, 14.0, tint);
    resp.on_hover_text(tooltip)
}

/// An icon-only button of the window's title bar, a size up from [`small_button`].
pub fn bar_button(ui: &mut Ui, icon: &str, tooltip: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, t.radius, t.pressed);
    }
    let tint = if resp.hovered() { t.text } else { t.icon };
    paint(ui, rect, icon, 17.0, tint);
    resp.on_hover_text(tooltip)
}

/// An icon laid out inline, like a label.
pub fn inline(ui: &mut Ui, name: &str, size: f32, tint: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint(ui, rect, name, size, tint);
}
