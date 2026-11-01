//! The side panel's search view, as VS Code's: a query with its options, a replacement, files to
//! include and exclude, and every match in the open folder. The folder is searched on background
//! threads (skipping what `.gitignore` lists) while the results fill in.

use std::collections::HashSet;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use egui::text::{LayoutJob, TextFormat};
use egui::{Id, Key, Modifiers, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::file_icons::{self, FileIcons};
use crate::find::{self, Query};
use crate::icons;
use crate::theme::{self, Tokens};

const ROW_H: f32 = 22.0;
/// Typing waits this long before searching.
const DEBOUNCE: Duration = Duration::from_millis(300);
/// Bigger files aren't searched.
const MAX_FILE: u64 = 10 << 20;
/// Never searched (with `.git`), as VS Code's default `search.exclude`.
const EXCLUDED: &[&str] = &["node_modules", "bower_components"];
/// Characters kept before a match in its line's preview.
const LEAD: usize = 26;

/// A match: its line (0-based) and columns (characters), and the line as shown.
struct Hit {
    line: usize,
    cols: Range<usize>,
    preview: String,
    /// Where the match is in `preview` (bytes).
    in_preview: Range<usize>,
}

struct FileHits {
    path: PathBuf,
    /// Path from the searched folder, with `/`.
    rel: String,
    hits: Vec<Hit>,
}

#[derive(Default)]
struct Results {
    /// By path.
    files: Vec<FileHits>,
    total: usize,
    done: bool,
    /// Stopped at [`find::MAX_MATCHES`].
    limited: bool,
}

/// A search started on the background threads.
struct Run {
    results: Arc<Mutex<Results>>,
    cancel: Arc<AtomicBool>,
    re: Regex,
    query: Query,
}

impl Drop for Run {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// What a search is for: the query, files to include and exclude, the folder.
type Key4 = (Query, String, String, Option<PathBuf>);

pub enum Action {
    /// Show a match in the editor.
    Open {
        path: PathBuf,
        line: usize,
        cols: Range<usize>,
    },
    /// Replace the matches in these files (the app asks first).
    Replace(Vec<PathBuf>),
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchView {
    pub query: Query,
    pub replace: String,
    pub replace_open: bool,
    /// Files to include and exclude are shown.
    pub details_open: bool,
    pub include: String,
    pub exclude: String,
    /// Focus the search field (its text selected) on the next frame.
    #[serde(skip)]
    pub focus: bool,
    #[serde(skip)]
    collapsed: HashSet<PathBuf>,
    #[serde(skip)]
    run: Option<Run>,
    /// The query is a bad regular expression: why.
    #[serde(skip)]
    error: Option<String>,
    #[serde(skip)]
    searched: Option<Key4>,
    /// Changed fields waiting for the typing to stop, and since when.
    #[serde(skip)]
    pending: Option<(Key4, Instant)>,
    /// The match last opened, highlighted: (file, match).
    #[serde(skip)]
    selected: Option<(PathBuf, usize)>,
}

fn search_id() -> Id {
    Id::new("search-view-query")
}

fn replace_id() -> Id {
    Id::new("search-view-replace")
}

impl SearchView {
    fn key(&self, root: Option<&Path>) -> Key4 {
        (
            self.query.clone(),
            self.include.clone(),
            self.exclude.clone(),
            root.map(Path::to_owned),
        )
    }

    /// Searches `root` now (again, if it was already), dropping any search under way.
    pub fn search_now(&mut self, root: Option<&Path>, ctx: &egui::Context) {
        self.searched = Some(self.key(root));
        self.pending = None;
        self.run = None;
        self.error = None;
        let Some(root) = root else {
            return;
        };
        let re = match self.query.compile() {
            None => return,
            Some(Err(e)) => {
                self.error = Some(e);
                return;
            }
            Some(Ok(re)) => re,
        };
        let filter = Filter {
            root: root.to_owned(),
            include: globs(&self.include),
            exclude: globs(&self.exclude),
        };
        self.run = Some(spawn(filter, re, self.query.clone(), ctx));
    }

    /// What's kept for the next start: the fields, not the results.
    pub fn persisted(&self) -> Self {
        Self {
            query: self.query.clone(),
            replace: self.replace.clone(),
            replace_open: self.replace_open,
            details_open: self.details_open,
            include: self.include.clone(),
            exclude: self.exclude.clone(),
            ..Self::default()
        }
    }

    /// Whether `id` is one of the view's fields.
    pub fn owns(id: Id) -> bool {
        id == search_id()
            || id == replace_id()
            || id == Id::new(("search-view", "include"))
            || id == Id::new(("search-view", "exclude"))
    }

    /// The pattern of the last search and what it was for, to replace its matches with.
    pub fn replacing(&self) -> Option<(Regex, Query, String)> {
        let run = self.run.as_ref()?;
        Some((run.re.clone(), run.query.clone(), self.replace.clone()))
    }

    /// (matches, files) of these files, as last found.
    pub fn count(&self, files: &[PathBuf]) -> (usize, usize) {
        let Some(run) = &self.run else {
            return (0, 0);
        };
        let r = lock(&run.results);
        r.files
            .iter()
            .filter(|f| files.contains(&f.path))
            .fold((0, 0), |(m, n), f| (m + f.hits.len(), n + 1))
    }

    fn fields_focused(ctx: &egui::Context) -> bool {
        ctx.memory(|m| m.has_focus(search_id()) || m.has_focus(replace_id()))
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        root: Option<&Path>,
        icons_theme: &FileIcons,
    ) -> Option<Action> {
        let t = Tokens::get(ui.ctx());
        let mut action = None;
        let ctx = ui.ctx().clone();

        // Header: the view's name and its buttons.
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.set_height(18.0);
            ui.add_space(12.0);
            ui.label(
                RichText::new("SEARCH")
                    .font(theme::semibold(10.5))
                    .color(t.text_faint),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                ui.spacing_mut().item_spacing.x = 2.0;
                let has_results = self.run.is_some();
                ui.add_enabled_ui(has_results, |ui| {
                    if icons::small_button(ui, "chevrons-down-up", "Collapse All").clicked()
                        && let Some(run) = &self.run
                    {
                        self.collapsed = lock(&run.results)
                            .files
                            .iter()
                            .map(|f| f.path.clone())
                            .collect();
                    }
                });
                ui.add_enabled_ui(has_results || !self.query.text.is_empty(), |ui| {
                    if icons::small_button(ui, "list-x", "Clear Search Results").clicked() {
                        self.query.text.clear();
                        self.replace.clear();
                        self.search_now(root, &ctx);
                    }
                });
                if icons::small_button(ui, "refresh-cw", "Refresh").clicked() {
                    self.search_now(root, &ctx);
                }
            });
        });
        ui.add_space(6.0);

        let Some(root) = root else {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(RichText::new("Open a folder to search in its files.").color(t.text_dim));
            });
            return None;
        };

        // The fields.
        if Self::fields_focused(&ctx) {
            self.query.option_keys(ui);
        }
        let mut enter = false;
        let pad = 12.0;
        ui.horizontal(|ui| {
            ui.add_space(pad - 8.0);
            ui.spacing_mut().item_spacing.x = 2.0;
            let chevron = if self.replace_open {
                "chevron-down"
            } else {
                "chevron-right"
            };
            if icons::small_button(ui, chevron, "Toggle Replace (Ctrl+Shift+H)").clicked() {
                self.replace_open = !self.replace_open;
            }
            ui.allocate_ui(vec2(ui.available_width() - pad, 26.0), |ui| {
                let error = self.error.is_some();
                let (text, mut toggles) = self.query.parts();
                let resp = find::field(
                    ui,
                    search_id(),
                    text,
                    "Search",
                    error,
                    self.focus,
                    &mut toggles,
                );
                self.focus &= !resp.has_focus();
                if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    enter = true;
                    resp.request_focus();
                }
            });
        });
        if self.replace_open {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(pad + 14.0);
                ui.spacing_mut().item_spacing.x = 2.0;
                let mut replace_all = false;
                ui.allocate_ui(vec2(ui.available_width() - pad - 22.0, 26.0), |ui| {
                    let focused = ui.memory(|m| m.has_focus(replace_id()));
                    if focused
                        && ui.input_mut(|i| {
                            i.consume_key(Modifiers::COMMAND | Modifiers::ALT, Key::Enter)
                        })
                    {
                        replace_all = true;
                    }
                    find::field(
                        ui,
                        replace_id(),
                        &mut self.replace,
                        "Replace",
                        false,
                        false,
                        &mut [],
                    );
                });
                let any = self
                    .run
                    .as_ref()
                    .is_some_and(|r| lock(&r.results).total > 0);
                ui.add_enabled_ui(any, |ui| {
                    if icons::small_button(ui, "replace-all", "Replace All (Ctrl+Alt+Enter)")
                        .clicked()
                    {
                        replace_all = true;
                    }
                });
                if replace_all && any {
                    action = Some(Action::Replace(self.files()));
                }
            });
        }
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(pad - 2.0);
                let resp = icons::small_button(ui, "ellipsis", "Toggle Search Details");
                if self.details_open {
                    ui.painter().rect_stroke(
                        resp.rect,
                        t.radius_sm,
                        Stroke::new(1.0, t.field_border),
                        egui::StrokeKind::Inside,
                    );
                }
                if resp.clicked() {
                    self.details_open = !self.details_open;
                }
            });
        });
        if self.details_open {
            for (label, text, salt) in [
                ("files to include", &mut self.include, "include"),
                ("files to exclude", &mut self.exclude, "exclude"),
            ] {
                ui.horizontal(|ui| {
                    ui.add_space(pad + 14.0);
                    ui.label(RichText::new(label).color(t.text_faint).small());
                });
                ui.horizontal(|ui| {
                    ui.add_space(pad + 14.0);
                    ui.allocate_ui(vec2(ui.available_width() - pad, 26.0), |ui| {
                        let hint = "e.g. *.rs, src/**/include";
                        let id = Id::new(("search-view", salt));
                        let resp = find::field(ui, id, text, hint, false, false, &mut []);
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            enter = true;
                        }
                    });
                });
            }
        }

        // Search once the typing stops, or on Enter.
        let key = self.key(Some(root));
        if enter {
            self.search_now(Some(root), &ctx);
        } else if self.searched.as_ref() != Some(&key) {
            let since = match &self.pending {
                Some((k, since)) if *k == key => *since,
                _ => {
                    self.pending = Some((key, Instant::now()));
                    Instant::now()
                }
            };
            let waited = since.elapsed();
            if waited >= DEBOUNCE {
                self.search_now(Some(root), &ctx);
            } else {
                ctx.request_repaint_after(DEBOUNCE - waited);
            }
        }

        ui.add_space(6.0);
        if let Some(e) = &self.error {
            ui.horizontal_wrapped(|ui| {
                ui.add_space(pad);
                ui.label(RichText::new(e).color(t.danger));
            });
            return action;
        }
        let Some(run) = &self.run else {
            return action;
        };
        let replacing = self.replace_open;
        let results = lock(&run.results);
        let files = results.files.len();
        let message = match (results.total, results.done) {
            (0, false) => "Searching…".to_owned(),
            (0, true) => {
                "No results found. Review the files to include and exclude, and .gitignore."
                    .to_owned()
            }
            (n, _) => {
                let s = |n: usize, one: &str, many: &str| {
                    if n == 1 {
                        format!("1 {one}")
                    } else {
                        format!("{n} {many}")
                    }
                };
                let mut m = format!(
                    "{} in {}",
                    s(n, "result", "results"),
                    s(files, "file", "files")
                );
                if results.limited {
                    m.push_str(" (the first ones only)");
                }
                m
            }
        };
        ui.horizontal_wrapped(|ui| {
            ui.add_space(pad);
            ui.label(RichText::new(message).color(t.text_faint).small());
        });
        ui.add_space(4.0);

        // Files, each followed by its matches unless collapsed.
        let rows: Vec<(usize, Option<usize>)> = results
            .files
            .iter()
            .enumerate()
            .flat_map(|(i, f)| {
                let open = !self.collapsed.contains(&f.path);
                std::iter::once((i, None))
                    .chain((0..if open { f.hits.len() } else { 0 }).map(move |h| (i, Some(h))))
            })
            .collect();
        let with = self.replace.clone();
        let mut toggle = None;
        egui::ScrollArea::vertical()
            .id_salt("search-results")
            .auto_shrink(false)
            .show_rows(ui, ROW_H, rows.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for &(fi, hit) in rows.get(range).unwrap_or_default() {
                    let Some(file) = results.files.get(fi) else {
                        continue;
                    };
                    let (rect, resp) =
                        ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
                    let y = rect.center().y;
                    let selected = hit.is_some_and(|h| {
                        self.selected
                            .as_ref()
                            .is_some_and(|(p, s)| *p == file.path && *s == h)
                    });
                    if selected {
                        ui.painter().rect_filled(rect, 0.0, t.row_selected);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(rect, 0.0, t.hover);
                    }
                    match hit.and_then(|h| file.hits.get(h).map(|hit| (h, hit))) {
                        None => {
                            let open = !self.collapsed.contains(&file.path);
                            let right = file_row(ui, rect, file, open, icons_theme, &t);
                            if replacing && resp.hovered() {
                                let b =
                                    Rect::from_center_size(pos2(right - 12.0, y), vec2(20.0, 20.0));
                                let r = ui.put(b, |ui: &mut Ui| {
                                    icons::small_button(
                                        ui,
                                        "replace-all",
                                        "Replace All in this File",
                                    )
                                });
                                if r.clicked() {
                                    action = Some(Action::Replace(vec![file.path.clone()]));
                                    continue;
                                }
                            }
                            if resp.clicked() {
                                toggle = Some(file.path.clone());
                            }
                        }
                        Some((h, hit)) => {
                            let x = rect.left() + 44.0;
                            let job = preview_job(
                                hit,
                                replacing.then_some((&run.re, &run.query, with.as_str())),
                                &t,
                            );
                            let galley = ui.fonts_mut(|f| f.layout_job(job));
                            ui.painter()
                                .with_clip_rect(rect.shrink2(vec2(4.0, 0.0)))
                                .galley(pos2(x, y - galley.size().y / 2.0), galley, t.text);
                            let resp = resp.on_hover_text(format!("{}:{}", file.rel, hit.line + 1));
                            if resp.clicked() {
                                self.selected = Some((file.path.clone(), h));
                                action = Some(Action::Open {
                                    path: file.path.clone(),
                                    line: hit.line,
                                    cols: hit.cols.clone(),
                                });
                            }
                        }
                    }
                }
            });
        drop(results);
        if let Some(path) = toggle
            && !self.collapsed.remove(&path)
        {
            self.collapsed.insert(path);
        }
        action
    }

    /// Every file with matches.
    fn files(&self) -> Vec<PathBuf> {
        self.run.as_ref().map_or_else(Vec::new, |run| {
            lock(&run.results)
                .files
                .iter()
                .map(|f| f.path.clone())
                .collect()
        })
    }
}

