//! Forms (#288): several fields answered together, with each field's
//! error shown under it (#472).
//!
//! Fields are text (an [`Input`], with its validation, placeholder and
//! default), masked fields for passwords, a choice among options (fixed,
//! or worked out from the answers above it), several options at once, a
//! file or directory picked with a [`FilePicker`], yes/no toggles, and
//! notes (read-only text). Each may have a description under it. Tab and
//! the arrows move between fields; Enter moves to the next field and, on
//! the last, submits. Submitting checks every field and focuses the first
//! that fails, with its message under it.
//!
//! [`Form::group`] splits a form into pages (0.0.21), shown one at a time
//! with a [`Paginator`] under them: Enter on a page's last field, or Page
//! Down, checks that page's fields and moves on only when they pass; Page
//! Up goes back.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rich::Segment;

use crate::component::{Component, Context, Flow, View};
use crate::components::{
    display_path, fit, plain, question, text, FilePicker, Input, Paginator, PaginatorStyle, Theme,
};
use crate::event::{Event, Key, KeyCode};
use crate::keymap::{keys, Keymap};
use crate::policy::{LineIo, NotInteractive};

/// A field's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// A text field's or a choice's answer.
    Text(String),
    /// A toggle's answer.
    Flag(bool),
    /// A multi-select's answer: the options chosen, in their order.
    List(Vec<String>),
    /// A file field's answer.
    Path(PathBuf),
}

/// A submitted form's answers, by field name, in field order. Notes have
/// none.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Answers(pub Vec<(String, Value)>);

impl Answers {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.0
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    /// A text or choice field's answer.
    pub fn text(&self, name: &str) -> Option<&str> {
        match self.get(name)? {
            Value::Text(text) => Some(text),
            _ => None,
        }
    }

    /// A toggle's answer.
    pub fn flag(&self, name: &str) -> Option<bool> {
        match self.get(name)? {
            Value::Flag(flag) => Some(*flag),
            _ => None,
        }
    }

    /// A multi-select's answer.
    pub fn list(&self, name: &str) -> Option<&[String]> {
        match self.get(name)? {
            Value::List(list) => Some(list),
            _ => None,
        }
    }

    /// A file field's answer.
    pub fn path(&self, name: &str) -> Option<&Path> {
        match self.get(name)? {
            Value::Path(path) => Some(path),
            _ => None,
        }
    }
}

/// Works out a dynamic choice's options from the answers above it.
type Options = Box<dyn Fn(&Answers) -> Vec<String>>;

enum Kind {
    Text(Box<Input>),
    Choice {
        options: Vec<String>,
        index: usize,
        /// For a dynamic choice: its options from the answers so far.
        source: Option<Options>,
    },
    Multi {
        options: Vec<String>,
        chosen: Vec<bool>,
        /// The option the cursor is on.
        cursor: usize,
        /// At most this many may be chosen.
        limit: Option<usize>,
    },
    Toggle(bool),
    Note(String),
    File {
        /// Always `Some`, but for a moment while a builder method runs.
        picker: Option<Box<FilePicker>>,
        path: Option<PathBuf>,
    },
}

struct Field {
    name: String,
    label: String,
    description: Option<String>,
    /// The page (group) the field is on.
    page: usize,
    kind: Kind,
    error: Option<String>,
}

impl Field {
    fn value(&self) -> Result<Value, String> {
        match &self.kind {
            Kind::Text(input) => input.resolve().map(Value::Text),
            Kind::Choice { options, index, .. } => Ok(Value::Text(
                options.get(*index).cloned().unwrap_or_default(),
            )),
            Kind::Multi {
                options, chosen, ..
            } => Ok(Value::List(
                options
                    .iter()
                    .zip(chosen)
                    .filter(|(_, on)| **on)
                    .map(|(option, _)| option.clone())
                    .collect(),
            )),
            Kind::Toggle(on) => Ok(Value::Flag(*on)),
            Kind::Note(_) => Err(String::new()),
            Kind::File { .. } => self
                .file_path()
                .0
                .map(Value::Path)
                .ok_or_else(|| "nothing picked yet".to_string()),
        }
    }

    /// A file field's path, and whether it was picked (not its default).
    fn file_path(&self) -> (Option<PathBuf>, bool) {
        match &self.kind {
            Kind::File {
                path: Some(path), ..
            } => (Some(path.clone()), true),
            Kind::File { picker, .. } => (picker.as_ref().and_then(|p| p.default_value()), false),
            _ => (None, false),
        }
    }

    fn is_note(&self) -> bool {
        matches!(self.kind, Kind::Note(_))
    }

    /// The answer as shown once submitted.
    fn summary(&self) -> String {
        match (&self.kind, self.value()) {
            (Kind::Text(input), Ok(Value::Text(text))) => input.display_value(&text),
            (_, Ok(Value::Text(text))) => text,
            (_, Ok(Value::Flag(on))) => if on { "yes" } else { "no" }.to_string(),
            (_, Ok(Value::List(list))) => list.join(", "),
            (_, Ok(Value::Path(path))) => display_path(&path),
            (_, Err(_)) => String::new(),
        }
    }

    fn picker(&mut self) -> Option<&mut FilePicker> {
        match &mut self.kind {
            Kind::File { picker, .. } => picker.as_deref_mut(),
            _ => None,
        }
    }
}

