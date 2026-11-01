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
use syntect::highlighting::{
    HighlightIterator, HighlightState, Highlighter as ThemeHighlighter, Theme, ThemeSet,
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

fn default_themes() -> &'static ThemeSet {
    static SET: OnceLock<ThemeSet> = OnceLock::new();
    SET.get_or_init(ThemeSet::load_defaults)
}

/// Loads the syntax definitions and themes on a background thread, so opening the first file
/// doesn't wait for them.
pub fn warm_up() {
    std::thread::spawn(|| {
        syntaxes();
        default_themes();
    });
}

/// syntect's own theme for when VS Code's isn't used.
pub fn default_theme(dark: bool) -> Option<&'static Theme> {
    default_themes().themes.get(if dark {
        "base16-ocean.dark"
    } else {
        "InspiredGitHub"
    })
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
        let Some(theme) = default_theme(true) else {
            return;
        };
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
    fn plain_text_is_one_section() {
        let mut h = Highlighter::default();
        let job = h.layout_job("hello\n", None, None, FontId::monospace(12.0), Color32::RED);
        assert_eq!(job.sections.len(), 1);
        assert!(syntax_for("notes.txt", "").is_none());
    }
}
