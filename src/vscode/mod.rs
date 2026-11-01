//! What VS Code is using: its colour theme ([`theme`]) and file icon theme ([`icons`]), read
//! from its user settings and the extensions that contribute them.
//!
//! Finding either means scanning every extension's `package.json` (built-in ones included), so
//! [`load`] does it once and the result is cached until VS Code's settings file changes.

pub mod icons;
pub mod theme;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use egui::Color32;
use serde_json::Value;

/// VS Code's colour and icon themes, as far as they could be found.
#[derive(Default)]
pub struct Loaded {
    pub theme: Option<theme::EditorTheme>,
    pub icons: Option<icons::IconTheme>,
}

/// The themes selected in VS Code. Read again only when VS Code's settings file changed; an
/// empty result when VS Code isn't installed. A call made while another is reading waits for
/// it and shares its result.
pub fn load() -> Arc<Loaded> {
    type Cache = Option<(PathBuf, Option<SystemTime>, Arc<Loaded>)>;
    static CACHE: Mutex<Cache> = Mutex::new(None);

    let Some(editor) = installed_editors()
        .into_iter()
        .find(|e| e.settings.is_file())
    else {
        return Arc::default();
    };
    let modified = std::fs::metadata(&editor.settings)
        .and_then(|m| m.modified())
        .ok();
    let Ok(mut cache) = CACHE.lock() else {
        return Arc::default();
    };
    if let Some((path, when, loaded)) = cache.as_ref()
        && *path == editor.settings
        && *when == modified
    {
        return Arc::clone(loaded);
    }
    let loaded = Arc::new(read(&editor));
    *cache = Some((editor.settings.clone(), modified, Arc::clone(&loaded)));
    loaded
}

fn read(editor: &Editor) -> Loaded {
    let Some(settings) = read_jsonc(&editor.settings) else {
        return Loaded::default();
    };
    let extensions = scan(&editor.extension_dirs);
    let setting = |key: &str| settings.get(key).and_then(Value::as_str);
    // VS Code's own defaults when the user never picked one.
    let color_theme = setting("workbench.colorTheme").unwrap_or("Default Dark Modern");
    let icon_theme = match settings.get("workbench.iconTheme") {
        Some(Value::Null) => None,
        Some(v) => v.as_str(),
        None => Some("vs-seti"),
    };
    Loaded {
        theme: theme::load(&extensions, color_theme),
        icons: icon_theme.and_then(|id| icons::load(&extensions, id)),
    }
}

/// One VS Code flavour: its user settings file and where its extensions live.
struct Editor {
    settings: PathBuf,
    extension_dirs: Vec<PathBuf>,
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn config_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| home().map(|h| h.join(".config")))
    }
}

/// Folders a VS Code install keeps its built-in extensions in.
fn builtin_dirs(install: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![
        install.join("extensions"),
        install.join("resources/app/extensions"),
    ];
    // Newer Windows installs put each version in a folder of its own.
    if let Ok(entries) = std::fs::read_dir(install) {
        for e in entries.flatten() {
            dirs.push(e.path().join("resources/app/extensions"));
        }
    }
    dirs.into_iter().filter(|d| d.is_dir()).collect()
}

fn installed_editors() -> Vec<Editor> {
    let (Some(config), Some(home)) = (config_dir(), home()) else {
        return Vec::new();
    };
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let program_files = std::env::var_os("ProgramFiles").map(PathBuf::from);
    // (settings folder, user extensions folder, install folder names)
    let flavours: [(&str, &str, &[&str]); 3] = [
        (
            "Code",
            ".vscode",
            &[
                "Microsoft VS Code",
                "Visual Studio Code.app/Contents/Resources/app",
                "code",
                "visual-studio-code",
            ],
        ),
        (
            "Code - Insiders",
            ".vscode-insiders",
            &[
                "Microsoft VS Code Insiders",
                "Visual Studio Code - Insiders.app/Contents/Resources/app",
                "code-insiders",
            ],
        ),
        (
            "VSCodium",
            ".vscode-oss",
            &[
                "VSCodium",
                "VSCodium.app/Contents/Resources/app",
                "codium",
                "vscodium",
            ],
        ),
    ];
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(l) = &local {
        roots.push(l.join("Programs"));
    }
    if let Some(p) = &program_files {
        roots.push(p.clone());
    }
    roots.extend(["/Applications", "/usr/share", "/usr/lib", "/opt"].map(PathBuf::from));

    flavours
        .iter()
        .map(|(settings, extensions, installs)| {
            let mut extension_dirs = vec![home.join(extensions).join("extensions")];
            for root in &roots {
                for name in *installs {
                    extension_dirs.extend(builtin_dirs(&root.join(name)));
                }
            }
            Editor {
                settings: config.join(settings).join("User/settings.json"),
                extension_dirs,
            }
        })
        .collect()
}

/// A theme an extension contributes (colour or icon).
pub struct Contribution {
    pub id: Option<String>,
    pub label: Option<String>,
    /// The theme file, absolute.
    pub path: PathBuf,
    /// `uiTheme` of a colour theme (`vs`, `vs-dark`, `hc-black`, `hc-light`).
    pub ui_theme: Option<String>,
}

