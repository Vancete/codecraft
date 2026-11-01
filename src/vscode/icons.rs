//! VS Code's file icon theme: which icon a file or folder gets, by name, extension or language,
//! as the theme's JSON maps them.
//!
//! Both kinds of theme work: font glyphs (Seti, VS Code's default, ships a WOFF font that is
//! unpacked here to TrueType for egui) and SVG/PNG pictures (Material Icon Theme and most
//! others). A WOFF2 font isn't supported; its icons fall back to the built-in ones.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use egui::Color32;
use serde_json::Value;

use super::{Extensions, find, parse_color32, read_jsonc};

/// One icon of the theme.
#[derive(Clone, Debug, PartialEq)]
pub enum IconDef {
    /// A glyph of one of the theme's fonts.
    Glyph {
        /// egui font family the font was installed as.
        family: String,
        ch: char,
        color: Option<Color32>,
        /// Glyph size relative to the text around it (`fonts[].size`, e.g. "150%").
        scale: f32,
    },
    /// An SVG or PNG file.
    Image(PathBuf),
}

/// The theme's associations (the top level, or its `light` overrides).
#[derive(Default)]
struct Associations {
    file: Option<String>,
    folder: Option<String>,
    folder_expanded: Option<String>,
    file_extensions: HashMap<String, String>,
    file_names: HashMap<String, String>,
    folder_names: HashMap<String, String>,
    folder_names_expanded: HashMap<String, String>,
    language_ids: HashMap<String, String>,
}

impl Associations {
    fn read(json: &Value) -> Self {
        let id = |key: &str| json.get(key).and_then(Value::as_str).map(str::to_owned);
        let map = |key: &str| {
            json.get(key)
                .and_then(Value::as_object)
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| Some((k.to_lowercase(), v.as_str()?.to_owned())))
                        .collect()
                })
                .unwrap_or_default()
        };
        Self {
            file: id("file"),
            folder: id("folder"),
            folder_expanded: id("folderExpanded"),
            file_extensions: map("fileExtensions"),
            file_names: map("fileNames"),
            folder_names: map("folderNames"),
            folder_names_expanded: map("folderNamesExpanded"),
            language_ids: map("languageIds"),
        }
    }

    /// The definition id for a file named `name` (lower case) in `language`.
    fn file_id(&self, name: &str, language: Option<&str>) -> Option<&str> {
        if let Some(id) = self.file_names.get(name) {
            return Some(id);
        }
        let mut rest = name;
        while let Some((_, ext)) = rest.split_once('.') {
            if let Some(id) = self.file_extensions.get(ext) {
                return Some(id);
            }
            rest = ext;
        }
        language
            .and_then(|l| self.language_ids.get(l))
            .map(String::as_str)
    }

    fn folder_id(&self, name: &str, expanded: bool) -> Option<&str> {
        let named = if expanded {
            self.folder_names_expanded.get(name)
        } else {
            self.folder_names.get(name)
        };
        named.map(String::as_str)
    }
}

pub struct IconTheme {
    pub name: String,
    defs: HashMap<String, IconDef>,
    dark: Associations,
    light: Associations,
    /// Fonts the glyphs need, as (egui family name, TrueType bytes).
    pub fonts: Vec<(String, Vec<u8>)>,
    /// The theme draws no chevrons next to folders.
    pub hides_arrows: bool,
    language_by_extension: HashMap<String, String>,
    language_by_name: HashMap<String, String>,
    /// SVG/PNG files read so far.
    images: Mutex<HashMap<PathBuf, Arc<[u8]>>>,
}

impl IconTheme {
    fn def(&self, light: bool, pick: impl Fn(&Associations) -> Option<&str>) -> Option<&IconDef> {
        let id = light
            .then(|| pick(&self.light))
            .flatten()
            .or_else(|| pick(&self.dark))?;
        self.defs.get(id)
    }

    /// The icon of a file; `None` when the theme has none for it (not even a default).
    pub fn file(&self, name: &str, light: bool) -> Option<&IconDef> {
        let name = name.to_lowercase();
        let language = self.language_of(&name);
        self.def(light, |a| a.file_id(&name, language))
            .or_else(|| self.def(light, |a| a.file.as_deref()))
    }

    /// The icon of a folder; `None` when the theme has no folder icons (Seti).
    pub fn folder(&self, name: &str, expanded: bool, light: bool) -> Option<&IconDef> {
        let name = name.to_lowercase();
        self.def(light, |a| a.folder_id(&name, expanded))
            .or_else(|| {
                self.def(light, |a| {
                    if expanded {
                        a.folder_expanded.as_deref().or(a.folder.as_deref())
                    } else {
                        a.folder.as_deref()
                    }
                })
            })
    }

