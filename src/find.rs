//! Text matching shared by the editor's find bar and the search view: a query with VS Code's
//! three options (match case, whole word, regular expression), the input field that edits it,
//! and the find bar drawn over the editor.

use std::ops::Range;

use egui::text::{CCursor, CCursorRange};
use egui::{Id, Key, Modifiers, Rect, Response, RichText, Sense, Stroke, Ui, vec2};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::icons;
use crate::theme::Tokens;

/// No more matches are looked for past this many (VS Code stops at 20 000 too).
pub const MAX_MATCHES: usize = 20_000;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Query {
    pub text: String,
    pub case: bool,
    pub word: bool,
    pub regex: bool,
}

impl Query {
    /// The pattern to look for: None for an empty query, an error for a bad regular expression.
    pub fn compile(&self) -> Option<Result<Regex, String>> {
        if self.text.is_empty() {
            return None;
        }
        let mut pattern = if self.regex {
            self.text.clone()
        } else {
            regex::escape(&self.text)
        };
        if self.word {
            pattern = format!(r"\b(?:{pattern})\b");
        }
        Some(
            RegexBuilder::new(&pattern)
                .case_insensitive(!self.case)
                .multi_line(true)
                .build()
                .map_err(|e| {
                    let msg = e.to_string();
                    let last = msg.lines().rev().find(|l| !l.trim().is_empty());
                    last.unwrap_or("invalid regular expression")
                        .trim_start_matches("error: ")
                        .to_owned()
                }),
        )
    }

    /// The text, and the options as the field's buttons.
    pub fn parts(&mut self) -> (&mut String, [Toggle<'_>; 3]) {
        (
            &mut self.text,
            [
                Toggle("case-sensitive", &mut self.case, "Match Case (Alt+C)"),
                Toggle("whole-word", &mut self.word, "Match Whole Word (Alt+W)"),
                Toggle("regex", &mut self.regex, "Use Regular Expression (Alt+R)"),
            ],
        )
    }

    /// Alt+C, Alt+W and Alt+R turn the options on and off, as in VS Code.
    pub fn option_keys(&mut self, ui: &Ui) {
        ui.input_mut(|i| {
            if i.consume_key(Modifiers::ALT, Key::C) {
                self.case = !self.case;
            }
            if i.consume_key(Modifiers::ALT, Key::W) {
                self.word = !self.word;
            }
            if i.consume_key(Modifiers::ALT, Key::R) {
                self.regex = !self.regex;
            }
        });
    }
}

/// Byte ranges of the matches of `re` in `text`, at most [`MAX_MATCHES`]; empty ones are skipped.
pub fn find_all(re: &Regex, text: &str) -> Vec<Range<usize>> {
    re.find_iter(text)
        .filter(|m| !m.is_empty())
        .take(MAX_MATCHES)
        .map(|m| m.range())
        .collect()
}

/// The text that replaces the match at `range`: with a regular expression, `$1` and `${name}`
/// are its groups.
pub fn replacement(
    re: &Regex,
    query: &Query,
    text: &str,
    range: Range<usize>,
    with: &str,
) -> String {
    if !query.regex {
        return with.to_owned();
    }
    match re.captures_at(text, range.start) {
        Some(caps) if caps.get(0).map(|m| m.range()) == Some(range) => {
            let mut out = String::new();
            caps.expand(with, &mut out);
            out
        }
        _ => with.to_owned(),
    }
}

/// `text` with every match replaced, and how many there were.
pub fn replace_all(re: &Regex, query: &Query, text: &str, with: &str) -> (String, usize) {
    let ranges = find_all(re, text);
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for r in &ranges {
        out.push_str(text.get(last..r.start).unwrap_or_default());
        out.push_str(&replacement(re, query, text, r.clone(), with));
        last = r.end;
    }
    out.push_str(text.get(last..).unwrap_or_default());
    (out, ranges.len())
}

/// An option button inside a field (match case, whole word, regular expression, preserve…).
pub fn option_button(ui: &mut Ui, icon: &str, on: bool, tooltip: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
    if on {
        ui.painter()
            .rect_filled(rect, t.radius_sm, t.accent.gamma_multiply(0.25));
        ui.painter().rect_stroke(
            rect,
            t.radius_sm,
            Stroke::new(1.0, t.accent),
            egui::StrokeKind::Inside,
        );
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, t.radius_sm, t.hover);
    }
    let tint = if on || resp.hovered() { t.text } else { t.icon };
    icons::paint(ui, rect, icon, 15.0, tint);
    resp.on_hover_text(tooltip)
}

