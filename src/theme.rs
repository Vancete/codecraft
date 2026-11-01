//! Design tokens, shared with gitcraft (modelled on PhotoCraft's "Pro" themes): flat grey
//! panels, a darker tab strip, a blue accent, Inter for the UI and JetBrains Mono for code.
//!
//! Widgets read [`Tokens::get`] instead of hard-coding colours, so every theme applies
//! everywhere. With the VS Code setting on, [`Tokens::with_vscode`] repaints them in the
//! workbench colours of VS Code's theme.

use std::sync::Arc;

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
    Visuals,
};
use serde::{Deserialize, Serialize};

use crate::vscode::theme::EditorTheme;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeKind {
    /// Charcoal panels (PhotoCraft's "Pro (Dark)").
    #[default]
    Dark,
    /// Mid-grey panels (PhotoCraft's default "Pro (Medium Gray)").
    MediumGray,
    /// Light panels (PhotoCraft's "Pro (Light)").
    Light,
}

impl ThemeKind {
    pub const ALL: [ThemeKind; 3] = [ThemeKind::Dark, ThemeKind::MediumGray, ThemeKind::Light];

    pub fn label(self) -> &'static str {
        match self {
            ThemeKind::Dark => "Dark",
            ThemeKind::MediumGray => "Medium Gray",
            ThemeKind::Light => "Light",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub kind: ThemeKind,
    /// Toolbars and the active tab.
    pub chrome: Color32,
    /// Behind panels (the start page).
    pub dock: Color32,
    /// Dialogs and menus.
    pub card: Color32,
    /// Inputs.
    pub field: Color32,
    pub field_border: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub icon: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub separator: Color32,
    pub shadow: Color32,
    pub danger: Color32,
    pub warning: Color32,
    pub success: Color32,
    /// Window tab strip and panel headers.
    pub tab_strip: Color32,
    pub row_selected: Color32,
    /// The active editor tab.
    pub tab_active: Color32,
    /// The editor's background.
    pub editor: Color32,
    /// The side panel (explorer).
    pub sidebar: Color32,
    pub activity_bar: Color32,
    pub status_bar: Color32,
    /// Line numbers, and the cursor's line number.
    pub gutter: Color32,
    pub gutter_active: Color32,
    /// Behind the cursor's line.
    pub line_highlight: Color32,
    /// Behind selected text.
    pub text_selection: Color32,
    /// Matches of the find bar's query, and the one it's on.
    pub find_match: Color32,
    pub find_match_current: Color32,
    pub radius_sm: f32,
    pub radius: f32,
    pub radius_lg: f32,
}

impl Tokens {
    pub fn for_kind(kind: ThemeKind) -> Self {
        match kind {
            ThemeKind::Dark => Tokens {
                kind,
                chrome: Color32::from_rgb(50, 50, 50),
                dock: Color32::from_rgb(40, 40, 40),
                card: Color32::from_rgb(50, 50, 50),
                field: Color32::from_rgb(36, 36, 36),
                field_border: Color32::from_rgb(74, 74, 74),
                hover: Color32::from_rgb(66, 66, 66),
                pressed: Color32::from_rgb(78, 78, 78),
                text: Color32::from_rgb(222, 222, 222),
                text_dim: Color32::from_rgb(178, 178, 178),
                text_faint: Color32::from_rgb(128, 128, 128),
                icon: Color32::from_rgb(200, 200, 200),
                accent: Color32::from_rgb(55, 142, 240),
                accent_text: Color32::WHITE,
                separator: Color32::from_rgb(30, 30, 30),
                shadow: Color32::from_black_alpha(150),
                danger: Color32::from_rgb(236, 91, 98),
                warning: Color32::from_rgb(232, 176, 70),
                success: Color32::from_rgb(92, 196, 122),
                tab_strip: Color32::from_rgb(38, 38, 38),
                row_selected: Color32::from_rgb(38, 76, 122),
                tab_active: Color32::from_rgb(40, 40, 40),
                editor: Color32::from_rgb(40, 40, 40),
                sidebar: Color32::from_rgb(50, 50, 50),
                activity_bar: Color32::from_rgb(44, 44, 44),
                status_bar: Color32::from_rgb(38, 38, 38),
                gutter: Color32::from_rgb(110, 110, 110),
                gutter_active: Color32::from_rgb(200, 200, 200),
                line_highlight: Color32::from_rgb(46, 46, 46),
                text_selection: Color32::from_rgb(38, 79, 120),
                find_match: over(Color32::from_rgb(40, 40, 40), FIND_HIGHLIGHT),
                find_match_current: Color32::from_rgb(158, 106, 3),
                radius_sm: 3.0,
                radius: 4.0,
                radius_lg: 6.0,
            },
            ThemeKind::MediumGray => Tokens {
                kind,
                chrome: Color32::from_rgb(83, 83, 83),
                dock: Color32::from_rgb(66, 66, 66),
                card: Color32::from_rgb(83, 83, 83),
                field: Color32::from_rgb(69, 69, 69),
                field_border: Color32::from_rgb(104, 104, 104),
                hover: Color32::from_rgb(98, 98, 98),
                pressed: Color32::from_rgb(112, 112, 112),
                text: Color32::from_rgb(238, 238, 238),
                text_dim: Color32::from_rgb(212, 212, 212),
                text_faint: Color32::from_rgb(160, 160, 160),
                icon: Color32::from_rgb(226, 226, 226),
                separator: Color32::from_rgb(62, 62, 62),
                tab_strip: Color32::from_rgb(74, 74, 74),
                row_selected: Color32::from_rgb(52, 98, 150),
                tab_active: Color32::from_rgb(66, 66, 66),
                editor: Color32::from_rgb(66, 66, 66),
                sidebar: Color32::from_rgb(83, 83, 83),
                activity_bar: Color32::from_rgb(76, 76, 76),
                status_bar: Color32::from_rgb(74, 74, 74),
                gutter: Color32::from_rgb(150, 150, 150),
                gutter_active: Color32::from_rgb(230, 230, 230),
                line_highlight: Color32::from_rgb(72, 72, 72),
                text_selection: Color32::from_rgb(52, 98, 150),
                find_match: over(Color32::from_rgb(66, 66, 66), FIND_HIGHLIGHT),
                ..Tokens::for_kind(ThemeKind::Dark)
            },
            ThemeKind::Light => Tokens {
                kind,
                chrome: Color32::from_rgb(240, 240, 240),
                dock: Color32::from_rgb(250, 250, 250),
                card: Color32::from_rgb(240, 240, 240),
                field: Color32::WHITE,
                field_border: Color32::from_rgb(186, 186, 186),
                hover: Color32::from_rgb(226, 226, 226),
                pressed: Color32::from_rgb(206, 206, 206),
                text: Color32::from_rgb(20, 20, 20),
                text_dim: Color32::from_rgb(64, 64, 64),
                text_faint: Color32::from_rgb(120, 120, 120),
                icon: Color32::from_rgb(46, 46, 46),
                accent: Color32::from_rgb(16, 110, 220),
                accent_text: Color32::WHITE,
                separator: Color32::from_rgb(214, 214, 214),
                shadow: Color32::from_black_alpha(50),
                danger: Color32::from_rgb(200, 50, 60),
                warning: Color32::from_rgb(180, 120, 10),
                success: Color32::from_rgb(30, 140, 70),
                tab_strip: Color32::from_rgb(226, 226, 226),
                row_selected: Color32::from_rgb(198, 220, 246),
                tab_active: Color32::from_rgb(252, 252, 252),
                editor: Color32::from_rgb(252, 252, 252),
                sidebar: Color32::from_rgb(240, 240, 240),
                activity_bar: Color32::from_rgb(232, 232, 232),
                status_bar: Color32::from_rgb(226, 226, 226),
                gutter: Color32::from_rgb(150, 150, 150),
                gutter_active: Color32::from_rgb(40, 40, 40),
                line_highlight: Color32::from_rgb(242, 242, 242),
                text_selection: Color32::from_rgb(190, 214, 245),
                find_match: over(Color32::from_rgb(252, 252, 252), FIND_HIGHLIGHT),
                find_match_current: Color32::from_rgb(168, 172, 148),
                ..Tokens::for_kind(ThemeKind::Dark)
            },
        }
    }

