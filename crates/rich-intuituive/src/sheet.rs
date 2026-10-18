//! Stylesheets: a CSS subset that styles and lays out nodes.
//!
//! ```css
//! /* A comment. */
//! table { color: $muted; }
//! #preview { size: 3fr; padding: 0 1; border: round $accent; }
//! .danger:focus { background: red; text-style: bold; }
//! panel label:hover { text-style: underline; }
//! ```
//!
//! A selector names a node's kind (`label`, `table`, `panel`, `column`:
//! what the inspector shows), its [name](crate::Node::name) (`#preview`),
//! its [classes](crate::Node::class) (`.danger`) and its states (`:focus`,
//! `:focus-within`, `:hover`, `:selected`, `:disabled`, `:error`), with
//! descendants separated by spaces. The more specific rule wins, then the
//! later one.
//!
//! Colours and text styles follow the states and
//! [`class_when`](crate::Node::class_when); layout (sizes, padding, the
//! border, `display`, `dock`, gaps and grid tracks) is decided by the kind,
//! name and fixed classes, again whenever the terminal's size crosses an
//! `@media` rule's bounds. What code sets wins over the sheet: `.fixed(3)`
//! beats `size: 5`.
//!
//! ```css
//! /* Rules that hold while the app is at most 80 columns wide. */
//! @media (max-width: 80) {
//!     #sidebar { display: none; }
//!     #main { grid-columns: 1fr; }
//! }
//! ```
//!
//! An `@media` condition is `(min-width: N)`, `(max-width: N)`,
//! `(min-height: N)` or `(max-height: N)`, in cells, inclusive as in CSS,
//! and several are joined with `and`. Width and height are the app's
//! [`viewport`](crate::viewport()).

use std::fmt;
use std::rc::Rc;

use rich::{Color, Overflow, Style};

use crate::border::{Border, Edge, Sides, Title};
use crate::layout::{Align, AlignContent, Justify, Size, TextAlign};
use crate::reactive::NodeId;

/// A parsed stylesheet: give it to [`App::stylesheet`](crate::App::stylesheet).
#[derive(Clone, Debug, Default)]
pub struct Stylesheet {
    /// One per selector, most specific (then latest) last.
    rules: Vec<Rule>,
}

/// Where a stylesheet stopped parsing, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl fmt::Display for SheetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}:{}: {}", self.line, self.column, self.message)
    }
}

impl std::error::Error for SheetError {}

#[derive(Clone, Debug)]
struct Rule {
    selector: Selector,
    decls: Rc<Decls>,
    /// The `@media` block it is in, if any.
    media: Option<Rc<Media>>,
}

/// An `@media` block's conditions, in cells, inclusive; all must hold.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Media {
    min_width: Option<u16>,
    max_width: Option<u16>,
    min_height: Option<u16>,
    max_height: Option<u16>,
}

impl Media {
    fn holds(&self, (width, height): (u16, u16)) -> bool {
        self.min_width.is_none_or(|n| width >= n)
            && self.max_width.is_none_or(|n| width <= n)
            && self.min_height.is_none_or(|n| height >= n)
            && self.max_height.is_none_or(|n| height <= n)
    }
}

impl Rule {
    /// Whether its `@media` conditions, if any, hold at `viewport`.
    fn applies(&self, viewport: (u16, u16)) -> bool {
        self.media
            .as_ref()
            .is_none_or(|media| media.holds(viewport))
    }
}

/// Compounds from the outermost ancestor to the node itself.
#[derive(Clone, Debug)]
struct Selector {
    parts: Vec<Compound>,
}

#[derive(Clone, Debug, Default)]
struct Compound {
    kind: Option<String>,
    name: Option<String>,
    classes: Vec<String>,
    states: Vec<State>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Focus,
    FocusWithin,
    Hover,
    Selected,
    Disabled,
    /// `:error`, or CSS's `:invalid`.
    Error,
}

/// A colour, or a theme style's colour by name (`$accent`).
#[derive(Clone, Debug, PartialEq)]
enum Paint {
    Color(Color),
    Var(String),
}

/// Which edge a node keeps to in a [`column`](crate::column) (`top`,
/// `bottom`) or [`row`](crate::row) (`left`, `right`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dock {
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Clone, Debug, Default)]
struct Decls {
    color: Option<Paint>,
    background: Option<Paint>,
    text_style: Option<Style>,
    /// `Some(None)`: `border: none`.
    border: Option<Option<(Border, Option<Paint>)>>,
    border_title: Option<String>,
    title_align: Option<Align>,
    subtitle: Option<String>,
    subtitle_align: Option<Align>,
    sides: Option<Sides>,
    collapse: Option<bool>,
    text_align: Option<TextAlign>,
    text_wrap: Option<bool>,
    text_overflow: Option<Overflow>,
    size: Option<Size>,
    min: Option<u16>,
    max: Option<u16>,
    padding: Option<[u16; 4]>,
    gap: Option<u16>,
    grid_columns: Option<Vec<Size>>,
    grid_rows: Option<Vec<Size>>,
    hidden: Option<bool>,
    dock: Option<Dock>,
    align_items: Option<Align>,
    /// `Some(None)` is `align-self: auto`, which undoes an earlier rule's.
    align_self: Option<Option<Align>>,
    justify: Option<Justify>,
    wrap: Option<bool>,
    align_content: Option<AlignContent>,
}

/// What a sheet decides about a node's layout.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Layout {
    pub size: Option<Size>,
    pub min: Option<u16>,
    pub max: Option<u16>,
    /// Top, right, bottom, left.
    pub padding: [u16; 4],
    pub border: Option<(Border, Style)>,
    /// `border: none` was said, which takes a panel's own border away.
    pub no_border: bool,
    pub border_title: Option<String>,
    /// `border-title-align`, `border-subtitle` (the bottom edge's title)
    /// and `border-subtitle-align`.
    pub title_align: Option<Align>,
    pub subtitle: Option<String>,
    pub subtitle_align: Option<Align>,
    /// `border-sides`.
    pub sides: Option<Sides>,
    /// `border-collapse: collapse`.
    pub collapse: Option<bool>,
    /// `text-align`, `text-wrap` and `text-overflow`.
    pub text_align: Option<TextAlign>,
    pub text_wrap: Option<bool>,
    pub text_overflow: Option<Overflow>,
    pub hidden: bool,
    pub dock: Option<Dock>,
    pub gap: Option<u16>,
    pub grid_columns: Option<Vec<Size>>,
    pub grid_rows: Option<Vec<Size>>,
    pub align_items: Option<Align>,
    pub align_self: Option<Align>,
    pub justify: Option<Justify>,
    pub wrap: Option<bool>,
    pub align_content: Option<AlignContent>,
}

