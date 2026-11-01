//! An open file and its text editor: line numbers, the cursor's line highlighted, syntax
//! colours, Ctrl+S to save (handled by the app).

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use egui::text::{CCursor, LayoutJob};
use egui::{Color32, FontId, Rect, Sense, Ui, pos2, vec2};
use syntect::parsing::SyntaxReference;

use crate::highlight::{self, Highlighter};
use crate::theme::Tokens;

/// Files bigger than this aren't opened for editing.
const MAX_SIZE: u64 = 20 << 20;

pub enum Content {
    Text(String),
    /// Can't be edited here (binary, too big, unreadable): why.
    Unsupported(String),
}

pub struct Document {
    pub path: PathBuf,
    pub name: String,
    pub content: Content,
    /// Hash of the text as it is on disk.
    saved_hash: u64,
    pub dirty: bool,
    /// The file used CRLF line ends; the editor works with LF and saves them back.
    crlf: bool,
    /// When the file was last read or written, to notice changes made elsewhere.
    modified: Option<SystemTime>,
    syntax: Option<&'static SyntaxReference>,
    highlighter: Highlighter,
    /// Cursor line and column (1-based) for the status bar.
    pub cursor: (usize, usize),
    /// Focus the editor on the next frame.
    pub focus: bool,
}

impl Document {
    pub fn open(path: &Path) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let mut doc = Self {
            path: path.to_owned(),
            name,
            content: Content::Unsupported(String::new()),
            saved_hash: 0,
            dirty: false,
            crlf: false,
            modified: None,
            syntax: None,
            highlighter: Highlighter::default(),
            cursor: (1, 1),
            focus: true,
        };
        doc.load();
        doc
    }

    /// (Re)reads the file from disk.
    fn load(&mut self) {
        self.modified = modified(&self.path);
        self.dirty = false;
        let size = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        if size > MAX_SIZE {
            self.content =
                Content::Unsupported(format!("This file is too big to edit ({} MB).", size >> 20));
            return;
        }
        let bytes = match std::fs::read(&self.path) {
            Ok(b) => b,
            Err(e) => {
                self.content = Content::Unsupported(format!("Couldn't read the file: {e}"));
                return;
            }
        };
        let text = match String::from_utf8(bytes) {
            Ok(t) if !t.contains('\0') => t,
            _ => {
                self.content =
                    Content::Unsupported("This is a binary file, or not UTF-8 text.".to_owned());
                return;
            }
        };
        let text = text
            .strip_prefix('\u{feff}')
            .map(str::to_owned)
            .unwrap_or(text);
        self.crlf = text.contains("\r\n");
        let text = if self.crlf {
            text.replace("\r\n", "\n")
        } else {
            text
        };
        self.saved_hash = highlight::hash_of(&text);
        let first_line = text.lines().next().unwrap_or("");
        self.syntax = highlight::syntax_for(&self.name, first_line);
        self.content = Content::Text(text);
    }

    pub fn save(&mut self) -> Result<(), String> {
        let Content::Text(text) = &self.content else {
            return Ok(());
        };
        let out = if self.crlf {
            text.replace('\n', "\r\n")
        } else {
            text.clone()
        };
        std::fs::write(&self.path, out).map_err(|e| format!("Couldn't save {}: {e}", self.name))?;
        self.saved_hash = highlight::hash_of(text);
        self.dirty = false;
        self.modified = modified(&self.path);
        Ok(())
    }

    /// Reloads the file if it changed on disk and has no unsaved edits here.
    pub fn reload_if_changed(&mut self) {
        if !self.dirty && modified(&self.path) != self.modified {
            self.load();
        }
    }

    /// "Rust", "Plain Text", ... for the status bar.
    pub fn language(&self) -> &str {
        self.syntax.map_or("Plain Text", |s| s.name.as_str())
    }

    pub fn line_ending(&self) -> &'static str {
        if self.crlf { "CRLF" } else { "LF" }
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        font_size: f32,
        syntax_theme: Option<&syntect::highlighting::Theme>,
    ) {
        let t = Tokens::get(ui.ctx());
        let text = match &mut self.content {
            Content::Text(text) => text,
            Content::Unsupported(why) => {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new(why.as_str()).color(t.text_faint));
                });
                return;
            }
        };
        // Selected text gets the theme's selection colour, not the accent.
        ui.visuals_mut().selection.bg_fill = t.text_selection;
        ui.visuals_mut().selection.stroke.color = t.text;
        let font = FontId::monospace(font_size);
        let row_height = ui.fonts_mut(|f| f.row_height(&font));
        let lines = text.bytes().filter(|&b| b == b'\n').count() + 1;
        let digits = lines.to_string().len().max(3);
        let char_width = ui.fonts_mut(|f| f.glyph_width(&font, '0'));
        let gutter_w = digits as f32 * char_width + 28.0;

        let syntax = self.syntax;
        let highlighter = &mut self.highlighter;
        let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
            let job: LayoutJob =
                highlighter.layout_job(buf.as_str(), syntax, syntax_theme, font.clone(), t.text);
            ui.fonts_mut(|f| f.layout_job(job))
        };

        let id = ui.make_persistent_id(("editor", &self.path));
        let view_height = ui.available_height();
        // A file opens with the cursor at its start (egui would put it at the end).
        if self.focus {
            let mut state = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
            if state.cursor.char_range().is_none() {
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::one(CCursor::new(0))));
                state.store(ui.ctx(), id);
            }
        }
        let mut changed = false;
        let mut cursor = None;
        egui::ScrollArea::vertical()
            .id_salt(("editor-v", &self.path))
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let (gutter, _) =
                        ui.allocate_exact_size(vec2(gutter_w, row_height), Sense::hover());
                    // Painted under the text: the cursor line's band, filled in once it's known.
                    let band = ui.painter().add(egui::Shape::Noop);
                    let output = egui::ScrollArea::horizontal()
                        .id_salt(("editor-h", &self.path))
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            egui::TextEdit::multiline(text)
                                .id(id)
                                .code_editor()
                                .frame(egui::Frame::NONE)
                                .margin(egui::Margin {
                                    left: 6,
                                    right: 24,
                                    top: 4,
                                    bottom: 4,
                                })
                                .desired_width(f32::INFINITY)
                                .desired_rows(1)
                                .min_size(vec2(0.0, view_height))
                                .layouter(&mut layouter)
                                .show(ui)
                        })
                        .inner;
                    if self.focus {
                        output.response.request_focus();
                        self.focus = false;
                    }
                    changed = output.response.changed();
                    let galley = &output.galley;
                    let origin = output.galley_pos;
                    let cursor_index = output.cursor_range.map(|r| r.primary.index);
                    let cursor_row =
                        cursor_index.map(|i| galley.layout_from_cursor(CCursor::new(i)).row);

                    // Line numbers, beside the rows that start a line.
                    let painter = ui.painter();
                    let clip = ui.clip_rect();
                    let full = output.response.response.rect;
                    let mut line = 1;
                    let mut starts_line = true;
                    for (i, row) in galley.rows.iter().enumerate() {
                        let y = origin.y + row.pos.y;
                        let visible = y + row_height >= clip.top() && y <= clip.bottom();
                        let current = cursor_row == Some(i);
                        if current {
                            painter.set(
                                band,
                                egui::Shape::rect_filled(
                                    Rect::from_min_max(
                                        pos2(gutter.left(), y),
                                        pos2(full.right().max(clip.right()), y + row_height),
                                    ),
                                    0.0,
                                    t.line_highlight,
                                ),
                            );
                        }
                        if starts_line && visible {
                            painter.text(
                                pos2(gutter.right() - 14.0, y + row_height / 2.0),
                                egui::Align2::RIGHT_CENTER,
                                line.to_string(),
                                font.clone(),
                                if current { t.gutter_active } else { t.gutter },
                            );
                        }
                        starts_line = row.ends_with_newline;
                        if row.ends_with_newline {
                            line += 1;
                        }
                    }
                    if let Some(i) = cursor_index {
                        cursor = Some(line_col(galley.text(), i.0));
                    }
                });
            });
        if changed && let Content::Text(text) = &self.content {
            self.dirty = highlight::hash_of(text) != self.saved_hash;
        }
        if let Some(c) = cursor {
            self.cursor = c;
        }
    }
}

