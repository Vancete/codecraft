//! An open file and its text editor: line numbers, the cursor's line highlighted, syntax
//! colours, the find bar's matches, Ctrl+S to save (handled by the app).

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use egui::text::{CCursor, CCursorRange, LayoutJob};
use egui::{Color32, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};
use syntect::parsing::SyntaxReference;

use crate::find::{self, FindAction, FindBar, Query};
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
    /// The selection, in characters (empty: just the cursor, at its start).
    selection: Range<usize>,
    /// Select these characters and scroll to them on the next frame.
    select: Option<Range<usize>>,
    /// Bumped whenever the text changes, so the find bar's matches know they're stale.
    version: u64,
    found: Found,
}

/// The find bar's matches in this file.
#[derive(Default)]
struct Found {
    /// What they were found for: the text's version and the query.
    key: Option<(u64, Query)>,
    /// Byte ranges.
    matches: Vec<Range<usize>>,
    error: Option<String>,
    /// The match the cursor is on.
    current: Option<usize>,
    /// After a replacement: go to the first match from this byte.
    jump_from: Option<usize>,
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
            selection: 0..0,
            select: None,
            version: 0,
            found: Found::default(),
        };
        doc.load();
        doc
    }

    /// (Re)reads the file from disk.
    fn load(&mut self) {
        self.version += 1;
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

    /// The selected text, when it's on one line (to seed the find fields with).
    pub fn selected_text(&self) -> Option<String> {
        let Content::Text(text) = &self.content else {
            return None;
        };
        let selected: String = text
            .chars()
            .skip(self.selection.start)
            .take(self.selection.len())
            .collect();
        (!selected.is_empty() && !selected.contains('\n')).then_some(selected)
    }

    /// Selects columns `cols` (in characters) of line `line` (0-based) and scrolls to them.
    pub fn select_in_line(&mut self, line: usize, cols: Range<usize>) {
        let Content::Text(text) = &self.content else {
            return;
        };
        let start: usize = text
            .split('\n')
            .take(line)
            .map(|l| l.chars().count() + 1)
            .sum();
        self.select = Some(start + cols.start..start + cols.end);
        self.focus = true;
    }

    /// Replaces every match of `re` (from the search view); how many there were.
    pub fn replace_matches(&mut self, re: &regex::Regex, query: &Query, with: &str) -> usize {
        let Content::Text(text) = &mut self.content else {
            return 0;
        };
        let (new, n) = find::replace_all(re, query, text, with);
        if n > 0 {
            *text = new;
            self.edited();
        }
        n
    }

    /// The text was changed here (not typed): its matches and the dirty flag follow.
    fn edited(&mut self) {
        self.version += 1;
        if let Content::Text(text) = &self.content {
            self.dirty = highlight::hash_of(text) != self.saved_hash;
        }
    }

    /// Finds `query`'s matches again if the text or the query changed. A new query moves to its
    /// first match from the cursor, as VS Code does while you type.
    fn update_matches(&mut self, query: &Query) {
        if self
            .found
            .key
            .as_ref()
            .is_some_and(|(v, q)| *v == self.version && q == query)
        {
            return;
        }
        let new_query = self.found.key.as_ref().is_none_or(|(_, q)| q != query);
        self.found.key = Some((self.version, query.clone()));
        self.found.current = None;
        self.found.matches.clear();
        self.found.error = None;
        let Content::Text(text) = &self.content else {
            return;
        };
        match query.compile() {
            None => {}
            Some(Err(e)) => self.found.error = Some(e),
            Some(Ok(re)) => self.found.matches = find::find_all(&re, text),
        }
        let from = match self.found.jump_from.take() {
            Some(byte) => Some(byte),
            None if new_query => Some(char_to_byte(text, self.selection.start)),
            None => None,
        };
        if let Some(from) = from
            && !self.found.matches.is_empty()
        {
            let i = self
                .found
                .matches
                .iter()
                .position(|r| r.start >= from)
                .unwrap_or(0);
            self.go_to_match(i);
        }
    }

    fn go_to_match(&mut self, i: usize) {
        let (Content::Text(text), Some(range)) = (&self.content, self.found.matches.get(i)) else {
            return;
        };
        let start = byte_to_char(text, range.start);
        let len = text.get(range.clone()).map_or(0, |m| m.chars().count());
        self.found.current = Some(i);
        self.select = Some(start..start + len);
    }

    /// Moves to the next (or previous) match.
    pub fn find_step(&mut self, query: &Query, forward: bool) {
        self.update_matches(query);
        let n = self.found.matches.len();
        let Content::Text(text) = &self.content else {
            return;
        };
        if n == 0 {
            return;
        }
        let i = match self.found.current {
            Some(c) if forward => (c + 1) % n,
            Some(c) => (c + n - 1) % n,
            None => {
                let at = char_to_byte(text, self.selection.start);
                let matches = &self.found.matches;
                if forward {
                    matches.iter().position(|r| r.start >= at).unwrap_or(0)
                } else {
                    matches.iter().rposition(|r| r.start < at).unwrap_or(n - 1)
                }
            }
        };
        self.go_to_match(i);
    }

    /// Replaces the match the cursor is on and moves to the next; with none, finds the next.
    fn replace_one(&mut self, query: &Query, with: &str) {
        self.update_matches(query);
        let current = self
            .found
            .current
            .and_then(|c| self.found.matches.get(c))
            .cloned();
        let (Some(range), Some(Ok(re)), Content::Text(text)) =
            (current, query.compile(), &mut self.content)
        else {
            return self.find_step(query, true);
        };
        let new = find::replacement(&re, query, text, range.clone(), with);
        text.replace_range(range.clone(), &new);
        self.found.jump_from = Some(range.start + new.len());
        self.edited();
    }

    /// "3 of 12", "? of 12" or "No results", for the find bar.
    fn find_status(&self) -> String {
        let n = self.found.matches.len();
        let more = if n >= find::MAX_MATCHES { "+" } else { "" };
        match (n, self.found.current) {
            (0, _) => "No results".to_owned(),
            (_, Some(c)) => format!("{} of {n}{more}", c + 1),
            (_, None) => format!("? of {n}{more}"),
        }
    }

    /// Carries out what the find bar asked for; a note for the status bar.
    fn find_action(&mut self, action: FindAction, bar: &mut FindBar) -> Option<String> {
        match action {
            FindAction::Next => self.find_step(&bar.query, true),
            FindAction::Previous => self.find_step(&bar.query, false),
            FindAction::ReplaceOne => self.replace_one(&bar.query, &bar.replace),
            FindAction::ReplaceAll => {
                let n = match bar.query.compile() {
                    Some(Ok(re)) => self.replace_matches(&re, &bar.query, &bar.replace),
                    _ => 0,
                };
                return Some(match n {
                    1 => "Replaced 1 occurrence".to_owned(),
                    n => format!("Replaced {n} occurrences"),
                });
            }
            FindAction::Close => {
                bar.open = false;
                self.focus = true;
            }
        }
        None
    }

    /// Draws the editor (and the find bar, when open). Returns a note for the status bar.
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        font_size: f32,
        syntax_theme: Option<&syntect::highlighting::Theme>,
        bar: &mut FindBar,
    ) -> Option<String> {
        let t = Tokens::get(ui.ctx());
        let editor_rect = ui.max_rect();
        if bar.open {
            self.update_matches(&bar.query);
        }
        let text = match &mut self.content {
            Content::Text(text) => text,
            Content::Unsupported(why) => {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new(why.as_str()).color(t.text_faint));
                });
                return None;
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
        let marks = bar
            .open
            .then_some((self.found.matches.as_slice(), self.found.current));
        // The match the find bar is on gets a border too, as some themes colour it like the rest.
        let current = marks.and_then(|(m, c)| c.and_then(|c| m.get(c))).cloned();
        let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
            let mut job: LayoutJob =
                highlighter.layout_job(buf.as_str(), syntax, syntax_theme, font.clone(), t.text);
            if let Some((matches, current)) = marks {
                mark_matches(
                    &mut job,
                    matches,
                    current,
                    t.find_match,
                    t.find_match_current,
                );
            }
            ui.fonts_mut(|f| f.layout_job(job))
        };

        let id = ui.make_persistent_id(("editor", &self.path));
        let view_height = ui.available_height();
        // A match or search result to select and bring into view.
        let reveal = self.select.take().map(|r| {
            let mut state = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::two(
                CCursor::new(r.start),
                CCursor::new(r.end),
            )));
            state.store(ui.ctx(), id);
            r.end
        });
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
        let mut selection = None;
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
                    if let Some(r) = output.cursor_range {
                        let (a, b) = (r.primary.index.0, r.secondary.index.0);
                        selection = Some(a.min(b)..a.max(b));
                    }
                    if let Some(i) = reveal {
                        let at = galley
                            .pos_from_cursor(CCursor::new(i))
                            .translate(origin.to_vec2());
                        ui.scroll_to_rect(at.expand2(vec2(60.0, 0.0)), Some(egui::Align::Center));
                    }
                    if let Some(r) = &current
                        && let Some(text) = galley.text().get(..r.end)
                        && text.is_char_boundary(r.start)
                    {
                        let end = text.chars().count();
                        let start = text.get(..r.start).map_or(end, |s| s.chars().count());
                        let a = galley.pos_from_cursor(CCursor::new(start));
                        let b = galley.pos_from_cursor(CCursor::new(end));
                        if a.top() == b.top() {
                            ui.painter().rect_stroke(
                                a.union(b).translate(origin.to_vec2()),
                                2.0,
                                Stroke::new(1.0, t.text_faint),
                                egui::StrokeKind::Outside,
                            );
                        }
                    }
                });
            });
        if changed {
            self.edited();
        }
        if let Some(c) = cursor {
            self.cursor = c;
        }
        if let Some(s) = selection {
            self.selection = s;
        }

        if !bar.open {
            return None;
        }
        let before = bar.query.clone();
        let error = self.found.error.is_some();
        let action = bar.ui(ui.ctx(), editor_rect, &self.find_status(), error);
        if bar.query != before || action.is_some() {
            ui.ctx().request_repaint();
        }
        action.and_then(|a| self.find_action(a, bar))
    }
}

