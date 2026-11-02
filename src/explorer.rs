//! The side panel's file tree: the open folder, read lazily as folders are expanded.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use egui::{Key, Rect, RichText, Sense, Ui, pos2, vec2};

use crate::file_icons::{self, FileIcons};
use crate::file_ops;
use crate::icons;
use crate::theme::{self, Tokens};

const ROW_H: f32 = 22.0;
const INDENT: f32 = 12.0;

/// Never listed (as VS Code's default `files.exclude`).
const HIDDEN: &[&str] = &[".git", ".svn", ".hg", ".DS_Store", "Thumbs.db"];

#[derive(Clone)]
struct Node {
    path: PathBuf,
    name: String,
    is_dir: bool,
}

/// A row of the tree: an entry at its depth, or the field of a new one.
enum Row {
    Node(usize, Node),
    New(usize),
}

pub enum Action {
    /// A single click: in the preview tab.
    Preview(PathBuf),
    /// A double click: in a tab of its own.
    Open(PathBuf),
    OpenFolder,
    /// A new file or folder.
    Created(PathBuf),
    /// Renamed, or cut and pasted.
    Moved {
        from: PathBuf,
        to: PathBuf,
    },
    /// Sent to the Recycle Bin.
    Deleted(PathBuf),
    Error(String),
}

/// What the context menu was asked for.
enum Menu {
    NewFile(PathBuf),
    NewFolder(PathBuf),
    Reveal(PathBuf),
    /// Cut (true) or copy.
    Clip(PathBuf, bool),
    /// Into this folder.
    Paste(PathBuf),
    /// Relative to the open folder (true) or absolute.
    CopyPath(PathBuf, bool),
    Rename(PathBuf),
    Delete(PathBuf),
}

/// A name being typed in the tree.
struct Edit {
    kind: EditKind,
    text: String,
    /// Give the field the keyboard until it has it.
    focus: bool,
    /// Characters to select once it has it (the name's stem).
    select: Option<usize>,
}

#[derive(PartialEq)]
enum EditKind {
    /// In this folder.
    NewFile(PathBuf),
    NewFolder(PathBuf),
    Rename(PathBuf),
}

#[derive(Clone)]
struct Clip {
    path: PathBuf,
    cut: bool,
}

#[derive(Default)]
pub struct Explorer {
    pub root: Option<PathBuf>,
    /// Expanded folders.
    pub expanded: BTreeSet<PathBuf>,
    /// Folder listings read so far.
    children: HashMap<PathBuf, Vec<Node>>,
    /// The file shown in the editor, highlighted.
    pub selected: Option<PathBuf>,
    edit: Option<Edit>,
    /// Cut or copied, to paste.
    clip: Option<Clip>,
    /// Waiting on "delete …?".
    confirm_delete: Option<PathBuf>,
}

impl Explorer {
    pub fn set_root(&mut self, root: Option<PathBuf>) {
        if root != self.root {
            self.expanded.clear();
            self.children.clear();
            self.root = root;
        }
    }

    /// Forgets the listings so they're read again (files may have changed elsewhere).
    pub fn refresh(&mut self) {
        self.children.clear();
        self.expanded.retain(|p| p.is_dir());
    }

    fn list(&mut self, dir: &Path) -> &[Node] {
        self.children
            .entry(dir.to_owned())
            .or_insert_with(|| read_dir(dir))
    }

    /// Rows to show, depth first, with the field of a new entry at the top of its folder.
    fn rows(&mut self) -> Vec<Row> {
        let Some(root) = self.root.clone() else {
            return Vec::new();
        };
        let new_in = match self.edit.as_ref().map(|e| &e.kind) {
            Some(EditKind::NewFile(dir) | EditKind::NewFolder(dir)) => Some(dir.clone()),
            _ => None,
        };
        let mut out = Vec::new();
        if new_in.as_ref() == Some(&root) {
            out.push(Row::New(0));
        }
        let mut stack: Vec<(usize, Node)> = self
            .list(&root)
            .iter()
            .rev()
            .map(|n| (0, n.clone()))
            .collect();
        while let Some((depth, node)) = stack.pop() {
            if node.is_dir && self.expanded.contains(&node.path) {
                let kids = self.list(&node.path).to_vec();
                stack.extend(kids.into_iter().rev().map(|n| (depth + 1, n)));
            }
            let new_here = new_in.as_ref() == Some(&node.path);
            out.push(Row::Node(depth, node));
            if new_here {
                out.push(Row::New(depth + 1));
            }
        }
        out
    }

