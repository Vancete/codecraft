//! The side panel's file tree: the open folder, read lazily as folders are expanded.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use egui::{Rect, RichText, Sense, Ui, pos2, vec2};

use crate::file_icons::{self, FileIcons};
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

pub enum Action {
    Open(PathBuf),
    OpenFolder,
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

    /// Rows to show: (depth, node), depth first.
    fn rows(&mut self) -> Vec<(usize, Node)> {
        let Some(root) = self.root.clone() else {
            return Vec::new();
        };
        let mut out = Vec::new();
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
            out.push((depth, node));
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
        let arrows = !icons_theme.hides_arrows();
        egui::ScrollArea::vertical()
            .id_salt("explorer")
            .auto_shrink(false)
            .show_rows(ui, ROW_H, rows.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (depth, node) in &rows[range] {
                    let (rect, resp) =
                        ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
                    let selected = self.selected.as_deref() == Some(node.path.as_path());
                    if selected {
                        ui.painter().rect_filled(rect, 0.0, t.row_selected);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(rect, 0.0, t.hover);
                    }
                    let x = rect.left() + 10.0 + *depth as f32 * INDENT;
                    let y = rect.center().y;
                    let expanded = node.is_dir && self.expanded.contains(&node.path);
                    if node.is_dir && arrows {
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
                    let icon_x = x + if arrows { 16.0 } else { 0.0 };
                    let icon_rect = Rect::from_min_size(
                        pos2(icon_x, y - file_icons::SIZE / 2.0),
                        vec2(file_icons::SIZE, file_icons::SIZE),
                    );
                    let drew_icon = if node.is_dir {
                        icons_theme.paint_folder(ui, icon_rect, &node.name, expanded)
                    } else {
                        icons_theme.paint_file(ui, icon_rect, &node.name);
                        true
                    };
                    let text_x = if drew_icon {
                        icon_rect.right() + 6.0
                    } else {
                        icon_x
                    };
                    let color = if selected { t.text } else { t.text_dim };
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
                            action = Some(Action::Open(node.path.clone()));
                        }
                    }
                    resp.on_hover_text(node.path.display().to_string());
                }
            });
        action
    }
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
            e.rows().into_iter().map(|(d, n)| (d, n.name)).collect()
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
