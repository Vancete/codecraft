//! Ctrl+P: VS Code's quick open. Type part of a file's name or path and pick it from a list
//! ranked by how well it matches; with nothing typed it lists the open files.
//!
//! Ctrl+Shift+P shows the same box as the command palette: the commands extensions add.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use egui::text::{LayoutJob, TextFormat};
use egui::{Id, Key, Modifiers, Rect, Sense, Stroke, pos2, vec2};

use crate::file_icons::{self, FileIcons};
use crate::theme::Tokens;

const ROW_H: f32 = 24.0;
/// Rows listed at most.
const SHOWN: usize = 60;
/// Files listed from the folder at most.
const MAX_FILES: usize = 200_000;

/// A file or command listed: its path or title and the bytes of it the query matched.
type Row = (String, Vec<usize>);

/// A command the palette offers.
#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    pub id: String,
    pub title: String,
}

/// What was picked.
pub enum Picked {
    File(PathBuf),
    Command(String),
}

#[derive(Default)]
pub struct QuickOpen {
    pub open: bool,
    /// The command palette's commands while it shows them; `None` while it lists files.
    commands: Option<Vec<Command>>,
    query: String,
    root: Option<PathBuf>,
    /// The folder's files, relative to it with `/` between parts, filled in the background.
    files: Arc<Mutex<Vec<String>>>,
    selected: usize,
    /// Give the field the keyboard until it has it (a new area's first frame only measures it).
    focus: bool,
    /// Where it was drawn last, so a click elsewhere closes it.
    rect: Option<Rect>,
    /// The ranked files for (query, how many files were listed), kept while neither changes.
    ranked: Option<((String, usize), Vec<Row>)>,
}

impl QuickOpen {
    pub fn field_id() -> Id {
        Id::new("quick-open-field")
    }

    /// Whether it shows the command palette.
    pub fn commands_shown(&self) -> bool {
        self.open && self.commands.is_some()
    }

    fn reset(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.focus = true;
        self.rect = None;
        self.ranked = None;
    }

    /// Opens it as the command palette.
    pub fn show_commands(&mut self, commands: Vec<Command>) {
        self.reset();
        self.commands = Some(commands);
    }