/// Gives the matches (byte ranges, in order) a background in `job`; `current` a stronger one.
/// Ranges that don't fit the text (found before an edit made this frame) are left out.
fn mark_matches(
    job: &mut LayoutJob,
    matches: &[Range<usize>],
    current: Option<usize>,
    color: Color32,
    current_color: Color32,
) {
    let text = &job.text;
    let fits = |r: &Range<usize>| {
        r.end <= text.len() && text.is_char_boundary(r.start) && text.is_char_boundary(r.end)
    };
    if matches.is_empty() || !matches.iter().all(fits) {
        return;
    }
    let mut out = Vec::with_capacity(job.sections.len() + 2 * matches.len());
    let mut m = 0;
    for section in job.sections.drain(..) {
        let (first, end) = (section.byte_range.start.0, section.byte_range.end.0);
        let mut start = first;
        while start < end {
            while matches.get(m).is_some_and(|r| r.end <= start) {
                m += 1;
            }
            let (to, background) = match matches.get(m) {
                Some(r) if r.start <= start => {
                    let bg = if current == Some(m) {
                        current_color
                    } else {
                        color
                    };
                    (r.end.min(end), Some(bg))
                }
                Some(r) => (r.start.min(end), None),
                None => (end, None),
            };
            let mut piece = section.clone();
            piece.byte_range = egui::text::ByteIndex(start)..egui::text::ByteIndex(to);
            if start != first {
                piece.leading_space = 0.0;
            }
            if let Some(bg) = background {
                piece.format.background = bg;
            }
            out.push(piece);
            start = to;
        }
    }
    job.sections = out;
}