/// A form: named fields answered together, on one page or several.
/// Returns [`Answers`].
///
/// ```
/// use rich_interact::headless::{self, Script};
/// use rich_interact::{Form, Input};
///
/// let form = Form::new("New service")
///     .note("Welcome", "Two pages: the service, then where it runs.")
///     .input("name", Input::new("Name"))
///     .describe("Lower case, no spaces")
///     .multi("features", "Features", ["tls", "metrics", "logs"])
///     .group("Where")
///     .choice("cloud", "Cloud", ["aws", "gcp"])
///     .dynamic_choice("region", "Region", |answers| match answers.text("cloud") {
///         Some("gcp") => vec!["europe-west1".into(), "us-central1".into()],
///         _ => vec!["eu-west-1".into(), "us-east-1".into()],
///     });
/// // Page one: a name and two features; Enter on its last field moves to
/// // page two, where GCP's regions are offered.
/// let script = Script::new()
///     .text("api")
///     .keys("tab space right space enter right tab right enter");
/// let (outcome, _) = headless::run(form, script, 60, 16);
/// let answers = outcome.unwrap().value().unwrap();
/// assert_eq!(answers.list("features").unwrap(), ["tls", "metrics"]);
/// assert_eq!(answers.text("region"), Some("us-central1"));
/// ```
pub struct Form {
    title: String,
    fields: Vec<Field>,
    focus: usize,
    theme: Theme,
    answer: Option<bool>,
    mouse: bool,
    /// Actions rebound on this form ([`Form::rebind`]), with their keys.
    rebound: Vec<(String, Vec<Key>)>,
    /// Each page's title (the first page's may be empty).
    groups: Vec<String>,
    /// Which page shows; its page count is the number of groups.
    pager: Paginator,
    /// The focused file field's picker is open.
    browsing: bool,
}

/// What a row of the form is, for a click.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Hit {
    None,
    Field(usize),
    /// A multi-select's row: each option's columns.
    Options(usize, Vec<Range<usize>>),
    /// An open file picker, whose view starts at this row.
    Picker(usize),
    Buttons,
    Indicator,
}

/// What [`Form::layout`] draws.
struct Layout {
    lines: Vec<Vec<Segment>>,
    hits: Vec<Hit>,
    cursor: Option<(usize, usize)>,
}

impl Layout {
    fn push(&mut self, line: Vec<Segment>, hit: Hit) {
        self.lines.push(line);
        self.hits.push(hit);
    }
}

const SUBMIT: &str = " Submit ";
const CANCEL: &str = " Cancel ";

impl Form {
    pub fn new(title: impl Into<String>) -> Form {
        Form {
            title: title.into(),
            fields: Vec::new(),
            focus: 0,
            theme: Theme::default(),
            answer: None,
            mouse: false,
            rebound: Vec::new(),
            groups: vec![String::new()],
            pager: Paginator::new(1),
            browsing: false,
        }
    }

    /// Make `keys` do `action` on this form only; no keys unbinds it. The
    /// actions are the form's (`submit`, `cancel`, `next`, `previous`, and
    /// with pages `next-page` and `previous-page`), a choice field's
    /// (`next-option`, `previous-option`), a multi-select's
    /// (`next-choice`, `previous-choice`, `choose`), a file field's
    /// (`browse`) and a toggle's (`toggle`, `yes`, `no`); see
    /// [`keymap`](Component::keymap). A text field's keys are its
    /// [`Input`]'s, and an open picker's its [`FilePicker`]'s.
    pub fn rebind(mut self, action: &str, keys: impl IntoIterator<Item = Key>) -> Self {
        self.rebound
            .push((action.to_string(), keys.into_iter().collect()));
        self
    }

    /// `keymap` with this form's rebinding.
    fn rebound(&self, mut keymap: Keymap) -> Keymap {
        for (action, keys) in &self.rebound {
            keymap.rebind(action, keys.clone());
        }
        keymap
    }

    /// The form's own keys, without the focused field's.
    fn form_keymap(&self) -> Keymap {
        let mut keymap = Keymap::new("form")
            .bind("submit", keys("ctrl+s"), "submit (Enter on the last field)")
            .bind("cancel", keys("escape"), "cancel")
            .bind("next", keys("enter tab down"), "next field")
            .bind("previous", keys("shift+tab up"), "previous field");
        if self.groups.len() > 1 {
            keymap = keymap
                .bind(
                    "next-page",
                    keys("pagedown"),
                    "next page, once this one passes",
                )
                .bind("previous-page", keys("pageup"), "previous page");
        }
        self.rebound(keymap)
    }

    fn choice_keymap(&self) -> Keymap {
        self.rebound(
            Keymap::new("form-choice")
                .bind("next-option", keys("right space"), "next option")
                .bind("previous-option", keys("left"), "previous option"),
        )
    }

    fn multi_keymap(&self) -> Keymap {
        self.rebound(
            Keymap::new("form-multi")
                .bind("next-choice", keys("right"), "next option")
                .bind("previous-choice", keys("left"), "previous option")
                .bind("choose", keys("space x"), "choose or unchoose the option"),
        )
    }

    fn file_keymap(&self) -> Keymap {
        self.rebound(Keymap::new("form-file").bind(
            "browse",
            keys("space ctrl+o"),
            "browse for the path",
        ))
    }

    fn toggle_keymap(&self) -> Keymap {
        self.rebound(
            Keymap::new("form-toggle")
                .bind("toggle", keys("space left right"), "switch")
                .bind("yes", keys("y"), "on")
                .bind("no", keys("n"), "off"),
        )
    }