    pub fn ui(&mut self, ui: &mut Ui, icons_theme: &FileIcons) -> Option<Action> {
        let t = Tokens::get(ui.ctx());
        let mut action = None;

        // Header: the panel's name, then the folder's with its buttons.
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(
                RichText::new("EXPLORER")
                    .font(theme::semibold(10.5))
                    .color(t.text_faint),
            );
        });
        ui.add_space(4.0);
        let Some(root) = self.root.clone() else {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(RichText::new("No folder open.").color(t.text_dim));
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                if ui
                    .add(egui::Button::new("Open Folder").min_size(vec2(120.0, 28.0)))
                    .clicked()
                {
                    action = Some(Action::OpenFolder);
                }
            });
            return action;
        };
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_uppercase())
            .unwrap_or_else(|| root.display().to_string());
        ui.horizontal(|ui| {
            ui.set_height(24.0);
            ui.add_space(12.0);
            ui.label(
                RichText::new(name)
                    .font(theme::semibold(11.0))
                    .color(t.text),
            )
            .on_hover_text(root.display().to_string());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                ui.spacing_mut().item_spacing.x = 2.0;
                if icons::small_button(ui, "chevrons-down-up", "Collapse folders").clicked() {
                    self.expanded.clear();
                }
                if icons::small_button(ui, "refresh-cw", "Refresh").clicked() {
                    self.refresh();
                }
                if icons::small_button(ui, "folder-open", "Open another folder (Ctrl+O)").clicked()
                {
                    action = Some(Action::OpenFolder);
                }
            });
        });

        let rows = self.rows();
        let mut menu = None;
        let mut done = None;
        egui::ScrollArea::vertical()
            .id_salt("explorer")
            .auto_shrink(false)
            .show_rows(ui, ROW_H, rows.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                // Under the rows: a right click on empty space is for the folder itself.
                let empty = ui.interact(ui.clip_rect(), ui.id().with("empty"), Sense::click());
                egui::Popup::context_menu(&empty)
                    .style(theme::menu_style)
                    .show(|ui| {
                        if let Some(m) = self.menu(ui, &root, &root) {
                            menu = Some(m);
                        }
                    });
                for row in &rows[range] {
                    let (depth, node) = match row {
                        Row::Node(depth, node) => (*depth, Some(node)),
                        Row::New(depth) => (*depth, None),
                    };
                    let (rect, resp) =
                        ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
                    let path = node.map(|n| n.path.as_path());
                    let selected = path.is_some() && self.selected.as_deref() == path;
                    let menu_open = egui::Popup::is_id_open(ui.ctx(), Self::menu_id(&resp));
                    if selected {
                        ui.painter().rect_filled(rect, 0.0, t.row_selected);
                    } else if resp.hovered() || menu_open {
                        ui.painter().rect_filled(rect, 0.0, t.hover);
                    }
                    if menu_open {
                        ui.painter().rect_stroke(
                            rect.shrink(0.5),
                            0.0,
                            egui::Stroke::new(1.0, t.accent),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let x = rect.left() + 10.0 + depth as f32 * INDENT;
                    let y = rect.center().y;
                    let is_dir = match (node, &self.edit) {
                        (Some(n), _) => n.is_dir,
                        (None, Some(e)) => matches!(e.kind, EditKind::NewFolder(_)),
                        (None, None) => false,
                    };
                    let expanded =
                        node.is_some_and(|n| n.is_dir && self.expanded.contains(&n.path));
                    if is_dir && node.is_some() {
                        icons::paint(
                            ui,
                            Rect::from_center_size(pos2(x + 6.0, y), vec2(14.0, 14.0)),
                            if expanded {
                                "chevron-down"
                            } else {
                                "chevron-right"
                            },
                            14.0,
                            t.text_faint,
                        );
                    }
                    let icon_x = x + 16.0;
                    let icon_rect = Rect::from_min_size(
                        pos2(icon_x, y - file_icons::SIZE / 2.0),
                        vec2(file_icons::SIZE, file_icons::SIZE),
                    );
                    let editing_here = match (&self.edit, node) {
                        (Some(e), None) => !matches!(e.kind, EditKind::Rename(_)),
                        (Some(e), Some(n)) => e.kind == EditKind::Rename(n.path.clone()),
                        (None, _) => false,
                    };
                    let shown_name = match (node, &self.edit) {
                        (Some(n), _) => n.name.as_str(),
                        (None, Some(e)) => e.text.as_str(),
                        (None, None) => "",
                    };
                    if is_dir {
                        icons_theme.paint_folder(ui, icon_rect, expanded);
                    } else {
                        icons_theme.paint_file(ui, icon_rect, shown_name);
                    }
                    let text_x = icon_rect.right() + 6.0;
                    if editing_here && let Some(edit) = &mut self.edit {
                        let field = Rect::from_min_max(
                            pos2(text_x - 4.0, rect.top() + 1.0),
                            pos2(rect.right() - 6.0, rect.bottom() - 1.0),
                        );
                        if let Some(commit) = name_field(ui, field, edit) {
                            done = Some(commit);
                        }
                        continue;
                    }
                    let Some(node) = node else { continue };
                    let cut = self
                        .clip
                        .as_ref()
                        .is_some_and(|c| c.cut && c.path == node.path);
                    let color = if cut {
                        t.text_faint
                    } else if selected {
                        t.text
                    } else {
                        t.text_dim
                    };
                    ui.painter().with_clip_rect(rect).text(
                        pos2(text_x, y),
                        egui::Align2::LEFT_CENTER,
                        &node.name,
                        egui::FontId::proportional(12.5),
                        color,
                    );
                    if resp.clicked() {
                        if node.is_dir {
                            if !self.expanded.remove(&node.path) {
                                self.expanded.insert(node.path.clone());
                            }
                        } else {
                            action = Some(Action::Preview(node.path.clone()));
                        }
                    }
                    if resp.double_clicked() && !node.is_dir {
                        action = Some(Action::Open(node.path.clone()));
                    }
                    let dir = if node.is_dir {
                        node.path.clone()
                    } else {
                        node.path
                            .parent()
                            .map(Path::to_owned)
                            .unwrap_or(root.clone())
                    };
                    egui::Popup::context_menu(&resp)
                        .id(Self::menu_id(&resp))
                        .style(theme::menu_style)
                        .show(|ui| {
                            if let Some(m) = self.menu(ui, &node.path, &dir) {
                                menu = Some(m);
                            }
                        });
                    if !menu_open {
                        resp.on_hover_text(node.path.display().to_string());
                    }
                }
            });

        if let Some(commit) = done
            && let Some(a) = self.finish_edit(commit)
        {
            action = Some(a);
        }
        if let Some(m) = menu
            && let Some(a) = self.run(m, &root, ui.ctx())
        {
            action = Some(a);
        }
        if let Some(a) = self.confirm_delete_ui(ui.ctx()) {
            action = Some(a);
        }
        action
    }

    fn menu_id(resp: &egui::Response) -> egui::Id {
        resp.id.with("menu")
    }

    /// The context menu of `path`; `dir` is where new and pasted files go. Returns what was
    /// picked.
    fn menu(&self, ui: &mut Ui, path: &Path, dir: &Path) -> Option<Menu> {
        let root = self.root.as_deref() == Some(path);
        let mut picked = None;
        let mut item = |ui: &mut Ui, label: &str, enabled: bool, m: Menu| {
            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                picked = Some(m);
            }
        };
        item(ui, "New File…", true, Menu::NewFile(dir.to_owned()));
        item(ui, "New Folder…", true, Menu::NewFolder(dir.to_owned()));
        ui.separator();
        item(
            ui,
            file_ops::file_manager(),
            true,
            Menu::Reveal(path.to_owned()),
        );
        ui.separator();
        if !root {
            item(ui, "Cut", true, Menu::Clip(path.to_owned(), true));
            item(ui, "Copy", true, Menu::Clip(path.to_owned(), false));
        }
        let paste = self.clip.as_ref().is_some_and(|c| c.path.exists());
        item(ui, "Paste", paste, Menu::Paste(dir.to_owned()));
        ui.separator();
        item(
            ui,
            "Copy Path",
            true,
            Menu::CopyPath(path.to_owned(), false),
        );
        item(
            ui,
            "Copy Relative Path",
            true,
            Menu::CopyPath(path.to_owned(), true),
        );
        if !root {
            ui.separator();
            item(ui, "Rename…", true, Menu::Rename(path.to_owned()));
            item(ui, "Delete", true, Menu::Delete(path.to_owned()));
        }
        picked
    }

    fn run(&mut self, menu: Menu, root: &Path, ctx: &egui::Context) -> Option<Action> {
        match menu {
            Menu::NewFile(dir) | Menu::NewFolder(dir) if !dir.is_dir() => None,
            Menu::NewFile(dir) => {
                self.start_edit(EditKind::NewFile(dir), String::new());
                None
            }
            Menu::NewFolder(dir) => {
                self.start_edit(EditKind::NewFolder(dir), String::new());
                None
            }
            Menu::Rename(path) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.start_edit(EditKind::Rename(path), name);
                None
            }
            Menu::Reveal(path) => file_ops::reveal(&path).err().map(Action::Error),
            Menu::Clip(path, cut) => {
                self.clip = Some(Clip { path, cut });
                None
            }
            Menu::Paste(dir) => {
                let clip = self.clip.clone()?;
                let result = if clip.cut {
                    file_ops::move_into(&clip.path, &dir)
                } else {
                    file_ops::copy_into(&clip.path, &dir)
                };
                self.refresh();
                match result {
                    Ok(to) if clip.cut => {
                        self.clip = None;
                        self.expanded.insert(dir);
                        Some(Action::Moved {
                            from: clip.path,
                            to,
                        })
                    }
                    Ok(_) => {
                        self.expanded.insert(dir);
                        None
                    }
                    Err(e) => Some(Action::Error(e)),
                }
            }
            Menu::CopyPath(path, relative) => {
                let shown = if relative {
                    path.strip_prefix(root).unwrap_or(&path).to_owned()
                } else {
                    path
                };
                ctx.copy_text(shown.display().to_string());
                None
            }
            Menu::Delete(path) => {
                self.confirm_delete = Some(path);
                None
            }
        }
    }

    fn start_edit(&mut self, kind: EditKind, text: String) {
        if let EditKind::NewFile(dir) | EditKind::NewFolder(dir) = &kind {
            self.expanded.insert(dir.clone());
        }
        // A name's stem is selected first, as VS Code does: "main" of "main.rs".
        let select = match text.rfind('.') {
            Some(i) if i > 0 => text[..i].chars().count(),
            _ => text.chars().count(),
        };
        self.edit = Some(Edit {
            kind,
            text,
            focus: true,
            select: Some(select),
        });
    }

    /// Creates or renames what the field names (`commit`), or drops the field.
    fn finish_edit(&mut self, commit: bool) -> Option<Action> {
        let edit = self.edit.take()?;
        let name = edit.text.trim();
        if !commit || name.is_empty() {
            return None;
        }
        let result = match &edit.kind {
            EditKind::NewFile(dir) => file_ops::new_file(dir, name).map(Action::Created),
            EditKind::NewFolder(dir) => file_ops::new_folder(dir, name).map(Action::Created),
            EditKind::Rename(path) => {
                if path.file_name().is_some_and(|n| n == name) {
                    return None;
                }
                file_ops::rename(path, name).map(|to| Action::Moved {
                    from: path.clone(),
                    to,
                })
            }
        };
        self.refresh();
        Some(result.unwrap_or_else(Action::Error))
    }

    /// "Delete …?": the file goes to the Recycle Bin (trash), so it can be got back.
    fn confirm_delete_ui(&mut self, ctx: &egui::Context) -> Option<Action> {
        let path = self.confirm_delete.clone()?;
        let t = Tokens::get(ctx);
        let bin = if cfg!(windows) {
            "Recycle Bin"
        } else {
            "Trash"
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let frame = egui::Frame::new()
            .fill(t.card)
            .stroke(egui::Stroke::new(1.0, t.field_border))
            .corner_radius(t.radius_lg)
            .inner_margin(egui::Margin::same(18))
            .shadow(ctx.global_style().visuals.window_shadow);
        let mut answer = None;
        let resp = egui::Modal::new(egui::Id::new("confirm-delete"))
            .frame(frame)
            .backdrop_color(egui::Color32::from_black_alpha(110))
            .show(ctx, |ui| {
                ui.set_width(400.0);
                let what = if path.is_dir() {
                    format!("Delete the folder '{name}' and everything in it?")
                } else {
                    format!("Delete '{name}'?")
                };
                ui.label(RichText::new(what).font(theme::semibold(15.0)));
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!("It goes to the {bin}, where it can be restored."))
                        .color(t.text_dim),
                );
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    let delete = egui::Button::new(
                        RichText::new(format!("Move to {bin}")).color(t.accent_text),
                    )
                    .fill(t.accent)
                    .min_size(vec2(120.0, 30.0));
                    if ui.add(delete).clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
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
        let yes = answer?;
        self.confirm_delete = None;
        if !yes {
            return None;
        }
        let result = file_ops::trash(&path);
        self.refresh();
        Some(match result {
            Ok(()) => {
                if self
                    .clip
                    .as_ref()
                    .is_some_and(|c| c.path.starts_with(&path))
                {
                    self.clip = None;
                }
                Action::Deleted(path)
            }
            Err(e) => Action::Error(e),
        })
    }
}