    /// The tokens recoloured with a VS Code theme's workbench colours. The base is gitcraft's
    /// dark or light palette, whichever matches the theme, for anything it doesn't set.
    pub fn with_vscode(vs: &EditorTheme) -> Self {
        let base = Tokens::for_kind(if vs.dark {
            ThemeKind::Dark
        } else {
            ThemeKind::Light
        });
        let editor = vs
            .color("editor.background")
            .unwrap_or(base.editor)
            .to_opaque();
        // Translucent colours are laid over the editor's background.
        let get = |keys: &[&str]| keys.iter().find_map(|k| vs.color(k));
        let solid =
            |keys: &[&str], fallback: Color32| get(keys).map_or(fallback, |c| over(editor, c));
        let text = solid(&["editor.foreground", "foreground"], base.text);
        let sidebar = solid(&["sideBar.background"], editor);
        let tab_strip = solid(&["editorGroupHeader.tabsBackground"], sidebar);
        // The colour VS Code marks the active tab or view with.
        let accent = solid(
            &[
                "tab.activeBorderTop",
                "tab.activeBorder",
                "activityBar.activeBorder",
                "activityBarBadge.background",
                "focusBorder",
            ],
            base.accent,
        );
        // Dark text on a bright accent, white on a dark one.
        let [r, g, b, _] = accent.to_srgba_unmultiplied();
        let luma = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
        let accent_text = if luma > 150.0 {
            Color32::from_rgb(30, 30, 30)
        } else {
            Color32::WHITE
        };
        let mix = |a: Color32, b: Color32, t: f32| a.lerp_to_gamma(b, t);
        Tokens {
            chrome: sidebar,
            tab_active: solid(&["tab.activeBackground"], editor),
            dock: editor,
            card: solid(&["editorWidget.background", "menu.background"], sidebar),
            field: solid(&["input.background"], mix(editor, Color32::BLACK, 0.1)),
            field_border: solid(
                &["input.border", "dropdown.border", "widget.border"],
                mix(editor, text, 0.25),
            ),
            hover: solid(&["list.hoverBackground"], mix(sidebar, text, 0.08)),
            pressed: mix(sidebar, text, 0.16),
            text,
            text_dim: solid(&["sideBar.foreground"], mix(text, editor, 0.25)),
            text_faint: solid(&["descriptionForeground"], mix(text, editor, 0.45)),
            icon: solid(&["icon.foreground"], mix(text, editor, 0.15)),
            accent,
            accent_text,
            separator: solid(
                &["sideBar.border", "editorGroup.border", "panel.border"],
                mix(editor, Color32::BLACK, 0.25),
            ),
            tab_strip,
            row_selected: solid(
                &[
                    "list.activeSelectionBackground",
                    "list.inactiveSelectionBackground",
                ],
                base.row_selected,
            ),
            editor,
            sidebar,
            activity_bar: solid(&["activityBar.background"], sidebar),
            status_bar: solid(&["statusBar.background"], tab_strip),
            gutter: solid(&["editorLineNumber.foreground"], mix(text, editor, 0.55)),
            gutter_active: solid(&["editorLineNumber.activeForeground"], text),
            line_highlight: solid(&["editor.lineHighlightBackground"], mix(editor, text, 0.04)),
            text_selection: solid(&["editor.selectionBackground"], base.text_selection),
            find_match: solid(
                &["editor.findMatchHighlightBackground"],
                over(editor, FIND_HIGHLIGHT),
            ),
            find_match_current: solid(&["editor.findMatchBackground"], base.find_match_current),
            ..base
        }
    }