    /// Report the mouse (#476): a click focuses a field (and flips a
    /// toggle, moves a choice on, chooses a multi-select's option or opens
    /// a file field's picker), a click on a page's dot goes to that page,
    /// and Submit and Cancel buttons show under the fields.
    pub fn with_mouse(mut self, on: bool) -> Self {
        self.mouse = on;
        for field in &mut self.fields {
            if let Kind::File { picker, .. } = &mut field.kind {
                *picker = picker.take().map(|p| Box::new(p.with_mouse(on)));
            }
        }
        self
    }

    fn push(mut self, name: impl Into<String>, label: String, kind: Kind) -> Self {
        self.fields.push(Field {
            name: name.into(),
            label,
            description: None,
            page: self.groups.len() - 1,
            kind,
            error: None,
        });
        self
    }

    /// A text field.
    pub fn text(self, name: impl Into<String>, label: impl Into<String>) -> Self {
        let label = label.into();
        let input = Input::new(label.clone());
        self.push(name, label, Kind::Text(Box::new(input)))
    }

    /// A text field from a configured [`Input`] (validation, placeholder,
    /// default, suggestions); its prompt is the label.
    pub fn input(self, name: impl Into<String>, input: Input) -> Self {
        let label = input.label().to_string();
        self.push(name, label, Kind::Text(Box::new(input)))
    }

    /// A masked text field, for passwords and tokens (see
    /// [`Input::masked`]).
    pub fn masked(self, name: impl Into<String>, label: impl Into<String>) -> Self {
        let label = label.into();
        let input = Input::masked(label.clone());
        self.push(name, label, Kind::Text(Box::new(input)))
    }

    /// One of `options`, changed with Left and Right (or Space).
    pub fn choice<I, S>(self, name: impl Into<String>, label: impl Into<String>, options: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let options = options.into_iter().map(Into::into).collect();
        let kind = Kind::Choice {
            options,
            index: 0,
            source: None,
        };
        self.push(name, label.into(), kind)
    }

    /// A choice whose options depend on the answers above it: `options`
    /// gets the answers of the fields before this one (those that have an
    /// answer yet) and returns the options. They are worked out again after
    /// every key; the option chosen stays chosen while it is still offered.
    pub fn dynamic_choice(
        self,
        name: impl Into<String>,
        label: impl Into<String>,
        options: impl Fn(&Answers) -> Vec<String> + 'static,
    ) -> Self {
        let kind = Kind::Choice {
            options: Vec::new(),
            index: 0,
            source: Some(Box::new(options)),
        };
        let mut form = self.push(name, label.into(), kind);
        form.refresh();
        form
    }

    /// Any number of `options` (Huh's multi-select): Left and Right move
    /// between them, Space (or `x`) chooses one. Answers a
    /// [`Value::List`], in the options' order.
    pub fn multi<I, S>(self, name: impl Into<String>, label: impl Into<String>, options: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let options: Vec<String> = options.into_iter().map(Into::into).collect();
        let kind = Kind::Multi {
            chosen: vec![false; options.len()],
            options,
            cursor: 0,
            limit: None,
        };
        self.push(name, label.into(), kind)
    }

    /// Choose these options of the multi-select just added, to begin with.
    pub fn chosen<I, S>(mut self, options: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let wanted: Vec<S> = options.into_iter().collect();
        if let Some(Kind::Multi {
            options, chosen, ..
        }) = self.fields.last_mut().map(|field| &mut field.kind)
        {
            for (option, on) in options.iter().zip(chosen.iter_mut()) {
                *on = wanted.iter().any(|w| w.as_ref() == option);
            }
        }
        self
    }

    /// Let at most `count` options of the multi-select just added be
    /// chosen.
    pub fn limit(mut self, count: usize) -> Self {
        if let Some(Kind::Multi { limit, .. }) = self.fields.last_mut().map(|field| &mut field.kind)
        {
            *limit = Some(count);
        }
        self
    }

    /// Yes or no, changed with Space, Left, Right, `y` and `n`.
    pub fn toggle(self, name: impl Into<String>, label: impl Into<String>, on: bool) -> Self {
        self.push(name, label.into(), Kind::Toggle(on))
    }

    /// Read-only text: a title and a body, wrapped to the width. Focus
    /// passes over it and it has no answer.
    pub fn note(self, title: impl Into<String>, body: impl Into<String>) -> Self {
        self.push(String::new(), title.into(), Kind::Note(body.into()))
    }

    /// A path picked with `picker` (a [`FilePicker`]: its root, mode,
    /// extensions and jail apply). Space (or a click) opens it under the
    /// field; picking closes it, and Escape closes it with no change.
    /// Answers a [`Value::Path`]; with nothing picked, the picker's
    /// [`default`](FilePicker::default), and without one the field fails.
    pub fn file(
        self,
        name: impl Into<String>,
        label: impl Into<String>,
        picker: FilePicker,
    ) -> Self {
        let mouse = self.mouse;
        let kind = Kind::File {
            picker: Some(Box::new(picker.with_mouse(mouse))),
            path: None,
        };
        self.push(name, label.into(), kind)
    }

    /// A line of help under the field just added, shown dim.
    pub fn describe(mut self, description: impl Into<String>) -> Self {
        if let Some(field) = self.fields.last_mut() {
            field.description = Some(description.into());
        }
        self
    }

