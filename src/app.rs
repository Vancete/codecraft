//! Window shell: the tab strip, the activity bar, the side panel (explorer or search), the
//! editor, the terminal panel and the status bar.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use egui::{Event, Key, Modifiers, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use serde::{Deserialize, Serialize};

use crate::editor::{self, Document};
use crate::explorer::{self, Explorer};
use crate::extensions::{self, Extensions};
use crate::file_icons::{self, FileIcons};
use crate::find::FindBar;
use crate::highlight;
use crate::icons;
use crate::quick_open::{self, QuickOpen};
use crate::search::{self, SearchView};
use crate::settings::{CodeTheme, Settings};
use crate::terminal::{self, ShellSpec, Terminal};
use crate::theme::{self, ThemeKind, Tokens};
use crate::update;
use crate::vscode;

const MAX_RECENT: usize = 12;
const TAB_H: f32 = 40.0;
const ACTIVITY_W: f32 = 48.0;

/// Set by `main` when the window was closed maximized: it opens restored and is maximized once
/// it shows its first frame. Created maximized, winit shows the hidden window for an instant
/// (`ShowWindow(SW_MAXIMIZE)`) before anything is painted, a white flash on Windows.
pub static MAXIMIZE_WHEN_SHOWN: AtomicBool = AtomicBool::new(false);

/// Starts reading VS Code's themes on a background thread, to overlap with the window's
/// creation; `App::new` then finds them cached instead of scanning VS Code's extensions.
pub fn preload() {
    highlight::warm_up();
}

/// What survives a restart.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    folder: Option<PathBuf>,
    /// Open files, in tab order.
    open: Vec<PathBuf>,
    /// Where the settings tab is among them, when it's open.
    settings_tab: Option<usize>,
    active: usize,
    expanded: Vec<PathBuf>,
    recent: Vec<PathBuf>,
    theme: ThemeKind,
    settings: Settings,
    side_hidden: bool,
    side_view: SideView,
    search: SearchView,
    find: FindBar,
    /// When GitHub was last asked for a newer release (Unix seconds).
    update_checked: i64,
    /// A release the user chose to skip.
    skipped_version: String,
}

/// What the side panel shows.
#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
enum SideView {
    #[default]
    Explorer,
    Search,
    Extensions,
}

/// Something waiting on "save your changes?".
#[derive(Clone, Copy, PartialEq)]
enum Closing {
    Tab(usize),
    Window,
}

/// An editor tab: a file, or the settings page.
enum Tab {
    Doc(Box<Document>),
    Settings,
}

impl Tab {
    fn doc(&self) -> Option<&Document> {
        match self {
            Tab::Doc(doc) => Some(doc.as_ref()),
            Tab::Settings => None,
        }
    }

    fn doc_mut(&mut self) -> Option<&mut Document> {
        match self {
            Tab::Doc(doc) => Some(doc.as_mut()),
            Tab::Settings => None,
        }
    }

    fn name(&self) -> &str {
        match self {
            Tab::Doc(doc) => &doc.name,
            Tab::Settings => "Settings",
        }
    }

    fn dirty(&self) -> bool {
        self.doc().is_some_and(|d| d.dirty)
    }

    /// Stays the same while the tab moves along the strip.
    fn id(&self) -> egui::Id {
        match self {
            Tab::Doc(doc) => egui::Id::new(("tab", &doc.path)),
            Tab::Settings => egui::Id::new("tab-settings"),
        }
    }
}

/// A tab being dragged along the strip.
#[derive(Clone, Copy)]
struct TabDrag {
    /// Its index when the drag started.
    from: usize,
    /// Where the pointer holds it, from its left edge.
    grab: f32,
}

/// Seconds tabs take to slide into their place.
const TAB_SLIDE: f32 = 0.15;

pub struct App {
    explorer: Explorer,
    tabs: Vec<Tab>,
    active: usize,
    recent: Vec<PathBuf>,
    theme: ThemeKind,
    settings: Settings,
    side_open: bool,
    side_view: SideView,
    search: SearchView,
    find: FindBar,
    terminals: Vec<Terminal>,
    active_terminal: usize,
    terminal_open: bool,
    terminal_error: Option<String>,
    /// Files whose search matches wait on "replace them all?".
    confirm_replace: Option<Vec<PathBuf>>,
    tab_drag: Option<TabDrag>,
    quick_open: QuickOpen,
    extensions: Extensions,
    /// Files other instances were asked to open (see `single_instance`).
    incoming: Option<std::sync::mpsc::Receiver<PathBuf>>,
    /// A newer release, shown as a pill in the tab strip.
    update: Option<update::Release>,
    update_rx: Option<std::sync::mpsc::Receiver<Result<update::Release, String>>>,
    update_checked: i64,
    skipped_version: String,
    /// VS Code's themes, when the setting is on.
    /// The VS Code colour theme in use, when one is chosen and found.
    code_theme: Option<Arc<vscode::theme::EditorTheme>>,
    file_icons: FileIcons,
    /// A short note in the status bar: (text, is an error, until).
    note: Option<(String, bool, Instant)>,
    closing: Option<Closing>,
    /// The user agreed to close the window (unsaved changes handled).
    close_confirmed: bool,
    was_focused: bool,
}

impl App {
    /// `open_arg`: a folder or file from the command line, opened on start.
    pub fn new(cc: &eframe::CreationContext<'_>, open_arg: Option<PathBuf>) -> Self {
        let saved: Saved = cc
            .storage
            .and_then(|s| eframe::get_value(s, eframe::APP_KEY))
            .unwrap_or_default();
        let ctx = &cc.egui_ctx;
        egui_extras::install_image_loaders(ctx);
        saved.settings.set(ctx);

        let extensions = Extensions::load(&saved.settings.enabled_extensions);
        let mut app = Self {
            explorer: Explorer::default(),
            tabs: Vec::new(),
            active: 0,
            recent: saved.recent,
            theme: saved.theme,
            settings: saved.settings,
            side_open: !saved.side_hidden,
            side_view: saved.side_view,
            search: saved.search,
            find: saved.find,
            terminals: Vec::new(),
            active_terminal: 0,
            terminal_open: false,
            terminal_error: None,
            confirm_replace: None,
            tab_drag: None,
            quick_open: QuickOpen::default(),
            extensions,
            incoming: None,
            update: None,
            update_rx: None,
            update_checked: saved.update_checked,
            skipped_version: saved.skipped_version,
            code_theme: None,
            file_icons: FileIcons,
            note: None,
            closing: None,
            close_confirmed: false,
            was_focused: true,
        };
        theme::install_fonts(ctx);
        app.apply_theme(ctx);
        app.check_for_update(ctx);

        if let Some(folder) = saved.folder.filter(|f| f.is_dir()) {
            app.explorer.set_root(Some(folder));
            app.explorer.expanded = saved.expanded.into_iter().filter(|p| p.is_dir()).collect();
        }
        for path in saved.open.iter().filter(|p| p.is_file()) {
            app.tabs.push(Tab::Doc(Box::new(Document::open(path))));
        }
        if let Some(at) = saved.settings_tab {
            app.tabs.insert(at.min(app.tabs.len()), Tab::Settings);
        }
        app.active = saved.active.min(app.tabs.len().saturating_sub(1));
        app.explorer.selected = app.active_path();
        if let Some(path) = open_arg {
            app.open_path(&path);
        }
        app
    }