    /// Opens it, listing the folder's files again.
    pub fn show(&mut self, root: Option<&Path>, ctx: &egui::Context) {
        self.reset();
        self.commands = None;
        self.root = root.map(Path::to_owned);
        let files = Arc::new(Mutex::new(Vec::new()));
        self.files = Arc::clone(&files);
        if let Some(root) = root {
            list_files(root.to_owned(), files, ctx.clone());
        }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    /// Draws it centred in the window, at the top of `editor`; returns what was picked.
    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        editor: Rect,
        open_files: &[PathBuf],
        icons: &FileIcons,
    ) -> Option<Picked> {
        if !self.open {
            return None;
        }
        let t = Tokens::get(ctx);
        let (pressed, pointer) = ctx.input(|i| (i.pointer.any_pressed(), i.pointer.interact_pos()));
        if pressed
            && let (Some(rect), Some(pos)) = (self.rect, pointer)
            && !rect.contains(pos)
        {
            self.open = false;
            return None;
        }

        // Candidates: the open files with nothing typed, else the folder's files ranked.
        let root = self.root.clone();
        let relative = |p: &Path| -> String {
            root.as_deref()
                .and_then(|r| p.strip_prefix(r).ok())
                .unwrap_or(p)
                .to_string_lossy()
                .replace('\\', "/")
        };
        let needle: Vec<char> = self
            .query
            .chars()
            .filter(|c| !c.is_whitespace())
            .flat_map(char::to_lowercase)
            .collect();
        let rows: Vec<Row> = if let Some(commands) = &self.commands {
            let mut ranked: Vec<(i32, Row)> = commands
                .iter()
                .filter_map(|c| {
                    if needle.is_empty() {
                        return Some((0, (c.title.clone(), Vec::new())));
                    }
                    score(&needle, &c.title).map(|(s, hits)| (s, (c.title.clone(), hits)))
                })
                .collect();
            ranked.sort_by_key(|r| std::cmp::Reverse(r.0));
            ranked.into_iter().map(|(_, r)| r).collect()
        } else if needle.is_empty() {
            open_files
                .iter()
                .map(|p| (relative(p), Vec::new()))
                .collect()
        } else {
            let files = self.files.lock().map(|f| f.clone()).unwrap_or_default();
            let key = (self.query.clone(), files.len());
            match &self.ranked {
                Some((k, rows)) if *k == key => rows.clone(),
                _ => {
                    let mut ranked: Vec<(i32, String, Vec<usize>)> = files
                        .into_iter()
                        .filter_map(|f| score(&needle, &f).map(|(s, hits)| (s, f, hits)))
                        .collect();
                    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.len().cmp(&b.1.len())));
                    ranked.truncate(SHOWN);
                    let rows: Vec<_> = ranked.into_iter().map(|(_, f, hits)| (f, hits)).collect();
                    self.ranked = Some((key, rows.clone()));
                    rows
                }
            }
        };
        self.selected = self.selected.min(rows.len().saturating_sub(1));

        let mut picked = None;
        let mut close = false;
        // Its keys whether or not the field has the keyboard: Escape takes it from the field
        // before the field sees it.
        {
            ctx.input_mut(|i| {
                if i.consume_key(Modifiers::NONE, Key::ArrowDown) && !rows.is_empty() {
                    self.selected = (self.selected + 1) % rows.len();
                }
                if i.consume_key(Modifiers::NONE, Key::ArrowUp) && !rows.is_empty() {
                    self.selected = (self.selected + rows.len() - 1) % rows.len();
                }
                if i.consume_key(Modifiers::NONE, Key::Enter) {
                    picked = rows.get(self.selected).map(|r| r.0.clone());
                }
                if i.consume_key(Modifiers::NONE, Key::Escape) {
                    close = true;
                }
            });
        }

        let window = ctx.content_rect();
        let width = (window.width() - 40.0).clamp(240.0, 640.0);
        let area = egui::Area::new(Id::new("quick-open"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(window.center().x - width / 2.0, editor.top() + 6.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(t.card)
                    .stroke(Stroke::new(1.0, t.field_border))
                    .corner_radius(t.radius)
                    .shadow(egui::Shadow {
                        offset: [0, 4],
                        blur: 16,
                        spread: 0,
                        color: t.shadow,
                    })
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.spacing_mut().item_spacing.y = 4.0;
                        let before = self.query.clone();
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut self.query)
                                .id(Self::field_id())
                                .hint_text(if self.commands.is_some() {
                                    "Type a command"
                                } else {
                                    "Search files by name"
                                })
                                .desired_width(f32::INFINITY)
                                .margin(vec2(6.0, 5.0)),
                        );
                        if self.focus {
                            resp.request_focus();
                        }
                        self.focus &= !resp.has_focus();
                        if self.query != before {
                            self.selected = 0;
                        }

                        if rows.is_empty() {
                            let note = match (&self.commands, needle.is_empty()) {
                                (Some(c), _) if c.is_empty() => {
                                    "No commands yet: extensions add them"
                                }
                                (Some(_), _) => "No matching commands",
                                (None, true) => "No open files",
                                (None, false) if self.root.is_none() => {
                                    "Open a folder to search its files"
                                }
                                (None, false) => "No matching files",
                            };
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(note).color(t.text_faint));
                            ui.add_space(2.0);
                            return;
                        }
                        egui::ScrollArea::vertical()
                            .max_height(ROW_H * 14.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                for (i, (path, hits)) in rows.iter().enumerate() {
                                    let (rect, row) = ui.allocate_exact_size(
                                        vec2(ui.available_width(), ROW_H),
                                        Sense::click(),
                                    );
                                    let selected = i == self.selected;
                                    if selected {
                                        ui.painter().rect_filled(rect, 3.0, t.text_selection);
                                        if self.query != before {
                                            ui.scroll_to_rect(rect, None);
                                        }
                                    } else if row.hovered() {
                                        ui.painter().rect_filled(rect, 3.0, t.hover);
                                    }
                                    let y = rect.center().y;
                                    let (job, x) = if self.commands.is_some() {
                                        (highlighted(path, 0, hits, &t, ""), rect.left() + 10.0)
                                    } else {
                                        let (dir, name) = match path.rsplit_once('/') {
                                            Some((dir, name)) => (dir, name),
                                            None => ("", path.as_str()),
                                        };
                                        let icon = Rect::from_min_size(
                                            pos2(rect.left() + 8.0, y - file_icons::SIZE / 2.0),
                                            vec2(file_icons::SIZE, file_icons::SIZE),
                                        );
                                        icons.paint_file(ui, icon, name);
                                        let start = path.len() - name.len();
                                        let job = highlighted(path, start, hits, &t, dir);
                                        (job, icon.right() + 8.0)
                                    };
                                    let galley = ui.painter().layout_job(job);
                                    ui.painter().with_clip_rect(rect).galley(
                                        pos2(x, y - galley.size().y / 2.0),
                                        galley,
                                        t.text,
                                    );
                                    if row.clicked() {
                                        picked = Some(path.clone());
                                    }
                                    if row.hovered()
                                        && ui.input(|i| i.pointer.delta() != vec2(0.0, 0.0))
                                    {
                                        self.selected = i;
                                    }
                                }
                            });
                    });
            });
        self.rect = Some(area.response.rect);

        if close {
            self.open = false;
        }
        let picked = picked.map(|rel| match (&self.commands, &self.root) {
            (Some(commands), _) => Picked::Command(
                commands
                    .iter()
                    .find(|c| c.title == rel)
                    .map(|c| c.id.clone())
                    .unwrap_or_default(),
            ),
            (None, Some(root)) if !Path::new(&rel).is_absolute() => Picked::File(root.join(rel)),
            (None, _) => Picked::File(PathBuf::from(rel)),
        });
        if picked.is_some() {
            self.open = false;
        }
        picked
    }
}

