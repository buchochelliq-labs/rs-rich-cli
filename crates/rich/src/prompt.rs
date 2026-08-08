//! Interactive prompts.
//!
//! Port of upstream `rich/prompt.py`: [`Prompt`] for free text, [`Confirm`] for
//! yes/no, and [`IntPrompt`]/[`FloatPrompt`] for numbers. Each renders a styled
//! question, reads a line, and re-asks until the answer validates.
//!
//! Reading is behind the [`InputSource`] trait so the whole loop — including the
//! re-ask path — is testable without a terminal. [`StdinInput`] is the default.

use std::io::{BufRead, Write};

use crate::console::Console;
use crate::text::Text;

/// Where a prompt reads its answers from. Upstream takes an optional `stream`
/// argument for the same purpose.
pub trait InputSource {
    /// Read one line, without its trailing newline. `None` means end of input.
    fn read_line(&mut self) -> std::io::Result<Option<String>>;
}

/// Reads from standard input — the default for [`Prompt::ask`].
pub struct StdinInput;

impl InputSource for StdinInput {
    fn read_line(&mut self) -> std::io::Result<Option<String>> {
        let mut buffer = String::new();
        let read = std::io::stdin().lock().read_line(&mut buffer)?;
        if read == 0 {
            return Ok(None);
        }
        Ok(Some(buffer.trim_end_matches(['\r', '\n']).to_string()))
    }
}

/// A canned list of answers. Handy for tests and for scripted runs.
pub struct ScriptedInput {
    lines: std::vec::IntoIter<String>,
}

impl ScriptedInput {
    pub fn new(lines: impl IntoIterator<Item = impl Into<String>>) -> Self {
        ScriptedInput {
            lines: lines
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into_iter(),
        }
    }
}

impl InputSource for ScriptedInput {
    fn read_line(&mut self) -> std::io::Result<Option<String>> {
        Ok(self.lines.next())
    }
}

/// Why an answer was rejected. Carries the console markup upstream stores in
/// `validate_error_message` / `illegal_choice_message`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidResponse(pub String);

/// The shared prompt behaviour: rendering the question and running the ask loop.
/// Port of `PromptBase`.
///
/// The type parameter is supplied by the concrete prompts below rather than by
/// generics, so each keeps a plain, obvious signature.
#[derive(Debug, Clone)]
struct PromptBase {
    prompt: String,
    suffix: String,
    choices: Option<Vec<String>>,
    show_default: bool,
    show_choices: bool,
    case_sensitive: bool,
}

impl PromptBase {
    fn new(prompt: impl Into<String>, choices: Option<Vec<String>>) -> Self {
        PromptBase {
            prompt: prompt.into(),
            suffix: ": ".to_string(),
            choices,
            show_default: true,
            show_choices: true,
            case_sensitive: true,
        }
    }

    /// Build the question line: the prompt, then `[a/b/c]`, then `(default)`,
    /// then the suffix. Port of `PromptBase.make_prompt`.
    fn make_prompt(&self, console: &Console, default: Option<&str>) -> Text {
        // The prompt itself is markup, matching upstream's `Text.from_markup`
        // default for a `str` prompt.
        let mut text = console.build_text(&self.prompt);

        // The style *names* go on the spans, as upstream passes them
        // (`prompt.append(choices, "prompt.choices")`), so a console with a
        // custom theme restyles the question without the prompt knowing.
        if self.show_choices {
            if let Some(choices) = &self.choices {
                text.append(" ", None);
                text.append(
                    &format!("[{}]", choices.join("/")),
                    Some("prompt.choices".into()),
                );
            }
        }
        if self.show_default {
            if let Some(default) = default {
                text.append(" ", None);
                text.append(&format!("({default})"), Some("prompt.default".into()));
            }
        }
        text.append(&self.suffix, None);
        text
    }

    /// True when `value` is one of the choices (or there are no choices).
    /// Port of `PromptBase.check_choice`.
    fn check_choice(&self, value: &str) -> bool {
        let Some(choices) = &self.choices else {
            return true;
        };
        let value = value.trim();
        if self.case_sensitive {
            choices.iter().any(|choice| choice == value)
        } else {
            choices
                .iter()
                .any(|choice| choice.eq_ignore_ascii_case(value))
        }
    }