    /// Start a new page titled `title`: the fields added after it are
    /// shown together, and the form's pages one at a time. Before any
    /// field it titles the first page.
    pub fn group(mut self, title: impl Into<String>) -> Self {
        let current = self.groups.len() - 1;
        if self.fields.iter().any(|field| field.page == current) {
            self.groups.push(title.into());
        } else {
            self.groups[current] = title.into();
        }
        self.pager.set_pages(self.groups.len());
        self
    }

    /// How the page indicator looks (default: dots).
    pub fn page_style(mut self, style: PaginatorStyle) -> Self {
        self.pager = Paginator::new(self.groups.len())
            .style(style)
            .theme(self.theme.clone());
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.pager = std::mem::replace(&mut self.pager, Paginator::new(1)).theme(theme.clone());
        self.theme = theme;
        self
    }

    /// The page showing, from 0.
    pub fn page(&self) -> usize {
        self.pager.page()
    }

    /// The fields on page `page`.
    fn on_page(&self, page: usize) -> Range<usize> {
        match self.fields.iter().position(|f| f.page == page) {
            Some(start) => {
                let end = self.fields[start..]
                    .iter()
                    .position(|f| f.page != page)
                    .map_or(self.fields.len(), |n| start + n);
                start..end
            }
            None => 0..0,
        }
    }

    /// The focus stops on the current page: its fields but notes, or the
    /// first field when it has only notes.
    fn stops(&self) -> Vec<usize> {
        let range = self.on_page(self.pager.page());
        let stops: Vec<usize> = range
            .clone()
            .filter(|&index| !self.fields[index].is_note())
            .collect();
        if stops.is_empty() {
            range.take(1).collect()
        } else {
            stops
        }
    }

    fn move_focus(&mut self, delta: isize) {
        let stops = self.stops();
        if stops.is_empty() {
            return;
        }
        let at = stops.iter().position(|&s| s == self.focus).unwrap_or(0);
        let next = (at as isize + delta).rem_euclid(stops.len() as isize) as usize;
        self.focus = stops[next];
    }

    /// Show `page`, focused on its first stop.
    fn show_page(&mut self, page: usize) {
        self.pager.set_page(page);
        self.focus = self.stops().first().copied().unwrap_or(0);
        self.browsing = false;
    }

    /// Whether the focus is on the current page's last stop.
    fn on_last_stop(&self) -> bool {
        self.stops().last() == Some(&self.focus)
    }

    /// Check the fields in `range`, marking those that fail; the first
    /// that fails.
    fn check(&mut self, range: Range<usize>) -> Option<usize> {
        let mut first = None;
        for index in range {
            let field = &mut self.fields[index];
            if field.is_note() {
                continue;
            }
            match field.value() {
                Ok(_) => field.error = None,
                Err(message) => {
                    field.error = Some(message);
                    first.get_or_insert(index);
                }
            }
        }
        first
    }

    /// Move on a page if this one's fields pass.
    fn next_page(&mut self) -> Flow<Answers> {
        if self.pager.on_last() {
            return Flow::Continue;
        }
        match self.check(self.on_page(self.pager.page())) {
            Some(index) => self.focus = index,
            None => self.show_page(self.pager.page() + 1),
        }
        Flow::Continue
    }

    /// Go to `page` from a click: back freely, forward through every
    /// page's checks.
    fn go_to(&mut self, page: usize) {
        while self.pager.page() < page {
            let at = self.pager.page();
            self.next_page();
            if self.pager.page() == at {
                return;
            }
        }
        if page < self.pager.page() {
            self.show_page(page);
        }
    }

    /// The answers of the fields before `end` that have one now.
    fn answers_before(&self, end: usize) -> Answers {
        Answers(
            self.fields[..end]
                .iter()
                .filter(|field| !field.is_note())
                .filter_map(|field| Some((field.name.clone(), field.value().ok()?)))
                .collect(),
        )
    }

    /// Work out the dynamic choices' options again.
    fn refresh(&mut self) {
        for at in 0..self.fields.len() {
            if !matches!(
                &self.fields[at].kind,
                Kind::Choice {
                    source: Some(_),
                    ..
                }
            ) {
                continue;
            }
            let answers = self.answers_before(at);
            if let Kind::Choice {
                options,
                index,
                source: Some(source),
            } = &mut self.fields[at].kind
            {
                let fresh = source(&answers);
                if fresh != *options {
                    let current = options.get(*index).cloned();
                    *index = current
                        .and_then(|c| fresh.iter().position(|o| *o == c))
                        .unwrap_or(0);
                    *options = fresh;
                }
            }
        }
    }

    fn submit(&mut self) -> Flow<Answers> {
        if let Some(index) = self.check(0..self.fields.len()) {
            let page = self.fields[index].page;
            if page != self.pager.page() {
                self.show_page(page);
            }
            self.focus = index;
            return Flow::Continue;
        }
        let answers = self
            .fields
            .iter()
            .filter(|field| !field.is_note())
            .filter_map(|field| Some((field.name.clone(), field.value().ok()?)))
            .collect();
        self.answer = Some(true);
        Flow::Done(Answers(answers))
    }

    fn open_picker(&mut self) {
        if let Some(picker) = self.fields[self.focus].picker() {
            picker.reopen();
            self.browsing = true;
        }
    }

