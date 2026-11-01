//! Syntax colouring for the editor (syntect, pure-Rust regex engine).
//!
//! Highlighting is incremental: each line keeps the parser state it ended in, so an edit only
//! re-colours from the changed line until the state settles back into what it was before
//! (usually the next line), not the whole file.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;

use egui::text::{ByteIndex, LayoutJob, LayoutSection, TextFormat};
use egui::{Color32, FontId};
use serde::{Deserialize, Serialize};
use syntect::highlighting::{
    Color, HighlightIterator, HighlightState, Highlighter as ThemeHighlighter, StyleModifier,
    Theme, ThemeItem, ThemeSettings,
};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};

/// Bigger files stay uncoloured.
const MAX_BYTES: usize = 4 << 20;
/// A line this long (minified code) turns colouring off for its file.
const MAX_LINE: usize = 20_000;

pub fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// Loads the syntax definitions on a background thread, so opening the first file doesn't
/// wait for them.
pub fn warm_up() {
    std::thread::spawn(|| {
        syntaxes();
    });
}

/// CodeCraft's own code colours, for when VS Code's aren't used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyntaxStyle {
    /// Warm: orange keywords, olive strings, gold functions.
    Ember,
    /// Cool: blue keywords, mint strings, violet types.
    Tide,
    /// Bright and balanced: purple keywords, green strings, blue functions, yellow types.
    #[default]
    Orchard,
    /// Mostly plain text: muted keywords and strings, dim comments.
    Quiet,
}

/// A style's colours (0xRRGGBB) for each kind of token; `None` leaves it as plain text.
struct Palette {
    comment: u32,
    keyword: u32,
    operator: Option<u32>,
    string: u32,
    number: u32,
    function: Option<u32>,
    kind: u32,
    macros: u32,
    this: u32,
    namespace: Option<u32>,
}

