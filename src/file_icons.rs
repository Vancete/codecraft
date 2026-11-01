//! The icon next to a file or folder (explorer rows, editor tabs): from VS Code's file icon
//! theme when that setting is on and the theme was found, otherwise a built-in Lucide icon
//! coloured by file type.

use std::sync::Arc;

use egui::{Color32, FontFamily, FontId, Rect, Ui, Vec2};

use crate::icons;
use crate::theme::Tokens;
use crate::vscode::icons::{IconDef, IconTheme};

/// Size icons are drawn at.
pub const SIZE: f32 = 16.0;

#[derive(Clone, Default)]
pub struct FileIcons {
    theme: Option<Arc<crate::vscode::Loaded>>,
}

impl FileIcons {
    /// Icons from `loaded`'s icon theme, or the built-in ones without it.
    pub fn new(loaded: Option<Arc<crate::vscode::Loaded>>) -> Self {
        Self {
            theme: loaded.filter(|l| l.icons.is_some()),
        }
    }

    fn theme(&self) -> Option<&IconTheme> {
        self.theme.as_ref().and_then(|l| l.icons.as_ref())
    }

    /// The theme hides the chevrons of folders (it draws open and closed folders instead).
    pub fn hides_arrows(&self) -> bool {
        self.theme().is_some_and(|t| t.hides_arrows)
    }

    pub fn paint_file(&self, ui: &Ui, rect: Rect, name: &str) {
        let t = Tokens::get(ui.ctx());
        if let Some(theme) = self.theme()
            && let Some(def) = theme.file(name, !t.dark())
        {
            paint_def(ui, theme, rect, def, t.icon);
            return;
        }
        let (icon, color) = builtin(name, &t);
        icons::paint(ui, rect, icon, SIZE - 2.0, color);
    }

    /// Returns false when the theme draws no folders, so the caller can draw its own.
    pub fn paint_folder(&self, ui: &Ui, rect: Rect, name: &str, expanded: bool) -> bool {
        let t = Tokens::get(ui.ctx());
        let Some(theme) = self.theme() else {
            let icon = if expanded { "folder-open" } else { "folder" };
            icons::paint(ui, rect, icon, SIZE - 2.0, t.text_faint);
            return true;
        };
        match theme.folder(name, expanded, !t.dark()) {
            Some(def) => {
                paint_def(ui, theme, rect, def, t.icon);
                true
            }
            None => false,
        }
    }
}

fn paint_def(ui: &Ui, theme: &IconTheme, rect: Rect, def: &IconDef, fallback: Color32) {
    let rect = Rect::from_center_size(rect.center(), Vec2::splat(SIZE));
    match def {
        IconDef::Glyph {
            family,
            ch,
            color,
            scale,
        } => {
            let font = FontId::new(12.0 * scale, FontFamily::Name(family.as_str().into()));
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                ch,
                font,
                color.unwrap_or(fallback),
            );
        }
        IconDef::Image(path) => {
            if let Some(bytes) = theme.image_bytes(path) {
                let uri = format!("bytes://vscode-icons/{}", path.display());
                egui::Image::from_bytes(uri, bytes)
                    .fit_to_exact_size(Vec2::splat(SIZE))
                    .paint_at(ui, rect);
            }
        }
    }
}

/// A Lucide icon and a colour per file type.
fn builtin(name: &str, t: &Tokens) -> (&'static str, Color32) {
    let lower = name.to_lowercase();
    let ext = lower.rsplit_once('.').map_or("", |(_, e)| e);
    let rgb = Color32::from_rgb;
    match ext {
        "rs" => ("file-code", rgb(222, 165, 132)),
        "js" | "mjs" | "cjs" => ("file-code", rgb(232, 210, 80)),
        "jsx" | "tsx" => ("file-code", rgb(97, 218, 251)),
        "ts" | "mts" | "cts" => ("file-code", rgb(66, 140, 220)),
        "py" | "pyw" => ("file-code", rgb(80, 150, 210)),
        "go" => ("file-code", rgb(0, 173, 216)),
        "c" | "h" => ("file-code", rgb(130, 160, 200)),
        "cpp" | "cc" | "cxx" | "hpp" | "hh" => ("file-code", rgb(220, 90, 130)),
        "cs" => ("file-code", rgb(150, 110, 210)),
        "java" | "kt" | "kts" => ("file-code", rgb(200, 130, 60)),
        "html" | "htm" | "xml" | "vue" | "svelte" => ("file-code", rgb(228, 100, 60)),
        "css" | "scss" | "sass" | "less" => ("file-code", rgb(80, 160, 230)),
        "json" | "jsonc" => ("file-code", rgb(203, 203, 65)),
        "sh" | "bash" | "zsh" | "ps1" | "bat" | "cmd" => ("file-code", rgb(137, 200, 90)),
        "toml" | "yaml" | "yml" | "ini" | "cfg" | "conf" | "env" => {
            ("settings", rgb(160, 160, 170))
        }
        "md" | "markdown" | "rst" => ("file-text", rgb(80, 154, 186)),
        "txt" | "log" => ("file-text", t.text_faint),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "ico" | "svg" | "webp" | "avif" => {
            ("image", rgb(160, 116, 196))
        }
        _ => ("file", t.text_faint),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_icons_by_extension() {
        let t = Tokens::for_kind(crate::theme::ThemeKind::Dark);
        assert_eq!(builtin("main.rs", &t).0, "file-code");
        assert_eq!(builtin("README.MD", &t).0, "file-text");
        assert_eq!(builtin("Cargo.toml", &t).0, "settings");
        assert_eq!(builtin("logo.png", &t).0, "image");
        assert_eq!(builtin("LICENSE", &t), ("file", t.text_faint));
    }
}