impl Layout {
    /// Cells the border and padding take: top, right, bottom, left. Only a
    /// node with a border has a box (and titles on it), each side of it a
    /// cell when it is drawn or holds a title.
    pub(crate) fn insets(&self) -> [u16; 4] {
        let frame = match &self.border {
            Some((kind, _)) => crate::border::insets(*kind, self.sides(), &self.titles()),
            None => [0; 4],
        };
        let [t, r, b, l] = self.padding;
        let mut out = [t, r, b, l];
        for (side, edge) in out.iter_mut().zip(frame) {
            *side = side.saturating_add(edge);
        }
        out
    }

    /// The sides its border draws: all, unless `border-sides` says.
    pub(crate) fn sides(&self) -> Sides {
        self.sides.unwrap_or_default()
    }

    /// Its `border-title` and `border-subtitle`, on the top and bottom
    /// edges.
    pub(crate) fn titles(&self) -> Vec<Title> {
        let title = |text: &Option<String>, edge, align: Option<Align>| {
            text.as_ref()
                .filter(|text| !text.is_empty())
                .map(|text| Title::new(edge, align.unwrap_or(Align::Start), text.clone()))
        };
        title(&self.border_title, Edge::Top, self.title_align)
            .into_iter()
            .chain(title(&self.subtitle, Edge::Bottom, self.subtitle_align))
            .collect()
    }
}

/// A node as selectors see it.
pub(crate) struct Element {
    pub id: NodeId,
    pub kind: &'static str,
    pub name: Option<String>,
    /// Fixed classes, then the [`class_when`](crate::Node::class_when)
    /// classes that hold now.
    pub classes: Vec<String>,
    pub fixed: usize,
    /// Every class it may have while a condition holds.
    pub maybe: Vec<String>,
    pub selected: bool,
    pub disabled: bool,
    pub error: bool,
}

/// How strictly a selector is matched.
#[derive(Clone, Copy)]
enum Mode<'a> {
    /// For layout: no states, fixed classes only.
    Fixed,
    /// Could it ever match: states and conditional classes as if they held.
    Loose,
    /// Now.
    Exact(&'a States<'a>),
}

/// The states selectors ask about, as the app has them now.
pub(crate) struct States<'a> {
    pub focus: &'a [NodeId],
    pub hover: &'a [NodeId],
}

/// Fetch a theme style by name, for `$name`.
pub(crate) type Vars<'a> = &'a dyn Fn(&str) -> Option<Style>;

impl Stylesheet {
    /// Parse `css`, or say where and why it does not parse.
    ///
    /// ```
    /// use intuituive::Stylesheet;
    ///
    /// assert!(Stylesheet::parse("label { color: red; }").is_ok());
    /// let error = Stylesheet::parse("label {\n  colour: red;\n}").unwrap_err();
    /// assert_eq!((error.line, error.message.as_str()), (2, "unknown property `colour`"));
    /// ```
    pub fn parse(css: &str) -> Result<Stylesheet, SheetError> {
        let plain = without_comments(css)
            .map_err(|at| Parser::new(css).error(at, "a comment that never ends"))?;
        Parser::new(&plain).sheet()
    }

    /// Whether it has no rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Whether each of its `@media` blocks holds at `viewport`, in order:
    /// what the layout depends on beyond the nodes. Empty when it has none.
    pub(crate) fn media_key(&self, viewport: (u16, u16)) -> Vec<bool> {
        let mut seen: Vec<&Rc<Media>> = Vec::new();
        for media in self.rules.iter().filter_map(|rule| rule.media.as_ref()) {
            if !seen.iter().any(|m| Rc::ptr_eq(m, media)) {
                seen.push(media);
            }
        }
        seen.iter().map(|media| media.holds(viewport)).collect()
    }

    /// Whether it has `@media` rules.
    pub(crate) fn has_media(&self) -> bool {
        self.rules.iter().any(|rule| rule.media.is_some())
    }

    /// Whether a rule with colours or a text style could ever match the
    /// last node of `chain`, and whether such a rule asks about the focus
    /// and the pointer: what the node must draw again for.
    pub(crate) fn watches(&self, chain: &[Element], viewport: (u16, u16)) -> (bool, bool, bool) {
        let mut out = (false, false, false);
        for rule in self.rules.iter().filter(|rule| rule.applies(viewport)) {
            let d = &rule.decls;
            if d.color.is_none() && d.background.is_none() && d.text_style.is_none() {
                continue;
            }
            if !rule.selector.matches(chain, Mode::Loose) {
                continue;
            }
            out.0 = true;
            for part in &rule.selector.parts {
                for state in &part.states {
                    match state {
                        State::Hover => out.2 = true,
                        State::Focus | State::FocusWithin => out.1 = true,
                        _ => {}
                    }
                }
            }
        }
        out
    }