/// An option inside a field: its icon, whether it's on, its tooltip.
pub struct Toggle<'a>(pub &'static str, pub &'a mut bool, pub &'static str);

/// A one-line input in a box, with `options` drawn inside its right end.
/// `select_all`: focus it with its text selected (the caller asks until it has the focus).
pub fn field(
    ui: &mut Ui,
    id: Id,
    text: &mut String,
    hint: &str,
    error: bool,
    select_all: bool,
    options: &mut [Toggle<'_>],
) -> Response {
    let t = Tokens::get(ui.ctx());
    if select_all {
        let mut state = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::two(
            CCursor::new(0),
            CCursor::new(text.chars().count()),
        )));
        state.store(ui.ctx(), id);
    }
    let focused = ui.memory(|m| m.has_focus(id));
    let border = if error {
        t.danger
    } else if focused {
        t.accent
    } else {
        t.field_border
    };
    let width = ui.available_width();
    egui::Frame::new()
        .fill(t.field)
        .stroke(Stroke::new(1.0, border))
        .corner_radius(t.radius_sm)
        .inner_margin(egui::Margin {
            left: 6,
            right: 2,
            top: 2,
            bottom: 2,
        })
        .show(ui, |ui| {
            ui.set_width(width - 10.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                let options_w = options.len() as f32 * 21.0;
                let resp = ui.add(
                    egui::TextEdit::singleline(text)
                        .id(id)
                        .hint_text(RichText::new(hint).color(t.text_faint))
                        .frame(egui::Frame::NONE)
                        .margin(egui::Margin::symmetric(0, 2))
                        .desired_width((ui.available_width() - options_w).max(40.0)),
                );
                if select_all {
                    resp.request_focus();
                }
                for Toggle(icon, on, tip) in options.iter_mut() {
                    if option_button(ui, icon, **on, tip).clicked() {
                        **on = !**on;
                    }
                }
                resp
            })
            .inner
        })
        .inner
}

/// The editor's find bar: what to find and what to replace it with, shared by every file.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FindBar {
    #[serde(skip)]
    pub open: bool,
    pub replace_open: bool,
    pub query: Query,
    pub replace: String,
    /// Focus the find field (its text selected) on the next frame.
    #[serde(skip)]
    pub focus: bool,
}

/// What the find bar asks the editor to do.
pub enum FindAction {
    Next,
    Previous,
    ReplaceOne,
    ReplaceAll,
    Close,
}

impl FindBar {
    pub fn find_id() -> Id {
        Id::new("find-bar-find")
    }

    pub fn replace_id() -> Id {
        Id::new("find-bar-replace")
    }

    /// Whether the keyboard is in one of the bar's fields.
    pub fn has_focus(ctx: &egui::Context) -> bool {
        ctx.memory(|m| m.has_focus(Self::find_id()) || m.has_focus(Self::replace_id()))
    }