    /// Opens the files other instances hand over, as they arrive.
    pub fn take_files_from(&mut self, incoming: Option<std::sync::mpsc::Receiver<PathBuf>>) {
        self.incoming = incoming;
    }

    /// Files dropped on the window, or handed over by another instance: each in a tab of its
    /// own (a folder opens in the explorer). Shows "Drop to open" while files are dragged over.
    fn open_arriving(&mut self, ctx: &egui::Context) {
        let mut paths: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_owned())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        let handed: Vec<PathBuf> = self
            .incoming
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        if !handed.is_empty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        paths.extend(handed);
        for path in paths {
            self.open_path(&path);
        }

        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let t = Tokens::get(ctx);
            let screen = ctx.content_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("drop-files"),
            ));
            painter.rect_filled(screen, 0.0, t.shadow);
            painter.rect_stroke(
                screen.shrink(6.0),
                t.radius,
                Stroke::new(2.0, t.accent),
                egui::StrokeKind::Inside,
            );
            painter.text(
                screen.center(),
                egui::Align2::CENTER_CENTER,
                "Drop to open",
                theme::semibold(20.0),
                t.text,
            );
        }
    }

    /// Opens a folder in the explorer, or a file in a tab (its folder too, if none is open).
    pub fn open_path(&mut self, path: &Path) {
        let path = std::fs::canonicalize(path)
            .map(strip_verbatim)
            .unwrap_or_else(|_| path.to_owned());
        if path.is_dir() {
            self.open_folder(&path);
        } else if path.is_file() {
            if self.explorer.root.is_none()
                && let Some(parent) = path.parent()
            {
                self.open_folder(parent);
            }
            self.open_file(&path);
        } else {
            self.show_note(format!("{} doesn't exist", path.display()), true);
        }
    }

    fn open_folder(&mut self, path: &Path) {
        self.explorer.set_root(Some(path.to_owned()));
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_owned());
        self.recent.truncate(MAX_RECENT);
        self.side_open = true;
    }

    fn pick_folder(&mut self) {
        if let Some(dir) = rfd::FileDialog::new()
            .set_title("Open Folder")
            .pick_folder()
        {
            self.open_path(&dir);
        }
    }

    /// Opens a file in a tab of its own (or pins its preview tab).
    fn open_file(&mut self, path: &Path) {
        self.open_file_as(path, false);
    }

    /// Opens a file; `preview` puts it in the preview tab, replacing the file shown there,
    /// unless preview tabs are turned off.
    fn open_file_as(&mut self, path: &Path, preview: bool) {
        let preview = preview && self.settings.preview_tabs;
        match self
            .tabs
            .iter()
            .position(|t| t.doc().is_some_and(|d| d.path == path))
        {
            Some(i) => {
                self.active = i;
                if !preview && let Some(doc) = self.tabs.get_mut(i).and_then(Tab::doc_mut) {
                    doc.preview = false;
                }
            }
            None => {
                let mut doc = Box::new(Document::open(path));
                doc.preview = preview;
                let reuse = preview
                    .then(|| {
                        self.tabs
                            .iter()
                            .position(|t| t.doc().is_some_and(|d| d.preview && !d.dirty))
                    })
                    .flatten();
                match reuse.and_then(|i| self.tabs.get_mut(i).map(|t| (i, t))) {
                    Some((i, tab)) => {
                        *tab = Tab::Doc(doc);
                        self.active = i;
                    }
                    None => {
                        self.tabs.push(Tab::Doc(doc));
                        self.active = self.tabs.len() - 1;
                    }
                }
            }
        }
        if let Some(doc) = self.tabs.get_mut(self.active).and_then(Tab::doc_mut) {
            doc.focus = true;
        }
        self.explorer.selected = Some(path.to_owned());
    }

    /// The command palette's commands: those of the enabled extensions.
    fn commands(&self) -> Vec<quick_open::Command> {
        self.extensions.commands()
    }

    /// Runs a command from the command palette on the open file.
    fn run_command(&mut self, id: &str) {
        let Some(doc) = self.active_doc() else {
            self.show_note("Open a file first".to_owned(), true);
            return;
        };
        let Some(text) = doc.text() else {
            self.show_note(format!("{} can't be edited here", doc.name), true);
            return;
        };
        let input = extensions::Doc {
            text: text.to_owned(),
            name: doc.name.clone(),
            path: doc.path.clone(),
        };
        match self.extensions.run(id, &input) {
            Ok(outcome) => {
                if let Some(text) = outcome.text
                    && let Some(doc) = self.active_doc_mut()
                {
                    doc.set_text(text);
                    doc.focus = true;
                }
                if let Some(note) = outcome.notes.into_iter().last() {
                    self.show_note(note, false);
                }
            }
            Err(e) => self.show_note(e, true),
        }
    }

    fn extension_action(&mut self, action: extensions::Action) {
        let enabled = &mut self.settings.enabled_extensions;
        match action {
            extensions::Action::Enable(id) => {
                self.extensions.set_enabled(&id, true);
                if !enabled.contains(&id) {
                    enabled.push(id);
                }
            }
            extensions::Action::Disable(id) => {
                self.extensions.set_enabled(&id, false);
                enabled.retain(|e| *e != id);
            }
            extensions::Action::Rescan => self.extensions.rescan(enabled),
            extensions::Action::OpenFolder => {
                let dir = Extensions::dir();
                let opened = std::fs::create_dir_all(&dir).and_then(|()| open::that_detached(&dir));
                if let Err(e) = opened {
                    self.show_note(format!("{}: {e}", dir.display()), true);
                }
            }
        }
    }

    /// Asks GitHub for the latest release in the background, when that's on and it's been 12
    /// hours since the last time.
    fn check_for_update(&mut self, ctx: &egui::Context) {
        if !self.settings.check_updates || now() - self.update_checked <= update::INTERVAL {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(update::latest());
            ctx.request_repaint();
        });
        self.update_rx = Some(rx);
    }

    fn poll_update(&mut self) {
        let Some(rx) = &self.update_rx else { return };
        let Ok(result) = rx.try_recv() else { return };
        self.update_rx = None;
        // A failed check (offline…) is tried again next start.
        if let Ok(release) = result {
            self.update_checked = now();
            if update::newer(&release.version, update::CURRENT)
                && release.version != self.skipped_version
            {
                self.update = Some(release);
            }
        }
    }

    /// Files under `from` were renamed or moved to `to`: their tabs follow.
    fn moved(&mut self, from: &Path, to: &Path) {
        for doc in self.tabs.iter_mut().filter_map(Tab::doc_mut) {
            if let Ok(rest) = doc.path.strip_prefix(from) {
                let path = if rest.as_os_str().is_empty() {
                    to.to_owned()
                } else {
                    to.join(rest)
                };
                doc.moved_to(path);
            }
        }
        self.explorer.selected = self.active_path();
    }

    /// Files under `path` were deleted: their tabs close, unless they hold unsaved changes.
    fn deleted(&mut self, path: &Path) {
        while let Some(i) = self.tabs.iter().position(|t| {
            t.doc()
                .is_some_and(|d| d.path.starts_with(path) && !d.dirty)
        }) {
            self.remove_tab(i);
        }
    }

    /// Shows the settings tab, opening it after the others if it isn't open.
    pub fn open_settings(&mut self) {
        match self.tabs.iter().position(|t| matches!(t, Tab::Settings)) {
            Some(i) => self.active = i,
            None => {
                self.tabs.push(Tab::Settings);
                self.active = self.tabs.len() - 1;
            }
        }
    }

    /// The file of the active tab.
    fn active_path(&self) -> Option<PathBuf> {
        self.tabs
            .get(self.active)
            .and_then(Tab::doc)
            .map(|d| d.path.clone())
    }

    fn select_tab(&mut self, i: usize) {
        let Some(tab) = self.tabs.get_mut(i) else {
            return;
        };
        self.active = i;
        if let Some(doc) = tab.doc_mut() {
            doc.focus = true;
            self.explorer.selected = Some(doc.path.clone());
        }
    }

    fn active_doc(&self) -> Option<&Document> {
        self.tabs.get(self.active).and_then(Tab::doc)
    }

    fn active_doc_mut(&mut self) -> Option<&mut Document> {
        self.tabs.get_mut(self.active).and_then(Tab::doc_mut)
    }

    /// Shows a view in the side panel; the button of the view already shown hides the panel.
    fn toggle_view(&mut self, view: SideView) {
        if self.side_open && self.side_view == view {
            self.side_open = false;
        } else {
            self.side_open = true;
            self.side_view = view;
        }
    }

    /// The search view with its field focused, holding the editor's selection if any.
    fn show_search(&mut self, replace: bool) {
        self.side_open = true;
        self.side_view = SideView::Search;
        self.search.focus = true;
        self.search.replace_open |= replace;
        if let Some(text) = self.active_doc().and_then(Document::selected_text) {
            self.search.query.text = text;
        }
    }

    /// The editor's find bar, holding the selection if any.
    fn show_find(&mut self, replace: bool) {
        let Some(doc) = self.active_doc() else {
            return;
        };
        if let Some(text) = doc.selected_text() {
            self.find.query.text = text;
        }
        self.find.open = true;
        self.find.focus = true;
        self.find.replace_open |= replace;
    }

    fn toggle_terminal(&mut self, ctx: &egui::Context) {
        self.terminal_open = !self.terminal_open;
        if !self.terminal_open {
            return;
        }
        match self.terminals.get(self.active_terminal) {
            Some(term) => ctx.memory_mut(|m| m.request_focus(term.id)),
            None => self.start_terminal(None, ctx),
        }
    }

    /// A new shell in the open folder (the home folder without one), focused.
    fn start_terminal(&mut self, shell: Option<ShellSpec>, ctx: &egui::Context) {
        let shell = shell.unwrap_or_else(|| terminal::default_shell(&self.settings.shell));
        let cwd = self
            .explorer
            .root
            .clone()
            .or_else(|| {
                std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                    .map(PathBuf::from)
            })
            .unwrap_or_else(|| PathBuf::from("."));
        self.terminal_open = true;
        match Terminal::spawn(&cwd, &shell, ctx) {
            Ok(term) => {
                ctx.memory_mut(|m| m.request_focus(term.id));
                self.terminals.push(term);
                self.active_terminal = self.terminals.len() - 1;
                self.terminal_error = None;
            }
            Err(e) => self.terminal_error = Some(e),
        }
    }

    fn close_terminal(&mut self, i: usize) {
        if i < self.terminals.len() {
            self.terminals.remove(i);
        }
        self.active_terminal = self
            .active_terminal
            .min(self.terminals.len().saturating_sub(1));
        if self.terminals.is_empty() {
            self.terminal_open = false;
        }
    }

    /// The bottom panel: the shells' tabs and the active one. A shell that exits is closed.
    fn terminal_panel(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        let action = terminal::tabs(ui, &self.terminals, self.active_terminal);
        if let Some(err) = &self.terminal_error {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.colored_label(t.danger, err);
            });
        }
        let i = self.active_terminal;
        if let Some(term) = self.terminals.get_mut(i) {
            if term.has_exited() {
                self.close_terminal(i);
            } else {
                term.ui(ui, self.settings.terminal_font);
            }
        }
        let ctx = ui.ctx().clone();
        match action {
            Some(terminal::TabAction::Select(i)) => {
                self.active_terminal = i;
                if let Some(term) = self.terminals.get(i) {
                    ctx.memory_mut(|m| m.request_focus(term.id));
                }
            }
            Some(terminal::TabAction::Close(i)) => self.close_terminal(i),
            Some(terminal::TabAction::New(shell)) => self.start_terminal(shell, &ctx),
            Some(terminal::TabAction::Hide) => self.terminal_open = false,
            None => {}
        }
    }

    /// Replaces the search view's matches in `files`: in the editor for open files (saved
    /// unless they had unsaved changes), on disk for the others. Then searches again.
    fn replace_in_files(&mut self, files: &[PathBuf], ctx: &egui::Context) {
        let Some((re, query, with)) = self.search.replacing() else {
            return;
        };
        let (mut total, mut changed) = (0, 0);
        let mut error = None;
        for path in files {
            let open = self
                .tabs
                .iter_mut()
                .filter_map(Tab::doc_mut)
                .find(|d| d.path == *path && matches!(d.content, editor::Content::Text(_)));
            let n = match open {
                Some(doc) => {
                    let had_changes = doc.dirty;
                    let n = doc.replace_matches(&re, &query, &with);
                    if n > 0
                        && !had_changes
                        && let Err(e) = doc.save()
                    {
                        error = Some(e);
                    }
                    n
                }
                None => search::replace_in_file(path, &re, &query, &with).unwrap_or_else(|e| {
                    error = Some(e);
                    0
                }),
            };
            total += n;
            changed += usize::from(n > 0);
        }
        match error {
            Some(e) => self.show_note(e, true),
            None => self.show_note(
                format!(
                    "Replaced {total} occurrence{} in {changed} file{}",
                    if total == 1 { "" } else { "s" },
                    if changed == 1 { "" } else { "s" }
                ),
                false,
            ),
        }
        let root = self.explorer.root.clone();
        self.search.search_now(root.as_deref(), ctx);
    }

    /// "Replace all?" before the search view changes files.
    fn confirm_replace_ui(&mut self, ctx: &egui::Context) {
        let Some(files) = &self.confirm_replace else {
            return;
        };
        let (matches, count) = self.search.count(files);
        let t = Tokens::get(ctx);
        let frame = egui::Frame::new()
            .fill(t.card)
            .stroke(Stroke::new(1.0, t.field_border))
            .corner_radius(t.radius_lg)
            .inner_margin(egui::Margin::same(18))
            .shadow(ctx.global_style().visuals.window_shadow);
        let plural = |n: usize, one: &str, many: &str| {
            if n == 1 {
                format!("1 {one}")
            } else {
                format!("{n} {many}")
            }
        };
        let question = format!(
            "Replace {} across {} with '{}'?",
            plural(matches, "occurrence", "occurrences"),
            plural(count, "file", "files"),
            self.search.replace
        );
        let mut answer = None;
        let resp = egui::Modal::new(egui::Id::new("confirm-replace"))
            .frame(frame)
            .backdrop_color(egui::Color32::from_black_alpha(110))
            .show(ctx, |ui| {
                ui.set_width(400.0);
                ui.label(RichText::new(question).font(theme::semibold(15.0)));
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Files that aren't open are changed on disk.").color(t.text_dim),
                );
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    let replace = egui::Button::new(RichText::new("Replace").color(t.accent_text))
                        .fill(t.accent)
                        .min_size(vec2(90.0, 30.0));
                    if ui.add(replace).clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                        answer = Some(true);
                    }
                    if ui
                        .add(egui::Button::new("Cancel").min_size(vec2(80.0, 30.0)))
                        .clicked()
                    {
                        answer = Some(false);
                    }
                });
            });
        if resp.should_close() {
            answer = Some(false);
        }
        if let Some(yes) = answer
            && let Some(files) = self.confirm_replace.take()
            && yes
        {
            self.replace_in_files(&files, ctx);
        }
    }

    /// Closes a tab, asking first when it has unsaved changes.
    fn close_tab(&mut self, i: usize) {
        match self.tabs.get(i) {
            Some(tab) if tab.dirty() => self.closing = Some(Closing::Tab(i)),
            Some(_) => self.remove_tab(i),
            None => {}
        }
    }

    fn remove_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        self.tabs.remove(i);
        if self.active > i || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        self.explorer.selected = self.active_path();
    }

    fn save(&mut self, i: usize) -> bool {
        let Some(doc) = self.tabs.get_mut(i).and_then(Tab::doc_mut) else {
            return false;
        };
        match doc.save() {
            Ok(()) => {
                let text = format!("Saved {}", doc.name);
                self.show_note(text, false);
                true
            }
            Err(e) => {
                self.show_note(e, true);
                false
            }
        }
    }

    fn show_note(&mut self, text: String, error: bool) {
        let secs = if error { 8 } else { 3 };
        self.note = Some((text, error, Instant::now() + Duration::from_secs(secs)));
    }

    /// Applies the theme chosen: a VS Code one colours the whole window, CodeCraft's own leave
    /// it in the light or dark look picked.
    fn apply_theme(&mut self, ctx: &egui::Context) {
        self.code_theme = self.settings.vscode_theme().and_then(vscode::color_theme);
        let tokens = match &self.code_theme {
            Some(vs) => Tokens::with_vscode(vs),
            None => Tokens::for_kind(self.theme),
        };
        theme::apply(ctx, tokens);
    }

    fn set_theme(&mut self, ctx: &egui::Context, kind: ThemeKind) {
        self.theme = kind;
        self.apply_theme(ctx);
    }

    /// Editor tabs on a dark strip (gitcraft's repository tabs), the window's buttons at the end.
    /// A tab can be dragged along the strip, the others sliding out of its way, as in Opera.
    fn tab_strip(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        let mut select = None;
        let mut close = None;
        let mut open_settings = false;
        let mut moved = None;
        let mut pin = None;
        ui.horizontal(|ui| {
            ui.set_min_height(TAB_H);
            ui.spacing_mut().item_spacing.x = 1.0;
            const GAP: f32 = 1.0;
            let ctx = ui.ctx().clone();
            let n = self.tabs.len();
            // Each tab's title and width.
            let titles: Vec<_> = self
                .tabs
                .iter()
                .enumerate()
                .map(|(i, tab)| {
                    let active = i == self.active;
                    let font = if active {
                        theme::medium(12.5)
                    } else {
                        egui::FontId::proportional(12.5)
                    };
                    let color = if active { t.text } else { t.text_dim };
                    let mut job = egui::text::LayoutJob::simple_singleline(
                        tab.name().to_owned(),
                        font,
                        color,
                    );
                    if let Some(section) = job.sections.first_mut() {
                        section.format.italics = tab.doc().is_some_and(|d| d.preview);
                    }
                    let galley = ui.painter().layout_job(job);
                    let w = (galley.size().x + 66.0).clamp(110.0, 260.0);
                    (galley, color, w)
                })
                .collect();
            let width = |i: usize| titles.get(i).map_or(0.0, |g| g.2);
            let total: f32 = titles.iter().map(|g| g.2 + GAP).sum();
            let (strip, _) = ui.allocate_exact_size(vec2(total, TAB_H), Sense::hover());

            // The dragged tab follows the pointer; the slot it would drop into is where its
            // middle is among the others.
            let dragged = self.tab_drag.filter(|d| d.from < n).and_then(|d| {
                let x = ctx.pointer_interact_pos()?.x - d.grab;
                let max = (strip.right() - width(d.from) - GAP).max(strip.left());
                Some((d.from, x.clamp(strip.left(), max)))
            });
            let mut order: Vec<usize> = (0..n).collect();
            if let Some((from, x)) = dragged {
                let middle = x + width(from) / 2.0;
                let mut left = strip.left();
                let mut slot = 0;
                for j in (0..n).filter(|&j| j != from) {
                    if left + width(j) / 2.0 < middle {
                        slot += 1;
                    }
                    left += width(j) + GAP;
                }
                order.retain(|&j| j != from);
                order.insert(slot, from);
                if !ctx.input(|i| i.pointer.primary_down()) {
                    moved = Some((from, slot));
                }
            }
            let mut place = vec![strip.left(); n];
            let mut x = strip.left();
            for &j in &order {
                if let Some(p) = place.get_mut(j) {
                    *p = x;
                }
                x += width(j) + GAP;
            }

            // The dragged tab is drawn last, over the others.
            let dragging = dragged.map(|(from, _)| from);
            let paint_order = (0..n).filter(|&j| Some(j) != dragging).chain(dragging);
            for i in paint_order {
                let (Some(tab), Some((galley, color, w))) = (self.tabs.get(i), titles.get(i))
                else {
                    continue;
                };
                let (galley, color, w) = (galley.clone(), *color, *w);
                let active = i == self.active;
                let dirty = tab.dirty();
                let id = tab.id();
                let x = match dragged {
                    Some((from, x)) if from == i => {
                        ctx.animate_value_with_time(id.with("x"), x, 0.0)
                    }
                    _ => ctx.animate_value_with_time(
                        id.with("x"),
                        place.get(i).copied().unwrap_or(strip.left()),
                        TAB_SLIDE,
                    ),
                };
                let rect = Rect::from_min_size(pos2(x, strip.top()), vec2(w, TAB_H));
                let resp = ui.interact(rect, id, Sense::click_and_drag());
                let hovered = resp.hovered() && dragging.is_none();
                let p = ui.painter();
                if dragging == Some(i) {
                    p.rect_filled(rect, 0.0, if active { t.tab_active } else { t.hover });
                }
                if active {
                    p.rect_filled(rect, 0.0, t.tab_active);
                    p.line_segment(
                        [
                            rect.left_top() + vec2(0.0, 1.0),
                            rect.right_top() + vec2(0.0, 1.0),
                        ],
                        Stroke::new(2.0, t.accent),
                    );
                } else if hovered {
                    p.rect_filled(rect, 0.0, t.hover.gamma_multiply(0.6));
                }
                let y = rect.center().y;
                let icon_rect = Rect::from_center_size(
                    pos2(rect.left() + 18.0, y),
                    vec2(file_icons::SIZE, file_icons::SIZE),
                );
                match tab {
                    Tab::Doc(doc) => self.file_icons.paint_file(ui, icon_rect, &doc.name),
                    Tab::Settings => icons::paint(ui, icon_rect, "settings", 14.0, t.icon),
                }
                let text_clip = Rect::from_min_max(
                    pos2(rect.left() + 32.0, rect.top()),
                    pos2(rect.right() - 28.0, rect.bottom()),
                );
                ui.painter().with_clip_rect(text_clip).galley(
                    pos2(text_clip.left(), y - galley.size().y / 2.0),
                    galley,
                    color,
                );

                // Unsaved: a dot where the close button goes, the button while hovered.
                let close_rect =
                    Rect::from_center_size(pos2(rect.right() - 15.0, y), vec2(18.0, 18.0));
                let c = ui.interact(close_rect, resp.id.with("close"), Sense::click());
                if c.hovered() || ((active || hovered) && !dirty) {
                    if c.hovered() {
                        ui.painter().rect_filled(close_rect, t.radius_sm, t.pressed);
                    }
                    icons::paint(
                        ui,
                        close_rect,
                        "x",
                        12.0,
                        if c.hovered() { t.text } else { t.text_faint },
                    );
                } else if dirty {
                    editor::paint_dirty_dot(
                        ui,
                        close_rect,
                        if active { t.text } else { t.text_dim },
                    );
                }
                if c.clicked() {
                    close = Some(i);
                }
                if resp.drag_started_by(egui::PointerButton::Primary)
                    && let Some(origin) = ctx.input(|i| i.pointer.press_origin())
                {
                    self.tab_drag = Some(TabDrag {
                        from: i,
                        grab: origin.x - rect.left(),
                    });
                    select = Some(i);
                }
                if dragging.is_none() {
                    c.on_hover_text("Close (Ctrl+W)");
                    let tip = match tab {
                        Tab::Doc(doc) => doc.path.display().to_string(),
                        Tab::Settings => "Settings".to_owned(),
                    };
                    resp.clone().on_hover_text(tip);
                }
                if resp.clicked() && close.is_none() {
                    select = Some(i);
                }
                if resp.middle_clicked() {
                    close = Some(i);
                }
                if resp.double_clicked() {
                    pin = Some(i);
                }
            }
            if !ctx.input(|i| i.pointer.primary_down()) {
                self.tab_drag = None;
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                ui.spacing_mut().item_spacing.x = 6.0;
                if icons::bar_button(ui, "settings", "Settings (Ctrl+,)").clicked() {
                    open_settings = true;
                }
                let resp = icons::bar_button(ui, "palette", "Theme");
                egui::Popup::menu(&resp)
                    .style(theme::menu_style)
                    .show(|ui| {
                        ui.label(
                            RichText::new("THEME")
                                .font(theme::semibold(10.5))
                                .color(t.text_faint),
                        );
                        let following = self.code_theme.as_ref().map(|vs| vs.name.clone());
                        ui.add_enabled_ui(following.is_none(), |ui| {
                            for kind in ThemeKind::ALL {
                                if ui.radio(self.theme == kind, kind.label()).clicked() {
                                    self.set_theme(ui.ctx(), kind);
                                }
                            }
                        });
                        if let Some(name) = following {
                            ui.separator();
                            ui.label(
                                RichText::new(format!("VS Code theme: {name}"))
                                    .color(t.text_faint)
                                    .small(),
                            );
                            if ui.button("Use CodeCraft's themes").clicked() {
                                self.settings.theme = CodeTheme::default();
                                self.settings.set(ui.ctx());
                                self.apply_theme(ui.ctx());
                            }
                        }
                    });
                if icons::bar_button(ui, "folder-open", "Open Folder (Ctrl+O)").clicked() {
                    self.pick_folder();
                }
                // A newer release: first in the row, before the icons.
                if let Some(release) = self.update.clone() {
                    ui.add_space(4.0);
                    let pill = ui.add(
                        egui::Button::new(
                            RichText::new(format!("v{} available", release.version))
                                .color(t.accent_text)
                                .font(theme::medium(11.5)),
                        )
                        .fill(t.accent)
                        .corner_radius(10.0),
                    );
                    egui::Popup::menu(&pill)
                        .style(theme::menu_style)
                        .show(|ui| {
                            // Menus have no side margin (their buttons pad themselves); the
                            // text gets the buttons' own.
                            egui::Frame::new()
                                .inner_margin(egui::Margin {
                                    left: 16,
                                    right: 16,
                                    top: 4,
                                    bottom: 8,
                                })
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "CodeCraft {} is out (you have {}).",
                                            release.version,
                                            update::CURRENT
                                        ))
                                        .color(t.text_dim),
                                    );
                                });
                            if ui.button("Open the release page").clicked() {
                                let _ = open::that_detached(&release.url);
                            }
                            if ui.button("Skip this version").clicked() {
                                self.skipped_version = release.version.clone();
                                self.update = None;
                            }
                        });
                }
            });
        });
        if let Some(i) = select {
            self.select_tab(i);
        }
        if let Some(doc) = pin
            .and_then(|i| self.tabs.get_mut(i))
            .and_then(Tab::doc_mut)
        {
            doc.preview = false;
        }
        if let Some((from, to)) = moved {
            self.move_tab(from, to);
        }
        if let Some(i) = close {
            self.close_tab(i);
        }
        if open_settings {
            self.open_settings();
        }
    }

    /// Moves tab `from` to index `to`; the same tab stays active.
    fn move_tab(&mut self, from: usize, to: usize) {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            return;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.active = if self.active == from {
            to
        } else if from < self.active && to >= self.active {
            self.active - 1
        } else if from > self.active && to <= self.active {
            self.active + 1
        } else {
            self.active
        };
    }

    /// The narrow bar on the far left: the explorer's button, settings at the bottom.
    fn activity_bar(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        let item = |ui: &mut Ui, icon: &str, tip: &str, on: bool| {
            let (rect, resp) = ui.allocate_exact_size(vec2(ACTIVITY_W, ACTIVITY_W), Sense::click());
            let tint = if on || resp.hovered() {
                t.text
            } else {
                t.text_faint
            };
            if on {
                ui.painter().line_segment(
                    [
                        rect.left_top() + vec2(1.0, 0.0),
                        rect.left_bottom() + vec2(1.0, 0.0),
                    ],
                    Stroke::new(2.0, t.accent),
                );
            }
            icons::paint(ui, rect, icon, 22.0, tint);
            resp.on_hover_text(tip)
        };
        ui.spacing_mut().item_spacing.y = 0.0;
        let shown = |view| self.side_open && self.side_view == view;
        let (files, search, exts) = (
            shown(SideView::Explorer),
            shown(SideView::Search),
            shown(SideView::Extensions),
        );
        if item(ui, "files", "Explorer (Ctrl+Shift+E)", files).clicked() {
            self.toggle_view(SideView::Explorer);
        }
        if item(ui, "search", "Search (Ctrl+Shift+F)", search).clicked() {
            if search {
                self.side_open = false;
            } else {
                self.show_search(false);
            }
        }
        if item(ui, "blocks", "Extensions (Ctrl+Shift+X)", exts).clicked() {
            self.toggle_view(SideView::Extensions);
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            let on = matches!(self.tabs.get(self.active), Some(Tab::Settings));
            if item(ui, "settings", "Settings (Ctrl+,)", on).clicked() {
                self.open_settings();
            }
            let tip = "Terminal (Ctrl+`)";
            if item(ui, "square-terminal", tip, self.terminal_open).clicked() {
                self.toggle_terminal(ui.ctx());
            }
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        if self
            .note
            .as_ref()
            .is_some_and(|(_, _, until)| Instant::now() > *until)
        {
            self.note = None;
        }
        ui.horizontal_centered(|ui| {
            ui.add_space(10.0);
            ui.spacing_mut().item_spacing.x = 18.0;
            let small = |text: String, color| RichText::new(text).size(11.5).color(color);
            match &self.note {
                Some((text, error, until)) => {
                    let color = if *error { t.danger } else { t.text_dim };
                    ui.label(small(text.clone(), color));
                    ui.ctx()
                        .request_repaint_after(until.saturating_duration_since(Instant::now()));
                }
                None => {
                    if let Some(root) = &self.explorer.root {
                        ui.label(small(root.display().to_string(), t.text_faint));
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                if let Some(doc) = self.tabs.get(self.active).and_then(Tab::doc)
                    && matches!(doc.content, editor::Content::Text(_))
                {
                    ui.label(small(doc.language().to_owned(), t.text_dim));
                    ui.label(small(doc.line_ending().to_owned(), t.text_dim));
                    ui.label(small(
                        format!("Ln {}, Col {}", doc.cursor.0, doc.cursor.1),
                        t.text_dim,
                    ));
                }
            });
        });
    }

    /// Shown when no file is open.
    fn welcome(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        let mut open = None;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.set_max_width(520.0);
                    ui.add_space((ui.available_height() * 0.14).max(32.0));
                    ui.horizontal(|ui| {
                        icons::inline(ui, "code", 34.0, t.accent);
                        ui.label(RichText::new("CodeCraft").font(theme::semibold(30.0)));
                    });
                    ui.add_space(6.0);
                    let hint = if self.explorer.root.is_some() {
                        "Pick a file in the explorer to start editing."
                    } else {
                        "Open a folder to start."
                    };
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.label(RichText::new(hint).color(t.text_dim));
                        ui.add_space(18.0);
                        if ui
                            .add(egui::Button::new("Open Folder…").min_size(vec2(140.0, 30.0)))
                            .clicked()
                        {
                            self.pick_folder();
                        }
                        if !self.recent.is_empty() {
                            ui.add_space(26.0);
                            ui.label(
                                RichText::new("RECENT")
                                    .font(theme::semibold(10.5))
                                    .color(t.text_faint),
                            );
                            ui.add_space(4.0);
                            for path in &self.recent {
                                let name = path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| path.display().to_string());
                                ui.horizontal(|ui| {
                                    if ui.link(name).clicked() {
                                        open = Some(path.clone());
                                    }
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(path.display().to_string())
                                                .color(t.text_faint)
                                                .small(),
                                        )
                                        .truncate(),
                                    );
                                });
                            }
                        }
                    });
                });
            });
        if let Some(path) = open {
            self.open_path(&path);
        }
    }

    /// The settings tab's page, in the editor's place.
    fn settings_page(&mut self, ui: &mut Ui) {
        let mut changed = false;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(40, 28))
                    .show(ui, |ui| {
                        ui.set_max_width(640.0);
                        // Fields can share the editor's background (VS Code themes): keep
                        // the slider rails visible on it.
                        let t = Tokens::get(ui.ctx());
                        ui.visuals_mut().widgets.inactive.bg_fill = t.field_border;
                        ui.label(RichText::new("Settings").font(theme::semibold(22.0)));
                        ui.add_space(6.0);
                        changed = self.settings.ui(ui);
                    });
            });
        if changed {
            self.settings.set(ui.ctx());
            self.apply_theme(ui.ctx());
        }
    }

    /// "Save your changes?" before closing a tab or the window.
    fn closing_ui(&mut self, ctx: &egui::Context) {
        let Some(closing) = self.closing else {
            return;
        };
        let dirty: Vec<usize> = match closing {
            Closing::Tab(i) => vec![i],
            Closing::Window => (0..self.tabs.len())
                .filter(|&i| self.tabs[i].dirty())
                .collect(),
        };
        let t = Tokens::get(ctx);
        let frame = egui::Frame::new()
            .fill(t.card)
            .stroke(Stroke::new(1.0, t.field_border))
            .corner_radius(t.radius_lg)
            .inner_margin(egui::Margin::same(18))
            .shadow(ctx.global_style().visuals.window_shadow);
        let question = match dirty.as_slice() {
            [i] => format!(
                "Save the changes to {}?",
                self.tabs.get(*i).map_or("", Tab::name)
            ),
            many => format!("Save the changes to {} files?", many.len()),
        };
        #[derive(PartialEq)]
        enum Answer {
            Save,
            Discard,
            Cancel,
        }
        let mut answer = None;
        let resp = egui::Modal::new(egui::Id::new("save-changes"))
            .frame(frame)
            .backdrop_color(egui::Color32::from_black_alpha(110))
            .show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(RichText::new(question).font(theme::semibold(15.0)));
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Your changes are lost if you don't save them.")
                        .color(t.text_dim),
                );
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    let save = egui::Button::new(RichText::new("Save").color(t.accent_text))
                        .fill(t.accent)
                        .min_size(vec2(90.0, 30.0));
                    if ui.add(save).clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                        answer = Some(Answer::Save);
                    }
                    if ui
                        .add(egui::Button::new("Don't Save").min_size(vec2(90.0, 30.0)))
                        .clicked()
                    {
                        answer = Some(Answer::Discard);
                    }
                    if ui
                        .add(egui::Button::new("Cancel").min_size(vec2(80.0, 30.0)))
                        .clicked()
                    {
                        answer = Some(Answer::Cancel);
                    }
                });
            });
        if resp.should_close() {
            answer = Some(Answer::Cancel);
        }
        let Some(answer) = answer else { return };
        self.closing = None;
        if answer == Answer::Cancel {
            return;
        }
        if answer == Answer::Save {
            for &i in &dirty {
                if !self.save(i) {
                    return;
                }
            }
        }
        match closing {
            Closing::Tab(i) => self.remove_tab(i),
            Closing::Window => {
                self.close_confirmed = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// VS Code's shortcuts. While a shell has the keyboard, only the ones for the panels are
    /// taken; the rest go to the shell.
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.closing.is_some() || self.confirm_replace.is_some() {
            return;
        }
        // Shift and Alt don't have to match unless asked for: the longer shortcuts go first.
        let pressed = |mods: Modifiers, key: Key| ctx.input_mut(|i| i.consume_key(mods, key));
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let (new_terminal, toggle_terminal) = terminal_keys(ctx);
        let explorer = pressed(cmd_shift, Key::E);
        let extensions = pressed(cmd_shift, Key::X);
        let search = pressed(cmd_shift, Key::F);
        let search_replace = pressed(cmd_shift, Key::H);
        let side = pressed(Modifiers::COMMAND, Key::B);
        if new_terminal {
            self.start_terminal(None, ctx);
        } else if toggle_terminal {
            self.toggle_terminal(ctx);
        }
        if explorer {
            self.side_open = true;
            self.side_view = SideView::Explorer;
        }
        if extensions {
            self.side_open = true;
            self.side_view = SideView::Extensions;
        }
        if search || search_replace {
            self.show_search(search_replace);
        }
        if side {
            self.side_open = !self.side_open;
        }
        let in_terminal = self.terminal_open
            && self
                .terminals
                .get(self.active_terminal)
                .is_some_and(|t| t.has_focus(ctx));
        if in_terminal {
            return;
        }

        let save = pressed(Modifiers::COMMAND, Key::S);
        let close = pressed(Modifiers::COMMAND, Key::W);
        let open = pressed(Modifiers::COMMAND, Key::O);
        let settings = pressed(Modifiers::COMMAND, Key::Comma);
        let palette = pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::P);
        let quick_open = pressed(Modifiers::COMMAND, Key::P);
        let prev = pressed(Modifiers::CTRL | Modifiers::SHIFT, Key::Tab);
        let next = pressed(Modifiers::CTRL, Key::Tab);
        let find = pressed(Modifiers::COMMAND, Key::F);
        let replace = pressed(Modifiers::COMMAND, Key::H);
        let find_prev = pressed(Modifiers::SHIFT, Key::F3);
        let find_next = pressed(Modifiers::NONE, Key::F3);
        // Escape closes the find bar from it or from the editor, not from other fields.
        let escape = self.find.open
            && !self.quick_open.open
            && ctx.input(|i| i.key_pressed(Key::Escape))
            && (FindBar::has_focus(ctx)
                || !ctx.memory(|m| m.focused().is_some_and(|id| self.is_other_field(id))));
        if find || replace {
            self.show_find(replace);
        }
        if find_prev || find_next {
            self.find.open = self.active_doc().is_some();
            let query = self.find.query.clone();
            if let Some(doc) = self.active_doc_mut() {
                doc.find_step(&query, find_next);
            }
        }
        if escape {
            self.find.open = false;
            if let Some(doc) = self.active_doc_mut() {
                doc.focus = true;
            }
        }
        if save {
            self.save(self.active);
        }
        if close {
            self.close_tab(self.active);
        }
        if open {
            self.pick_folder();
        }
        if settings {
            self.open_settings();
        }
        if palette {
            if self.quick_open.commands_shown() {
                self.quick_open.close();
            } else {
                self.quick_open.show_commands(self.commands());
            }
        } else if quick_open {
            if self.quick_open.open && !self.quick_open.commands_shown() {
                self.quick_open.close();
            } else {
                self.quick_open.show(self.explorer.root.as_deref(), ctx);
            }
        }
        let n = self.tabs.len();
        if n > 0 && (next || prev) {
            let to = if next {
                (self.active + 1) % n
            } else {
                (self.active + n - 1) % n
            };
            self.select_tab(to);
        }
    }

    /// A text field of the side panel or a shell: Escape there isn't for the find bar.
    fn is_other_field(&self, id: egui::Id) -> bool {
        SearchView::owns(id)
            || id == QuickOpen::field_id()
            || self.terminals.iter().any(|t| t.id == id)
    }
}

