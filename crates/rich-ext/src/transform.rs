//! Composable transforms: rewrite what is about to be rendered, one stage at a
//! time.
//!
//! A [`Transform<T>`] takes a value and returns a changed one: a [`Text`] with
//! some lines dropped, a data `Document` (`data::transform`, with the
//! `jsonpath` feature) narrowed to a JSONPath, a [`TableData`](crate::table::TableData) sorted, a
//! git [`Patch`](crate::diff::git::Patch) limited to some files. A
//! [`Pipeline`] runs named stages in the order they were added, and says which
//! stage failed.
//!
//! Text transforms are the plugin-facing kind: they implement
//! [`TextTransform`] from the plugin contract, so a plugin can register one
//! (see [`ExtensionRegistry::text_pipeline`](crate::ExtensionRegistry::text_pipeline)).
//! Every [`TextTransform`] is also a `Transform<Text>`.
//!
//! ```
//! use rich::{Style, Text};
//! use rich_ext::transform::{HighlightMatches, KeepLines, Pipeline};
//!
//! let pipeline = Pipeline::new()
//!     .then("filter", KeepLines::new("ERROR|WARN").unwrap())
//!     .then("highlight", HighlightMatches::new("ERROR", Style::parse("bold red").unwrap()).unwrap());
//! assert_eq!(pipeline.names(), ["filter", "highlight"]);
//!
//! let log = Text::new("INFO start\nWARN slow\nERROR failed\n");
//! let text = pipeline.apply(log).unwrap();
//! assert_eq!(text.plain(), "WARN slow\nERROR failed\n");
//! ```

use std::fmt;

use rich::{Style, Text};
use rich_plugin_api::PluginError;
pub use rich_plugin_api::TextTransform;

/// Why a transform failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransformError {
    message: String,
}

impl TransformError {
    pub fn new(message: impl Into<String>) -> Self {
        TransformError {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for TransformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TransformError {}

impl From<PluginError> for TransformError {
    fn from(error: PluginError) -> Self {
        TransformError::new(error.to_string())
    }
}

/// Rewrites a `T`.
pub trait Transform<T>: Send + Sync {
    fn apply(&self, input: T) -> Result<T, TransformError>;
}

impl<X: TextTransform + ?Sized> Transform<Text> for X {
    fn apply(&self, input: Text) -> Result<Text, TransformError> {
        Ok(self.transform(input)?)
    }
}

/// A transform from a function.
pub fn from_fn<T, F>(f: F) -> impl Transform<T>
where
    F: Fn(T) -> Result<T, TransformError> + Send + Sync,
{
    FromFn(f)
}

struct FromFn<F>(F);

impl<T, F> Transform<T> for FromFn<F>
where
    F: Fn(T) -> Result<T, TransformError> + Send + Sync,
{
    fn apply(&self, input: T) -> Result<T, TransformError> {
        (self.0)(input)
    }
}

/// Which stage of a [`Pipeline`] failed, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineError {
    /// The stage's name, as given to [`Pipeline::then`].
    pub stage: String,
    pub error: TransformError,
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.stage, self.error)
    }
}

impl std::error::Error for PipelineError {}

/// Named transforms run in order: each stage gets the previous one's output.
pub struct Pipeline<T> {
    stages: Vec<(String, Box<dyn Transform<T>>)>,
}

impl<T> Default for Pipeline<T> {
    fn default() -> Self {
        Pipeline { stages: Vec::new() }
    }
}

impl<T> fmt::Debug for Pipeline<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pipeline")
            .field("stages", &self.names())
            .finish()
    }
}

impl<T> Pipeline<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a stage at the end.
    pub fn then(mut self, name: impl Into<String>, transform: impl Transform<T> + 'static) -> Self {
        self.push(name, Box::new(transform));
        self
    }

    /// Add a boxed stage at the end.
    pub fn push(&mut self, name: impl Into<String>, transform: Box<dyn Transform<T>>) {
        self.stages.push((name.into(), transform));
    }

    /// The stages' names, in order.
    pub fn names(&self) -> Vec<&str> {
        self.stages.iter().map(|(name, _)| name.as_str()).collect()
    }

    pub fn len(&self) -> usize {
        self.stages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    /// Run every stage in order.
    pub fn apply(&self, input: T) -> Result<T, PipelineError> {
        self.stages
            .iter()
            .try_fold(input, |value, (stage, transform)| {
                transform.apply(value).map_err(|error| PipelineError {
                    stage: stage.clone(),
                    error,
                })
            })
    }
}

/// A pipeline is itself a transform, so pipelines nest.
impl<T> Transform<T> for Pipeline<T> {
    fn apply(&self, input: T) -> Result<T, TransformError> {
        Pipeline::apply(self, input).map_err(|error| TransformError::new(error.to_string()))
    }
}

/// Keeps the lines of a text that match a regular expression (or, inverted,
/// those that do not). Styles are kept, and the result ends with a newline
/// only if the input did.
#[derive(Clone, Debug)]
pub struct KeepLines {
    regex: fancy_regex::Regex,
    invert: bool,
}

impl KeepLines {
    /// Keep lines matching `pattern` anywhere.
    pub fn new(pattern: &str) -> Result<Self, TransformError> {
        let regex = fancy_regex::Regex::new(pattern)
            .map_err(|e| TransformError::new(format!("invalid pattern {pattern:?}: {e}")))?;
        Ok(KeepLines {
            regex,
            invert: false,
        })
    }

