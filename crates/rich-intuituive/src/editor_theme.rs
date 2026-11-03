//! A text area's look as one theme, and bracket matching (#814): Textual's
//! `TextAreaTheme` and its matched bracket.
//!
//! An [`EditorTheme`] styles the parts of an editor together: the text's
//! base, the gutter, the caret, the caret's line, the selection and the
//! matched bracket. Give one to a text area ([`Node::editor_theme`]), or
//! put its styles in the app's [`Theme`] under the `editor.*` names
//! ([`EditorTheme::styles`], [`EditorTheme::apply`]) so every text area
//! takes them. A part left unset falls back to the theme's `editor.*`
//! style, then to the text area's own `text_area.*` one.

use rich::style::Style;
use rich_interact::edit::{Buffer, Pos};

use crate::app::Theme;
use crate::node::Node;

/// How far, in lines either way, a bracket's match is looked for.
const BRACKET_LINES: usize = 2_000;

/// A text area's look: each part a style, or `None` for the app theme's.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::EditorTheme;
///
/// let monokai = EditorTheme::named("monokai").unwrap();
/// assert!(monokai.gutter.is_some());
/// // Into the app's theme, for every text area:
/// let theme = monokai.apply(Theme::dark());
/// assert!(theme.styles.iter().any(|(name, _)| name == "editor.bracket"));
/// // Or on one area:
/// let app = App::new(move || text_area(signal("f(a)".into())).editor_theme(monokai.clone()));
/// # drop(app);
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct EditorTheme {
    /// The text and the empty rows (`editor.base`).
    pub base: Option<Style>,
    /// The line numbers (`editor.gutter`).
    pub gutter: Option<Style>,
    /// The caret drawn in the cells (`editor.cursor`).
    pub cursor: Option<Style>,
    /// The caret's line, with [`cursor_line`](Node::cursor_line)
    /// (`editor.cursor_line`).
    pub cursor_line: Option<Style>,
    /// The selection (`editor.selection`).
    pub selection: Option<Style>,
    /// The bracket at the caret and its match, with
    /// [`match_brackets`](Node::match_brackets) (`editor.bracket`).
    pub bracket: Option<Style>,
}

/// A part of an editor, for its style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    Base,
    Gutter,
    Cursor,
    CursorLine,
    Selection,
    Bracket,
}

impl Part {
    /// Its name in the app's theme.
    fn name(self) -> &'static str {
        match self {
            Part::Base => "editor.base",
            Part::Gutter => "editor.gutter",
            Part::Cursor => "editor.cursor",
            Part::CursorLine => "editor.cursor_line",
            Part::Selection => "editor.selection",
            Part::Bracket => "editor.bracket",
        }
    }
}

fn parse(definition: &str) -> Option<Style> {
    Style::parse(definition).ok()
}

impl EditorTheme {
    /// No part set: every part comes from the app's theme.
    pub fn new() -> EditorTheme {
        EditorTheme::default()
    }

    /// The presets' names, for a picker.
    pub fn names() -> &'static [&'static str] {
        &["monokai", "dracula", "vscode_dark", "github_light"]
    }

    /// A preset by name, after Textual's: `monokai`, `dracula`,
    /// `vscode_dark` and `github_light`.
    pub fn named(name: &str) -> Option<EditorTheme> {
        let theme = |base, gutter, cursor, line, selection, bracket| EditorTheme {
            base: parse(base),
            gutter: parse(gutter),
            cursor: parse(cursor),
            cursor_line: parse(line),
            selection: parse(selection),
            bracket: parse(bracket),
        };
        Some(match name {
            "monokai" => theme(
                "#f8f8f2 on #272822",
                "#90908a on #272822",
                "#272822 on #f8f8f0",
                "on #3e3d32",
                "on #65686a",
                "bold on #838889",
            ),
            "dracula" => theme(
                "#f8f8f2 on #1e1f35",
                "#6272a4 on #282a36",
                "#282a36 on #f8f8f0",
                "on #282b45",
                "on #44475a",
                "bold underline on #99999d",
            ),
            "vscode_dark" => theme(
                "#cccccc on #1f1f1f",
                "#6e7681 on #1f1f1f",
                "#1e1e1e on #f0f0f0",
                "on #2b2b2b",
                "on #264f78",
                "bold on #3a3a3a",
            ),
            "github_light" => theme(
                "#24292e on #f0f0f0",
                "#bbbbbb on #f0f0f0",
                "#fafbfc on #24292e",
                "on #ebebeb",
                "on #c8c8fa",
                "bold underline #24292e",
            ),
            _ => return None,
        })
    }

    /// The parts set, by their `editor.*` names, for the app's theme.
    pub fn styles(&self) -> Vec<(String, Style)> {
        [
            (Part::Base, &self.base),
            (Part::Gutter, &self.gutter),
            (Part::Cursor, &self.cursor),
            (Part::CursorLine, &self.cursor_line),
            (Part::Selection, &self.selection),
            (Part::Bracket, &self.bracket),
        ]
        .into_iter()
        .filter_map(|(part, style)| Some((part.name().to_string(), style.clone()?)))
        .collect()
    }

    /// `theme` with this editor theme's parts in it, for every text area.
    pub fn apply(&self, theme: Theme) -> Theme {
        self.styles()
            .into_iter()
            .fold(theme, |theme, (name, style)| theme.style(&name, style))
    }

    fn part(&self, part: Part) -> Option<&Style> {
        match part {
            Part::Base => self.base.as_ref(),
            Part::Gutter => self.gutter.as_ref(),
            Part::Cursor => self.cursor.as_ref(),
            Part::CursorLine => self.cursor_line.as_ref(),
            Part::Selection => self.selection.as_ref(),
            Part::Bracket => self.bracket.as_ref(),
        }
    }
}