    /// The layout the rules without states or conditional classes give
    /// the last node of `chain`, at `viewport`.
    pub(crate) fn layout(&self, chain: &[Element], vars: Vars, viewport: (u16, u16)) -> Layout {
        let mut out = Layout::default();
        let mut border: Option<Option<(Border, Option<Paint>)>> = None;
        for rule in self.rules.iter().filter(|rule| rule.applies(viewport)) {
            if !rule.selector.matches(chain, Mode::Fixed) {
                continue;
            }
            let d = &rule.decls;
            out.size = d.size.or(out.size);
            out.min = d.min.or(out.min);
            out.max = d.max.or(out.max);
            out.padding = d.padding.unwrap_or(out.padding);
            if d.border.is_some() {
                border = d.border.clone();
            }
            if d.border_title.is_some() {
                out.border_title = d.border_title.clone();
            }
            if d.subtitle.is_some() {
                out.subtitle = d.subtitle.clone();
            }
            out.title_align = d.title_align.or(out.title_align);
            out.subtitle_align = d.subtitle_align.or(out.subtitle_align);
            out.sides = d.sides.or(out.sides);
            out.collapse = d.collapse.or(out.collapse);
            out.text_align = d.text_align.or(out.text_align);
            out.text_wrap = d.text_wrap.or(out.text_wrap);
            out.text_overflow = d.text_overflow.or(out.text_overflow);
            out.hidden = d.hidden.unwrap_or(out.hidden);
            out.dock = d.dock.or(out.dock);
            out.gap = d.gap.or(out.gap);
            out.align_items = d.align_items.or(out.align_items);
            if let Some(align_self) = d.align_self {
                out.align_self = align_self;
            }
            out.justify = d.justify.or(out.justify);
            out.wrap = d.wrap.or(out.wrap);
            out.align_content = d.align_content.or(out.align_content);
            if d.grid_columns.is_some() {
                out.grid_columns = d.grid_columns.clone();
            }
            if d.grid_rows.is_some() {
                out.grid_rows = d.grid_rows.clone();
            }
        }
        out.no_border = matches!(border, Some(None));
        out.border = border.flatten().map(|(kind, paint)| {
            let style = paint
                .and_then(|paint| paint_style(&paint, vars))
                .map(|(color, style)| style.unwrap_or_else(|| Style::from_color(Some(color), None)))
                .unwrap_or_default();
            (kind, style)
        });
        out
    }

    /// The colours and text style the rules give the last node of `chain`
    /// in `states`, at `viewport`, if any do.
    pub(crate) fn style(
        &self,
        chain: &[Element],
        states: &States,
        vars: Vars,
        viewport: (u16, u16),
    ) -> Option<Style> {
        let mut color = None;
        let mut background = None;
        let mut text: Option<Style> = None;
        let mut any = false;
        for rule in self.rules.iter().filter(|rule| rule.applies(viewport)) {
            if !rule.selector.matches(chain, Mode::Exact(states)) {
                continue;
            }
            let d = &rule.decls;
            if let Some(paint) = &d.color {
                color = paint_style(paint, vars).map(|(c, _)| c).or(color);
                any = true;
            }
            if let Some(paint) = &d.background {
                background = paint_style(paint, vars).map(|(c, _)| c).or(background);
                any = true;
            }
            // A later rule's text style replaces an earlier one's.
            if let Some(style) = &d.text_style {
                text = Some(style.clone());
                any = true;
            }
        }
        any.then(|| {
            let colors = Style::from_color(color, background);
            match text {
                Some(text) => text.combine(&colors),
                None => colors,
            }
        })
    }
}

/// A paint's colour, and with `$name`, the theme style it named.
fn paint_style(paint: &Paint, vars: Vars) -> Option<(Color, Option<Style>)> {
    match paint {
        Paint::Color(color) => Some((color.clone(), None)),
        Paint::Var(name) => {
            let style = vars(name)?;
            let color = style.color().or(style.bgcolor()).cloned()?;
            Some((color, Some(style)))
        }
    }
}

impl Selector {
    fn specificity(&self) -> (usize, usize, usize) {
        self.parts.iter().fold((0, 0, 0), |(a, b, c), part| {
            (
                a + usize::from(part.name.is_some()),
                b + part.classes.len() + part.states.len(),
                c + usize::from(part.kind.is_some()),
            )
        })
    }

    /// Whether this matches the last node of `chain`.
    fn matches(&self, chain: &[Element], states: Mode) -> bool {
        let Some((subject, ancestors)) = self.parts.split_last() else {
            return false;
        };
        let Some((last, above)) = chain.split_last() else {
            return false;
        };
        if !subject.matches(last, states) {
            return false;
        }
        // Each ancestor compound, right to left, on some node further up.
        let mut up = above.len();
        for part in ancestors.iter().rev() {
            loop {
                if up == 0 {
                    return false;
                }
                up -= 1;
                if part.matches(&above[up], states) {
                    break;
                }
            }
        }
        true
    }
}

impl Compound {
    fn matches(&self, element: &Element, mode: Mode) -> bool {
        if self
            .kind
            .as_deref()
            .is_some_and(|kind| kind != element.kind)
        {
            return false;
        }
        if self.name.is_some() && self.name != element.name {
            return false;
        }
        let has = |class: &String| match mode {
            Mode::Fixed => element.classes[..element.fixed].contains(class),
            Mode::Loose => element.classes.contains(class) || element.maybe.contains(class),
            Mode::Exact(_) => element.classes.contains(class),
        };
        if !self.classes.iter().all(has) {
            return false;
        }
        let states = match mode {
            Mode::Fixed => return self.states.is_empty(),
            Mode::Loose => return true,
            Mode::Exact(states) => states,
        };
        self.states.iter().all(|state| match state {
            State::Focus => states.focus.last() == Some(&element.id),
            State::FocusWithin => states.focus.contains(&element.id),
            State::Hover => states.hover.contains(&element.id),
            State::Selected => element.selected,
            State::Disabled => element.disabled,
            State::Error => element.error,
        })
    }
}