/// 1-based line and column of character index `index` in `text`.
fn line_col(text: &str, index: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for c in text.chars().take(index) {
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// A dot drawn where an unsaved tab's close button goes.
pub fn paint_dirty_dot(ui: &Ui, rect: Rect, color: Color32) {
    ui.painter().circle_filled(rect.center(), 4.0, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_and_column() {
        assert_eq!(line_col("ab\ncd", 0), (1, 1));
        assert_eq!(line_col("ab\ncd", 2), (1, 3));
        assert_eq!(line_col("ab\ncd", 3), (2, 1));
        assert_eq!(line_col("ab\ncd", 5), (2, 3));
    }

    #[test]
    fn keeps_crlf_and_tracks_changes() {
        let dir = std::env::temp_dir().join(format!("codecraft-doc-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("a.txt");
        let _ = std::fs::write(&path, "one\r\ntwo\r\n");
        let mut doc = Document::open(&path);
        assert!(matches!(&doc.content, Content::Text(t) if t == "one\ntwo\n"));
        assert_eq!(doc.line_ending(), "CRLF");
        if let Content::Text(t) = &mut doc.content {
            t.push_str("three\n");
        }
        let saved = doc.save();
        let back = std::fs::read_to_string(&path).unwrap_or_default();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(saved.is_ok());
        assert_eq!(back, "one\r\ntwo\r\nthree\r\n");
        assert!(!doc.dirty);
    }
}