/// A file's row: chevron, icon, name, folder, count. Returns where its count badge starts.
fn file_row(
    ui: &Ui,
    rect: Rect,
    file: &FileHits,
    open: bool,
    icons_theme: &FileIcons,
    t: &Tokens,
) -> f32 {
    let y = rect.center().y;
    let chevron = if open {
        "chevron-down"
    } else {
        "chevron-right"
    };
    icons::paint(
        ui,
        Rect::from_center_size(pos2(rect.left() + 18.0, y), vec2(14.0, 14.0)),
        chevron,
        14.0,
        t.text_faint,
    );
    let (dir, name) = match file.rel.rsplit_once('/') {
        Some((dir, name)) => (dir, name),
        None => ("", file.rel.as_str()),
    };
    icons_theme.paint_file(
        ui,
        Rect::from_center_size(
            pos2(rect.left() + 36.0, y),
            vec2(file_icons::SIZE, file_icons::SIZE),
        ),
        name,
    );
    // The count, at the right end.
    let count = file.hits.len().to_string();
    let font = egui::FontId::proportional(11.0);
    let galley = ui.painter().layout_no_wrap(count, font, t.text);
    let badge = Rect::from_center_size(
        pos2(rect.right() - 14.0 - galley.size().x / 2.0, y),
        vec2(galley.size().x + 10.0, 16.0),
    );
    ui.painter().rect_filled(badge, 8.0, t.pressed);
    ui.painter()
        .galley(badge.center() - galley.size() / 2.0, galley, t.text);

    let mut job = LayoutJob::default();
    job.append(
        name,
        0.0,
        TextFormat::simple(egui::FontId::proportional(12.5), t.text),
    );
    if !dir.is_empty() {
        job.append(
            dir,
            8.0,
            TextFormat::simple(egui::FontId::proportional(11.0), t.text_faint),
        );
    }
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let clip = Rect::from_min_max(
        pos2(rect.left(), rect.top()),
        pos2(badge.left() - 6.0, rect.bottom()),
    );
    ui.painter().with_clip_rect(clip).galley(
        pos2(rect.left() + 48.0, y - galley.size().y / 2.0),
        galley,
        t.text,
    );
    badge.left()
}