struct Parser<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Parser<'a> {
        Parser { text, at: 0 }
    }

    fn error(&self, at: usize, message: impl Into<String>) -> SheetError {
        let before = &self.text[..at.min(self.text.len())];
        let line = before.matches('\n').count() + 1;
        let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
        SheetError {
            line,
            column,
            message: message.into(),
        }
    }

    fn rest(&self) -> &'a str {
        &self.text[self.at..]
    }

    /// Skip spaces (comments are spaces by now).
    fn skip(&mut self) {
        let rest = self.rest();
        self.at += rest.len() - rest.trim_start().len();
    }

    fn sheet(mut self) -> Result<Stylesheet, SheetError> {
        let mut rules: Vec<(Rule, (usize, usize, usize), usize)> = Vec::new();
        self.rules(&mut rules, None)?;
        rules.sort_by_key(|(_, specificity, order)| (*specificity, *order));
        Ok(Stylesheet {
            rules: rules.into_iter().map(|(rule, _, _)| rule).collect(),
        })
    }

    /// Rules up to the end of the text, or, inside an `@media` block
    /// (`media`, with where it starts), up to the `}` that closes it.
    fn rules(
        &mut self,
        rules: &mut Vec<(Rule, (usize, usize, usize), usize)>,
        media: Option<(Rc<Media>, usize)>,
    ) -> Result<(), SheetError> {
        loop {
            self.skip();
            if self.rest().is_empty() {
                return match media {
                    Some((_, at)) => Err(self.error(at, "an `@media` block with no `}`")),
                    None => Ok(()),
                };
            }
            let start = self.at;
            if media.is_some() && self.rest().starts_with('}') {
                self.at += 1;
                return Ok(());
            }
            if self.rest().starts_with('@') {
                if media.is_some() {
                    return Err(self.error(start, "an `@media` block inside another"));
                }
                let Some(open) = find_unquoted(self.rest(), '{') else {
                    return Err(self.error(start, "an `@media` rule with no `{`"));
                };
                let head = self.rest()[..open].trim_end();
                // `@media (max-width: 80)`, or `@media(max-width: 80)`.
                let query = match head.strip_prefix("@media") {
                    Some(query)
                        if query.is_empty()
                            || query.starts_with(char::is_whitespace)
                            || query.starts_with('(') =>
                    {
                        query
                    }
                    _ => {
                        let word = head.split_whitespace().next().unwrap_or(head);
                        return Err(self.error(
                            start,
                            format!("`{word}`: only `@media` rules are supported"),
                        ));
                    }
                };
                let conditions = parse_media(query.trim()).map_err(|e| self.error(start, e))?;
                self.at += open + 1;
                self.rules(rules, Some((Rc::new(conditions), start)))?;
                continue;
            }
            let Some(open) = find_unquoted(self.rest(), '{') else {
                return Err(self.error(start, "a selector with no `{`"));
            };
            let head = &self.rest()[..open];
            if head.contains('}') {
                return Err(self.error(start + head.find('}').unwrap_or(0), "a `}` with no rule"));
            }
            let selectors = head
                .split(',')
                .map(|s| parse_selector(s.trim()).map_err(|e| self.error(start, e)))
                .collect::<Result<Vec<_>, _>>()?;
            self.at += open + 1;
            let body_start = self.at;
            let Some(close) = find_unquoted(self.rest(), '}') else {
                return Err(self.error(start, "a rule with no `}`"));
            };
            let decls =
                Rc::new(self.decls(body_start, &self.text[body_start..body_start + close])?);
            self.at = body_start + close + 1;
            for selector in selectors {
                let specificity = selector.specificity();
                let order = rules.len();
                rules.push((
                    Rule {
                        selector,
                        decls: Rc::clone(&decls),
                        media: media.as_ref().map(|(media, _)| Rc::clone(media)),
                    },
                    specificity,
                    order,
                ));
            }
        }
    }

    fn decls(&self, start: usize, body: &str) -> Result<Decls, SheetError> {
        let mut decls = Decls::default();
        let mut offset = 0;
        for part in split_unquoted(body, ';') {
            let here = start + offset;
            offset += part.len() + 1;
            let line = part.trim();
            if line.is_empty() {
                continue;
            }
            let at = here
                + part
                    .find(line.split(':').next().unwrap_or("").trim())
                    .unwrap_or(0);
            let Some((property, value)) = line.split_once(':') else {
                return Err(self.error(at, format!("`{line}` is not `property: value`")));
            };
            declare(&mut decls, property.trim(), value.trim()).map_err(|e| self.error(at, e))?;
        }
        Ok(decls)
    }
}

/// `css` with each comment blanked to spaces (its line breaks kept), so
/// offsets, lines and columns still point into the text as written, and a
/// brace or a semicolon in a comment means nothing. A comment is only one
/// outside a quoted string. The error is where a comment never ends.
fn without_comments(css: &str) -> Result<String, usize> {
    let bytes = css.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut quote = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'"' || b == b'\'' => quote = Some(b),
            None if bytes[i..].starts_with(b"/*") => {
                let end = css[i + 2..].find("*/").ok_or(i)? + i + 4;
                out.extend(
                    bytes[i..end]
                        .iter()
                        .map(|&c| if c == b'\n' { c } else { b' ' }),
                );
                i = end;
                continue;
            }
            None => {}
        }
        out.push(b);
        i += 1;
    }
    // Only whole comments were replaced, by ASCII: still UTF-8.
    Ok(String::from_utf8(out).expect("comments blanked whole"))
}

/// Where `c` first appears in `text` outside a quoted string.
fn find_unquoted(text: &str, c: char) -> Option<usize> {
    let mut quote = None;
    text.char_indices().find_map(|(i, ch)| {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => {}
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if ch == c => return Some(i),
            None => {}
        }
        None
    })
}

/// `text` split at each `c` outside a quoted string.
fn split_unquoted(text: &str, c: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut rest = text;
    while let Some(i) = find_unquoted(rest, c) {
        parts.push(&rest[..i]);
        rest = &rest[i + c.len_utf8()..];
    }
    parts.push(rest);
    parts
}

fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// An `@media` query: conditions such as `(max-width: 80)`, joined by
/// `and`.
fn parse_media(query: &str) -> Result<Media, String> {
    let usage = "`@media` takes conditions such as `(max-width: 80)`, joined by `and`";
    let mut media = Media::default();
    let mut rest = query;
    loop {
        let Some(inner) = rest.strip_prefix('(') else {
            return Err(usage.into());
        };
        let Some(close) = inner.find(')') else {
            return Err(format!("`{query}`: a condition with no `)`"));
        };
        let condition = &inner[..close];
        let Some((feature, value)) = condition.split_once(':') else {
            return Err(format!(
                "`({condition})`: a condition is `(feature: cells)`, such as `(max-width: 80)`"
            ));
        };
        let value = value.trim();
        let cells: u16 = value
            .parse()
            .map_err(|_| format!("`{value}` is not a number of cells"))?;
        let slot = match feature.trim() {
            "min-width" => &mut media.min_width,
            "max-width" => &mut media.max_width,
            "min-height" => &mut media.min_height,
            "max-height" => &mut media.max_height,
            other => {
                return Err(format!(
                "unknown media feature `{other}`: min-width, max-width, min-height or max-height"
            ))
            }
        };
        *slot = Some(cells);
        rest = inner[close + 1..].trim_start();
        if rest.is_empty() {
            return Ok(media);
        }
        match rest.strip_prefix("and") {
            Some(after) if after.starts_with(char::is_whitespace) || after.starts_with('(') => {
                rest = after.trim_start();
            }
            _ => return Err(usage.into()),
        }
    }
}

fn parse_selector(text: &str) -> Result<Selector, String> {
    if text.is_empty() {
        return Err("an empty selector".into());
    }
    let mut parts = Vec::new();
    for word in text.split_whitespace() {
        if word.contains(['>', '+', '~']) {
            return Err(format!(
                "`{word}`: only descendant selectors (spaces) are supported"
            ));
        }
        parts.push(parse_compound(word)?);
    }
    Ok(Selector { parts })
}

fn parse_compound(word: &str) -> Result<Compound, String> {
    let mut compound = Compound::default();
    // Split before each `#`, `.` and `:`, keeping the mark.
    let mut pieces = Vec::new();
    let mut start = 0;
    for (i, c) in word.char_indices() {
        if i > 0 && matches!(c, '#' | '.' | ':') && !word[..i].ends_with(':') {
            pieces.push(&word[start..i]);
            start = i;
        }
    }
    pieces.push(&word[start..]);
    for piece in pieces {
        if let Some(name) = piece.strip_prefix('#') {
            if !is_ident(name) {
                return Err(format!("`{piece}` is not a name"));
            }
            compound.name = Some(name.to_string());
        } else if let Some(class) = piece.strip_prefix('.') {
            if !is_ident(class) {
                return Err(format!("`{piece}` is not a class"));
            }
            compound.classes.push(class.to_string());
        } else if let Some(state) = piece.strip_prefix(':') {
            compound.states.push(match state {
                "focus" => State::Focus,
                "focus-within" => State::FocusWithin,
                "hover" => State::Hover,
                "selected" => State::Selected,
                "disabled" => State::Disabled,
                "error" | "invalid" => State::Error,
                _ => return Err(format!("unknown state `:{state}`")),
            });
        } else if piece == "*" {
        } else if is_ident(piece) {
            compound.kind = Some(piece.to_string());
        } else {
            return Err(format!("`{piece}` is not a selector"));
        }
    }
    Ok(compound)
}

fn paint(value: &str) -> Result<Paint, String> {
    if let Some(name) = value.strip_prefix('$') {
        return if is_ident(name) {
            Ok(Paint::Var(name.to_string()))
        } else {
            Err(format!("`{value}` is not a theme variable"))
        };
    }
    Color::parse(value)
        .map(Paint::Color)
        .map_err(|_| format!("`{value}` is not a colour"))
}

fn size(value: &str) -> Result<Size, String> {
    let bad = || format!("`{value}` is not a size (3, 50%, 1/3, 2fr or auto)");
    if value == "auto" {
        Ok(Size::Auto)
    } else if let Some((a, b)) = value.split_once('/') {
        match (a.trim().parse(), b.trim().parse()) {
            (Ok(a), Ok(b)) => Ok(Size::Ratio(a, b)),
            _ => Err(bad()),
        }
    } else if let Some(n) = value.strip_suffix('%') {
        n.parse().map(Size::Percent).map_err(|_| bad())
    } else if let Some(n) = value.strip_suffix("fr") {
        n.parse().map(Size::Flex).map_err(|_| bad())
    } else {
        value.parse().map(Size::Fixed).map_err(|_| bad())
    }
}

fn cells(value: &str) -> Result<u16, String> {
    value
        .parse()
        .map_err(|_| format!("`{value}` is not a number of cells"))
}

