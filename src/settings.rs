//! User settings, stored with the app state.

use egui::{RichText, Ui};
use serde::{Deserialize, Serialize};

use crate::theme::{self, Tokens};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Interface zoom (1.0 = 100 %).
    pub ui_scale: f32,
    /// Editor font size in points.
    pub editor_font: f32,
    /// Colour code and file icons as VS Code does (its colour and file icon themes), when found.
    pub vscode_theme: bool,
    /// Program of the terminal's shell; empty = the platform's default.
    pub shell: String,
    pub terminal_font: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ui_scale: 1.0,
            editor_font: 13.0,
            vscode_theme: true,
            shell: String::new(),
            terminal_font: 13.0,
        }
    }
}

/// What the settings form shows next to the VS Code option.
pub struct VsCodeStatus {
    pub color_theme: Option<String>,
    pub icon_theme: Option<String>,
}

impl Settings {
    /// Applies the settings that live in egui (zoom).
    pub fn set(&self, ctx: &egui::Context) {
        ctx.set_zoom_factor(self.ui_scale.clamp(0.7, 2.0));
    }

    /// The settings form. Returns true when something changed.
    pub fn ui(&mut self, ui: &mut Ui, vscode: &VsCodeStatus) -> bool {
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
                ui.label("VS Code theme");
                ui.vertical(|ui| {
                    ui.checkbox(
                        &mut self.vscode_theme,
                        "Use VS Code's colour theme and file icons",
                    );
                    if self.vscode_theme {
                        let note = |ui: &mut Ui, what: &str, found: &Option<String>| {
                            let text = match found {
                                Some(name) => format!("{what}: {name}"),
                                None => format!("{what}: not found, CodeCraft's own"),
                            };
                            ui.label(RichText::new(text).color(t.text_faint).small());
                        };
                        note(ui, "Colours", &vscode.color_theme);
                        note(ui, "File icons", &vscode.icon_theme);
                    }
                });
                ui.end_row();
            });

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