impl SyntaxStyle {
    /// In the order the settings show them, two to a row.
    pub const ALL: [SyntaxStyle; 4] = [
        SyntaxStyle::Orchard,
        SyntaxStyle::Ember,
        SyntaxStyle::Quiet,
        SyntaxStyle::Tide,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SyntaxStyle::Ember => "Ember",
            SyntaxStyle::Tide => "Tide",
            SyntaxStyle::Orchard => "Orchard",
            SyntaxStyle::Quiet => "Quiet",
        }
    }

    /// The style as a syntect theme, for dark or light backgrounds. Plain text is left
    /// transparent: it takes the editor's text colour.
    pub fn theme(self, dark: bool) -> &'static Theme {
        static THEMES: OnceLock<Vec<Theme>> = OnceLock::new();
        let themes = THEMES.get_or_init(|| {
            Self::ALL
                .iter()
                .flat_map(|s| [s.build(true), s.build(false)])
                .collect()
        });
        let i = Self::ALL.iter().position(|s| *s == self).unwrap_or(0) * 2 + usize::from(!dark);
        &themes[i]
    }

    fn palette(self, dark: bool) -> Palette {
        match (self, dark) {
            (SyntaxStyle::Ember, true) => Palette {
                comment: 0x7d756c,
                keyword: 0xf28c5b,
                operator: Some(0xd9a385),
                string: 0xb5c77a,
                number: 0xd49be0,
                function: Some(0xf2c96b),
                kind: 0x7fc8c2,
                macros: 0xe6828f,
                this: 0xe6828f,
                namespace: Some(0xc9b79c),
            },
            (SyntaxStyle::Ember, false) => Palette {
                comment: 0x8c857c,
                keyword: 0xc2501b,
                operator: Some(0xa0622e),
                string: 0x5e7a1e,
                number: 0x8a3fa0,
                function: Some(0x9a6a00),
                kind: 0x1e7f78,
                macros: 0xb53a4c,
                this: 0xb53a4c,
                namespace: Some(0x7a6648),
            },
            (SyntaxStyle::Tide, true) => Palette {
                comment: 0x6a7a8c,
                keyword: 0x6fa8f7,
                operator: Some(0x8fb3d9),
                string: 0x7fd1b9,
                number: 0xf5b971,
                function: Some(0x82d3f0),
                kind: 0xb49cf5,
                macros: 0xf08fb5,
                this: 0xf08fb5,
                namespace: Some(0xa9bcd6),
            },
            (SyntaxStyle::Tide, false) => Palette {
                comment: 0x7d8a99,
                keyword: 0x1d5fc4,
                operator: Some(0x3f6a99),
                string: 0x0e7c64,
                number: 0xb0620c,
                function: Some(0x0b7aa3),
                kind: 0x6a45c9,
                macros: 0xb8336e,
                this: 0xb8336e,
                namespace: Some(0x4a5f7d),
            },
            (SyntaxStyle::Orchard, true) => Palette {
                comment: 0x6f7782,
                keyword: 0xc38ae6,
                operator: Some(0x5cc9c9),
                string: 0x98c97a,
                number: 0xe8a56a,
                function: Some(0x6eb4f0),
                kind: 0xe9c46a,
                macros: 0x5cc9c9,
                this: 0xe87a7a,
                namespace: Some(0xe9c46a),
            },
            (SyntaxStyle::Orchard, false) => Palette {
                comment: 0x8a9099,
                keyword: 0x8a3fb8,
                operator: Some(0x157f82),
                string: 0x3f7f2a,
                number: 0xb5600e,
                function: Some(0x1f64b8),
                kind: 0x946c00,
                macros: 0x157f82,
                this: 0xc03d3d,
                namespace: Some(0x946c00),
            },
            (SyntaxStyle::Quiet, true) => Palette {
                comment: 0x6b6b6b,
                keyword: 0x8fa8c8,
                operator: None,
                string: 0xa8c49a,
                number: 0xc8a88f,
                function: None,
                kind: 0xb8b0d0,
                macros: 0x8fa8c8,
                this: 0x8fa8c8,
                namespace: None,
            },
            (SyntaxStyle::Quiet, false) => Palette {
                comment: 0x9a9a9a,
                keyword: 0x3d5f8a,
                operator: None,
                string: 0x4e7240,
                number: 0x8a5a35,
                function: None,
                kind: 0x5a4e86,
                macros: 0x3d5f8a,
                this: 0x3d5f8a,
                namespace: None,
            },
        }
    }

    fn build(self, dark: bool) -> Theme {
        let p = self.palette(dark);
        let rules: [(&str, Option<u32>); 12] = [
            ("comment, punctuation.definition.comment", Some(p.comment)),
            (
                "keyword, storage, entity.name.tag, markup.heading",
                Some(p.keyword),
            ),
            ("keyword.operator", p.operator),
            ("string, punctuation.definition.string", Some(p.string)),
            (
                "constant.numeric, constant.language, constant.character, constant.other, markup.bold",
                Some(p.number),
            ),
            (
                "entity.name.function, support.function, variable.function, \
                 meta.function-call.identifier, entity.other.attribute-name",
                p.function,
            ),
            (
                "entity.name.type, entity.name.struct, entity.name.enum, entity.name.trait, \
                 entity.name.class, entity.name.impl, support.type, support.class, \
                 storage.type.primitive, storage.type.numeric",
                Some(p.kind),
            ),
            (
                "entity.name.macro, support.macro, entity.name.function.macro, \
                 meta.annotation, meta.attribute, punctuation.definition.annotation",
                Some(p.macros),
            ),
            ("variable.language", Some(p.this)),
            ("entity.name.namespace, entity.name.module", p.namespace),
            ("markup.inline.raw, markup.raw", Some(p.string)),
            ("invalid", Some(0xe05252)),
        ];
        let color = |rgb: u32| Color {
            r: (rgb >> 16) as u8,
            g: (rgb >> 8) as u8,
            b: rgb as u8,
            a: 0xff,
        };
        Theme {
            name: Some(format!(
                "CodeCraft {} {}",
                self.label(),
                if dark { "Dark" } else { "Light" }
            )),
            settings: ThemeSettings {
                foreground: Some(Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 0,
                }),
                ..Default::default()
            },
            scopes: rules
                .iter()
                .filter_map(|(selector, rgb)| {
                    Some(ThemeItem {
                        scope: selector.parse().ok()?,
                        style: StyleModifier {
                            foreground: Some(color((*rgb)?)),
                            background: None,
                            font_style: None,
                        },
                    })
                })
                .collect(),
            ..Default::default()
        }
    }
}