    fn language_of(&self, name: &str) -> Option<&str> {
        if let Some(l) = self.language_by_name.get(name) {
            return Some(l);
        }
        let mut rest = name;
        while let Some((_, ext)) = rest.split_once('.') {
            if let Some(l) = self.language_by_extension.get(ext) {
                return Some(l);
            }
            rest = ext;
        }
        None
    }

    /// The bytes of an image icon, read once.
    pub fn image_bytes(&self, path: &Path) -> Option<Arc<[u8]>> {
        let mut images = self.images.lock().ok()?;
        if let Some(b) = images.get(path) {
            return Some(Arc::clone(b));
        }
        let bytes: Arc<[u8]> = std::fs::read(path).ok()?.into();
        images.insert(path.to_owned(), Arc::clone(&bytes));
        Some(bytes)
    }
}

pub fn load(extensions: &Extensions, id: &str) -> Option<IconTheme> {
    let found = find(&extensions.icon_themes, id)?;
    let json = read_jsonc(&found.path)?;
    let dir = found.path.parent()?;

    // Fonts: id → (family, scale).
    let mut fonts = Vec::new();
    let mut families: HashMap<String, (String, f32)> = HashMap::new();
    for font in json
        .get("fonts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(font_id) = font.get("id").and_then(Value::as_str) else {
            continue;
        };
        let scale = font
            .get("size")
            .and_then(Value::as_str)
            .and_then(|s| s.strip_suffix('%'))
            .and_then(|s| s.trim().parse::<f32>().ok())
            .map_or(1.0, |p| p / 100.0);
        let bytes = font
            .get("src")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find_map(|src| {
                let path = dir.join(src.get("path").and_then(Value::as_str)?);
                let data = std::fs::read(&path).ok()?;
                if data.starts_with(b"wOFF") {
                    woff_to_sfnt(&data)
                } else if data.starts_with(b"wOF2") {
                    None
                } else {
                    Some(data)
                }
            });
        if let Some(bytes) = bytes {
            let family = format!("vscode-icons-{font_id}");
            fonts.push((family.clone(), bytes));
            families.insert(font_id.to_owned(), (family, scale));
        }
    }
    let default_font = json
        .pointer("/fonts/0/id")
        .and_then(Value::as_str)
        .map(str::to_owned);

    let mut defs = HashMap::new();
    for (name, def) in json
        .get("iconDefinitions")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        if let Some(path) = def.get("iconPath").and_then(Value::as_str) {
            defs.insert(name.clone(), IconDef::Image(dir.join(path)));
            continue;
        }
        let Some(ch) = def
            .get("fontCharacter")
            .and_then(Value::as_str)
            .and_then(font_character)
        else {
            continue;
        };
        let font_id = def
            .get("fontId")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| default_font.clone());
        let Some((family, scale)) = font_id.and_then(|f| families.get(&f)) else {
            continue;
        };
        defs.insert(
            name.clone(),
            IconDef::Glyph {
                family: family.clone(),
                ch,
                color: def
                    .get("fontColor")
                    .and_then(Value::as_str)
                    .and_then(parse_color32),
                scale: *scale,
            },
        );
    }

    Some(IconTheme {
        name: found.label.clone().unwrap_or_else(|| id.to_owned()),
        defs,
        dark: Associations::read(&json),
        light: json
            .get("light")
            .map(Associations::read)
            .unwrap_or_default(),
        fonts,
        hides_arrows: json
            .get("hidesExplorerArrows")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        language_by_extension: extensions.language_by_extension.clone(),
        language_by_name: extensions.language_by_name.clone(),
        images: Mutex::default(),
    })
}

/// `\E001` (as the JSON spells it) or the character itself.
fn font_character(s: &str) -> Option<char> {
    match s.strip_prefix('\\') {
        Some(hex) => char::from_u32(u32::from_str_radix(hex, 16).ok()?),
        None => s.chars().next(),
    }
}