/// A match's line: the match highlighted, or, while replacing, struck out and followed by what
/// replaces it.
fn preview_job(hit: &Hit, replacing: Option<(&Regex, &Query, &str)>, t: &Tokens) -> LayoutJob {
    let font = egui::FontId::proportional(12.5);
    let plain = TextFormat::simple(font.clone(), t.text_dim);
    let p = &hit.preview;
    let (before, matched, after) = (
        p.get(..hit.in_preview.start).unwrap_or_default(),
        p.get(hit.in_preview.clone()).unwrap_or_default(),
        p.get(hit.in_preview.end..).unwrap_or_default(),
    );
    let mut job = LayoutJob::default();
    job.append(before, 0.0, plain.clone());
    match replacing {
        None => job.append(
            matched,
            0.0,
            TextFormat {
                color: t.text,
                background: t.find_match,
                ..plain.clone()
            },
        ),
        Some((re, query, with)) => {
            let new = find::replacement(re, query, p, hit.in_preview.clone(), with);
            job.append(
                matched,
                0.0,
                TextFormat {
                    color: t.text,
                    background: t.danger.gamma_multiply(0.35),
                    strikethrough: Stroke::new(1.0, t.text_dim),
                    ..plain.clone()
                },
            );
            job.append(
                &new,
                0.0,
                TextFormat {
                    color: t.text,
                    background: t.success.gamma_multiply(0.35),
                    ..plain.clone()
                },
            );
        }
    }
    job.append(after, 0.0, plain);
    job.wrap.max_width = f32::INFINITY;
    job
}