fn char_to_byte(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map_or(text.len(), |(byte, _)| byte)
}

fn byte_to_char(text: &str, byte: usize) -> usize {
    text.get(..byte).map_or(0, |s| s.chars().count())
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
    fn marks_matches_across_sections() {
        let mut job = LayoutJob::default();
        let format = egui::TextFormat::default();
        job.append("let foo", 0.0, format.clone());
        job.append(" = foo;", 0.0, format);
        mark_matches(
            &mut job,
            &[4..7, 10..13],
            Some(1),
            Color32::RED,
            Color32::GREEN,
        );
        let pieces: Vec<(&str, Color32)> = job
            .sections
            .iter()
            .map(|s| {
                let r = s.byte_range.start.0..s.byte_range.end.0;
                (job.text.get(r).unwrap_or_default(), s.format.background)
            })
            .collect();
        assert_eq!(
            pieces,
            [
                ("let ", Color32::TRANSPARENT),
                ("foo", Color32::RED),
                (" = ", Color32::TRANSPARENT),
                ("foo", Color32::GREEN),
                (";", Color32::TRANSPARENT),
            ]
        );
    }

    #[test]
    fn steps_through_matches_and_replaces() {
        let dir = std::env::temp_dir().join(format!("codecraft-find-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("f.txt");
        let _ = std::fs::write(&path, "one two\ntwo one\n");
        let mut doc = Document::open(&path);
        let _ = std::fs::remove_dir_all(&dir);
        let query = Query {
            text: "one".to_owned(),
            ..Query::default()
        };
        doc.update_matches(&query);
        assert_eq!(doc.find_status(), "1 of 2");
        doc.find_step(&query, true);
        assert_eq!(doc.select, Some(12..15));
        doc.replace_one(&query, "1");
        assert!(matches!(&doc.content, Content::Text(t) if t == "one two\ntwo 1\n"));
        doc.update_matches(&query);
        assert_eq!(doc.find_status(), "1 of 1");
        doc.select_in_line(1, 4..5);
        assert_eq!(doc.select, Some(12..13));
        assert!(doc.dirty);
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