/// The syntax for a file name, `None` for plain text.
pub fn syntax_for(name: &str, first_line: &str) -> Option<&'static SyntaxReference> {
    let ss = syntaxes();
    let lower = name.to_lowercase();
    let by_ext = lower
        .rsplit_once('.')
        .and_then(|(_, ext)| ss.find_syntax_by_extension(ext));
    by_ext
        .or_else(|| ss.find_syntax_by_extension(&lower))
        .or_else(|| ss.find_syntax_by_first_line(first_line))
        .filter(|s| s.name != "Plain Text")
}

#[derive(Clone)]
struct Line {
    hash: u64,
    /// Parser state after the line.
    end: (ParseState, HighlightState),
    /// (length in bytes, colour) pieces covering the line.
    spans: Vec<(usize, Color32)>,
}

/// A document's colouring, kept between frames.
#[derive(Default)]
pub struct Highlighter {
    /// What the cached lines were coloured for (syntax name, theme name).
    key: Option<(String, String)>,
    lines: Vec<Line>,
    /// The last layout handed out: (text hash, font size, theme name, default colour, job).
    job: Option<(u64, u32, String, Color32, LayoutJob)>,
}

impl Highlighter {
    /// The text laid out for egui, coloured when `syntax` and `theme` are given.
    pub fn layout_job(
        &mut self,
        text: &str,
        syntax: Option<&SyntaxReference>,
        theme: Option<&Theme>,
        font: FontId,
        color: Color32,
    ) -> LayoutJob {
        let hash = hash_of(text);
        let theme_name = theme.and_then(|t| t.name.clone()).unwrap_or_default();
        let size = font.size.to_bits();
        if let Some((h, s, name, c, job)) = &self.job
            && *h == hash
            && *s == size
            && *name == theme_name
            && *c == color
        {
            return job.clone();
        }
        let colored = match (syntax, theme) {
            (Some(syntax), Some(theme)) if text.len() <= MAX_BYTES => {
                self.colour(text, syntax, theme)
            }
            _ => false,
        };
        let job = if colored {
            self.build(text, font, color)
        } else {
            self.lines.clear();
            self.key = None;
            plain(text, font, color)
        };
        self.job = Some((hash, size, theme_name, color, job.clone()));
        job
    }

    /// Re-colours the lines that changed; false when the text can't be coloured.
    fn colour(&mut self, text: &str, syntax: &SyntaxReference, theme: &Theme) -> bool {
        let key = (syntax.name.clone(), theme.name.clone().unwrap_or_default());
        if self.key.as_ref() != Some(&key) {
            self.lines.clear();
            self.key = Some(key);
        }
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        if lines.iter().any(|l| l.len() > MAX_LINE) {
            return false;
        }
        let hashes: Vec<u64> = lines.iter().map(|l| hash_of(l)).collect();
        let n = lines.len();

        let mut old = std::mem::take(&mut self.lines);
        let prefix = old
            .iter()
            .zip(&hashes)
            .take_while(|(o, h)| o.hash == **h)
            .count();
        let tail = old.split_off(prefix);
        let suffix = tail
            .iter()
            .rev()
            .zip(hashes[prefix..].iter().rev())
            .take_while(|(o, h)| o.hash == **h)
            .count();
        let mut out = old;

        let ss = syntaxes();
        let highlighter = ThemeHighlighter::new(theme);
        let (mut parse, mut highlight) = match out.last() {
            Some(l) => l.end.clone(),
            None => (
                ParseState::new(syntax),
                HighlightState::new(&highlighter, ScopeStack::new()),
            ),
        };
        let mut i = prefix;
        while i < n {
            // Inside the unchanged end of the file, once the parser is back in the state the
            // old line started in, the rest of the old colouring still holds.
            if i >= n - suffix {
                let k = i - prefix + tail.len() - (n - prefix);
                if k >= 1 && tail[k - 1].end == (parse.clone(), highlight.clone()) {
                    out.extend_from_slice(&tail[k..]);
                    break;
                }
            }
            let line = lines[i];
            let Ok(ops) = parse.parse_line(line, ss) else {
                return false;
            };
            let mut spans: Vec<(usize, Color32)> = Vec::new();
            for (style, piece) in HighlightIterator::new(&mut highlight, &ops, line, &highlighter) {
                let c = style.foreground;
                let color = Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a);
                match spans.last_mut() {
                    Some((len, last)) if *last == color => *len += piece.len(),
                    _ => spans.push((piece.len(), color)),
                }
            }
            out.push(Line {
                hash: hashes[i],
                end: (parse.clone(), highlight.clone()),
                spans,
            });
            i += 1;
        }
        self.lines = out;
        true
    }

    fn build(&self, text: &str, font: FontId, color: Color32) -> LayoutJob {
        let mut job = LayoutJob {
            text: text.to_owned(),
            ..Default::default()
        };
        job.wrap.max_width = f32::INFINITY;
        let mut at = 0;
        for line in &self.lines {
            for &(len, c) in &line.spans {
                let c = if c.a() == 0 { color } else { c };
                match job.sections.last_mut() {
                    Some(s) if s.format.color == c && s.byte_range.end.0 == at => {
                        s.byte_range.end = ByteIndex(at + len);
                    }
                    _ => job.sections.push(LayoutSection {
                        leading_space: 0.0,
                        byte_range: ByteIndex(at)..ByteIndex(at + len),
                        format: TextFormat::simple(font.clone(), c),
                    }),
                }
                at += len;
            }
        }
        if at < text.len() {
            job.sections.push(LayoutSection {
                leading_space: 0.0,
                byte_range: ByteIndex(at)..ByteIndex(text.len()),
                format: TextFormat::simple(font, color),
            });
        }
        job
    }
}

