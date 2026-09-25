//! The highlighter conformance kit (#526): the shipped syntect adapter passes,
//! and adapters broken in each way it checks are rejected with that check.
#![cfg(feature = "testing")]

use std::sync::Arc;
use std::time::Duration;

use rich::protocol::{
    CodeHighlighter, HighlightError, HighlightSpan, HighlightedCode, HighlightedLine,
};
use rich::{Style, SyntectHighlighter};
use rich_ext::testing::conformance::{self, Options};

#[test]
fn syntect_conforms_with_every_theme() {
    conformance::check_with(
        SyntectHighlighter::shared(),
        Options {
            all_themes: true,
            scaling: true,
        },
    )
    .unwrap_or_else(|error| panic!("{error}"));
}

/// What a broken adapter gets wrong.
#[derive(Clone, Copy)]
enum Fault {
    None,
    OneLine,
    Overlap,
    SplitsChar,
    ThemeFallback,
    StylesUnknownLanguage,
    DefaultNotListed,
    Slow,
    Fails,
}

/// Bolds each line's first word, then breaks in one way.
struct Adapter(Fault);

impl CodeHighlighter for Adapter {
    fn highlight(
        &self,
        code: &str,
        language: Option<&str>,
        theme: &str,
    ) -> Result<HighlightedCode, HighlightError> {
        if theme != "plain" && !matches!(self.0, Fault::ThemeFallback) {
            return Err(HighlightError::UnknownTheme(theme.into()));
        }
        if matches!(self.0, Fault::Fails) {
            return Err(HighlightError::Engine("broken".into()));
        }
        if matches!(self.0, Fault::Slow) && code.len() > 100_000 {
            std::thread::sleep(Duration::from_millis(300));
        }
        let known = matches!(language, Some("rust" | "rs" | "python"))
            || (matches!(self.0, Fault::StylesUnknownLanguage) && language.is_some());
        let bold = Style::parse("bold").unwrap();
        let mut lines: Vec<HighlightedLine> = code
            .split('\n')
            .map(|line| {
                let word = line.find(' ').unwrap_or(line.len());
                let mut spans = Vec::new();
                if known && word > 0 {
                    spans.push(HighlightSpan {
                        range: 0..word,
                        style: bold.clone(),
                    });
                    match self.0 {
                        Fault::Overlap => spans.push(HighlightSpan {
                            range: 0..word,
                            style: bold.clone(),
                        }),
                        Fault::SplitsChar if !line.is_ascii() => {
                            let inside = line
                                .char_indices()
                                .find(|(_, c)| c.len_utf8() > 1)
                                .unwrap()
                                .0
                                + 1;
                            spans = vec![HighlightSpan {
                                range: 0..inside,
                                style: bold.clone(),
                            }];
                        }
                        _ => {}
                    }
                }
                HighlightedLine {
                    spans,
                    newline_style: None,
                }
            })
            .collect();
        if matches!(self.0, Fault::OneLine) {
            lines.truncate(1);
        }
        Ok(HighlightedCode {
            lines,
            ..Default::default()
        })
    }
    fn default_theme(&self) -> &str {
        "plain"
    }
    fn themes(&self) -> Vec<String> {
        if matches!(self.0, Fault::DefaultNotListed) {
            vec!["other".into()]
        } else {
            vec!["plain".into()]
        }
    }
    fn languages(&self) -> Vec<String> {
        vec!["rust".into(), "python".into()]
    }
}

fn failed_checks(fault: Fault) -> Vec<&'static str> {
    match conformance::check(Arc::new(Adapter(fault))) {
        Ok(()) => Vec::new(),
        Err(error) => {
            let mut checks: Vec<_> = error.failures.iter().map(|f| f.check).collect();
            checks.dedup();
            checks
        }
    }
}

#[test]
fn a_correct_adapter_passes() {
    assert_eq!(failed_checks(Fault::None), Vec::<&str>::new());
}

#[test]
fn each_broken_adapter_is_rejected_by_its_check() {
    for (fault, expected) in [
        (Fault::OneLine, "line count"),
        (Fault::Overlap, "spans"),
        (Fault::SplitsChar, "spans"),
        (Fault::ThemeFallback, "unknown theme"),
        (Fault::StylesUnknownLanguage, "unknown language"),
        (Fault::DefaultNotListed, "themes"),
        (Fault::Slow, "scaling"),
        (Fault::Fails, "highlights"),
    ] {
        let checks = failed_checks(fault);
        assert!(
            checks.contains(&expected),
            "expected {expected:?}, got {checks:?}"
        );
    }
}

#[test]
fn the_report_lists_every_failure_with_its_input() {
    let error = conformance::check(Arc::new(Adapter(Fault::OneLine))).unwrap_err();
    let report = error.to_string();
    assert!(report.starts_with("the adapter failed "), "{report}");
    assert!(
        report.contains("- line count: a final newline, language Some(\"rust\")"),
        "{report}"
    );
}
