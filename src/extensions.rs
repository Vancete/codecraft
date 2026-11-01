//! Extensions: folders in `extensions/` next to the executable, each with an `extension.json`
//! (name, version, description, icon, script) and a Luau script that adds commands to the
//! command palette.
//!
//! An extension runs only once the user enables it, never because it was put in the folder.
//! Each enabled extension gets a Lua state of its own, sandboxed: Luau has no file, network or
//! process access, `require` is removed, its globals are read-only, and a command that runs
//! too long or takes too much memory is stopped.
//!
//! The script sees a `codecraft` table:
//! - `codecraft.command(id, title, run)`: adds "Name: title" to the command palette. `run(doc)`
//!   gets the open file as `{ text, name, path }` and returns its new text, or nothing to leave
//!   it as it is.
//! - `codecraft.notify(message)`: shows a note at the bottom of the window.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::load::Bytes;
use egui::{Rect, RichText, Sense, Stroke, Ui, Vec2, vec2};
use mlua::{Function, Lua, Value, VmState};
use serde::Deserialize;

use crate::icons;
use crate::quick_open::Command;
use crate::theme::{self, Tokens};

/// A command stops after running this long.
const TIME_LIMIT: Duration = Duration::from_secs(3);
/// Memory a script can take.
const MEMORY_LIMIT: usize = 256 << 20;
/// The manifest's file name.
const MANIFEST: &str = "extension.json";

/// `extension.json`.
#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    /// An SVG or PNG in the extension's folder.
    pub icon: Option<String>,
    /// The script, in the extension's folder.
    #[serde(default = "default_main")]
    pub main: String,
}

fn default_main() -> String {
    "main.lua".to_owned()
}

pub struct Extension {
    /// Its folder's name.
    pub id: String,
    pub dir: PathBuf,
    pub manifest: Result<Manifest, String>,
    /// The icon's file, read.
    icon: Option<(String, Arc<[u8]>)>,
    /// Loaded while enabled.
    runtime: Option<Runtime>,
    /// What went wrong loading or running it last.
    pub error: Option<String>,
}

/// An enabled extension's script.
struct Runtime {
    lua: Lua,
    commands: Rc<RefCell<Vec<Registered>>>,
    notes: Rc<RefCell<Vec<String>>>,
    /// When the command running must stop.
    deadline: Arc<Mutex<Instant>>,
}

struct Registered {
    id: String,
    title: String,
    run: Function,
}

/// The open file, as a command sees it.
pub struct Doc {
    pub text: String,
    pub name: String,
    pub path: PathBuf,
}

/// What running a command did.
#[derive(Debug, Default)]
pub struct Outcome {
    /// The file's new text, when the command changed it.
    pub text: Option<String>,
    /// Notes the script asked to show.
    pub notes: Vec<String>,
}

/// What the extensions view asks for.
pub enum Action {
    Enable(String),
    Disable(String),
    OpenFolder,
    Rescan,
}

#[derive(Default)]
pub struct Extensions {
    pub list: Vec<Extension>,
}

impl Extensions {
    /// `extensions/` next to the executable. Under cargo's `examples` or `deps` folders (the
    /// screenshot tool, tests), the one next to the main executable.
    pub fn dir() -> PathBuf {
        let exe = std::env::current_exe().unwrap_or_default();
        let mut dir = exe.parent().map(Path::to_owned).unwrap_or_default();
        if dir
            .file_name()
            .is_some_and(|n| n == "examples" || n == "deps")
            && let Some(up) = dir.parent()
        {
            dir = up.to_owned();
        }
        dir.join("extensions")
    }

    /// Reads the extensions folder, starting the ones in `enabled`.
    pub fn load(enabled: &[String]) -> Self {
        let mut out = Self::default();
        out.rescan(enabled);
        out
    }