    /// Draws the bar at the top right of `editor`. `status`: "3 of 12", "No results"…;
    /// `error`: the query is a bad regular expression.
    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        editor: Rect,
        status: &str,
        error: bool,
    ) -> Option<FindAction> {
        let t = Tokens::get(ctx);
        let width = 430.0_f32.min(editor.width() - 24.0).max(260.0);
        let mut action = None;
        egui::Area::new(Id::new("find-bar"))
            .fixed_pos(editor.right_top() + vec2(-width - 18.0, 0.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(t.card)
                    .stroke(Stroke::new(1.0, t.field_border))
                    .corner_radius(egui::CornerRadius {
                        nw: 0,
                        ne: 0,
                        sw: t.radius as u8,
                        se: t.radius as u8,
                    })
                    .shadow(egui::Shadow {
                        offset: [0, 4],
                        blur: 12,
                        spread: 0,
                        color: t.shadow,
                    })
                    .inner_margin(egui::Margin::symmetric(4, 4))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.spacing_mut().item_spacing = vec2(2.0, 4.0);
                        self.query.option_keys(ui);
                        if Self::has_focus(ui.ctx())
                            && ui.input_mut(|i| {
                                i.consume_key(Modifiers::COMMAND | Modifiers::ALT, Key::Enter)
                            })
                        {
                            action = Some(FindAction::ReplaceAll);
                        }
                        ui.horizontal(|ui| {
                            let chevron = if self.replace_open {
                                "chevron-down"
                            } else {
                                "chevron-right"
                            };
                            if icons::small_button(ui, chevron, "Toggle Replace (Ctrl+H)").clicked()
                            {
                                self.replace_open = !self.replace_open;
                            }
                            ui.add_space(2.0);
                            let buttons_w = 3.0 * 22.0 + 70.0;
                            let resp = ui
                                .allocate_ui(vec2(ui.available_width() - buttons_w, 26.0), |ui| {
                                    let (text, mut toggles) = self.query.parts();
                                    field(
                                        ui,
                                        Self::find_id(),
                                        text,
                                        "Find",
                                        error,
                                        self.focus,
                                        &mut toggles,
                                    )
                                })
                                .inner;
                            // A new bar's first frame only measures it: ask until it's taken.
                            self.focus &= !resp.has_focus();
                            if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                                let back = ui.input(|i| i.modifiers.shift);
                                action = Some(if back {
                                    FindAction::Previous
                                } else {
                                    FindAction::Next
                                });
                                resp.request_focus();
                            }
                            let color = if status == "No results" && !self.query.text.is_empty() {
                                t.danger
                            } else {
                                t.text_dim
                            };
                            let (rect, _) =
                                ui.allocate_exact_size(vec2(68.0, 20.0), Sense::hover());
                            ui.painter().with_clip_rect(rect).text(
                                rect.left_center() + vec2(4.0, 0.0),
                                egui::Align2::LEFT_CENTER,
                                status,
                                egui::FontId::proportional(11.5),
                                color,
                            );
                            if icons::small_button(ui, "arrow-up", "Previous Match (Shift+Enter)")
                                .clicked()
                            {
                                action = Some(FindAction::Previous);
                            }
                            if icons::small_button(ui, "arrow-down", "Next Match (Enter)").clicked()
                            {
                                action = Some(FindAction::Next);
                            }
                            if icons::small_button(ui, "x", "Close (Escape)").clicked() {
                                action = Some(FindAction::Close);
                            }
                        });
                        if self.replace_open {
                            ui.horizontal(|ui| {
                                ui.add_space(24.0);
                                let resp = ui
                                    .allocate_ui(
                                        vec2(ui.available_width() - 3.0 * 22.0 - 70.0, 26.0),
                                        |ui| {
                                            field(
                                                ui,
                                                Self::replace_id(),
                                                &mut self.replace,
                                                "Replace",
                                                false,
                                                false,
                                                &mut [],
                                            )
                                        },
                                    )
                                    .inner;
                                if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                                    action = Some(FindAction::ReplaceOne);
                                    resp.request_focus();
                                }
                                ui.add_space(2.0);
                                if icons::small_button(ui, "replace", "Replace (Enter)").clicked() {
                                    action = Some(FindAction::ReplaceOne);
                                }
                                if icons::small_button(
                                    ui,
                                    "replace-all",
                                    "Replace All (Ctrl+Alt+Enter)",
                                )
                                .clicked()
                                {
                                    action = Some(FindAction::ReplaceAll);
                                }
                            });
                        }
                    });
            });
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(text: &str, case: bool, word: bool, regex: bool) -> Query {
        Query {
            text: text.to_owned(),
            case,
            word,
            regex,
        }
    }

    fn compiled(q: &Query) -> Option<Regex> {
        q.compile().and_then(Result::ok)
    }

    #[test]
    fn options_change_what_matches() {
        let text = "Foo foo food (foo)";
        let count = |q: Query| compiled(&q).map_or(0, |re| find_all(&re, text).len());
        assert_eq!(count(query("foo", false, false, false)), 4);
        assert_eq!(count(query("foo", true, false, false)), 3);
        assert_eq!(count(query("foo", false, true, false)), 3);
        assert_eq!(count(query("(foo)", false, false, false)), 1);
        assert_eq!(count(query("fo+d?", true, false, true)), 3);
        assert!(
            query("(", false, false, true)
                .compile()
                .is_some_and(|r| r.is_err())
        );
        assert!(query("", false, false, false).compile().is_none());
    }

    #[test]
    fn replaces_with_groups() {
        let q = query(r"(\w+)@(\w+)", false, false, true);
        let swapped = compiled(&q).map(|re| replace_all(&re, &q, "a@b, c@d", "$2@$1"));
        assert_eq!(swapped, Some(("b@a, d@c".to_owned(), 2)));
        let lit = query("$1", false, false, false);
        let kept = compiled(&lit).map(|re| replace_all(&re, &lit, "x$1y", "$0").0);
        assert_eq!(kept.as_deref(), Some("x$0y"));
    }
}
