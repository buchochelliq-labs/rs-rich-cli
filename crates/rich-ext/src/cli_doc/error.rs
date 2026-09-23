//! Command-line errors as diagnostics (#407), with "did you mean"
//! suggestions from Jaro-Winkler similarity.

use super::spec::CommandSpec;
use crate::diagnostic::Diagnostic;
use crate::event::EventView;

/// What went wrong on the command line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CliErrorKind {
    UnknownArgument,
    MissingValue,
    InvalidValue,
    MissingRequired,
    UnexpectedValue,
    Conflict,
    UnknownSubcommand,
    Other,
}

/// A command-line error. Render it with [`CliError::to_diagnostic`].
///
/// ```
/// use rich::Console;
/// use rich_ext::cli_doc::CliError;
///
/// let error = CliError::unknown_argument("--colr", ["--color", "--width"]).usage("rich [OPTIONS]");
/// let out = Console::builder().width(60).build().render_to_string(&error.to_diagnostic());
/// assert_eq!(
///     out,
///     "error: unexpected argument '--colr'\n\
///      note: usage: rich [OPTIONS]\n\
///      help: a similar argument exists: '--color'"
/// );
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliError {
    pub kind: CliErrorKind,
    /// The headline; generated from the kind and fields when empty.
    pub message: String,
    /// The argument at fault, as typed (`--colr`) or as named (`--width <N>`).
    pub argument: Option<String>,
    /// The value at fault, or for a conflict the other argument.
    pub value: Option<String>,
    /// Similar spellings, best first.
    pub suggestions: Vec<String>,
    /// The allowed values, for an invalid value.
    pub possible_values: Vec<String>,
    /// A usage line shown as a note.
    pub usage: Option<String>,
    /// The help switch to point at (`--help`), shown as a closing tip.
    pub help_flag: Option<String>,
}

impl CliError {
    pub fn new(kind: CliErrorKind, message: impl Into<String>) -> Self {
        CliError {
            kind,
            message: message.into(),
            argument: None,
            value: None,
            suggestions: Vec::new(),
            possible_values: Vec::new(),
            usage: None,
            help_flag: None,
        }
    }

    /// An unknown option, with suggestions drawn from `candidates`.
    pub fn unknown_argument<S: AsRef<str>>(
        argument: impl Into<String>,
        candidates: impl IntoIterator<Item = S>,
    ) -> Self {
        let argument = argument.into();
        let suggestions = suggest(&argument, candidates);
        CliError::new(CliErrorKind::UnknownArgument, "")
            .argument(argument)
            .suggestions(suggestions)
    }

    /// An unknown subcommand, with suggestions drawn from `candidates`.
    pub fn unknown_subcommand<S: AsRef<str>>(
        name: impl Into<String>,
        candidates: impl IntoIterator<Item = S>,
    ) -> Self {
        let name = name.into();
        let suggestions = suggest(&name, candidates);
        CliError::new(CliErrorKind::UnknownSubcommand, "")
            .argument(name)
            .suggestions(suggestions)
    }

    /// An unknown option or subcommand in `spec`: an argument starting with
    /// `-` is checked against the switches, anything else against the
    /// subcommands. The usage comes from the spec.
    pub fn unknown_in(spec: &CommandSpec, argument: &str) -> Self {
        let error = if argument.starts_with('-') {
            CliError::unknown_argument(argument, spec.switch_names())
        } else {
            CliError::unknown_subcommand(argument, spec.subcommand_names())
        };
        error.usage(spec.usage_lines().join("\n"))
    }

    /// A value that is not allowed, suggesting the closest `possible` ones.
    pub fn invalid_value<S: AsRef<str>>(
        argument: impl Into<String>,
        value: impl Into<String>,
        possible: impl IntoIterator<Item = S>,
    ) -> Self {
        let value = value.into();
        let possible: Vec<String> = possible
            .into_iter()
            .map(|s| s.as_ref().to_string())
            .collect();
        let mut error = CliError::new(CliErrorKind::InvalidValue, "")
            .argument(argument)
            .suggestions(suggest(&value, &possible));
        error.value = Some(value);
        error.possible_values = possible;
        error
    }

    /// An option given without its value.
    pub fn missing_value(argument: impl Into<String>) -> Self {
        CliError::new(CliErrorKind::MissingValue, "").argument(argument)
    }