    /// The choice as originally spelled, for a case-insensitive match — upstream
    /// deliberately returns the canonical spelling, not what was typed.
    fn canonical_choice(&self, value: &str) -> Option<String> {
        let choices = self.choices.as_ref()?;
        if self.case_sensitive {
            return None;
        }
        choices
            .iter()
            .find(|choice| choice.eq_ignore_ascii_case(value.trim()))
            .cloned()
    }

    /// Write the question (no trailing newline) and read one line back.
    fn ask_once(
        &self,
        console: &Console,
        input: &mut dyn InputSource,
        default: Option<&str>,
    ) -> std::io::Result<Option<String>> {
        let prompt = self.make_prompt(console, default);
        // `end=""` upstream: the answer is typed on the same line as the question.
        print!("{}", console.render_to_string(&prompt));
        std::io::stdout().flush()?;
        input.read_line()
    }

    /// Report a rejected answer. Port of `PromptBase.on_validate_error`.
    fn on_validate_error(&self, console: &Console, error: &InvalidResponse) {
        console.print_str(&error.0);
    }
}

/// Ask for a line of text. Port of `rich.prompt.Prompt`.
///
/// ```no_run
/// # use rich::{Console, prompt::Prompt};
/// let console = Console::new();
/// let name = Prompt::new("What is your name").ask(&console, Some("World")).unwrap();
/// ```
#[derive(Debug, Clone)]
pub struct Prompt {
    base: PromptBase,
}

impl Prompt {
    pub fn new(prompt: impl Into<String>) -> Self {
        Prompt {
            base: PromptBase::new(prompt, None),
        }
    }

    /// Restrict answers to `choices`, shown as `[a/b/c]`.
    pub fn choices(mut self, choices: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.base.choices = Some(choices.into_iter().map(Into::into).collect());
        self
    }

    /// Match choices regardless of case, returning the choice as spelled in the
    /// list rather than as typed.
    pub fn case_sensitive(mut self, case_sensitive: bool) -> Self {
        self.base.case_sensitive = case_sensitive;
        self
    }

    /// Hide the `(default)` hint while still accepting an empty answer.
    pub fn show_default(mut self, show: bool) -> Self {
        self.base.show_default = show;
        self
    }

    /// Hide the `[a/b/c]` hint while still enforcing the choices.
    pub fn show_choices(mut self, show: bool) -> Self {
        self.base.show_choices = show;
        self
    }

    /// The rendered question, as [`ask`](Self::ask) would print it.
    pub fn make_prompt(&self, console: &Console, default: Option<&str>) -> Text {
        self.base.make_prompt(console, default)
    }

    /// Validate one answer. Port of `PromptBase.process_response`.
    pub fn process_response(&self, value: &str) -> Result<String, InvalidResponse> {
        let value = value.trim();
        if !self.base.check_choice(value) {
            return Err(InvalidResponse(
                "[prompt.invalid.choice]Please select one of the available options".to_string(),
            ));
        }
        Ok(self
            .base
            .canonical_choice(value)
            .unwrap_or_else(|| value.to_string()))
    }

    /// Ask on standard input, re-asking until the answer validates.
    pub fn ask(&self, console: &Console, default: Option<&str>) -> std::io::Result<String> {
        self.ask_from(console, &mut StdinInput, default)
    }

    /// As [`ask`](Self::ask), reading from `input`. Port of `PromptBase.__call__`.
    ///
    /// An empty answer takes the default when there is one. Exhausted input does
    /// the same, rather than looping forever.
    pub fn ask_from(
        &self,
        console: &Console,
        input: &mut dyn InputSource,
        default: Option<&str>,
    ) -> std::io::Result<String> {
        loop {
            let Some(value) = self.base.ask_once(console, input, default)? else {
                return Ok(default.unwrap_or_default().to_string());
            };
            if value.is_empty() {
                if let Some(default) = default {
                    return Ok(default.to_string());
                }
            }
            match self.process_response(&value) {
                Ok(value) => return Ok(value),
                Err(error) => self.base.on_validate_error(console, &error),
            }
        }
    }
}