/// The name field of a new or renamed entry. Some(true): Enter or a click elsewhere (keep the
/// name); Some(false): Escape.
fn name_field(ui: &mut Ui, rect: Rect, edit: &mut Edit) -> Option<bool> {
    let id = egui::Id::new("explorer-name");
    let resp = ui.put(
        rect,
        egui::TextEdit::singleline(&mut edit.text)
            .id(id)
            .font(egui::FontId::proportional(12.5))
            .margin(vec2(4.0, 1.0))
            .desired_width(rect.width()),
    );
    if edit.focus {
        resp.request_focus();
    }
    if resp.has_focus() {
        edit.focus = false;
        if let Some(n) = edit.select.take()
            && let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id)
        {
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(n),
                )));
            state.store(ui.ctx(), id);
        }
    }
    if resp.lost_focus() {
        return Some(!ui.input(|i| i.key_pressed(Key::Escape)));
    }
    None
}

/// A folder's entries: folders first, then files, each by name ignoring case.
fn read_dir(dir: &Path) -> Vec<Node> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut nodes: Vec<Node> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if HIDDEN.contains(&name.as_str()) {
                return None;
            }
            // Follows links, so a link to a folder opens like one.
            let is_dir = e.path().is_dir();
            Some(Node {
                path: e.path(),
                name,
                is_dir,
            })
        })
        .collect();
    nodes.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_folders_first_and_expands_lazily() {
        let dir = std::env::temp_dir().join(format!("codecraft-tree-{}", std::process::id()));
        let _ = std::fs::create_dir_all(dir.join("src"));
        let _ = std::fs::create_dir_all(dir.join(".git"));
        for f in ["b.txt", "A.md", "src/main.rs"] {
            let _ = std::fs::write(dir.join(f), "");
        }
        let mut e = Explorer::default();
        e.set_root(Some(dir.clone()));
        let names = |e: &mut Explorer| -> Vec<(usize, String)> {
            e.rows()
                .into_iter()
                .filter_map(|r| match r {
                    Row::Node(d, n) => Some((d, n.name)),
                    Row::New(_) => None,
                })
                .collect()
        };
        let collapsed = names(&mut e);
        e.expanded.insert(dir.join("src"));
        let expanded = names(&mut e);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            collapsed,
            [(0, "src".into()), (0, "A.md".into()), (0, "b.txt".into())]
        );
        assert_eq!(expanded[1], (1, "main.rs".to_owned()));
        assert_eq!(expanded.len(), 4);
    }
}