/// The row's text: the file name with the matched letters marked, then its folder, dimmer.
fn highlighted(path: &str, name_start: usize, hits: &[usize], t: &Tokens, dir: &str) -> LayoutJob {
    let font = egui::FontId::proportional(13.0);
    let mut job = LayoutJob::default();
    let mut push = |text: &str, color, strong: bool| {
        job.append(
            text,
            0.0,
            TextFormat {
                font_id: if strong {
                    crate::theme::semibold(13.0)
                } else {
                    font.clone()
                },
                color,
                ..Default::default()
            },
        );
    };
    for (i, c) in path[name_start..].char_indices() {
        let hit = hits.contains(&(name_start + i));
        let color = if hit { t.accent } else { t.text };
        push(c.encode_utf8(&mut [0; 4]), color, hit);
    }
    if !dir.is_empty() {
        push("   ", t.text_faint, false);
        for (i, c) in dir.char_indices() {
            let hit = hits.contains(&i);
            let color = if hit { t.accent } else { t.text_faint };
            push(c.encode_utf8(&mut [0; 4]), color, hit);
        }
    }
    job
}

/// How well `needle` (lowercase, no spaces) matches `path`, and the bytes it matched; `None`
/// when its letters aren't all in it, in order. Matching within the file name counts most, then
/// letters in a row and at the start of words.
fn score(needle: &[char], path: &str) -> Option<(i32, Vec<usize>)> {
    let name_start = path.rfind('/').map_or(0, |i| i + 1);
    let in_name = subsequence(needle, path, name_start).map(|(s, hits)| (s + 100, hits));
    in_name.or_else(|| subsequence(needle, path, 0))
}