    fn click(&mut self, context: &Context<'_>, row: usize, column: usize) -> Flow<Answers> {
        let layout = self.layout(context);
        match layout.hits.get(row).cloned().unwrap_or(Hit::None) {
            Hit::Buttons => {
                let submit = 2..2 + SUBMIT.len();
                let cancel = submit.end + 2..submit.end + 2 + CANCEL.len();
                if submit.contains(&column) {
                    return self.submit();
                } else if cancel.contains(&column) {
                    self.answer = Some(false);
                    return Flow::Cancel;
                }
            }
            Hit::Indicator => {
                if let Some(page) = column
                    .checked_sub(2)
                    .and_then(|c| self.pager.page_at(context.width.saturating_sub(2), c))
                {
                    self.go_to(page);
                }
            }
            Hit::Options(index, columns) => {
                self.focus = index;
                self.browsing = false;
                if let Some(option) = columns.iter().position(|r| r.contains(&column)) {
                    if let Kind::Multi { cursor, .. } = &mut self.fields[index].kind {
                        *cursor = option;
                    }
                    self.choose();
                }
            }
            Hit::Field(index) => {
                let again = index == self.focus;
                self.focus = index;
                self.browsing = false;
                let mut browse = false;
                match &mut self.fields[index].kind {
                    Kind::Toggle(on) => *on = !*on,
                    Kind::Choice { options, index, .. } if again => {
                        *index = (*index + 1) % options.len().max(1)
                    }
                    Kind::File { .. } => browse = true,
                    _ => {}
                }
                if browse {
                    self.open_picker();
                }
            }
            Hit::Picker(_) | Hit::None => {}
        }
        Flow::Continue
    }

    /// Choose or unchoose the focused multi-select's option, within its
    /// limit.
    fn choose(&mut self) {
        if let Kind::Multi {
            chosen,
            cursor,
            limit,
            ..
        } = &mut self.fields[self.focus].kind
        {
            if let Some(on) = chosen.get(*cursor).copied() {
                let count = chosen.iter().filter(|c| **c).count();
                if on || limit.is_none_or(|limit| count < limit) {
                    chosen[*cursor] = !on;
                }
            }
        }
    }

    /// Events for the open picker. A click above it closes it and lands
    /// on the form.
    fn browse(&mut self, event: &Event, context: &Context<'_>) -> Flow<Answers> {
        let mut event = event.clone();
        if let Some(mouse) = event.mouse() {
            let start = self.layout(context).hits.iter().find_map(|hit| match hit {
                Hit::Picker(start) => Some(*start),
                _ => None,
            });
            match start {
                Some(start) if mouse.row as usize >= start => {
                    let mut moved = mouse;
                    moved.row -= start as u16;
                    event = Event::Mouse(moved);
                }
                _ if mouse.is_click() => {
                    self.browsing = false;
                    return self.click(context, mouse.row as usize, mouse.column as usize);
                }
                _ => return Flow::Continue,
            }
        }
        let field = &mut self.fields[self.focus];
        let Kind::File {
            picker: Some(picker),
            path,
        } = &mut field.kind
        else {
            self.browsing = false;
            return Flow::Continue;
        };
        match picker.handle(&event, context) {
            Flow::Done(picked) => {
                *path = Some(picked);
                field.error = None;
                self.browsing = false;
            }
            Flow::Cancel => self.browsing = false,
            _ => {}
        }
        Flow::Continue
    }