/// Ask a yes/no question. Port of `rich.prompt.Confirm`.
#[derive(Debug, Clone)]
pub struct Confirm {
    base: PromptBase,
}

impl Confirm {
    pub fn new(prompt: impl Into<String>) -> Self {
        Confirm {
            base: PromptBase::new(prompt, Some(vec!["y".to_string(), "n".to_string()])),
        }
    }

    pub fn show_default(mut self, show: bool) -> Self {
        self.base.show_default = show;
        self
    }

    pub fn show_choices(mut self, show: bool) -> Self {
        self.base.show_choices = show;
        self
    }

    /// The rendered question. Unlike the other prompts the default renders as
    /// `(y)`/`(n)` rather than the value itself. Port of `Confirm.render_default`.
    pub fn make_prompt(&self, console: &Console, default: Option<bool>) -> Text {
        let rendered = default.map(|yes| if yes { "y" } else { "n" });
        self.base.make_prompt(console, rendered)
    }

    /// Port of `Confirm.process_response` — case-insensitive, and anything that
    /// is not a choice is rejected outright.
    pub fn process_response(&self, value: &str) -> Result<bool, InvalidResponse> {
        let value = value.trim().to_ascii_lowercase();
        match value.as_str() {
            "y" => Ok(true),
            "n" => Ok(false),
            _ => Err(InvalidResponse(
                "[prompt.invalid]Please enter Y or N".to_string(),
            )),
        }
    }

    pub fn ask(&self, console: &Console, default: Option<bool>) -> std::io::Result<bool> {
        self.ask_from(console, &mut StdinInput, default)
    }

    pub fn ask_from(
        &self,
        console: &Console,
        input: &mut dyn InputSource,
        default: Option<bool>,
    ) -> std::io::Result<bool> {
        let rendered = default.map(|yes| if yes { "y" } else { "n" });
        loop {
            let Some(value) = self.base.ask_once(console, input, rendered)? else {
                return Ok(default.unwrap_or(false));
            };
            if value.trim().is_empty() {
                if let Some(default) = default {
                    return Ok(default);
                }
            }
            match self.process_response(&value) {
                Ok(value) => return Ok(value),
                Err(error) => self.base.on_validate_error(console, &error),
            }
        }
    }
}

/// Ask for a whole number. Port of `rich.prompt.IntPrompt`.
#[derive(Debug, Clone)]
pub struct IntPrompt {
    base: PromptBase,
}

/// Ask for a number. Port of `rich.prompt.FloatPrompt`.
#[derive(Debug, Clone)]
pub struct FloatPrompt {
    base: PromptBase,
}

/// Generate the two numeric prompts, which differ only in their parse target and
/// their rejection message.
macro_rules! numeric_prompt {
    ($name:ident, $ty:ty, $message:expr) => {
        impl $name {
            pub fn new(prompt: impl Into<String>) -> Self {
                $name {
                    base: PromptBase::new(prompt, None),
                }
            }

            pub fn show_default(mut self, show: bool) -> Self {
                self.base.show_default = show;
                self
            }

            /// The rendered question, as [`ask`](Self::ask) would print it.
            pub fn make_prompt(&self, console: &Console, default: Option<$ty>) -> Text {
                self.base
                    .make_prompt(console, default.map(|d| d.to_string()).as_deref())
            }

            /// Parse one answer, rejecting anything that is not a number.
            pub fn process_response(&self, value: &str) -> Result<$ty, InvalidResponse> {
                value
                    .trim()
                    .parse::<$ty>()
                    .map_err(|_| InvalidResponse($message.to_string()))
            }

            pub fn ask(&self, console: &Console, default: Option<$ty>) -> std::io::Result<$ty> {
                self.ask_from(console, &mut StdinInput, default)
            }

            pub fn ask_from(
                &self,
                console: &Console,
                input: &mut dyn InputSource,
                default: Option<$ty>,
            ) -> std::io::Result<$ty> {
                let rendered = default.map(|d| d.to_string());
                loop {
                    let Some(value) = self.base.ask_once(console, input, rendered.as_deref())?
                    else {
                        return Ok(default.unwrap_or_default());
                    };
                    if value.trim().is_empty() {
                        if let Some(default) = default {
                            return Ok(default);
                        }
                    }
                    match self.process_response(&value) {
                        Ok(value) => return Ok(value),
                        Err(error) => self.base.on_validate_error(console, &error),
                    }
                }
            }
        }
    };
}