fn plain(text: &str, font: FontId, color: Color32) -> LayoutJob {
    let mut job = LayoutJob::simple(text.to_owned(), font, color, f32::INFINITY);
    job.wrap.max_width = f32::INFINITY;
    job
}

pub fn hash_of(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colours(job: &LayoutJob) -> Vec<(String, Color32)> {
        job.sections
            .iter()
            .map(|s| {
                let r = s.byte_range.start.0..s.byte_range.end.0;
                (job.text[r].to_owned(), s.format.color)
            })
            .collect()
    }

    #[test]
    fn incremental_matches_full() {
        let theme = SyntaxStyle::Orchard.theme(true);
        let syntax = syntax_for("a.rs", "");
        assert!(syntax.is_some());
        let font = FontId::monospace(12.0);
        let before = "fn main() {\n    let x = 1;\n}\n/* a\n b */\nfn f() {}\n";
        let after = "fn main() {\n    let x = \"/*\";\n}\n/* a\n b */\nfn f() {}\n";
        let mut h = Highlighter::default();
        let first = h.layout_job(before, syntax, Some(theme), font.clone(), Color32::WHITE);
        assert!(first.sections.len() > 3);
        let edited = h.layout_job(after, syntax, Some(theme), font.clone(), Color32::WHITE);
        let fresh =
            Highlighter::default().layout_job(after, syntax, Some(theme), font, Color32::WHITE);
        assert_eq!(colours(&edited), colours(&fresh));
        assert_eq!(edited.text, after);
    }

    #[test]
    fn every_style_parses_and_colours_keywords() {
        let syntax = syntax_for("a.rs", "");
        for style in SyntaxStyle::ALL {
            for dark in [true, false] {
                let theme = style.theme(dark);
                assert_eq!(
                    theme.scopes.len(),
                    if style == SyntaxStyle::Quiet { 9 } else { 12 }
                );
                let job = Highlighter::default().layout_job(
                    "fn x() {}\n",
                    syntax,
                    Some(theme),
                    FontId::monospace(12.0),
                    Color32::WHITE,
                );
                assert_ne!(
                    job.sections.first().map(|s| s.format.color),
                    Some(Color32::WHITE)
                );
            }
        }
    }

    #[test]
    fn plain_text_is_one_section() {
        let mut h = Highlighter::default();
        let job = h.layout_job("hello\n", None, None, FontId::monospace(12.0), Color32::RED);
        assert_eq!(job.sections.len(), 1);
        assert!(syntax_for("notes.txt", "").is_none());
    }
}