/// Which files are searched.
struct Filter {
    root: PathBuf,
    include: Option<Regex>,
    exclude: Option<Regex>,
}

impl Filter {
    fn rel(&self, path: &Path) -> String {
        let rel = path.strip_prefix(&self.root).unwrap_or(path);
        rel.to_string_lossy().replace('\\', "/")
    }

    fn excluded(&self, path: &Path) -> bool {
        self.exclude
            .as_ref()
            .is_some_and(|ex| ex.is_match(&self.rel(path)))
    }

    fn included(&self, rel: &str) -> bool {
        self.include.as_ref().is_none_or(|inc| inc.is_match(rel))
    }
}

fn spawn(filter: Filter, re: Regex, query: Query, ctx: &egui::Context) -> Run {
    let results = Arc::new(Mutex::new(Results::default()));
    let cancel = Arc::new(AtomicBool::new(false));
    let run = Run {
        results: Arc::clone(&results),
        cancel: Arc::clone(&cancel),
        re: re.clone(),
        query,
    };
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let filter = Arc::new(filter);
        let mut walk = ignore::WalkBuilder::new(&filter.root);
        let f = Arc::clone(&filter);
        walk.hidden(false)
            .require_git(false)
            .filter_entry(move |e| {
                let name = e.file_name();
                name != ".git" && !EXCLUDED.iter().any(|x| name == *x) && !f.excluded(e.path())
            });
        walk.build_parallel().run(|| {
            let (filter, results, cancel, re, ctx) = (
                Arc::clone(&filter),
                Arc::clone(&results),
                Arc::clone(&cancel),
                re.clone(),
                ctx.clone(),
            );
            Box::new(move |entry| {
                use ignore::WalkState;
                if cancel.load(Ordering::Relaxed) {
                    return WalkState::Quit;
                }
                let Ok(entry) = entry else {
                    return WalkState::Continue;
                };
                if !entry.file_type().is_some_and(|t| t.is_file()) {
                    return WalkState::Continue;
                }
                let rel = filter.rel(entry.path());
                if !filter.included(&rel) {
                    return WalkState::Continue;
                }
                let hits = search_file(entry.path(), &re);
                if hits.is_empty() {
                    return WalkState::Continue;
                }
                let mut r = lock(&results);
                if r.limited {
                    return WalkState::Quit;
                }
                r.total += hits.len();
                let at = r
                    .files
                    .binary_search_by(|f| f.rel.cmp(&rel))
                    .unwrap_or_else(|i| i);
                r.files.insert(
                    at,
                    FileHits {
                        path: entry.into_path(),
                        rel,
                        hits,
                    },
                );
                if r.total >= find::MAX_MATCHES {
                    r.limited = true;
                }
                ctx.request_repaint();
                WalkState::Continue
            })
        });
        lock(&results).done = true;
        ctx.request_repaint();
    });
    run
}