    /// The view, and what each row is for a click.
    fn layout(&self, context: &Context<'_>) -> Layout {
        let width = context.width;
        let theme = &self.theme;
        let mut title = question(theme, &self.title);
        title.truncate(title.len() - 3);
        let mut out = Layout {
            lines: Vec::new(),
            hits: Vec::new(),
            cursor: None,
        };
        let label_width = self
            .fields
            .iter()
            .filter(|field| !field.is_note())
            .map(|field| rich::cells::cell_len(&field.label))
            .max()
            .unwrap_or(0);
        if let Some(submitted) = self.answer {
            if !submitted {
                title.push(text(" › cancelled", &theme.hint));
                out.push(fit(title, width), Hit::None);
                return out;
            }
            out.push(fit(title, width), Hit::None);
            for field in self.fields.iter().filter(|field| !field.is_note()) {
                let label = format!("  {:width$}  ", field.label, width = label_width);
                let line = vec![
                    text(label, &theme.hint),
                    text(field.summary(), &theme.answer),
                ];
                out.push(fit(line, width), Hit::None);
            }
            return out;
        }
        out.push(fit(title, width), Hit::None);
        let page = self.pager.page();
        let paged = self.groups.len() > 1;
        if !self.groups[page].is_empty() {
            let line = vec![plain("  "), text(self.groups[page].clone(), &theme.prompt)];
            out.push(fit(line, width), Hit::None);
        }
        let indent = " ".repeat(label_width + 4);
        for index in self.on_page(page) {
            let field = &self.fields[index];
            let focused = index == self.focus;
            if let Kind::Note(body) = &field.kind {
                let line = vec![plain("  "), text(field.label.clone(), &theme.focused)];
                out.push(fit(line, width), Hit::None);
                let body = rich::Text::new(body.clone());
                for line in context.lines_at(&body, width.saturating_sub(4)) {
                    let mut row = vec![plain("    ")];
                    row.extend(line);
                    out.push(fit(row, width), Hit::None);
                }
                continue;
            }
            let mut row: Vec<Segment> = if focused {
                vec![text(format!("{} ", theme.pointer), &theme.pointer_style)]
            } else {
                vec![plain("  ")]
            };
            let padding = label_width - rich::cells::cell_len(&field.label);
            row.push(if focused {
                text(field.label.clone(), &theme.focused)
            } else {
                plain(field.label.clone())
            });
            row.push(plain(format!("{}  ", " ".repeat(padding))));
            let start = crate::components::width(&row);
            let mut hit = Hit::Field(index);
            match &field.kind {
                Kind::Text(input) => {
                    let (segments, column) = input.field(width.saturating_sub(start));
                    row.extend(segments);
                    if focused {
                        let column = (start + column).min(width.saturating_sub(1));
                        out.cursor = Some((out.lines.len(), column));
                    }
                }
                Kind::Choice { options, index, .. } => {
                    let option = options.get(*index).cloned().unwrap_or_default();
                    if focused {
                        row.push(text("◂ ", &theme.hint));
                        row.push(text(option, &theme.answer));
                        row.push(text(" ▸", &theme.hint));
                    } else {
                        row.push(text(option, &theme.answer));
                    }
                }
                Kind::Multi {
                    options,
                    chosen,
                    cursor,
                    ..
                } => {
                    let current = theme
                        .focused
                        .combine(&rich::Style::parse("underline").expect("style"));
                    let mut columns = Vec::new();
                    let mut at = start;
                    for (k, (option, on)) in options.iter().zip(chosen).enumerate() {
                        if k > 0 {
                            row.push(plain("  "));
                            at += 2;
                        }
                        let (mark, mark_style) = if *on {
                            (&theme.checked, &theme.checked_style)
                        } else {
                            (&theme.unchecked, &theme.hint)
                        };
                        row.push(text(format!("{mark} "), mark_style));
                        let style = if focused && k == *cursor {
                            &current
                        } else if *on {
                            &theme.answer
                        } else {
                            &theme.prompt
                        };
                        row.push(text(option.clone(), style));
                        let end =
                            at + rich::cells::cell_len(mark) + 1 + rich::cells::cell_len(option);
                        columns.push(at..end);
                        at = end;
                    }
                    hit = Hit::Options(index, columns);
                }
                Kind::Toggle(on) => row.push(if *on {
                    text(format!("{} yes", theme.checked), &theme.checked_style)
                } else {
                    text(format!("{} no", theme.unchecked), &theme.hint)
                }),
                Kind::File { .. } => row.push(match field.file_path() {
                    (Some(path), true) => text(display_path(&path), &theme.answer),
                    (Some(path), false) => text(format!("({})", display_path(&path)), &theme.hint),
                    (None, _) => text("(space to browse)", &theme.hint),
                }),
                Kind::Note(_) => {}
            }
            out.push(fit(row, width), hit);
            if let (true, Kind::Text(input)) = (focused, &field.kind) {
                for line in input.suggestion_rows(width, &" ".repeat(label_width + 2)) {
                    out.push(line, Hit::Field(index));
                }
            }
            if let (
                true,
                true,
                Kind::File {
                    picker: Some(picker),
                    ..
                },
            ) = (focused, self.browsing, &field.kind)
            {
                let start = out.lines.len();
                let view = picker.render(context);
                if let Some((row, column)) = view.cursor {
                    out.cursor = Some((start + row, column));
                }
                for line in view.lines {
                    out.push(line, Hit::Picker(start));
                }
            }
            if let Some(description) = &field.description {
                let line = vec![text(format!("{indent}{description}"), &theme.hint)];
                out.push(fit(line, width), Hit::Field(index));
            }
            if let Some(error) = &field.error {
                let line = vec![text(format!("{indent}✗ {error}"), &theme.error)];
                out.push(fit(line, width), Hit::Field(index));
            }
        }
        if paged {
            let mut line = vec![plain("  ")];
            line.extend(self.pager.indicator(width.saturating_sub(2)));
            out.push(line, Hit::Indicator);
        }
        if self.mouse {
            let button = theme
                .focused
                .combine(&rich::Style::parse("reverse").expect("style"));
            let line = vec![
                plain("  "),
                text(SUBMIT, &button),
                plain("  "),
                text(CANCEL, &button),
            ];
            out.push(fit(line, width), Hit::Buttons);
        }
        let hints = if self.browsing {
            "  enter pick · esc close the picker".to_string()
        } else {
            let enter = match (self.on_last_stop(), self.pager.on_last()) {
                (true, true) => "enter submit",
                (true, false) => "enter next page",
                _ => "enter next",
            };
            let mut hints = format!("  tab/↑↓ move · {enter}");
            if paged {
                hints.push_str(" · pgup/pgdn page");
            }
            if let Some(Kind::File { .. }) = self.fields.get(self.focus).map(|f| &f.kind) {
                hints.push_str(" · space browse");
            }
            hints.push_str(" · ctrl+s submit · esc cancel");
            hints
        };
        out.push(fit(vec![text(hints, &theme.hint)], width), Hit::None);
        out
    }