fn subsequence(needle: &[char], path: &str, from: usize) -> Option<(i32, Vec<usize>)> {
    let mut hits = Vec::with_capacity(needle.len());
    let mut want = needle.iter().peekable();
    let mut score = 0;
    let mut prev: Option<char> = None;
    let mut last_hit: Option<usize> = None;
    for (i, c) in path[from..].char_indices() {
        let i = i + from;
        let Some(&&w) = want.peek() else { break };
        let boundary = match prev {
            None => true,
            Some(p) => !p.is_alphanumeric() || (p.is_lowercase() && c.is_uppercase()),
        };
        if c.to_lowercase().eq(std::iter::once(w)) {
            score += 1;
            if boundary {
                score += 8;
            }
            if last_hit.is_some_and(|l| path[l..i].chars().count() == 1) {
                score += 10;
            }
            hits.push(i);
            last_hit = Some(i);
            want.next();
        }
        prev = Some(c);
    }
    if want.peek().is_some() {
        return None;
    }
    // Shorter paths first among equals.
    Some((score * 4 - path.len() as i32 / 8, hits))
}

/// Lists the files under `root` (as the search does: `.gitignore` honoured, `.git` and
/// dependency folders skipped) into `files`, on background threads.
fn list_files(root: PathBuf, files: Arc<Mutex<Vec<String>>>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let mut walk = ignore::WalkBuilder::new(&root);
        walk.hidden(false).require_git(false).filter_entry(|e| {
            let name = e.file_name();
            name != ".git" && !crate::search::EXCLUDED.iter().any(|x| name == *x)
        });
        let found = Mutex::new(Vec::new());
        walk.build_parallel().run(|| {
            let (root, found) = (&root, &found);
            Box::new(move |entry| {
                use ignore::WalkState;
                let Ok(entry) = entry else {
                    return WalkState::Continue;
                };
                if !entry.file_type().is_some_and(|t| t.is_file()) {
                    return WalkState::Continue;
                }
                let Ok(rel) = entry.path().strip_prefix(root) else {
                    return WalkState::Continue;
                };
                let Ok(mut found) = found.lock() else {
                    return WalkState::Quit;
                };
                found.push(rel.to_string_lossy().replace('\\', "/"));
                if found.len() >= MAX_FILES {
                    WalkState::Quit
                } else {
                    WalkState::Continue
                }
            })
        });
        let mut found = found.into_inner().unwrap_or_default();
        found.sort();
        if let Ok(mut files) = files.lock() {
            *files = found;
        }
        ctx.request_repaint();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn needle(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn prefers_the_file_name_and_word_starts() {
        let rank = |q: &str, paths: &[&str]| {
            let mut v: Vec<_> = paths
                .iter()
                .filter_map(|p| score(&needle(q), p).map(|(s, _)| (s, *p)))
                .collect();
            v.sort_by_key(|a| std::cmp::Reverse(a.0));
            v.into_iter().map(|(_, p)| p.to_owned()).collect::<Vec<_>>()
        };
        assert_eq!(
            rank("app", &["src/apple/x.rs", "src/app.rs", "a/p/p.txt"]),
            ["src/app.rs", "src/apple/x.rs", "a/p/p.txt"]
        );
        assert_eq!(
            rank("qo", &["src/quick_open.rs", "src/qoo.rs"])[0],
            "src/qoo.rs"
        );
        assert_eq!(rank("srcqo", &["src/quick_open.rs"]), ["src/quick_open.rs"]);
        assert!(score(&needle("zz"), "src/app.rs").is_none());
    }

    #[test]
    fn marks_the_matched_letters() {
        let (_, hits) = score(&needle("ar"), "src/app.rs").unwrap_or_default();
        assert_eq!(hits, [4, 8]);
    }
}