    /// Reads the extensions folder again; running extensions that are still there keep going.
    pub fn rescan(&mut self, enabled: &[String]) {
        let mut found: Vec<Extension> = std::fs::read_dir(Self::dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().join(MANIFEST).is_file())
            .map(|e| read_extension(&e.path()))
            .collect();
        found.sort_by_key(|e| e.id.to_lowercase());
        for ext in &mut found {
            if let Some(old) = self.list.iter_mut().find(|o| o.id == ext.id) {
                ext.runtime = old.runtime.take();
            }
            if ext.runtime.is_none() && enabled.contains(&ext.id) {
                ext.start();
            }
        }
        self.list = found;
    }

    /// Starts or stops an extension.
    pub fn set_enabled(&mut self, id: &str, on: bool) {
        if let Some(ext) = self.list.iter_mut().find(|e| e.id == id) {
            if on {
                ext.start();
            } else {
                ext.runtime = None;
                ext.error = None;
            }
        }
    }

    /// The commands of the running extensions, for the command palette.
    pub fn commands(&self) -> Vec<Command> {
        let mut out = Vec::new();
        for ext in &self.list {
            let (Some(rt), Ok(manifest)) = (&ext.runtime, &ext.manifest) else {
                continue;
            };
            for c in rt.commands.borrow().iter() {
                out.push(Command {
                    id: format!("{}/{}", ext.id, c.id),
                    title: format!("{}: {}", manifest.name, c.title),
                });
            }
        }
        out
    }

    /// Runs a command (`extension/command`) on the open file.
    pub fn run(&mut self, id: &str, doc: &Doc) -> Result<Outcome, String> {
        let (ext_id, command) = id.split_once('/').unwrap_or((id, ""));
        let ext = self
            .list
            .iter_mut()
            .find(|e| e.id == ext_id)
            .ok_or_else(|| format!("No extension {ext_id}"))?;
        let rt = ext
            .runtime
            .as_ref()
            .ok_or_else(|| format!("{ext_id} isn't enabled"))?;
        let result = rt.run(command, doc);
        ext.error = result.as_ref().err().cloned();
        result
    }

    /// The side panel's extensions view: a card per extension with its switch.
    pub fn ui(&self, ui: &mut Ui) -> Option<Action> {
        let t = Tokens::get(ui.ctx());
        let mut action = None;
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(
                RichText::new("EXTENSIONS")
                    .font(theme::semibold(11.0))
                    .color(t.text_dim),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                if icons::small_button(ui, "refresh-cw", "Look for extensions again").clicked() {
                    action = Some(Action::Rescan);
                }
                if icons::small_button(ui, "folder-open", "Open the extensions folder").clicked() {
                    action = Some(Action::OpenFolder);
                }
            });
        });
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(12, 0))
                    .show(ui, |ui| {
                        if self.list.is_empty() {
                            ui.label(
                                RichText::new(format!(
                                    "No extensions. Put each in a folder of its own in {}.",
                                    Self::dir().display()
                                ))
                                .color(t.text_faint),
                            );
                        }
                        for ext in &self.list {
                            if let Some(a) = ext.card(ui) {
                                action = Some(a);
                            }
                            ui.add_space(8.0);
                        }
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "Extensions run code: enable only the ones you trust. \
                                 They start disabled.",
                            )
                            .color(t.text_faint)
                            .small(),
                        );
                    });
            });
        action
    }
}

impl Extension {
    pub fn enabled(&self) -> bool {
        self.runtime.is_some()
    }

    /// Loads its script; on failure it stays disabled with the error shown.
    fn start(&mut self) {
        let manifest = match &self.manifest {
            Ok(m) => m,
            Err(e) => {
                self.error = Some(e.clone());
                return;
            }
        };
        match Runtime::start(&self.dir.join(&manifest.main), &manifest.name) {
            Ok(rt) => {
                self.runtime = Some(rt);
                self.error = None;
            }
            Err(e) => {
                self.runtime = None;
                self.error = Some(e);
            }
        }
    }