fn declare(decls: &mut Decls, property: &str, value: &str) -> Result<(), String> {
    match property {
        "color" => decls.color = Some(paint(value)?),
        "background" => decls.background = Some(paint(value)?),
        "text-style" => {
            let words: Vec<&str> = value.split_whitespace().collect();
            for word in &words {
                if !matches!(
                    *word,
                    "bold"
                        | "dim"
                        | "italic"
                        | "underline"
                        | "reverse"
                        | "strike"
                        | "blink"
                        | "none"
                ) {
                    return Err(format!("`{word}` is not a text style"));
                }
            }
            // `none` turns every attribute off, so it clears what the
            // node would otherwise take from around it.
            let words: Vec<&str> = words.into_iter().filter(|w| *w != "none").collect();
            decls.text_style = Some(if words.is_empty() {
                Style::parse(
                    "not bold not dim not italic not underline not reverse not strike not blink",
                )
                .map_err(|e| e.to_string())?
            } else {
                Style::parse(&words.join(" ")).map_err(|e| e.to_string())?
            });
        }
        "border" => {
            let mut words = value.split_whitespace();
            let kind = match words.next() {
                Some("none") => {
                    decls.border = Some(None);
                    return Ok(());
                }
                Some(word) if Border::parse(word).is_some() => {
                    Border::parse(word).expect("checked")
                }
                _ => return Err(format!(
                    "`{value}`: a border is none, round, square, heavy, double, ascii, a dashed \
                         kind (dashed, light-triple-dashed, ...) or quadrant-inside or \
                         quadrant-outside, then a colour"
                )),
            };
            let color = words.next().map(paint).transpose()?;
            if let Some(extra) = words.next() {
                return Err(format!("`{extra}`: a border takes a box and one colour"));
            }
            decls.border = Some(Some((kind, color)));
        }
        "border-title" | "border-subtitle" => {
            let title = value.trim_matches(|c| c == '"' || c == '\'').to_string();
            if property == "border-title" {
                decls.border_title = Some(title);
            } else {
                decls.subtitle = Some(title);
            }
        }
        "border-title-align" | "border-subtitle-align" => {
            let align = match value {
                "left" | "start" => Align::Start,
                "center" => Align::Center,
                "right" | "end" => Align::End,
                _ => return Err(format!("`{value}`: {property} is left, center or right")),
            };
            if property == "border-title-align" {
                decls.title_align = Some(align);
            } else {
                decls.subtitle_align = Some(align);
            }
        }
        "border-sides" => {
            let mut sides = Sides::NONE;
            for word in value.split_whitespace() {
                sides |= match word {
                    "top" => Sides::TOP,
                    "right" => Sides::RIGHT,
                    "bottom" => Sides::BOTTOM,
                    "left" => Sides::LEFT,
                    "all" => Sides::ALL,
                    "none" => Sides::NONE,
                    _ => {
                        return Err(format!(
                            "`{word}`: border-sides names top, right, bottom and left (or all, or none)"
                        ))
                    }
                };
            }
            decls.sides = Some(sides);
        }
        "border-collapse" => {
            decls.collapse = Some(match value {
                "collapse" => true,
                "separate" => false,
                _ => {
                    return Err(format!(
                        "`{value}`: border-collapse is collapse or separate"
                    ))
                }
            })
        }
        "text-align" => {
            decls.text_align = Some(match value {
                "left" | "start" => TextAlign::Left,
                "center" => TextAlign::Center,
                "right" | "end" => TextAlign::Right,
                "justify" => TextAlign::Justify,
                _ => {
                    return Err(format!(
                        "`{value}`: text-align is left, center, right or justify"
                    ))
                }
            })
        }
        "text-wrap" => {
            decls.text_wrap = Some(match value {
                "wrap" => true,
                "nowrap" => false,
                _ => return Err(format!("`{value}`: text-wrap is wrap or nowrap")),
            })
        }
        "text-overflow" => {
            decls.text_overflow = Some(match value {
                "clip" => Overflow::Crop,
                "ellipsis" => Overflow::Ellipsis,
                "fold" => Overflow::Fold,
                _ => {
                    return Err(format!(
                        "`{value}`: text-overflow is clip, ellipsis or fold"
                    ))
                }
            })
        }
        "size" => decls.size = Some(size(value)?),
        "min-size" => decls.min = Some(cells(value)?),
        "max-size" => decls.max = Some(cells(value)?),
        "padding" => {
            let n = value
                .split_whitespace()
                .map(cells)
                .collect::<Result<Vec<_>, _>>()?;
            decls.padding = Some(match n[..] {
                [all] => [all; 4],
                [v, h] => [v, h, v, h],
                [t, h, b] => [t, h, b, h],
                [t, r, b, l] => [t, r, b, l],
                _ => return Err(format!("`{value}`: padding takes one to four numbers")),
            });
        }
        "gap" => decls.gap = Some(cells(value)?),
        "grid-columns" | "grid-rows" => {
            let sizes = value
                .split_whitespace()
                .map(size)
                .collect::<Result<Vec<_>, _>>()?;
            if sizes.is_empty() {
                return Err(format!("`{property}` needs at least one size"));
            }
            if property == "grid-columns" {
                decls.grid_columns = Some(sizes);
            } else {
                decls.grid_rows = Some(sizes);
            }
        }
        "display" => {
            decls.hidden = Some(match value {
                "none" => true,
                "block" | "flex" => false,
                _ => return Err(format!("`{value}`: display is none or block")),
            })
        }
        "dock" => {
            decls.dock = Some(match value {
                "top" => Dock::Top,
                "bottom" => Dock::Bottom,
                "left" => Dock::Left,
                "right" => Dock::Right,
                _ => return Err(format!("`{value}`: dock is top, bottom, left or right")),
            })
        }
        "align-items" => decls.align_items = Some(align(value, false)?.expect("not auto")),
        "align-self" => decls.align_self = Some(align(value, true)?),
        "justify-content" => {
            decls.justify = Some(match value {
                "start" | "flex-start" | "normal" => Justify::Start,
                "center" => Justify::Center,
                "end" | "flex-end" => Justify::End,
                "space-between" => Justify::SpaceBetween,
                "space-around" => Justify::SpaceAround,
                "space-evenly" => Justify::SpaceEvenly,
                _ => {
                    return Err(format!(
                        "`{value}`: justify-content is flex-start, center, flex-end, \
                         space-between, space-around or space-evenly"
                    ))
                }
            })
        }
        "flex-wrap" => {
            decls.wrap = Some(match value {
                "nowrap" => false,
                "wrap" => true,
                _ => return Err(format!("`{value}`: flex-wrap is nowrap or wrap")),
            })
        }
        "align-content" => {
            decls.align_content = Some(match value {
                "start" | "flex-start" => AlignContent::Start,
                "center" => AlignContent::Center,
                "end" | "flex-end" => AlignContent::End,
                "space-between" => AlignContent::SpaceBetween,
                "space-around" => AlignContent::SpaceAround,
                "space-evenly" => AlignContent::SpaceEvenly,
                "stretch" | "normal" => AlignContent::Stretch,
                _ => {
                    return Err(format!(
                        "`{value}`: align-content is flex-start, center, flex-end, \
                         space-between, space-around, space-evenly or stretch"
                    ))
                }
            })
        }
        _ => return Err(format!("unknown property `{property}`")),
    }
    Ok(())
}