    /// Keep the lines that do *not* match instead.
    pub fn invert(mut self, invert: bool) -> Self {
        self.invert = invert;
        self
    }
}

impl TextTransform for KeepLines {
    fn transform(&self, text: Text) -> Result<Text, PluginError> {
        if text.plain().is_empty() {
            return Ok(text);
        }
        let mut kept = text.blank_copy();
        for line in text.split("\n", true, false) {
            let content = line.plain().strip_suffix('\n').unwrap_or(line.plain());
            let matched = self
                .regex
                .is_match(content)
                .map_err(|e| PluginError::Other(format!("filter failed: {e}")))?;
            if matched != self.invert {
                kept = kept.append_text(&line);
            }
        }
        // End the way the input did: a kept middle line brings its `\n`.
        if !text.plain().ends_with('\n') && kept.plain().ends_with('\n') {
            kept.right_crop(1);
        }
        Ok(kept)
    }
}

/// Styles every match of a regular expression.
///
/// Only whole matches are styled: unlike [`Text::highlight_regex`], a named
/// group is just a group here, never a style name. A match the regex engine
/// cannot finish (its backtrack limit) is an error, as it is for [`KeepLines`].
#[derive(Clone, Debug)]
pub struct HighlightMatches {
    regex: fancy_regex::Regex,
    style: Style,
}

impl HighlightMatches {
    pub fn new(pattern: &str, style: Style) -> Result<Self, TransformError> {
        let regex = fancy_regex::Regex::new(pattern)
            .map_err(|e| TransformError::new(format!("invalid pattern {pattern:?}: {e}")))?;
        Ok(HighlightMatches { regex, style })
    }
}

impl TextTransform for HighlightMatches {
    fn transform(&self, mut text: Text) -> Result<Text, PluginError> {
        let mut ranges = Vec::new();
        for found in self.regex.find_iter(text.plain()) {
            let found = found.map_err(|e| PluginError::Other(format!("highlight failed: {e}")))?;
            ranges.push(found.range());
        }
        // `Text` spans are byte offsets, the same units the regex reports.
        for range in ranges {
            text.stylize(self.style.clone(), range.start, range.end);
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keep_lines_keeps_endings_and_styles() {
        let mut text = Text::new("one\ntwo\nthree");
        text.stylize(Style::parse("bold").unwrap(), 4, 7);
        let kept = KeepLines::new("t").unwrap().transform(text).unwrap();
        assert_eq!(kept.plain(), "two\nthree");
        assert_eq!(kept.spans().len(), 1);
        assert_eq!((kept.spans()[0].start, kept.spans()[0].end), (0, 3));

        let dropped = KeepLines::new("t")
            .unwrap()
            .invert(true)
            .transform(Text::new("one\ntwo\n"))
            .unwrap();
        assert_eq!(dropped.plain(), "one\n");
        let first = KeepLines::new("a")
            .unwrap()
            .transform(Text::new("a\nb"))
            .unwrap();
        assert_eq!(first.plain(), "a");
        assert_eq!(
            KeepLines::new("x")
                .unwrap()
                .transform(Text::new("a\nb"))
                .unwrap()
                .plain(),
            ""
        );
        assert!(KeepLines::new("(").is_err());
    }

    #[test]
    fn highlight_matches_styles_each_match() {
        let text = HighlightMatches::new("o+", Style::parse("red").unwrap())
            .unwrap()
            .transform(Text::new("foo boo"))
            .unwrap();
        let ranges: Vec<_> = text.spans().iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(ranges, [(1, 3), (5, 7)]);
    }

    #[test]
    fn highlight_matches_styles_whole_matches_only() {
        // A named group is not a style name here, unlike `highlight_regex`.
        let text = HighlightMatches::new("(?P<blink>ERR)OR", Style::parse("reverse").unwrap())
            .unwrap()
            .transform(Text::new("an ERROR"))
            .unwrap();
        assert_eq!(text.spans().len(), 1);
        assert_eq!((text.spans()[0].start, text.spans()[0].end), (3, 8));
    }

    #[test]
    fn highlight_matches_reports_the_backtrack_limit_like_keep_lines() {
        let input = || Text::new("a".repeat(30));
        let pattern = r"^(a|a)*\1b";
        assert!(KeepLines::new(pattern).unwrap().transform(input()).is_err());
        assert!(
            HighlightMatches::new(pattern, Style::parse("reverse").unwrap())
                .unwrap()
                .transform(input())
                .is_err()
        );
    }

    #[test]
    fn a_pipeline_runs_in_order_and_names_the_failing_stage() {
        let pipeline: Pipeline<Vec<u8>> = Pipeline::new()
            .then(
                "push",
                from_fn(|mut v: Vec<u8>| {
                    v.push(1);
                    Ok(v)
                }),
            )
            .then(
                "double",
                from_fn(|v: Vec<u8>| Ok(v.into_iter().map(|x| x * 2).collect())),
            );
        assert_eq!(pipeline.apply(vec![3]).unwrap(), [6, 2]);

        let failing = pipeline.then("fail", from_fn(|_: Vec<u8>| Err(TransformError::new("no"))));
        let error = failing.apply(Vec::new()).unwrap_err();
        assert_eq!(error.stage, "fail");
        assert_eq!(error.to_string(), "fail: no");
    }
}