/// Unix seconds.
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Ctrl+` toggles the terminal panel and Ctrl+Shift+` opens a new terminal, as in VS Code. The
/// key left of 1 also does, whatever it types, and so does Ñ, VS Code's key for it on Spanish
/// keyboards (it arrives as the key at the same place on an English one, `;`).
fn terminal_keys(ctx: &egui::Context) -> (bool, bool) {
    let mut found = (false, false);
    ctx.input_mut(|i| {
        i.events.retain(|e| match e {
            Event::Key {
                key,
                physical_key,
                pressed: true,
                modifiers,
                ..
            } if modifiers.command
                && !modifiers.alt
                && (*key == Key::Backtick
                    || *physical_key == Some(Key::Backtick)
                    || *key == Key::Semicolon) =>
            {
                if modifiers.shift {
                    found.0 = true;
                } else {
                    found.1 = true;
                }
                false
            }
            _ => true,
        });
    });
    found
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // eframe shows the window after the first frame; maximize it on the next one.
        if MAXIMIZE_WHEN_SHOWN.load(Ordering::Relaxed) {
            if ctx.cumulative_frame_nr() >= 1 {
                MAXIMIZE_WHEN_SHOWN.store(false, Ordering::Relaxed);
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            } else {
                ctx.request_repaint();
            }
        }

        // Unsaved files: ask before the window closes.
        if ctx.input(|i| i.viewport().close_requested())
            && !self.close_confirmed
            && self.tabs.iter().any(Tab::dirty)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.closing = Some(Closing::Window);
        }

        // Coming back to the window: files and VS Code's theme may have changed elsewhere.
        let focused = ctx.input(|i| i.focused);
        if focused && !self.was_focused {
            self.explorer.refresh();
            for doc in self.tabs.iter_mut().filter_map(Tab::doc_mut) {
                doc.reload_if_changed();
            }
            self.apply_theme(&ctx);
        }
        self.was_focused = focused;

        self.shortcuts(&ctx);
        self.open_arriving(&ctx);
        self.poll_update();
        let t = Tokens::get(&ctx);

        egui::Panel::bottom("status")
            .exact_size(24.0)
            .frame(egui::Frame::new().fill(t.status_bar))
            .show_separator_line(false)
            .show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("activity")
            .exact_size(ACTIVITY_W)
            .resizable(false)
            .frame(egui::Frame::new().fill(t.activity_bar))
            .show(ui, |ui| self.activity_bar(ui));
        if self.side_open {
            // egui remembers a panel's width by its id: a new default needs a new id.
            let panel = egui::Panel::left("sidebar")
                .resizable(true)
                .default_size(380.0)
                .size_range(160.0..=900.0)
                .frame(egui::Frame::new().fill(t.sidebar));
            match self.side_view {
                SideView::Explorer => {
                    let action = panel
                        .show(ui, |ui| self.explorer.ui(ui, &self.file_icons))
                        .inner;
                    match action {
                        Some(explorer::Action::Preview(path)) => self.open_file_as(&path, true),
                        Some(explorer::Action::Open(path)) => self.open_file(&path),
                        Some(explorer::Action::OpenFolder) => self.pick_folder(),
                        Some(explorer::Action::Created(path)) => {
                            if path.is_file() {
                                self.open_file(&path);
                            }
                        }
                        Some(explorer::Action::Moved { from, to }) => self.moved(&from, &to),
                        Some(explorer::Action::Deleted(path)) => self.deleted(&path),
                        Some(explorer::Action::Error(e)) => self.show_note(e, true),
                        None => {}
                    }
                }
                SideView::Search => {
                    let root = self.explorer.root.clone();
                    let action = panel
                        .show(ui, |ui| {
                            self.search.ui(ui, root.as_deref(), &self.file_icons)
                        })
                        .inner;
                    match action {
                        Some(search::Action::Open { path, line, cols }) => {
                            self.open_file_as(&path, true);
                            if let Some(doc) = self.active_doc_mut() {
                                doc.select_in_line(line, cols);
                            }
                        }
                        Some(search::Action::Replace(files)) => self.confirm_replace = Some(files),
                        None => {}
                    }
                }
                SideView::Extensions => {
                    let action = panel.show(ui, |ui| self.extensions.ui(ui)).inner;
                    if let Some(action) = action {
                        self.extension_action(action);
                    }
                }
            }
        }
        // After the side panels, so the tabs start where the editor does.
        egui::Panel::top("tabs")
            .frame(
                egui::Frame::new()
                    .fill(t.tab_strip)
                    .inner_margin(egui::Margin {
                        left: 0,
                        right: 6,
                        top: 0,
                        bottom: 0,
                    }),
            )
            .show(ui, |ui| self.tab_strip(ui));
        if self.terminal_open {
            egui::Panel::bottom("terminal")
                .resizable(true)
                .default_size(260.0)
                .size_range(90.0..=900.0)
                .frame(egui::Frame::new().fill(t.editor))
                .show(ui, |ui| self.terminal_panel(ui));
        }

        let code_theme = self.code_theme.clone();
        let syntax_theme = match (&code_theme, &self.settings.theme) {
            (Some(vs), _) => Some(&vs.syntax),
            (None, CodeTheme::Builtin(style)) => Some(style.theme(t.dark())),
            (None, CodeTheme::VsCode(_)) => Some(highlight::SyntaxStyle::default().theme(t.dark())),
        };
        let font_size = self.settings.editor_font;
        let mut note = None;
        let editor = egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(t.editor))
            .show(ui, |ui| match self.tabs.get_mut(self.active) {
                Some(Tab::Doc(doc)) => note = doc.ui(ui, font_size, syntax_theme, &mut self.find),
                Some(Tab::Settings) => self.settings_page(ui),
                None => self.welcome(ui),
            })
            .response
            .rect;
        let open_files: Vec<PathBuf> = self
            .tabs
            .iter()
            .filter_map(Tab::doc)
            .map(|d| d.path.clone())
            .collect();
        match self
            .quick_open
            .ui(&ctx, editor, &open_files, &self.file_icons)
        {
            Some(quick_open::Picked::File(path)) => self.open_file(&path),
            Some(quick_open::Picked::Command(id)) => self.run_command(&id),
            None => {}
        }
        if let Some(note) = note {
            self.show_note(note, false);
        }
        for doc in self.tabs.iter_mut().filter_map(Tab::doc_mut) {
            doc.preview &= !doc.dirty;
        }

        self.closing_ui(&ctx);
        self.confirm_replace_ui(&ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let saved = Saved {
            folder: self.explorer.root.clone(),
            open: self
                .tabs
                .iter()
                .filter_map(Tab::doc)
                .map(|d| d.path.clone())
                .collect(),
            settings_tab: self.tabs.iter().position(|t| matches!(t, Tab::Settings)),
            active: self.active,
            expanded: self.explorer.expanded.iter().cloned().collect(),
            recent: self.recent.clone(),
            theme: self.theme,
            settings: self.settings.clone(),
            side_hidden: !self.side_open,
            side_view: self.side_view,
            search: self.search.persisted(),
            find: self.find.clone(),
            update_checked: self.update_checked,
            skipped_version: self.skipped_version.clone(),
        };
        eframe::set_value(storage, eframe::APP_KEY, &saved);
    }
}