    pub fn dark(&self) -> bool {
        self.kind != ThemeKind::Light
    }

    pub fn get(ctx: &egui::Context) -> Tokens {
        ctx.data(|d| d.get_temp::<Tokens>(tokens_id()))
            .unwrap_or_else(|| Tokens::for_kind(ThemeKind::default()))
    }
}

/// VS Code's colour for the find bar's other matches, laid over the editor.
const FIND_HIGHLIGHT: Color32 = Color32::from_rgba_unmultiplied_const(234, 92, 0, 85);

/// `top` painted over `bottom`, opaque.
fn over(bottom: Color32, top: Color32) -> Color32 {
    let [r, g, b, a] = top.to_srgba_unmultiplied();
    let solid = Color32::from_rgb(r, g, b);
    bottom.lerp_to_gamma(solid, a as f32 / 255.0)
}

fn tokens_id() -> egui::Id {
    egui::Id::new("codecraft-theme")
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}

/// Inter and JetBrains Mono first; egui's default fonts stay last for symbols Inter lacks.
/// `extra`: more fonts, each its own family (the glyphs of VS Code's icon theme).
pub fn install_fonts(ctx: &egui::Context, extra: &[(String, Vec<u8>)]) {
    let mut fonts = FontDefinitions::default();
    let mut add = |name: &str, bytes: &'static [u8]| {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    add("Inter", include_bytes!("../assets/fonts/Inter-Regular.ttf"));
    add(
        "Inter-Medium",
        include_bytes!("../assets/fonts/Inter-Medium.ttf"),
    );
    add(
        "Inter-SemiBold",
        include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
    );
    add(
        "JetBrainsMono",
        include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
    );
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.insert(0, "Inter".to_owned());
    let fallback = proportional.clone();
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "JetBrainsMono".to_owned());
    for (family, primary) in [("medium", "Inter-Medium"), ("semibold", "Inter-SemiBold")] {
        let mut stack = vec![primary.to_owned()];
        stack.extend(fallback.iter().cloned());
        fonts
            .families
            .insert(FontFamily::Name(family.into()), stack);
    }
    for (family, bytes) in extra {
        fonts.font_data.insert(
            family.clone(),
            Arc::new(FontData::from_owned(bytes.clone())),
        );
        fonts.families.insert(
            FontFamily::Name(family.as_str().into()),
            vec![family.clone()],
        );
    }
    ctx.set_fonts(fonts);
}