/// The text of a file worth searching: not too big, not binary, UTF-8 (without its BOM).
fn read_text(path: &Path) -> Option<String> {
    if std::fs::metadata(path).ok()?.len() > MAX_FILE {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.iter().take(8192).any(|&b| b == 0) {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    Some(match text.strip_prefix('\u{feff}') {
        Some(rest) => rest.to_owned(),
        None => text,
    })
}

fn search_file(path: &Path, re: &Regex) -> Vec<Hit> {
    read_text(path).map_or_else(Vec::new, |text| hits(&text, re))
}

/// Every match of `re` in `text`, with its line.
fn hits(text: &str, re: &Regex) -> Vec<Hit> {
    let mut out = Vec::new();
    let (mut line, mut line_start, mut scanned) = (0, 0, 0);
    for r in find::find_all(re, text) {
        for (i, b) in text.bytes().enumerate().take(r.start).skip(scanned) {
            if b == b'\n' {
                line += 1;
                line_start = i + 1;
            }
        }
        scanned = r.start;
        let rest = text.get(line_start..).unwrap_or_default();
        let line_text = rest.split('\n').next().unwrap_or_default();
        let line_text = line_text.strip_suffix('\r').unwrap_or(line_text);
        let start = (r.start - line_start).min(line_text.len());
        let end = (r.end - line_start).clamp(start, line_text.len());
        let col = |b: usize| line_text.get(..b).map_or(0, |s| s.chars().count());
        let (preview, in_preview) = preview(line_text, start..end);
        out.push(Hit {
            line,
            cols: col(start)..col(end),
            preview,
            in_preview,
        });
    }
    out
}

/// The part of a line shown for a match at `m` (bytes): from its first non-blank character, or
/// from a little before the match when it's far along; where the match is in it.
fn preview(line: &str, m: Range<usize>) -> (String, Range<usize>) {
    let blank = line.len() - line.trim_start().len();
    let mut from = blank.min(m.start);
    let before = line.get(from..m.start).unwrap_or_default();
    let mut prefix = "";
    if before.chars().count() > LEAD + 4 {
        from = before
            .char_indices()
            .rev()
            .nth(LEAD - 1)
            .map_or(m.start, |(i, _)| from + i);
        prefix = "…";
    }
    let mut to = line.len();
    if to - m.end > 250 {
        to = m.end + 250;
        while !line.is_char_boundary(to) {
            to -= 1;
        }
    }
    let shown = format!("{prefix}{}", line.get(from..to).unwrap_or_default());
    let shift = prefix.len() + m.start - from;
    (shown, shift..shift + (m.end - m.start))
}

/// Replaces the matches in a file on disk, keeping its CRLF line ends and BOM; how many.
pub fn replace_in_file(
    path: &Path,
    re: &Regex,
    query: &Query,
    with: &str,
) -> Result<usize, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
    let (bom, text) = match raw.strip_prefix('\u{feff}') {
        Some(rest) => ("\u{feff}", rest),
        None => ("", raw.as_str()),
    };
    let crlf = text.contains("\r\n");
    let text = if crlf {
        text.replace("\r\n", "\n")
    } else {
        text.to_owned()
    };
    let (new, n) = find::replace_all(re, query, &text, with);
    if n > 0 {
        let new = if crlf { new.replace('\n', "\r\n") } else { new };
        std::fs::write(path, format!("{bom}{new}"))
            .map_err(|e| format!("Couldn't save {}: {e}", path.display()))?;
    }
    Ok(n)
}

/// Comma-separated globs (`*.rs, src/**`, `{a,b}`) as one pattern for paths relative to the
/// folder: like VS Code's, a glob without a `/` at its start matches at any depth, and a folder
/// matches everything in it.
fn globs(list: &str) -> Option<Regex> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0, 0);
    for (i, c) in list.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth = (depth - 1).max(0),
            ',' if depth == 0 => {
                parts.push(list.get(start..i).unwrap_or_default());
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(list.get(start..).unwrap_or_default());
    let patterns: Vec<String> = parts
        .into_iter()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(glob_regex)
        .collect();
    if patterns.is_empty() {
        return None;
    }
    RegexBuilder::new(&format!("^(?:{})$", patterns.join("|")))
        .case_insensitive(cfg!(windows))
        .build()
        .ok()
}