/// An `align-items` or `align-self` value; `auto` (`align-self` only) is
/// `None`: the parent's `align-items`.
fn align(value: &str, auto: bool) -> Result<Option<Align>, String> {
    Ok(Some(match value {
        "auto" if auto => return Ok(None),
        "start" | "flex-start" | "self-start" => Align::Start,
        "center" => Align::Center,
        "end" | "flex-end" | "self-end" => Align::End,
        "stretch" | "normal" => Align::Stretch,
        _ => {
            return Err(format!(
                "`{value}`: {} is flex-start, center, flex-end or stretch{}",
                if auto { "align-self" } else { "align-items" },
                if auto { " (or auto)" } else { "" }
            ))
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::next_node;

    fn element(kind: &'static str, name: Option<&str>, classes: &[&str]) -> Element {
        Element {
            id: next_node(),
            kind,
            name: name.map(str::to_string),
            classes: classes.iter().map(|c| c.to_string()).collect(),
            fixed: classes.len(),
            maybe: Vec::new(),
            selected: false,
            disabled: false,
            error: false,
        }
    }

    const WIDE: (u16, u16) = (120, 40);

    fn no_vars(_: &str) -> Option<Style> {
        None
    }

    #[test]
    fn specificity_then_order_decide() {
        let sheet = Stylesheet::parse(
            "#a { color: red; } .b { color: green; } label { color: blue; } label { color: yellow; }",
        )
        .unwrap();
        let states = States {
            focus: &[],
            hover: &[],
        };
        let style = |chain: &[Element]| sheet.style(chain, &states, &no_vars, WIDE).unwrap();
        let red = Style::parse("red").unwrap();
        assert_eq!(style(&[element("label", Some("a"), &["b"])]), red);
        let green = Style::parse("green").unwrap();
        assert_eq!(style(&[element("label", None, &["b"])]), green);
        let yellow = Style::parse("yellow").unwrap();
        assert_eq!(style(&[element("label", None, &[])]), yellow);
    }

    #[test]
    fn descendants_states_and_fixed_classes() {
        let sheet = Stylesheet::parse(
            "panel label { size: 3; } panel:focus-within label { color: red; } .x { size: 4; }",
        )
        .unwrap();
        let panel = element("panel", None, &[]);
        let mut label = element("label", None, &["x"]);
        let chain = [panel, element("column", None, &[])];
        let mut full: Vec<Element> = chain.into_iter().collect();
        // A class that holds now, not a fixed one: no layout from it.
        label.fixed = 0;
        full.push(label);
        let layout = sheet.layout(&full, &no_vars, WIDE);
        assert_eq!(layout.size, Some(Size::Fixed(3)));
        let focus = [full[0].id, full[2].id];
        let states = States {
            focus: &focus,
            hover: &[],
        };
        assert!(sheet.style(&full, &states, &no_vars, WIDE).is_some());
        let states = States {
            focus: &[],
            hover: &[],
        };
        assert!(sheet.style(&full, &states, &no_vars, WIDE).is_none());
    }

    #[test]
    fn flexbox_properties_take_css_names_and_values() {
        let sheet = Stylesheet::parse(
            "row { align-items: center; justify-content: space-between; flex-wrap: wrap; align-content: flex-end; } \
             .pin { align-self: flex-end; } .free { align-self: auto; }",
        )
        .unwrap();
        let row = sheet.layout(&[element("row", None, &[])], &|_| None, WIDE);
        assert_eq!(row.align_items, Some(Align::Center));
        assert_eq!(row.justify, Some(Justify::SpaceBetween));
        assert_eq!(row.wrap, Some(true));
        assert_eq!(row.align_content, Some(AlignContent::End));
        let pin = sheet.layout(&[element("text", None, &["pin"])], &|_| None, WIDE);
        assert_eq!(pin.align_self, Some(Align::End));
        let free = sheet.layout(&[element("text", None, &["free"])], &|_| None, WIDE);
        assert_eq!(free.align_self, None, "auto: the parent decides");
        // A later `auto` undoes an earlier rule's alignment.
        let both = sheet.layout(&[element("text", None, &["pin", "free"])], &|_| None, WIDE);
        assert_eq!(both.align_self, None, "auto after flex-end");
        for (css, message) in [
            (
                "row { align-items: baseline; }",
                "align-items is flex-start",
            ),
            (
                "row { flex-wrap: wrap-reverse; }",
                "flex-wrap is nowrap or wrap",
            ),
            (
                "row { justify-content: left; }",
                "justify-content is flex-start",
            ),
        ] {
            let error = Stylesheet::parse(css).unwrap_err().to_string();
            assert!(error.contains(message), "{css}: {error}");
        }
    }

    #[test]
    fn borders_titles_text_and_ratios_take_css_names() {
        let sheet = Stylesheet::parse(
            "panel { border: thick red; border-title: Files; border-title-align: center; \
             border-subtitle: \"q quits\"; border-subtitle-align: right; border-sides: top bottom; } \
             row { border-collapse: collapse; size: 1/3; } \
             label { text-align: justify; text-wrap: nowrap; text-overflow: ellipsis; } \
             .dashed { border: light-triple-dashed; } .plain { border: plain; } \
             grid { grid-columns: 1/3 2/3; }",
        )
        .unwrap();
        let panel = sheet.layout(&[element("panel", None, &[])], &no_vars, WIDE);
        assert_eq!(panel.border.as_ref().map(|b| b.0), Some(Border::Thick));
        assert_eq!(panel.title_align, Some(Align::Center));
        assert_eq!(panel.subtitle.as_deref(), Some("q quits"));
        assert_eq!(panel.subtitle_align, Some(Align::End));
        assert_eq!(panel.sides, Some(Sides::TOP | Sides::BOTTOM));
        // Top and bottom each hold a line and a title; no sides.
        assert_eq!(panel.insets(), [1, 0, 1, 0]);
        let row = sheet.layout(&[element("row", None, &[])], &no_vars, WIDE);
        assert_eq!(
            (row.collapse, row.size),
            (Some(true), Some(Size::Ratio(1, 3)))
        );
        let label = sheet.layout(&[element("label", None, &[])], &no_vars, WIDE);
        assert_eq!(label.text_align, Some(TextAlign::Justify));
        assert_eq!(label.text_wrap, Some(false));
        assert_eq!(label.text_overflow, Some(Overflow::Ellipsis));
        let dashed = sheet.layout(&[element("text", None, &["dashed"])], &no_vars, WIDE);
        assert_eq!(dashed.border.map(|b| b.0), Some(Border::LightTripleDashed));
        let plain = sheet.layout(&[element("text", None, &["plain"])], &no_vars, WIDE);
        assert_eq!(plain.border.map(|b| b.0), Some(Border::Plain));
        let grid = sheet.layout(&[element("grid", None, &[])], &no_vars, WIDE);
        assert_eq!(
            grid.grid_columns,
            Some(vec![Size::Ratio(1, 3), Size::Ratio(2, 3)])
        );
        for (css, message) in [
            ("panel { border-sides: up; }", "border-sides names top"),
            ("label { text-align: middle; }", "text-align is left"),
            (
                "label { text-wrap: balance; }",
                "text-wrap is wrap or nowrap",
            ),
            ("label { text-overflow: cut; }", "text-overflow is clip"),
            (
                "row { border-collapse: yes; }",
                "border-collapse is collapse",
            ),
            (
                "panel { border-title-align: top; }",
                "is left, center or right",
            ),
            ("panel { border: wavy; }", "a border is none, round"),
            ("row { size: 1/x; }", "is not a size"),
        ] {
            let error = Stylesheet::parse(css).unwrap_err().to_string();
            assert!(error.contains(message), "{css}: {error}");
        }
    }

    #[test]
    fn values_parse_and_errors_say_where() {
        let sheet = Stylesheet::parse(
            "/* top */ grid { grid-columns: 1fr 20 50%; gap: 1; padding: 1 2; border: heavy $accent; dock: top; display: none; }",
        )
        .unwrap();
        let accent = |name: &str| (name == "accent").then(|| Style::parse("bold magenta").unwrap());
        let layout = sheet.layout(&[element("grid", None, &[])], &accent, WIDE);
        assert_eq!(
            layout.grid_columns,
            Some(vec![Size::Flex(1), Size::Fixed(20), Size::Percent(50)])
        );
        assert_eq!(layout.padding, [1, 2, 1, 2]);
        assert_eq!(layout.insets(), [2, 3, 2, 3]);
        assert_eq!(
            layout.border,
            Some((Border::Thick, Style::parse("bold magenta").unwrap()))
        );
        assert!(layout.hidden);
        assert_eq!(layout.dock, Some(Dock::Top));
        for (css, message) in [
            (
                "label { size: big; }",
                "`big` is not a size (3, 50%, 1/3, 2fr or auto)",
            ),
            ("label:active { color: red; }", "unknown state `:active`"),
            (
                "label > text { color: red; }",
                "`>`: only descendant selectors (spaces) are supported",
            ),
            ("label { color: nope; }", "`nope` is not a colour"),
            ("label { color: red; ", "a rule with no `}`"),
            ("/* open", "a comment that never ends"),
        ] {
            let error = Stylesheet::parse(css).unwrap_err();
            assert_eq!(error.message, message, "{css}");
        }
        let error =
            Stylesheet::parse("a { color: red; }\n\nb {\n  text-style: shiny;\n}").unwrap_err();
        assert_eq!((error.line, error.column), (4, 3));
    }
    #[test]
    fn media_rules_hold_within_their_bounds() {
        let sheet = Stylesheet::parse(
            "#side { size: 20; } \
             @media (max-width: 80) { #side { display: none; } #main { size: 2fr; } } \
             @media (min-width: 60) and (max-height: 10) { #main { color: red; } }",
        )
        .unwrap();
        assert!(sheet.has_media());
        let side = [element("text", Some("side"), &[])];
        assert!(!sheet.layout(&side, &no_vars, (81, 24)).hidden);
        assert!(sheet.layout(&side, &no_vars, (80, 24)).hidden, "inclusive");
        assert_eq!(
            sheet.layout(&side, &no_vars, (80, 24)).size,
            Some(Size::Fixed(20))
        );
        let main = [element("text", Some("main"), &[])];
        assert_eq!(
            sheet.layout(&main, &no_vars, (40, 24)).size,
            Some(Size::Flex(2))
        );
        let states = States {
            focus: &[],
            hover: &[],
        };
        assert!(sheet.style(&main, &states, &no_vars, (70, 10)).is_some());
        assert!(sheet.style(&main, &states, &no_vars, (70, 11)).is_none());
        assert!(sheet.style(&main, &states, &no_vars, (59, 10)).is_none());
        // One entry a block: what changes when a bound is crossed.
        assert_eq!(sheet.media_key((120, 40)), [false, false]);
        assert_eq!(sheet.media_key((80, 40)), [true, false]);
        assert_eq!(sheet.media_key((70, 10)), [true, true]);
        assert!(!Stylesheet::parse("a { color: red; }").unwrap().has_media());
    }

    #[test]
    fn media_and_error_mistakes_say_where() {
        for (css, message) in [
            ("@media { a { color: red; } }", "`@media` takes conditions"),
            (
                "@media screen { a { color: red; } }",
                "`@media` takes conditions",
            ),
            ("@media (width: 80) { }", "unknown media feature `width`"),
            (
                "@media (max-width: wide) { }",
                "`wide` is not a number of cells",
            ),
            (
                "@media (max-width: 80) or (min-width: 9) { }",
                "`@media` takes conditions",
            ),
            (
                "@media (max-width: 80) { a { color: red; }",
                "an `@media` block with no `}`",
            ),
            (
                "@media (max-width: 80) { @media (min-width: 9) { } }",
                "an `@media` block inside another",
            ),
            (
                "@import \"x.css\" { }",
                "`@import`: only `@media` rules are supported",
            ),
        ] {
            let error = Stylesheet::parse(css).unwrap_err().to_string();
            assert!(error.contains(message), "{css}: {error}");
        }
        let error = Stylesheet::parse("a { color: red; }\n@media (max: 3) { }").unwrap_err();
        assert_eq!((error.line, error.column), (2, 1));
        // `:error` and CSS's `:invalid` are the same state.
        let sheet =
            Stylesheet::parse(":error { color: red; } .x:invalid { text-style: bold; }").unwrap();
        let mut field = element("text", None, &["x"]);
        let states = States {
            focus: &[],
            hover: &[],
        };
        assert!(sheet
            .style(std::slice::from_ref(&field), &states, &no_vars, WIDE)
            .is_none());
        field.error = true;
        let style = sheet.style(&[field], &states, &no_vars, WIDE).unwrap();
        assert_eq!(style, Style::parse("bold red").unwrap());
    }
}