/// Menus and context menus: no border, rows highlighted from edge to edge with room for the
/// text, a margin only above and below (where the menu's corners are rounded).
pub fn menu_style(s: &mut egui::Style) {
    s.spacing.menu_margin = egui::Margin::symmetric(0, 6);
    s.spacing.button_padding = egui::vec2(16.0, 7.0);
    s.spacing.item_spacing.y = 0.0;
    s.visuals.window_stroke = Stroke::NONE;
    // A shade darker than cards on dark themes, with lighter separators.
    let fill = s.visuals.window_fill;
    let (fill, line) = if s.visuals.dark_mode {
        let fill = fill.lerp_to_gamma(Color32::BLACK, 0.3);
        (fill, fill.lerp_to_gamma(Color32::WHITE, 0.2))
    } else {
        (fill, fill.lerp_to_gamma(Color32::BLACK, 0.15))
    };
    s.visuals.window_fill = fill;
    s.visuals.popup_shadow.color = s.visuals.popup_shadow.color.gamma_multiply(0.6);
    s.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, line);
    let w = &mut s.visuals.widgets;
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    for wv in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        wv.bg_stroke = Stroke::NONE;
        wv.corner_radius = CornerRadius::ZERO;
    }
}

/// Makes `t` the theme of the whole UI.
pub fn apply(ctx: &egui::Context, t: Tokens) {
    ctx.data_mut(|d| d.insert_temp(tokens_id(), t));
    let mut v = if t.dark() {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.panel_fill = t.chrome;
    v.window_fill = t.card;
    v.window_stroke = Stroke::new(1.0, t.field_border);
    v.extreme_bg_color = t.field;
    v.faint_bg_color = t.card;
    v.code_bg_color = t.field;
    v.override_text_color = Some(t.text);
    v.hyperlink_color = t.accent;
    v.warn_fg_color = t.warning;
    v.error_fg_color = t.danger;
    v.window_corner_radius = CornerRadius::same(t.radius_lg as u8);
    v.menu_corner_radius = CornerRadius::same(t.radius_lg as u8);
    v.window_shadow = egui::Shadow {
        offset: [0, 10],
        blur: 32,
        spread: 0,
        color: t.shadow,
    };
    v.popup_shadow = egui::Shadow {
        offset: [0, 6],
        blur: 20,
        spread: 0,
        color: t.shadow,
    };
    v.selection.bg_fill = t.accent;
    v.selection.stroke = Stroke::new(1.0, t.accent_text);
    v.text_cursor.stroke = Stroke::new(2.0, t.accent);
    v.striped = false;
    let r = CornerRadius::same(t.radius_sm as u8);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = t.card;
    w.noninteractive.weak_bg_fill = t.card;
    w.noninteractive.bg_stroke = Stroke::new(1.0, t.separator);
    w.noninteractive.fg_stroke = Stroke::new(1.0, t.text_dim);
    w.noninteractive.corner_radius = r;
    for (wv, bg, stroke) in [
        (&mut w.inactive, t.field, t.field_border),
        (&mut w.hovered, t.hover, t.field_border),
        (&mut w.active, t.pressed, t.accent),
        (&mut w.open, t.hover, t.field_border),
    ] {
        wv.bg_fill = bg;
        wv.weak_bg_fill = bg;
        wv.bg_stroke = Stroke::new(1.0, stroke);
        wv.fg_stroke = Stroke::new(1.0, t.text);
        wv.corner_radius = r;
        wv.expansion = 0.0;
    }
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.text_styles = [
            (TextStyle::Small, FontId::proportional(10.5)),
            (TextStyle::Body, FontId::proportional(12.5)),
            (TextStyle::Button, FontId::proportional(12.5)),
            (TextStyle::Heading, semibold(15.0)),
            (TextStyle::Monospace, FontId::monospace(12.0)),
        ]
        .into();
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
        s.spacing.interact_size = egui::vec2(24.0, 24.0);
        // Tooltips and other popups (menus set their own, see `menu_style`).
        s.spacing.menu_margin = egui::Margin::symmetric(12, 7);
        s.spacing.window_margin = egui::Margin::same(16);
        s.spacing.icon_width = 14.0;
        s.visuals.indent_has_left_vline = false;
        let mut scroll = egui::style::ScrollStyle::thin();
        scroll.interact_handle_opacity = 1.0;
        scroll.interact_background_opacity = 0.4;
        scroll.floating_width = 4.0;
        s.spacing.scroll = scroll;
        s.spacing.tooltip_width = 320.0;
    });
    // egui picks its style branch from the OS appearance; both branches get our palette.
    let style = ctx.global_style();
    ctx.set_style_of(egui::Theme::Light, Arc::clone(&style));
    ctx.set_style_of(egui::Theme::Dark, style);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translucent_colours_land_on_the_background() {
        let bg = Color32::from_rgb(0, 0, 0);
        assert_eq!(
            over(bg, Color32::from_rgb(10, 20, 30)),
            Color32::from_rgb(10, 20, 30)
        );
        let half = Color32::from_rgba_unmultiplied(255, 255, 255, 128);
        let c = over(bg, half);
        assert!(c.r() > 100 && c.r() < 160 && c.a() == 255);
    }
}