fn glob_regex(glob: &str) -> String {
    let glob = glob.trim_start_matches("./").trim_end_matches('/');
    let (anchored, glob) = match glob.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (glob.starts_with("**/"), glob),
    };
    let mut out = String::new();
    if !anchored {
        out.push_str("(?:.*/)?");
    }
    let chars: Vec<char> = glob.chars().collect();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        match c {
            '*' if chars.get(i + 1) == Some(&'*') => {
                if chars.get(i + 2) == Some(&'/') {
                    out.push_str("(?:.*/)?");
                    i += 3;
                } else {
                    out.push_str(".*");
                    i += 2;
                }
                continue;
            }
            '*' => out.push_str("[^/]*"),
            '?' => out.push_str("[^/]"),
            '{' => out.push_str("(?:"),
            '}' => out.push(')'),
            ',' => out.push('|'),
            c => out.push_str(&regex::escape(c.encode_utf8(&mut [0; 4]))),
        }
        i += 1;
    }
    out.push_str("(?:/.*)?");
    out
}

/// A poisoned lock still holds usable data.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_match_like_vscode() {
        let is = |globs_: &str, path: &str| globs(globs_).is_some_and(|re| re.is_match(path));
        assert!(is("*.rs", "main.rs"));
        assert!(is("*.rs", "src/app/main.rs"));
        assert!(!is("*.rs", "main.rsx"));
        assert!(is("src", "src/a/b.txt"));
        assert!(!is("src", "lib/src2/x"));
        assert!(is("src/**/*.rs", "src/a/b/c.rs"));
        assert!(is("src/**/*.rs", "src/c.rs"));
        assert!(!is("/b.txt", "a/b.txt"));
        assert!(is("{a,b}.txt, *.md", "x/b.txt"));
        assert!(is("{a,b}.txt, *.md", "README.md"));
        assert!(globs(" , ").is_none());
    }

    #[test]
    fn finds_lines_and_columns() {
        let re = Query {
            text: "two".to_owned(),
            ..Query::default()
        }
        .compile()
        .and_then(Result::ok);
        let text = "one\r\n    two three two\r\nfour";
        let found = re.map_or_else(Vec::new, |re| hits(text, &re));
        let lines: Vec<(usize, Range<usize>, &str)> = found
            .iter()
            .map(|h| {
                (
                    h.line,
                    h.cols.clone(),
                    h.preview.get(h.in_preview.clone()).unwrap_or_default(),
                )
            })
            .collect();
        assert_eq!(lines, [(1, 4..7, "two"), (1, 14..17, "two")]);
        assert_eq!(
            found.first().map(|h| h.preview.as_str()),
            Some("two three two")
        );
    }

    #[test]
    fn long_lines_are_cut_before_the_match() {
        let line = format!("{}needle", "x".repeat(100));
        let (shown, m) = preview(&line, 100..106);
        assert!(shown.starts_with('…'));
        assert_eq!(shown.get(m), Some("needle"));
    }

    #[test]
    fn replaces_on_disk_keeping_crlf() {
        let dir = std::env::temp_dir().join(format!("codecraft-search-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("a.txt");
        let _ = std::fs::write(&path, "\u{feff}a b\r\nb a\r\n");
        let q = Query {
            text: "a".to_owned(),
            word: true,
            ..Query::default()
        };
        let n = q
            .compile()
            .and_then(Result::ok)
            .and_then(|re| replace_in_file(&path, &re, &q, "c").ok());
        let back = std::fs::read_to_string(&path).unwrap_or_default();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(n, Some(2));
        assert_eq!(back, "\u{feff}c b\r\nb c\r\n");
    }
}