/// Unpacks a WOFF 1.0 font into the TrueType/OpenType file it wraps.
fn woff_to_sfnt(woff: &[u8]) -> Option<Vec<u8>> {
    let u16_at = |at: usize| Some(u16::from_be_bytes(woff.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_be_bytes(woff.get(at..at + 4)?.try_into().ok()?));
    let flavor = u32_at(4)?;
    let num_tables = u16_at(12)?;

    let mut tables = Vec::with_capacity(num_tables as usize);
    for i in 0..num_tables as usize {
        let entry = 44 + i * 20;
        let tag = u32_at(entry)?;
        let offset = u32_at(entry + 4)? as usize;
        let comp_len = u32_at(entry + 8)? as usize;
        let orig_len = u32_at(entry + 12)? as usize;
        let checksum = u32_at(entry + 16)?;
        let raw = woff.get(offset..offset + comp_len)?;
        let data = if comp_len < orig_len {
            miniz_oxide::inflate::decompress_to_vec_zlib(raw).ok()?
        } else {
            raw.to_vec()
        };
        if data.len() != orig_len {
            return None;
        }
        tables.push((tag, checksum, data));
    }

    let n = tables.len() as u16;
    let pow2 = if n == 0 {
        0
    } else {
        1u16 << (15 - n.leading_zeros())
    };
    let search_range = pow2 * 16;
    let entry_selector = if pow2 == 0 {
        0
    } else {
        pow2.trailing_zeros() as u16
    };
    let mut out = Vec::new();
    out.extend(flavor.to_be_bytes());
    out.extend(n.to_be_bytes());
    out.extend(search_range.to_be_bytes());
    out.extend(entry_selector.to_be_bytes());
    out.extend((n * 16 - search_range).to_be_bytes());
    let mut offset = 12 + 16 * tables.len();
    for (tag, checksum, data) in &tables {
        out.extend(tag.to_be_bytes());
        out.extend(checksum.to_be_bytes());
        out.extend((offset as u32).to_be_bytes());
        out.extend((data.len() as u32).to_be_bytes());
        offset += data.len().next_multiple_of(4);
    }
    for (_, _, data) in &tables {
        out.extend(data);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_characters() {
        assert_eq!(font_character("\\E001"), Some('\u{E001}'));
        assert_eq!(font_character("x"), Some('x'));
        assert_eq!(font_character("\\zz"), None);
    }

    #[test]
    fn woff_round_trip() {
        // A WOFF with one stored table and one zlib-compressed table.
        let plain = b"abcdabcdabcdabcdabcdabcdabcdabcd".to_vec();
        let packed = miniz_oxide::deflate::compress_to_vec_zlib(&plain, 9);
        let stored = b"xyz".to_vec();
        let mut woff = Vec::new();
        woff.extend(b"wOFF");
        woff.extend(0x0001_0000u32.to_be_bytes());
        woff.extend([0u8; 4]);
        woff.extend(2u16.to_be_bytes());
        woff.resize(44, 0);
        let data_start = 44 + 2 * 20;
        let mut entries = Vec::new();
        let mut body: Vec<u8> = Vec::new();
        for (tag, orig, raw) in [(b"aaaa", &stored, &stored), (b"bbbb", &plain, &packed)] {
            entries.extend(tag);
            entries.extend(((data_start + body.len()) as u32).to_be_bytes());
            entries.extend((raw.len() as u32).to_be_bytes());
            entries.extend((orig.len() as u32).to_be_bytes());
            entries.extend(0u32.to_be_bytes());
            body.extend(raw.iter());
        }
        woff.extend(entries);
        woff.extend(body);

        let sfnt = woff_to_sfnt(&woff).unwrap_or_default();
        assert_eq!(&sfnt[..4], &0x0001_0000u32.to_be_bytes());
        assert_eq!(&sfnt[4..6], &2u16.to_be_bytes());
        // Tables follow the 12-byte header and two 16-byte records, 4-byte aligned.
        assert_eq!(&sfnt[44..47], b"xyz");
        assert_eq!(&sfnt[48..48 + plain.len()], plain.as_slice());
    }

    #[test]
    fn resolves_names_extensions_and_languages() {
        let json: Value = serde_json::json!({
            "file": "default",
            "fileNames": { "Cargo.toml": "cargo" },
            "fileExtensions": { "d.ts": "dts", "toml": "toml" },
            "languageIds": { "rust": "rust" },
            "folder": "dir",
            "folderNames": { "src": "src" },
            "light": { "languageIds": { "rust": "rust_light" } }
        });
        let def = |n: &str| (n.to_owned(), IconDef::Image(PathBuf::from(n)));
        let theme = IconTheme {
            name: "t".into(),
            defs: [
                "default",
                "cargo",
                "dts",
                "toml",
                "rust",
                "rust_light",
                "dir",
                "src",
            ]
            .into_iter()
            .map(def)
            .collect(),
            dark: Associations::read(&json),
            light: json
                .get("light")
                .map(Associations::read)
                .unwrap_or_default(),
            fonts: Vec::new(),
            hides_arrows: false,
            language_by_extension: [("rs".to_owned(), "rust".to_owned())].into(),
            language_by_name: HashMap::new(),
            images: Mutex::default(),
        };
        let name = |d: Option<&IconDef>| match d {
            Some(IconDef::Image(p)) => p.display().to_string(),
            _ => String::new(),
        };
        assert_eq!(name(theme.file("cargo.toml", false)), "cargo");
        assert_eq!(name(theme.file("other.toml", false)), "toml");
        assert_eq!(name(theme.file("x.d.ts", false)), "dts");
        assert_eq!(name(theme.file("main.rs", false)), "rust");
        assert_eq!(name(theme.file("main.rs", true)), "rust_light");
        assert_eq!(name(theme.file("README", false)), "default");
        assert_eq!(name(theme.folder("src", false, false)), "src");
        assert_eq!(name(theme.folder("docs", true, false)), "dir");
    }
}