    fn handle_event(&mut self, event: &Event, context: &Context<'_>) -> Flow<Answers> {
        if self.browsing {
            return self.browse(event, context);
        }
        if let Some(mouse) = event.mouse() {
            if mouse.is_click() {
                return self.click(context, mouse.row as usize, mouse.column as usize);
            }
            return Flow::Continue;
        }
        if let Event::Tick = event {
            // Provider results for every text field, not only the focused.
            for field in &mut self.fields {
                if let Kind::Text(input) = &mut field.kind {
                    input.handle(event, context);
                }
            }
            return Flow::Continue;
        }
        // While a text field shows suggestions, the keys that pick one are
        // its own: `complete` and `up` and `down` (Tab and the arrows), and
        // `submit` (Enter) with one selected.
        let field = &mut self.fields[self.focus];
        if let (Some(key), Kind::Text(input)) = (event.key(), &mut field.kind) {
            if input.suggesting() {
                match input.keymap().action(key) {
                    Some("complete" | "up" | "down") => {
                        input.handle(event, context);
                        field.error = None;
                        return Flow::Continue;
                    }
                    Some("submit") if input.has_selection() => {
                        input.accept_selected();
                        field.error = None;
                        return Flow::Continue;
                    }
                    _ => {}
                }
            }
        }
        if let Some(key) = event.key() {
            let last = self.on_last_stop();
            match self.form_keymap().action(key) {
                Some("cancel") => {
                    self.answer = Some(false);
                    return Flow::Cancel;
                }
                Some("submit") => return self.submit(),
                // Enter on a page's last field moves to the next page, and
                // on the last page's submits.
                Some("next") if last && key.code == KeyCode::Enter => {
                    return if self.pager.on_last() {
                        self.submit()
                    } else {
                        self.next_page()
                    };
                }
                Some("next") => {
                    self.move_focus(1);
                    return Flow::Continue;
                }
                Some("previous") => {
                    self.move_focus(-1);
                    return Flow::Continue;
                }
                Some("next-page") => return self.next_page(),
                Some("previous-page") => {
                    if !self.pager.on_first() {
                        self.show_page(self.pager.page() - 1);
                    }
                    return Flow::Continue;
                }
                _ => {}
            }
        }
        let action = |keymap: Keymap| {
            event
                .key()
                .and_then(|key| keymap.action(key).map(str::to_string))
        };
        match &self.fields[self.focus].kind {
            Kind::Multi { .. } => {
                let action = action(self.multi_keymap());
                if let Kind::Multi {
                    options, cursor, ..
                } = &mut self.fields[self.focus].kind
                {
                    let count = options.len().max(1);
                    match action.as_deref() {
                        Some("next-choice") => *cursor = (*cursor + 1) % count,
                        Some("previous-choice") => *cursor = (*cursor + count - 1) % count,
                        Some("choose") => self.choose(),
                        _ => return Flow::Ignored,
                    }
                }
                return Flow::Continue;
            }
            Kind::File { .. } => {
                return match action(self.file_keymap()).as_deref() {
                    Some("browse") => {
                        self.open_picker();
                        Flow::Continue
                    }
                    _ => Flow::Ignored,
                };
            }
            Kind::Note(_) => return Flow::Ignored,
            _ => {}
        }
        let choice = action(self.choice_keymap());
        let toggle = action(self.toggle_keymap());
        let field = &mut self.fields[self.focus];
        match &mut field.kind {
            Kind::Text(input) => {
                let before = input.text().to_string();
                // A key the field does not use is left to the container.
                if let Flow::Ignored = input.handle(event, context) {
                    return Flow::Ignored;
                }
                if input.text() != before {
                    field.error = None;
                }
            }
            Kind::Choice { options, index, .. } => {
                let count = options.len().max(1);
                match choice.as_deref() {
                    Some("next-option") => *index = (*index + 1) % count,
                    Some("previous-option") => *index = (*index + count - 1) % count,
                    _ => return Flow::Ignored,
                }
            }
            Kind::Toggle(on) => match toggle.as_deref() {
                Some("toggle") => *on = !*on,
                Some("yes") => *on = true,
                Some("no") => *on = false,
                _ => return Flow::Ignored,
            },
            Kind::Multi { .. } | Kind::File { .. } | Kind::Note(_) => {}
        }
        Flow::Continue
    }

    /// Ask for a path line by line: what was typed, or the picker's
    /// default.
    fn prompt_path(
        io: &mut dyn LineIo,
        label: &str,
        default: Option<PathBuf>,
    ) -> Result<Value, NotInteractive> {
        match &default {
            Some(path) => io.write(&format!("{label} [{}]: ", display_path(path))),
            None => io.write(&format!("{label}: ")),
        }
        let line = io.read_line();
        let typed = line.as_deref().map(str::trim).unwrap_or_default();
        match (typed.is_empty(), default) {
            (false, _) => Ok(Value::Path(PathBuf::from(typed))),
            (true, Some(path)) => Ok(Value::Path(path)),
            (true, None) if line.is_none() => Err(NotInteractive::Ended),
            (true, None) => Err(NotInteractive::Invalid(format!("{label} needs a path"))),
        }
    }

    /// Ask for a multi-select's options line by line, comma-separated.
    fn prompt_list(
        io: &mut dyn LineIo,
        label: &str,
        options: &[String],
        chosen: &[bool],
        limit: Option<usize>,
    ) -> Result<Value, NotInteractive> {
        let current: Vec<String> = options
            .iter()
            .zip(chosen)
            .filter(|(_, on)| **on)
            .map(|(option, _)| option.clone())
            .collect();
        io.write(&format!(
            "{label}: {} (comma-separated) [{}]: ",
            options.join(" / "),
            current.join(", ")
        ));
        let line = io.read_line().unwrap_or_default();
        let mut picked = Vec::new();
        for word in line.split(',').map(str::trim).filter(|w| !w.is_empty()) {
            let found = options
                .iter()
                .find(|option| option.eq_ignore_ascii_case(word))
                .ok_or_else(|| NotInteractive::Invalid(format!("not an option: {word:?}")))?;
            picked.push(found.clone());
        }
        if line.trim().is_empty() {
            return Ok(Value::List(current));
        }
        // In the options' order, as the terminal form gives them.
        let picked: Vec<String> = options
            .iter()
            .filter(|option| picked.contains(option))
            .cloned()
            .collect();
        match limit {
            Some(limit) if picked.len() > limit => Err(NotInteractive::Invalid(format!(
                "{label}: at most {limit} options"
            ))),
            _ => Ok(Value::List(picked)),
        }
    }
}