/// A part's style: the area's own editor theme's, else the app theme's
/// `editor.*`, else its `text_area.*` style (`old`, with `fallback`).
pub(crate) fn part_style(
    console: &rich::Console,
    own: Option<&EditorTheme>,
    part: Part,
    old: &str,
    fallback: &str,
) -> Style {
    if let Some(style) = own.and_then(|theme| theme.part(part)) {
        return style.clone();
    }
    console
        .get_style(&rich::style::StyleType::Name(part.name().to_string()))
        .ok()
        .unwrap_or_else(|| crate::widget::theme_style(console, old, fallback))
}

/// The base part, only where something sets it.
pub(crate) fn base_style(console: &rich::Console, own: Option<&EditorTheme>) -> Option<Style> {
    own.and_then(|theme| theme.base.clone()).or_else(|| {
        console
            .get_style(&rich::style::StyleType::Name(Part::Base.name().to_string()))
            .ok()
    })
}

/// The bracket's partner, and which way it is looked for.
fn partner(c: char) -> Option<(char, bool)> {
    Some(match c {
        '(' => (')', true),
        '[' => (']', true),
        '{' => ('}', true),
        ')' => ('(', false),
        ']' => ('[', false),
        '}' => ('{', false),
        _ => return None,
    })
}

/// The bracket at `caret` (or, failing that, just before it) and its match,
/// as positions in `buffer`: nested pairs of the same kind are counted,
/// within [`BRACKET_LINES`] lines. `None` when the caret is at no bracket
/// or it has no match.
pub(crate) fn matching_bracket(buffer: &Buffer, caret: Pos) -> Option<(Pos, Pos)> {
    let line = buffer.line(caret.line);
    let at_caret = line[caret.at..].chars().next().map(|c| (caret.at, c));
    let before = line[..caret.at].char_indices().next_back();
    let (at, open) = [at_caret, before]
        .into_iter()
        .flatten()
        .find(|(_, c)| partner(*c).is_some())?;
    let (close, forward) = partner(open)?;
    let mut depth = 0usize;
    let count = buffer.line_count();
    let lines: Box<dyn Iterator<Item = usize>> = if forward {
        Box::new(caret.line..count.min(caret.line + BRACKET_LINES))
    } else {
        Box::new((caret.line.saturating_sub(BRACKET_LINES)..=caret.line).rev())
    };
    for index in lines {
        let text = buffer.line(index);
        let chars: Box<dyn Iterator<Item = (usize, char)>> = match (forward, index == caret.line) {
            (true, true) => Box::new(text[at..].char_indices().map(move |(i, c)| (i + at, c))),
            (true, false) => Box::new(text.char_indices()),
            (false, true) => Box::new(text[..at + open.len_utf8()].char_indices().rev()),
            (false, false) => Box::new(text.char_indices().rev()),
        };
        for (i, c) in chars {
            if c == open {
                depth += 1;
            } else if c == close {
                depth -= 1;
                if depth == 0 {
                    let here = Pos {
                        line: caret.line,
                        at,
                    };
                    return Some((here, Pos { line: index, at: i }));
                }
            }
        }
    }
    None
}

impl Node {
    /// On a [`text_area`](crate::text_area): style the bracket at the caret
    /// and the one that matches it (`()`, `[]`, `{}`, nested pairs
    /// counted), in the [editor theme](EditorTheme)'s bracket style
    /// (`editor.bracket`, else `text_area.bracket`, bold and underlined
    /// unless the theme names it). Textual's matched bracket.
    pub fn match_brackets(self) -> Node {
        self.editing(|e| e.match_brackets = true)
    }

    /// On a [`text_area`](crate::text_area): draw it with `theme`'s parts,
    /// over the app theme's `editor.*` styles.
    pub fn editor_theme(self, theme: EditorTheme) -> Node {
        self.editing(move |e| e.editor_theme = Some(theme))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str, line: usize, at: usize) -> Option<(Pos, Pos)> {
        let mut buffer = Buffer::new(text);
        buffer.set_caret(Pos { line, at }, false);
        matching_bracket(&buffer, buffer.caret())
    }

    #[test]
    fn brackets_match_across_nesting_and_lines() {
        // The caret on `(` in `f(a[b])`: the `)` at 6.
        assert_eq!(at("f(a[b])", 0, 1).map(|(_, m)| m.at), Some(6));
        assert_eq!(at("f(a[b])", 0, 3).map(|(_, m)| m.at), Some(5));
        // Backwards, from `)`, and from just after it.
        assert_eq!(at("f(a[b])", 0, 6).map(|(_, m)| m.at), Some(1));
        assert_eq!(at("f(a[b])", 0, 7).map(|(_, m)| m.at), Some(1));
        assert_eq!(
            at("fn x() {\n  (1)\n}", 0, 7).map(|(_, m)| (m.line, m.at)),
            Some((2, 0))
        );
        assert_eq!(at("abc", 0, 1), None);
        assert_eq!(at("(()", 0, 0), None);
    }

    #[test]
    fn presets_name_every_part() {
        for name in EditorTheme::names() {
            let theme = EditorTheme::named(name).unwrap();
            assert_eq!(theme.styles().len(), 6, "{name}");
        }
        assert!(EditorTheme::named("nope").is_none());
    }
}