    /// Required arguments that were not given.
    pub fn missing_required<S: AsRef<str>>(arguments: impl IntoIterator<Item = S>) -> Self {
        let joined: Vec<String> = arguments
            .into_iter()
            .map(|s| s.as_ref().to_string())
            .collect();
        CliError::new(CliErrorKind::MissingRequired, "").argument(joined.join(", "))
    }

    pub fn argument(mut self, argument: impl Into<String>) -> Self {
        self.argument = Some(argument.into());
        self
    }
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }
    pub fn suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestions.push(suggestion.into());
        self
    }
    pub fn suggestions(mut self, suggestions: impl IntoIterator<Item = String>) -> Self {
        self.suggestions.extend(suggestions);
        self
    }
    pub fn usage(mut self, usage: impl Into<String>) -> Self {
        self.usage = Some(usage.into());
        self
    }
    /// Close with "for more information, try '`flag`'".
    pub fn help_flag(mut self, flag: impl Into<String>) -> Self {
        self.help_flag = Some(flag.into());
        self
    }

    /// The exit status for a usage error, as clap and BSD `EX_USAGE`-style
    /// tools use: 2.
    pub fn exit_code(&self) -> i32 {
        2
    }

    /// The headline: the message, else one generated from the kind.
    pub fn headline(&self) -> String {
        if !self.message.is_empty() {
            return self.message.clone();
        }
        let arg = self.argument.as_deref().unwrap_or("");
        let value = self.value.as_deref().unwrap_or("");
        match self.kind {
            CliErrorKind::UnknownArgument => format!("unexpected argument '{arg}'"),
            CliErrorKind::MissingValue => {
                format!("a value is required for '{arg}' but none was supplied")
            }
            CliErrorKind::InvalidValue => format!("invalid value '{value}' for '{arg}'"),
            CliErrorKind::MissingRequired => {
                format!("the following required arguments were not provided: {arg}")
            }
            CliErrorKind::UnexpectedValue => format!("unexpected value '{value}' for '{arg}'"),
            CliErrorKind::Conflict if !value.is_empty() => {
                format!("the argument '{arg}' cannot be used with '{value}'")
            }
            CliErrorKind::Conflict => format!("the argument '{arg}' cannot be used here"),
            CliErrorKind::UnknownSubcommand => format!("unrecognized subcommand '{arg}'"),
            CliErrorKind::Other => "invalid command line".to_string(),
        }
    }

    /// The error as an expanded, error-level [`Diagnostic`]: the headline,
    /// the usage as a note, the possible values as a note, then a help line
    /// for the suggestions and the help flag.
    pub fn to_diagnostic(&self) -> Diagnostic {
        let mut diagnostic = Diagnostic::error(self.headline()).view(EventView::Expanded);
        if let Some(usage) = &self.usage {
            for line in usage.lines() {
                diagnostic = diagnostic.note(format!("usage: {line}"));
            }
        }
        if !self.possible_values.is_empty() {
            diagnostic = diagnostic.note(format!(
                "possible values: {}",
                self.possible_values.join(", ")
            ));
        }
        if !self.suggestions.is_empty() {
            let noun = match self.kind {
                CliErrorKind::UnknownSubcommand => ("subcommand", "subcommands"),
                CliErrorKind::InvalidValue => ("value", "values"),
                _ => ("argument", "arguments"),
            };
            let quoted: Vec<String> = self.suggestions.iter().map(|s| format!("'{s}'")).collect();
            diagnostic = diagnostic.help(match quoted.len() {
                1 => format!("a similar {} exists: {}", noun.0, quoted[0]),
                _ => format!("similar {} exist: {}", noun.1, quoted.join(", ")),
            });
        }
        if let Some(flag) = &self.help_flag {
            diagnostic = diagnostic.help(format!("for more information, try '{flag}'"));
        }
        diagnostic
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.headline())
    }
}

impl std::error::Error for CliError {}

