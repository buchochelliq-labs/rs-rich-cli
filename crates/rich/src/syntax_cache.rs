//! Optional repository-specific parser reuse; see DIVERGENCES §23.
//! The default mirror uses Syntect's uncached HighlightLines path.

use std::collections::HashMap;
use syntect::highlighting::{HighlightIterator, HighlightState, Highlighter, Style, Theme};
use syntect::parsing::{ParseState, ScopeStack, ScopeStackOp, SyntaxReference, SyntaxSet};

pub(super) struct CachedHighlighter<'theme, 'code> {
    highlighter: Highlighter<'theme>,
    highlight_state: HighlightState,
    parse_state: ParseState,
    reference_state: Option<ParseState>,
    first_line: bool,
    cache: HashMap<&'code str, Vec<(usize, ScopeStackOp)>>,
}

impl<'theme, 'code> CachedHighlighter<'theme, 'code> {
    pub(super) fn new(syntax: &SyntaxReference, theme: &'theme Theme) -> Self {
        let highlighter = Highlighter::new(theme);
        let highlight_state = HighlightState::new(&highlighter, ScopeStack::new());
        Self {
            highlighter,
            highlight_state,
            parse_state: ParseState::new(syntax),
            reference_state: None,
            first_line: true,
            cache: HashMap::new(),
        }
    }

    pub(super) fn highlight_line(
        &mut self,
        line: &'code str,
        syntaxes: &SyntaxSet,
    ) -> Result<Vec<(Style, &'code str)>, syntect::Error> {
        let at_reference = self.reference_state.as_ref() == Some(&self.parse_state);
        if at_reference {
            if let Some(ops) = self.cache.get(line) {
                return Ok(HighlightIterator::new(
                    &mut self.highlight_state,
                    ops,
                    line,
                    &self.highlighter,
                )
                .collect());
            }
        }
        let candidate = at_reference && line.len() <= 4096 && self.cache.len() < 64;
        let first_line = std::mem::replace(&mut self.first_line, false);
        let ops = self.parse_state.parse_line(line, syntaxes)?;
        // Take at most one snapshot, derived from a small first source line.
        // Never clone later states: they may retain multi-megabyte openers.
        // Cache entries contain operations only, never captured parser states.
        if first_line && line.len() <= 4096 {
            self.reference_state = Some(self.parse_state.clone());
        }
        let ranges =
            HighlightIterator::new(&mut self.highlight_state, &ops, line, &self.highlighter)
                .collect();
        if candidate && self.reference_state.as_ref() == Some(&self.parse_state) && ops.len() <= 256
        {
            self.cache.insert(line, ops);
        }
        Ok(ranges)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{syntax_set, theme_set, DEFAULT_THEME};
    use super::*;
    use syntect::util::LinesWithEndings;
    #[test]
    fn cached_parsing_matches_live_syntect_states() {
        use syntect::easy::HighlightLines;
        // Repeated bytes can occur in radically different parser states. Include
        // first-line rules, captured delimiters and embedded language contexts.
        let cases = [
            (
                "rust",
                "let x = 1;\n/*\nlet x = 1;\n*/\nlet x = 1;\nlet x = 1;\n",
            ),
            (
                "rust",
                "let s = r###\"\nvalue\n\"##;\nvalue\n\"###;\nvalue\nvalue",
            ),
            (
                "python",
                "#!/usr/bin/env python3\nx = 1\ns = \"\"\"\nx = 1\n\"\"\"\nx = 1\nx = 1\n",
            ),
            ("ruby", "s = <<ONE\nvalue\nTWO\nvalue\nONE\nvalue\nvalue\n"),
            (
                "html",
                "value\n<script>\nvalue\nlet x = 1;\nlet x = 1;\n</script>\nvalue\nvalue\n",
            ),
            (
                "yaml",
                "key: |\n  value\n  value\nother: value\nother: value",
            ),
        ];
        let syntaxes = syntax_set();
        for theme in theme_set().themes.values() {
            for (language, code) in cases {
                let syntax = syntaxes.find_syntax_by_token(language).unwrap();
                let mut cached = CachedHighlighter::new(syntax, theme);
                let mut direct = HighlightLines::new(syntax, theme);
                for line in LinesWithEndings::from(code) {
                    assert_eq!(
                        cached.highlight_line(line, syntaxes).unwrap(),
                        direct.highlight_line(line, syntaxes).unwrap(),
                        "language {language}, line {line:?}",
                    );
                }
            }
        }
    }

    #[test]
    fn cached_parsing_preserves_output_after_entry_limit_and_large_lines() {
        use syntect::easy::HighlightLines;
        let syntaxes = syntax_set();
        let theme = &theme_set().themes[DEFAULT_THEME];
        let syntax = syntaxes.find_syntax_by_token("rust").unwrap();
        let mut code = String::from("let initialize = 0;\n");
        for index in 0..100 {
            code.push_str(&format!("let value_{index} = {index};\n"));
        }
        code.push_str(&format!("// {}\n", "x".repeat(8192)));
        code.push_str("let value_0 = 0;\nlet value_0 = 0;");
        let mut cached = CachedHighlighter::new(syntax, theme);
        let mut direct = HighlightLines::new(syntax, theme);
        for line in LinesWithEndings::from(&code) {
            assert_eq!(
                cached.highlight_line(line, syntaxes).unwrap(),
                direct.highlight_line(line, syntaxes).unwrap()
            );
        }
        assert_eq!(cached.cache.len(), 64);
    }

    #[test]
    fn large_heredoc_capture_never_enters_the_cache() {
        use syntect::easy::HighlightLines;
        let syntaxes = syntax_set();
        let syntax = syntaxes.find_syntax_by_token("ruby").unwrap();
        let theme = &theme_set().themes[DEFAULT_THEME];
        for prefix in ["", "value = 1\n"] {
            let mut code = format!("{prefix}value = <<END; # {}\n", "x".repeat(16 * 1024));
            for index in 0..80 {
                code.push_str(&format!("body {index}\n"));
            }
            let mut cached = CachedHighlighter::new(syntax, theme);
            let mut direct = HighlightLines::new(syntax, theme);
            for (index, line) in LinesWithEndings::from(&code).enumerate() {
                assert_eq!(
                    cached.highlight_line(line, syntaxes).unwrap(),
                    direct.highlight_line(line, syntaxes).unwrap()
                );
                if prefix.is_empty() {
                    assert!(
                        cached.reference_state.is_none(),
                        "oversized first line must disable snapshots"
                    );
                } else if index > 0 {
                    assert_ne!(
                        cached.reference_state.as_ref(),
                        Some(&cached.parse_state),
                        "fixture must enter captured heredoc state"
                    );
                }
                assert!(
                    cached.cache.is_empty(),
                    "captured opener/body must not enter cache"
                );
            }
            for line in ["END\n", "value = 1\n", "value = 1\n"] {
                assert_eq!(
                    cached.highlight_line(line, syntaxes).unwrap(),
                    direct.highlight_line(line, syntaxes).unwrap()
                );
            }
        }
    }
}