/// The window and taskbar icon (the `.exe` gets the same picture from `build.rs`).
pub fn window_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/app-icon/codecraft-256.png"))
        .unwrap_or_default()
}

/// `canonicalize` on Windows returns `\\?\C:\…`; show and store the familiar form.
fn strip_verbatim(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A storage that keeps strings in memory, as eframe's own does on disk.
    #[derive(Default)]
    struct Memory(std::collections::HashMap<String, String>);

    impl eframe::Storage for Memory {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_owned(), value);
        }
        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }
        fn flush(&mut self) {}
    }

    #[test]
    fn session_survives_a_restart() {
        let saved = Saved {
            folder: Some("C:/code/app".into()),
            open: vec!["C:/code/app/src/main.rs".into()],
            active: 0,
            side_hidden: true,
            ..Default::default()
        };
        let mut storage = Memory::default();
        eframe::set_value(&mut storage, eframe::APP_KEY, &saved);
        let back: Option<Saved> = eframe::get_value(&storage, eframe::APP_KEY);
        assert!(
            back.is_some_and(|b| b.folder == saved.folder && b.open == saved.open && b.side_hidden)
        );
    }

    #[test]
    fn window_icon_decodes() {
        let icon = window_icon();
        assert_eq!((icon.width, icon.height), (256, 256));
        assert!(
            icon.rgba.chunks(4).any(|p| p[3] > 0),
            "the icon isn't blank"
        );
    }

    #[test]
    fn strips_verbatim_prefix() {
        assert_eq!(strip_verbatim(r"\\?\C:\x".into()), PathBuf::from(r"C:\x"));
        assert_eq!(
            strip_verbatim(r"\\?\UNC\srv\x".into()),
            PathBuf::from(r"\\?\UNC\srv\x")
        );
    }
}