impl Component for Form {
    type Output = Answers;

    /// A form with no fields has nothing to ask: done at once.
    fn start(&mut self, _: &Context<'_>) -> Flow<Answers> {
        if self.fields.is_empty() {
            self.answer = Some(true);
            return Flow::Done(Answers::default());
        }
        self.refresh();
        if !self.stops().contains(&self.focus) {
            self.show_page(self.pager.page());
        }
        Flow::Continue
    }

    fn keymap(&self) -> Keymap {
        let kind = self.fields.get(self.focus).map(|field| &field.kind);
        if let (
            true,
            Some(Kind::File {
                picker: Some(picker),
                ..
            }),
        ) = (self.browsing, kind)
        {
            return picker.keymap();
        }
        let mut keymap = self.form_keymap();
        match kind {
            Some(Kind::Text(input)) => keymap.extend(input.keymap()),
            Some(Kind::Choice { .. }) => keymap.extend(self.choice_keymap()),
            Some(Kind::Multi { .. }) => keymap.extend(self.multi_keymap()),
            Some(Kind::Toggle(_)) => keymap.extend(self.toggle_keymap()),
            Some(Kind::File { .. }) => keymap.extend(self.file_keymap()),
            Some(Kind::Note(_)) | None => {}
        }
        keymap
    }

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<Answers> {
        if self.fields.is_empty() {
            return Flow::Done(Answers::default());
        }
        let flow = self.handle_event(event, context);
        self.refresh();
        flow
    }

    fn render(&self, context: &Context<'_>) -> View {
        let layout = self.layout(context);
        let view = View::new(layout.lines);
        match layout.cursor {
            Some((row, column)) => view.with_cursor(row, column),
            None => view,
        }
    }

    fn mouse(&self) -> bool {
        self.mouse
    }

    /// Ticks for text fields with a suggestion provider, so its results
    /// arrive, and for an open picker's previews.
    fn tick(&self) -> Option<Duration> {
        self.fields
            .iter()
            .filter_map(|field| match &field.kind {
                Kind::Text(input) => input.tick(),
                Kind::File {
                    picker: Some(picker),
                    ..
                } if self.browsing => picker.tick(),
                _ => None,
            })
            .min()
    }

    /// Without a terminal: a line a field, in order, each page's title
    /// before its fields and each note's text where it is.
    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<Answers>, NotInteractive> {
        io.write(&format!("{}\n", self.title));
        let mut answers = Vec::new();
        let mut page = None;
        for at in 0..self.fields.len() {
            if page != Some(self.fields[at].page) {
                page = Some(self.fields[at].page);
                let title = &self.groups[self.fields[at].page];
                if !title.is_empty() {
                    io.write(&format!("{title}\n"));
                }
            }
            let so_far = Answers(answers.clone());
            let field = &mut self.fields[at];
            let value = match &mut field.kind {
                Kind::Text(input) => match input.prompt(io)? {
                    Some(text) => Value::Text(text),
                    None => return Ok(None),
                },
                Kind::Note(body) => {
                    io.write(&format!("{}\n{body}\n", field.label));
                    continue;
                }
                Kind::Choice {
                    options,
                    index,
                    source,
                } => {
                    if let Some(source) = source {
                        *options = source(&so_far);
                        *index = 0;
                    }
                    let current = options.get(*index).cloned().unwrap_or_default();
                    io.write(&format!(
                        "{}: {} [{current}]: ",
                        field.label,
                        options.join(" / ")
                    ));
                    // At the end of input, the current option answers, as
                    // for an empty line.
                    let line = io.read_line().unwrap_or_default();
                    let line = line.trim();
                    if line.is_empty() {
                        Value::Text(current)
                    } else {
                        let found = options
                            .iter()
                            .find(|option| option.eq_ignore_ascii_case(line))
                            .ok_or_else(|| {
                                NotInteractive::Invalid(format!("not an option: {line:?}"))
                            })?;
                        Value::Text(found.clone())
                    }
                }
                Kind::Multi {
                    options,
                    chosen,
                    limit,
                    ..
                } => Form::prompt_list(io, &field.label, options, chosen, *limit)?,
                Kind::Toggle(on) => {
                    io.write(&format!(
                        "{} [{}]: ",
                        field.label,
                        if *on { "Y/n" } else { "y/N" }
                    ));
                    let line = io.read_line().unwrap_or_default();
                    Value::Flag(match line.trim().to_lowercase().as_str() {
                        "" => *on,
                        "y" | "yes" => true,
                        "n" | "no" => false,
                        other => {
                            return Err(NotInteractive::Invalid(format!(
                                "not yes or no: {other:?}"
                            )))
                        }
                    })
                }
                Kind::File { .. } => {
                    let default = field.file_path().0;
                    Form::prompt_path(io, &field.label, default)?
                }
            };
            answers.push((field.name.clone(), value));
        }
        Ok(Some(Answers(answers)))
    }
}
