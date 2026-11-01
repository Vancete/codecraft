//! User settings, stored with the app state.

use egui::{RichText, Ui};
use serde::{Deserialize, Serialize};

use crate::highlight::{self, Highlighter, SyntaxStyle};
use crate::theme::{self, Tokens};
use crate::vscode;

/// The code each theme is shown with.
const SAMPLE: &str = "/// Opens the server on `port`.
pub fn listen(port: u16) -> Result<Server> {
    let addr = format!(\"0.0.0.0:{port}\");
    let socket = TcpListener::bind(&addr)?;
    log!(\"listening on {}\", addr); // ok
    Ok(Server { socket, open: true })
}";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Interface zoom (1.0 = 100 %).
    pub ui_scale: f32,
    /// Editor font size in points.
    pub editor_font: f32,
    /// The theme code is coloured with.
    pub theme: CodeTheme,
    /// VS Code colour themes imported into the settings.
    pub imported_themes: Vec<vscode::ThemeInfo>,
    /// The extensions the user enabled, by folder name; any other stays off.
    pub enabled_extensions: Vec<String>,
    /// A file opened with a single click takes the preview tab instead of a new one.
    pub preview_tabs: bool,
    /// Program of the terminal's shell; empty = the platform's default.
    pub shell: String,
    pub terminal_font: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ui_scale: 1.0,
            editor_font: 13.0,
            theme: CodeTheme::default(),
            imported_themes: Vec::new(),
            enabled_extensions: Vec::new(),
            preview_tabs: true,
            shell: String::new(),
            terminal_font: 13.0,
        }
    }
}

/// The editor's theme: one of CodeCraft's own, or a VS Code colour theme imported, which also
/// colours the rest of the window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CodeTheme {
    Builtin(SyntaxStyle),
    VsCode(String),
}

impl Default for CodeTheme {
    fn default() -> Self {
        CodeTheme::Builtin(SyntaxStyle::default())
    }
}

impl Settings {
    /// Applies the settings that live in egui (zoom).
    pub fn set(&self, ctx: &egui::Context) {
        ctx.set_zoom_factor(self.ui_scale.clamp(0.7, 2.0));
    }

    /// The settings form. Returns true when something changed.
    /// The imported VS Code theme chosen, if one is.
    pub fn vscode_theme(&self) -> Option<&vscode::ThemeInfo> {
        match &self.theme {
            CodeTheme::VsCode(name) => self.imported_themes.iter().find(|t| t.name == *name),
            CodeTheme::Builtin(_) => None,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) -> bool {
        let t = Tokens::get(ui.ctx());
        let before = self.clone();
        let section = |ui: &mut Ui, title: &str| {
            ui.add_space(10.0);
            ui.label(
                RichText::new(title)
                    .font(theme::semibold(10.5))
                    .color(t.text_faint),
            );
            ui.add_space(2.0);
        };

        section(ui, "INTERFACE");
        egui::Grid::new("settings-ui")
            .num_columns(2)
            .spacing([16.0, 8.0])
            .show(ui, |ui| {
                ui.label("Zoom");
                ui.add(
                    egui::Slider::new(&mut self.ui_scale, 0.7..=2.0)
                        .trailing_fill(true)
                        .step_by(0.05)
                        .custom_formatter(|v, _| format!("{:.0} %", v * 100.0)),
                );
                ui.end_row();
            });

        section(ui, "THEMES");
        egui::Grid::new("settings-themes")
            .num_columns(2)
            .spacing([12.0, 12.0])
            .show(ui, |ui| {
                for (i, style) in SyntaxStyle::ALL.into_iter().enumerate() {
                    let chosen = self.theme == CodeTheme::Builtin(style);
                    let card = Card {
                        label: style.label(),
                        theme: style.theme(t.dark()),
                        background: t.editor,
                        text: t.text,
                    };
                    if card.ui(ui, chosen, false).0.clicked() {
                        self.theme = CodeTheme::Builtin(style);
                    }
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
        ui.add_space(12.0);
        self.import_button(ui);

        if !self.imported_themes.is_empty() {
            section(ui, "VS CODE THEMES");
            let mut remove = None;
            egui::Grid::new("settings-vscode-themes")
                .num_columns(2)
                .spacing([12.0, 12.0])
                .show(ui, |ui| {
                    for (i, info) in self.imported_themes.iter().enumerate() {
                        let name = &info.name;
                        let chosen = self.theme == CodeTheme::VsCode(name.clone());
                        let (resp, removed) = match vscode::color_theme(info) {
                            Some(vs) => Card {
                                label: name,
                                theme: &vs.syntax,
                                background: vs.color("editor.background").unwrap_or(t.editor),
                                text: vs.color("editor.foreground").unwrap_or(t.text),
                            }
                            .ui(ui, chosen, true),
                            None => missing_card(ui, name),
                        };
                        if resp.clicked() {
                            self.theme = CodeTheme::VsCode(name.clone());
                        }
                        if removed {
                            remove = Some(i);
                        }
                        if i % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
            if let Some(i) = remove {
                let info = self.imported_themes.remove(i);
                if self.theme == CodeTheme::VsCode(info.name) {
                    self.theme = CodeTheme::default();
                }
            }
        }

        section(ui, "EDITOR");
        egui::Grid::new("settings-editor")
            .num_columns(2)
            .spacing([16.0, 8.0])
            .show(ui, |ui| {
                ui.label("Font size");
                ui.add(
                    egui::Slider::new(&mut self.editor_font, 9.0..=24.0)
                        .step_by(0.5)
                        .trailing_fill(true),
                );
                ui.end_row();
                ui.label("Preview tabs");
                ui.checkbox(
                    &mut self.preview_tabs,
                    "A single click opens the file in the preview tab (in italics)",
                );
                ui.end_row();
            });

        section(ui, "TERMINAL");
        egui::Grid::new("settings-term")
            .num_columns(2)
            .spacing([16.0, 8.0])
            .show(ui, |ui| {
                ui.label("Shell");
                let shells = crate::terminal::available_shells();
                let current = shells
                    .iter()
                    .find(|s| s.program == self.shell)
                    .map(|s| s.label.clone())
                    .unwrap_or_else(|| "System default".to_owned());
                egui::ComboBox::from_id_salt("settings-shell")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.shell, String::new(), "System default");
                        for s in &shells {
                            ui.selectable_value(&mut self.shell, s.program.clone(), &s.label);
                        }
                    });
                ui.end_row();
                ui.label("Font size");
                ui.add(
                    egui::Slider::new(&mut self.terminal_font, 9.0..=24.0)
                        .step_by(0.5)
                        .trailing_fill(true),
                );
                ui.end_row();
            });
        *self != before
    }
}

impl Settings {
    /// "Import VS Code themes": a list of the colour themes installed in VS Code; picking one
    /// adds it to the VS Code themes.
    fn import_button(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        let resp = ui.button("Import VS Code themes…");
        egui::Popup::menu(&resp)
            .style(theme::menu_style)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                let installed = vscode::color_themes();
                if installed.is_empty() {
                    ui.label(RichText::new("No VS Code themes found").color(t.text_faint));
                    return;
                }
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        ui.set_width(280.0);
                        for info in installed {
                            let imported = self.imported_themes.iter().any(|t| t.name == info.name);
                            let kind = if info.dark { "dark" } else { "light" };
                            let label = ui.add_enabled(
                                !imported,
                                egui::Button::new(&info.name)
                                    .right_text(RichText::new(kind).color(t.text_faint).small())
                                    .frame(false)
                                    .min_size(egui::vec2(280.0, 0.0)),
                            );
                            if label.clicked() {
                                self.imported_themes.push(info);
                            }
                        }
                    });
            });
    }
}