/// What the installed extensions contribute, user extensions first.
#[derive(Default)]
pub struct Extensions {
    pub color_themes: Vec<Contribution>,
    pub icon_themes: Vec<Contribution>,
    /// Lower-case extension without the dot (`rs`, `d.ts`) → language id.
    pub language_by_extension: HashMap<String, String>,
    /// Lower-case file name (`makefile`) → language id.
    pub language_by_name: HashMap<String, String>,
}

fn scan(dirs: &[PathBuf]) -> Extensions {
    let mut out = Extensions::default();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        // Newest version first when an extension is installed more than once.
        let mut exts: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        exts.sort();
        exts.reverse();
        for ext in exts {
            let Some(pkg) = read_jsonc(&ext.join("package.json")) else {
                continue;
            };
            let Some(contributes) = pkg.get("contributes") else {
                continue;
            };
            let mut nls = None;
            let mut contributions = |key: &str| -> Vec<Contribution> {
                let Some(list) = contributes.get(key).and_then(Value::as_array) else {
                    return Vec::new();
                };
                list.iter()
                    .filter_map(|t| {
                        let mut label = t.get("label").and_then(Value::as_str).map(str::to_owned);
                        if let Some(key) = label
                            .as_deref()
                            .and_then(|l| l.strip_prefix('%'))
                            .and_then(|l| l.strip_suffix('%'))
                        {
                            let nls = nls
                                .get_or_insert_with(|| read_jsonc(&ext.join("package.nls.json")));
                            label = nls
                                .as_ref()
                                .and_then(|n| n.get(key))
                                .and_then(Value::as_str)
                                .map(str::to_owned);
                        }
                        Some(Contribution {
                            id: t.get("id").and_then(Value::as_str).map(str::to_owned),
                            label,
                            path: ext.join(t.get("path").and_then(Value::as_str)?),
                            ui_theme: t.get("uiTheme").and_then(Value::as_str).map(str::to_owned),
                        })
                    })
                    .collect()
            };
            out.color_themes.extend(contributions("themes"));
            out.icon_themes.extend(contributions("iconThemes"));
            for lang in contributes
                .get("languages")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(id) = lang.get("id").and_then(Value::as_str) else {
                    continue;
                };
                let strings = |key: &str| {
                    lang.get(key)
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_lowercase)
                };
                for e in strings("extensions") {
                    let e = e.trim_start_matches('.').to_owned();
                    out.language_by_extension
                        .entry(e)
                        .or_insert_with(|| id.to_owned());
                }
                for name in strings("filenames") {
                    out.language_by_name
                        .entry(name)
                        .or_insert_with(|| id.to_owned());
                }
            }
        }
    }
    out
}

/// Finds the contribution called `name` (its id or label).
fn find<'a>(list: &'a [Contribution], name: &str) -> Option<&'a Contribution> {
    list.iter()
        .find(|c| c.id.as_deref() == Some(name) || c.label.as_deref() == Some(name))
}

/// `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`, as straight (unmultiplied) RGBA.
pub fn parse_color(s: &str) -> Option<[u8; 4]> {
    let hex = s.trim().strip_prefix('#')?;
    let digit =
        |i: usize, len: usize| u8::from_str_radix(hex.get(i * len..i * len + len)?, 16).ok();
    match hex.len() {
        3 | 4 => {
            let d = |i| digit(i, 1).map(|v| v * 17);
            Some([
                d(0)?,
                d(1)?,
                d(2)?,
                if hex.len() == 4 { d(3)? } else { 255 },
            ])
        }
        6 | 8 => Some([
            digit(0, 2)?,
            digit(1, 2)?,
            digit(2, 2)?,
            if hex.len() == 8 { digit(3, 2)? } else { 255 },
        ]),
        _ => None,
    }
}

pub fn parse_color32(s: &str) -> Option<Color32> {
    let [r, g, b, a] = parse_color(s)?;
    Some(Color32::from_rgba_unmultiplied(r, g, b, a))
}

fn read_jsonc(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&strip_jsonc(text.trim_start_matches('\u{feff}'))).ok()
}

/// Removes the comments and trailing commas VS Code allows in its JSON files.
fn strip_jsonc(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            match c {
                '\\' => out.extend(chars.next()),
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match (c, chars.peek()) {
            ('"', _) => {
                in_string = true;
                out.push(c);
            }
            ('/', Some('/')) => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut prev = ' ';
                for c in chars.by_ref() {
                    if prev == '*' && c == '/' {
                        break;
                    }
                    prev = c;
                }
                out.push(' ');
            }
            (']' | '}', _) => {
                // Drop a comma left right before the closing bracket.
                let kept = out.trim_end().len();
                if out[..kept].ends_with(',') {
                    out.truncate(kept - 1);
                }
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_comments_and_trailing_commas() {
        let src = r#"{
            // a comment
            "a": "http://x", /* block */ "b": [1, 2,],
            "c": "quote \" // not a comment",
        }"#;
        let v: Value = serde_json::from_str(&strip_jsonc(src)).unwrap_or_default();
        assert_eq!(v["a"], "http://x");
        assert_eq!(v["b"], serde_json::json!([1, 2]));
        assert_eq!(v["c"], "quote \" // not a comment");
    }

    #[test]
    fn parses_colours() {
        assert_eq!(parse_color("#fcfcfa"), Some([0xfc, 0xfc, 0xfa, 255]));
        assert_eq!(parse_color("#f0a8"), Some([255, 0, 0xaa, 0x88]));
        assert_eq!(parse_color("#12345"), None);
        assert_eq!(parse_color("red"), None);
    }
}