    fn card(&self, ui: &mut Ui) -> Option<Action> {
        let t = Tokens::get(ui.ctx());
        let mut action = None;
        egui::Frame::new()
            .fill(t.card)
            .stroke(Stroke::new(1.0, t.field_border))
            .corner_radius(t.radius)
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
                    match &self.icon {
                        Some((uri, bytes)) => {
                            egui::Image::from_bytes(uri.clone(), Bytes::Shared(Arc::clone(bytes)))
                                .fit_to_exact_size(Vec2::splat(36.0))
                                .paint_at(ui, rect);
                        }
                        None => icons::paint(
                            ui,
                            Rect::from_center_size(rect.center(), Vec2::splat(28.0)),
                            "blocks",
                            28.0,
                            t.text_faint,
                        ),
                    }
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        let (name, version, description) = match &self.manifest {
                            Ok(m) => (m.name.as_str(), m.version.as_str(), m.description.as_str()),
                            Err(_) => (self.id.as_str(), "", ""),
                        };
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(name)
                                    .font(theme::semibold(13.0))
                                    .color(t.text),
                            );
                            if !version.is_empty() {
                                ui.label(
                                    RichText::new(format!("v{version}"))
                                        .color(t.text_faint)
                                        .small(),
                                );
                            }
                        });
                        if !description.is_empty() {
                            ui.label(RichText::new(description).color(t.text_dim));
                        }
                        if let Some(e) = &self.error {
                            ui.label(RichText::new(e).color(t.danger).small());
                        }
                        ui.add_space(2.0);
                        let (label, next) = if self.enabled() {
                            ("Disable", Action::Disable(self.id.clone()))
                        } else {
                            ("Enable", Action::Enable(self.id.clone()))
                        };
                        let button = egui::Button::new(label).min_size(vec2(72.0, 0.0));
                        let button = if self.enabled() {
                            button
                        } else {
                            button.fill(t.accent)
                        };
                        let resp = ui.add_enabled(self.manifest.is_ok(), button);
                        if resp.clicked() {
                            action = Some(next);
                        }
                    });
                });
            });
        action
    }
}

fn read_extension(dir: &Path) -> Extension {
    let id = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let manifest = std::fs::read_to_string(dir.join(MANIFEST))
        .map_err(|e| format!("{MANIFEST}: {e}"))
        .and_then(|s| serde_json::from_str::<Manifest>(&s).map_err(|e| format!("{MANIFEST}: {e}")));
    let icon = manifest
        .as_ref()
        .ok()
        .and_then(|m| m.icon.as_deref())
        .and_then(|icon| {
            let bytes = std::fs::read(dir.join(icon)).ok()?;
            Some((format!("bytes://extensions/{id}/{icon}"), Arc::from(bytes)))
        });
    Extension {
        id,
        dir: dir.to_owned(),
        manifest,
        icon,
        runtime: None,
        error: None,
    }
}

impl Runtime {
    fn start(script: &Path, name: &str) -> Result<Self, String> {
        let source =
            std::fs::read_to_string(script).map_err(|e| format!("{}: {e}", script.display()))?;
        let lua = Lua::new();
        let commands: Rc<RefCell<Vec<Registered>>> = Rc::default();
        let notes: Rc<RefCell<Vec<String>>> = Rc::default();
        let deadline = Arc::new(Mutex::new(Instant::now() + TIME_LIMIT));
        let setup = || -> mlua::Result<()> {
            lua.set_memory_limit(MEMORY_LIMIT)?;
            let globals = lua.globals();
            // Modules come from disk: none of that.
            globals.raw_set("require", Value::Nil)?;
            let api = lua.create_table()?;
            let registered = Rc::clone(&commands);
            api.set(
                "command",
                lua.create_function(move |_, (id, title, run): (String, String, Function)| {
                    let mut list = registered.borrow_mut();
                    list.retain(|c| c.id != id);
                    list.push(Registered { id, title, run });
                    Ok(())
                })?,
            )?;
            let shown = Rc::clone(&notes);
            api.set(
                "notify",
                lua.create_function(move |_, message: String| {
                    shown.borrow_mut().push(message);
                    Ok(())
                })?,
            )?;
            globals.set("codecraft", api)?;
            let until = Arc::clone(&deadline);
            lua.set_interrupt(move |_| {
                let late = until.lock().is_ok_and(|d| Instant::now() > *d);
                if late {
                    Err(mlua::Error::runtime(format!(
                        "stopped after {} s",
                        TIME_LIMIT.as_secs()
                    )))
                } else {
                    Ok(VmState::Continue)
                }
            });
            lua.sandbox(true)?;
            lua.load(&source).set_name(format!("@{name}")).exec()
        };
        setup().map_err(|e| e.to_string())?;
        Ok(Self {
            lua,
            commands,
            notes,
            deadline,
        })
    }