numeric_prompt!(
    IntPrompt,
    i64,
    "[prompt.invalid]Please enter a valid integer number"
);
numeric_prompt!(FloatPrompt, f64, "[prompt.invalid]Please enter a number");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;

    fn console() -> Console {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(80)
            .no_color(false)
            .build()
    }

    #[test]
    fn empty_answer_takes_the_default() {
        let console = console();
        let mut input = ScriptedInput::new([""]);
        let answer = Prompt::new("Name")
            .ask_from(&console, &mut input, Some("World"))
            .unwrap();
        assert_eq!(answer, "World");
    }

    /// Exhausted input must not spin forever waiting for an answer.
    #[test]
    fn exhausted_input_falls_back_to_the_default() {
        let console = console();
        let mut input = ScriptedInput::new(Vec::<String>::new());
        let answer = Prompt::new("Name")
            .ask_from(&console, &mut input, Some("World"))
            .unwrap();
        assert_eq!(answer, "World");
    }

    /// A rejected answer is re-asked rather than returned.
    #[test]
    fn invalid_choice_is_re_asked() {
        let console = console();
        let mut input = ScriptedInput::new(["maybe", "yes"]);
        let answer = Prompt::new("Pick")
            .choices(["yes", "no"])
            .ask_from(&console, &mut input, None)
            .unwrap();
        assert_eq!(answer, "yes");
    }

    /// A case-insensitive match returns the choice as spelled in the list, not
    /// as the user typed it — upstream is explicit about this.
    #[test]
    fn case_insensitive_returns_the_canonical_spelling() {
        let prompt = Prompt::new("Pick")
            .choices(["Yes", "No"])
            .case_sensitive(false);
        assert_eq!(prompt.process_response("yES").unwrap(), "Yes");
        // With case sensitivity on, the same answer is rejected.
        let strict = Prompt::new("Pick").choices(["Yes", "No"]);
        assert!(strict.process_response("yES").is_err());
    }

    #[test]
    fn confirm_reads_y_and_n() {
        let console = console();
        let confirm = Confirm::new("Sure");
        assert!(confirm
            .ask_from(&console, &mut ScriptedInput::new(["Y"]), None)
            .unwrap());
        assert!(!confirm
            .ask_from(&console, &mut ScriptedInput::new(["n"]), None)
            .unwrap());
        // Empty takes the default; a junk answer is re-asked.
        assert!(confirm
            .ask_from(&console, &mut ScriptedInput::new([""]), Some(true))
            .unwrap());
        assert!(!confirm
            .ask_from(&console, &mut ScriptedInput::new(["what", "n"]), None)
            .unwrap());
    }

    #[test]
    fn numeric_prompts_reject_non_numbers() {
        let int = IntPrompt::new("Age");
        assert_eq!(int.process_response(" 42 ").unwrap(), 42);
        assert_eq!(
            int.process_response("4.5").unwrap_err(),
            InvalidResponse("[prompt.invalid]Please enter a valid integer number".to_string())
        );

        let float = FloatPrompt::new("Ratio");
        assert!((float.process_response("1.5").unwrap() - 1.5).abs() < f64::EPSILON);
        assert_eq!(
            float.process_response("abc").unwrap_err(),
            InvalidResponse("[prompt.invalid]Please enter a number".to_string())
        );
    }

    /// `show_choices(false)` hides the hint but still enforces the choices.
    #[test]
    fn hidden_choices_are_still_enforced() {
        let console = console();
        let prompt = Prompt::new("Pick").choices(["a", "b"]).show_choices(false);
        assert!(!prompt.make_prompt(&console, None).plain().contains("[a/b]"));
        assert!(prompt.process_response("c").is_err());
    }
}