/// The candidates similar to `input`, best first: Jaro-Winkler similarity
/// above 0.7, compared without leading dashes (clap's measure and cutoff).
/// At most three are returned; an exact match is never suggested.
///
/// Like is compared with like: a `--long` input only against `--long`
/// candidates, a `-s` input only against `-s` short flags, and a bare word (a
/// subcommand, key or value) only against bare words. A long-flag typo so
/// never draws a one-letter short flag, which clap never offers either.
///
/// ```
/// use rich_ext::cli_doc::suggest;
///
/// assert_eq!(suggest("--colr", ["--color", "--width", "--colour"]), ["--color", "--colour"]);
/// assert!(suggest("--zzz", ["--color"]).is_empty());
/// assert_eq!(suggest("--paralel", ["--parallel", "-r"]), ["--parallel"]);
/// ```
pub fn suggest<S: AsRef<str>>(input: &str, candidates: impl IntoIterator<Item = S>) -> Vec<String> {
    let needle = input.trim_start_matches('-');
    let kind = dashes(input);
    let mut scored: Vec<(f64, String)> = candidates
        .into_iter()
        .map(|c| c.as_ref().to_string())
        .filter(|c| c != input && dashes(c) == kind)
        .map(|c| (jaro_winkler(needle, c.trim_start_matches('-')), c))
        .filter(|(score, _)| *score > 0.7)
        .collect();
    // Stable: equal scores keep the candidates' order.
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.dedup_by(|a, b| a.1 == b.1);
    scored.into_iter().take(3).map(|(_, c)| c).collect()
}

/// Leading dashes, capped at two: a bare word, a `-s` short flag or a
/// `--long` flag.
fn dashes(word: &str) -> usize {
    word.bytes().take(2).take_while(|&b| b == b'-').count()
}

/// Jaro-Winkler similarity in `0.0..=1.0`, over chars.
fn jaro_winkler(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let window = (a.len().max(b.len()) / 2).saturating_sub(1);
    let mut a_hit = vec![false; a.len()];
    let mut b_hit = vec![false; b.len()];
    let mut matches = 0usize;
    for (i, ca) in a.iter().enumerate() {
        let lo = i.saturating_sub(window);
        let hi = (i + window + 1).min(b.len());
        for j in lo..hi {
            if !b_hit[j] && b[j] == *ca {
                a_hit[i] = true;
                b_hit[j] = true;
                matches += 1;
                break;
            }
        }
    }
    if matches == 0 {
        return 0.0;
    }
    let a_matched = a
        .iter()
        .zip(&a_hit)
        .filter(|(_, hit)| **hit)
        .map(|(c, _)| c);
    let b_matched = b
        .iter()
        .zip(&b_hit)
        .filter(|(_, hit)| **hit)
        .map(|(c, _)| c);
    let transpositions = a_matched.zip(b_matched).filter(|(x, y)| x != y).count() / 2;
    let m = matches as f64;
    let jaro = (m / a.len() as f64 + m / b.len() as f64 + (m - transpositions as f64) / m) / 3.0;
    let prefix = a.iter().zip(&b).take(4).take_while(|(x, y)| x == y).count();
    jaro + prefix as f64 * 0.1 * (1.0 - jaro)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jaro_winkler_reference_values() {
        // Classic reference pairs.
        assert!((jaro_winkler("MARTHA", "MARHTA") - 0.9611).abs() < 1e-3);
        assert!((jaro_winkler("DIXON", "DICKSONX") - 0.8133).abs() < 1e-3);
        assert_eq!(jaro_winkler("same", "same"), 1.0);
        assert_eq!(jaro_winkler("abc", "xyz"), 0.0);
    }

    #[test]
    fn subcommand_suggestions() {
        assert_eq!(
            suggest("confg", ["config", "print", "markdown"]),
            ["config"]
        );
    }

    #[test]
    fn suggestions_compare_like_with_like() {
        let names = ["--parallel", "-r", "--dry-run", "-n", "report", "-p"];
        assert_eq!(suggest("--paralel", names), ["--parallel"]);
        assert_eq!(suggest("--dry-rn", names), ["--dry-run"]);
        assert!(!suggest("--r", names).contains(&"-r".to_string()));
        assert!(suggest("-x", ["--x-ray", "--xx"]).is_empty());
        assert_eq!(suggest("reprt", names), ["report"]);
        assert!(suggest("paralel", names).is_empty());
    }

    #[test]
    fn suggestions_keep_the_top_three_and_the_cutoff() {
        let names = ["--color", "--colour", "--colors", "--colored", "--width"];
        assert_eq!(suggest("--colr", names).len(), 3);
        assert!(suggest("--zzz", names).is_empty());
    }
}