    fn run(&self, command: &str, doc: &Doc) -> Result<Outcome, String> {
        let run = self
            .commands
            .borrow()
            .iter()
            .find(|c| c.id == command)
            .map(|c| c.run.clone())
            .ok_or_else(|| format!("No command {command}"))?;
        if let Ok(mut d) = self.deadline.lock() {
            *d = Instant::now() + TIME_LIMIT;
        }
        let call = || -> mlua::Result<Option<String>> {
            let arg = self.lua.create_table()?;
            arg.set("text", doc.text.as_str())?;
            arg.set("name", doc.name.as_str())?;
            arg.set("path", doc.path.to_string_lossy().as_ref())?;
            run.call::<Option<String>>(arg)
        };
        let text = call().map_err(|e| e.to_string())?;
        Ok(Outcome {
            text,
            notes: std::mem::take(&mut *self.notes.borrow_mut()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime(source: &str) -> Result<Runtime, String> {
        let dir = std::env::temp_dir().join(format!(
            "codecraft-ext-{}-{}",
            std::process::id(),
            source.len()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let script = dir.join("main.lua");
        let _ = std::fs::write(&script, source);
        let rt = Runtime::start(&script, "test");
        let _ = std::fs::remove_dir_all(&dir);
        rt
    }

    fn doc(text: &str) -> Doc {
        Doc {
            text: text.to_owned(),
            name: "a.txt".to_owned(),
            path: PathBuf::from("a.txt"),
        }
    }

    #[test]
    fn runs_commands_and_notes() {
        let rt = runtime(
            r#"codecraft.command("up", "Upper", function(doc)
                codecraft.notify("done " .. doc.name)
                return string.upper(doc.text)
            end)"#,
        );
        assert!(rt.is_ok(), "{:?}", rt.as_ref().err());
        let Ok(rt) = rt else { return };
        let out = rt.run("up", &doc("abc")).unwrap_or_default();
        assert_eq!(out.text.as_deref(), Some("ABC"));
        assert_eq!(out.notes, ["done a.txt"]);
    }

    #[test]
    fn sandboxed_and_stopped() {
        assert!(runtime("require('x')").is_err());
        assert!(runtime("io.open('x')").is_err());
        assert!(runtime("os.execute('x')").is_err());
        // Globals are read-only.
        assert!(runtime("string.upper = nil").is_err());
        let rt = runtime(r#"codecraft.command("loop", "Loop", function() while true do end end)"#);
        let Ok(rt) = rt else { return };
        let started = Instant::now();
        assert!(rt.run("loop", &doc("")).is_err());
        assert!(started.elapsed() < TIME_LIMIT + Duration::from_secs(2));
    }

    #[test]
    fn the_example_tabifies() {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("extensions/tabify/main.lua");
        let rt = Runtime::start(&script, "Tabify");
        assert!(rt.is_ok(), "{:?}", rt.as_ref().err());
        let Ok(rt) = rt else { return };
        let text = "fn a() {\n  if x {\n    y();\n   z\n  }\n  \n}\n";
        let out = rt.run("format-with-tabs", &doc(text)).unwrap_or_default();
        assert_eq!(
            out.text.as_deref(),
            Some("fn a() {\n\tif x {\n\t\ty();\n\t z\n\t}\n  \n}\n")
        );
        assert_eq!(out.notes, ["4 lines re-indented with tabs (2 spaces each)"]);
        let again = rt
            .run("format-with-tabs", &doc("\tx\n"))
            .unwrap_or_default();
        assert_eq!(again.text, None);
    }
}