/// A theme's preview in the settings: its name over the sample code in its colours.
struct Card<'a> {
    label: &'a str,
    theme: &'a syntect::highlighting::Theme,
    background: egui::Color32,
    text: egui::Color32,
}

const CARD_W: f32 = 312.0;

impl Card<'_> {
    /// Draws it, the chosen one outlined; with `removable`, a button to remove it shows on
    /// hover. Returns its response and whether it was removed.
    fn ui(&self, ui: &mut Ui, chosen: bool, removable: bool) -> (egui::Response, bool) {
        let t = Tokens::get(ui.ctx());
        let job = Highlighter::default().layout_job(
            SAMPLE,
            highlight::syntax_for("sample.rs", ""),
            Some(self.theme),
            egui::FontId::monospace(11.0),
            self.text,
        );
        let galley = ui.painter().layout_job(job);
        let size = egui::vec2(CARD_W, galley.size().y + 42.0);
        let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
        card_frame(ui, rect, &resp, chosen, self.background);
        let p = ui.painter();
        let label = if chosen {
            self.text
        } else {
            self.text.gamma_multiply(0.75)
        };
        p.with_clip_rect(rect.shrink(2.0)).text(
            rect.left_top() + egui::vec2(12.0, 10.0),
            egui::Align2::LEFT_TOP,
            self.label,
            theme::semibold(12.0),
            label,
        );
        p.with_clip_rect(rect.shrink(2.0)).galley(
            rect.left_top() + egui::vec2(12.0, 32.0),
            galley,
            self.text,
        );
        let mut removed = false;
        if removable && ui.rect_contains_pointer(rect) {
            let close = egui::Rect::from_center_size(
                rect.right_top() + egui::vec2(-16.0, 16.0),
                egui::vec2(20.0, 20.0),
            );
            let x = ui.interact(close, resp.id.with("remove"), egui::Sense::click());
            let color = if x.hovered() { self.text } else { t.text_faint };
            crate::icons::paint(ui, close, "x", 14.0, color);
            removed = x.on_hover_text("Remove").clicked();
        }
        (resp, removed)
    }
}

/// An imported theme VS Code no longer has: a card saying so, to remove it.
fn missing_card(ui: &mut Ui, name: &str) -> (egui::Response, bool) {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(CARD_W, 60.0), egui::Sense::hover());
    card_frame(ui, rect, &resp, false, t.editor);
    ui.painter().text(
        rect.left_top() + egui::vec2(12.0, 10.0),
        egui::Align2::LEFT_TOP,
        format!("{name}\nnot found in VS Code"),
        egui::FontId::proportional(12.0),
        t.text_faint,
    );
    let close = egui::Rect::from_center_size(
        rect.right_top() + egui::vec2(-16.0, 16.0),
        egui::vec2(20.0, 20.0),
    );
    let x = ui.interact(close, resp.id.with("remove"), egui::Sense::click());
    crate::icons::paint(ui, close, "x", 14.0, t.text_faint);
    let removed = x.on_hover_text("Remove").clicked();
    (resp, removed)
}

fn card_frame(ui: &Ui, rect: egui::Rect, resp: &egui::Response, chosen: bool, fill: egui::Color32) {
    let t = Tokens::get(ui.ctx());
    let stroke = if chosen {
        egui::Stroke::new(2.0, t.accent)
    } else if resp.hovered() {
        egui::Stroke::new(1.0, t.text_faint)
    } else {
        egui::Stroke::new(1.0, t.field_border)
    };
    ui.painter()
        .rect(rect, t.radius, fill, stroke, egui::StrokeKind::Inside);
}
