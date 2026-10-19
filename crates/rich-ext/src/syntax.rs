//! Warming up syntax highlighting before it is first needed.
//!
//! Not a port of anything upstream: an rs-rich addition. Core's default
//! highlighter ([`SyntectHighlighter`]) loads syntect's bundled grammars
//! and themes the first time anything is highlighted, and compiles each
//! grammar's patterns the first time they are tried. In a debug build that
//! first highlight takes seconds, so the first Markdown preview or `Syntax`
//! in an interactive app stalls a frame. [`preload`] does that work on a
//! worker thread, through the highlighter's public API, so it is already
//! done (or under way) when the first frame asks; the state it warms is
//! the highlighter's own, shared process-wide, so nothing else changes.
//!
//! ```
//! use rich_ext::syntax::preload;
//!
//! // At start-up: the languages the app is likely to show.
//! let warming = preload(&["rust", "markdown", "toml"]);
//! // … build the app; the first highlight finds everything loaded.
//! let took = warming.wait();
//! # let _ = took;
//! ```

use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rich::protocol::CodeHighlighter;
use rich::SyntectHighlighter;

/// A few lines of code-like text: enough to try the main patterns of most
/// grammars (keywords, strings, numbers, comments, brackets).
const SAMPLE: &str =
    "fn main() { let x = \"text\"; # comment\n    // comment\n    return 1.5 + [a, b] -> c; }\n";

/// A warm-up running on its own thread (see [`preload`]). Dropping it lets
/// the thread finish on its own.
#[derive(Debug)]
pub struct Preload {
    handle: JoinHandle<Duration>,
}

impl Preload {
    /// Whether the warm-up has finished.
    pub fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }

    /// Wait for the warm-up to finish; how long it took on its thread.
    /// A warm-up that panicked (it cannot, short of a bug in the
    /// highlighter) took [`Duration::ZERO`].
    pub fn wait(self) -> Duration {
        self.handle.join().unwrap_or_default()
    }
}

/// Warm up core's default highlighter on a worker thread: load its
/// grammars and themes, and highlight a short sample in each of
/// `languages` (names or extensions, as `Syntax` takes them: `"rust"`,
/// `"md"`) with the default theme, so each grammar's patterns are compiled.
/// An unknown language warms the plain-text grammar and is otherwise
/// ignored. Returns at once.
pub fn preload(languages: &[&str]) -> Preload {
    preload_with(SyntectHighlighter::shared(), languages, &[])
}

/// [`preload`] for any highlighter, with `themes` warmed as well as its
/// default one (an unknown theme is skipped).
pub fn preload_with(
    highlighter: Arc<dyn CodeHighlighter>,
    languages: &[&str],
    themes: &[&str],
) -> Preload {
    let languages: Vec<String> = languages.iter().map(|l| l.to_string()).collect();
    let mut themes: Vec<String> = themes.iter().map(|t| t.to_string()).collect();
    themes.insert(0, highlighter.default_theme().to_string());
    let handle = std::thread::Builder::new()
        .name("rich-syntax-preload".into())
        .spawn(move || {
            let start = Instant::now();
            for theme in &themes {
                let _ = highlighter.highlight(SAMPLE, None, theme);
            }
            for language in &languages {
                let _ = highlighter.highlight(SAMPLE, Some(language), &themes[0]);
            }
            start.elapsed()
        })
        .expect("a thread for the syntax preload");
    Preload { handle }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rich::protocol::{HighlightError, HighlightedCode};
    use std::sync::Mutex;

    /// A highlighter that writes down what it was asked.
    #[derive(Default)]
    struct Recorder(Mutex<Vec<(Option<String>, String)>>);

    impl CodeHighlighter for Recorder {
        fn highlight(
            &self,
            _code: &str,
            language: Option<&str>,
            theme: &str,
        ) -> Result<HighlightedCode, HighlightError> {
            self.0
                .lock()
                .unwrap()
                .push((language.map(str::to_string), theme.to_string()));
            Ok(HighlightedCode::default())
        }

        fn default_theme(&self) -> &str {
            "plain"
        }

        fn themes(&self) -> Vec<String> {
            vec!["plain".into(), "dark".into()]
        }

        fn languages(&self) -> Vec<String> {
            Vec::new()
        }
    }

    #[test]
    fn warms_every_theme_then_every_language() {
        let recorder = Arc::new(Recorder::default());
        preload_with(recorder.clone(), &["rust", "toml"], &["dark"]).wait();
        let asked = recorder.0.lock().unwrap().clone();
        let some = |s: &str| Some(s.to_string());
        assert_eq!(
            asked,
            [
                (None, "plain".to_string()),
                (None, "dark".to_string()),
                (some("rust"), "plain".to_string()),
                (some("toml"), "plain".to_string()),
            ]
        );
    }

    #[test]
    fn the_default_highlighter_is_warm_afterwards() {
        let warming = preload(&["rust", "no-such-language"]);
        warming.wait();
        // Loaded: a highlight now takes the fast path (far under the
        // seconds a cold debug build spends loading).
        let start = Instant::now();
        let code = SyntectHighlighter::shared()
            .highlight(
                "let x = 1;\n",
                Some("rust"),
                SyntectHighlighter::shared().default_theme(),
            )
            .unwrap();
        assert!(!code.lines.is_empty());
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}
