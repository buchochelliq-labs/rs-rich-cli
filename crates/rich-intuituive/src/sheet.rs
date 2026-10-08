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
//! `:focus-within`, `:hover`, `:selected`, `:disabled`), with descendants
//! separated by spaces. The more specific rule wins, then the later one.
//!
//! Colours and text styles follow the states and
//! [`class_when`](crate::Node::class_when); layout (sizes, padding, the
//! border, `display`, `dock`, gaps and grid tracks) is decided once, by
//! the kind, name and fixed classes. What code sets wins over the sheet:
//! `.fixed(3)` beats `size: 5`.

use std::fmt;
use std::rc::Rc;

use rich::{Color, Style};

use crate::layout::Size;
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
}

/// A colour, or a theme style's colour by name (`$accent`).
#[derive(Clone, Debug, PartialEq)]
enum Paint {
    Color(Color),
    Var(String),
}

/// The box a border is drawn with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum BoxKind {
    #[default]
    Round,
    Square,
    Heavy,
    Double,
    Ascii,
}

impl BoxKind {
    /// Top left, top, top right, side, bottom left, bottom right.
    pub(crate) fn chars(self) -> [&'static str; 6] {
        match self {
            BoxKind::Round => ["╭", "─", "╮", "│", "╰", "╯"],
            BoxKind::Square => ["┌", "─", "┐", "│", "└", "┘"],
            BoxKind::Heavy => ["┏", "━", "┓", "┃", "┗", "┛"],
            BoxKind::Double => ["╔", "═", "╗", "║", "╚", "╝"],
            BoxKind::Ascii => ["+", "-", "+", "|", "+", "+"],
        }
    }
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
    border: Option<Option<(BoxKind, Option<Paint>)>>,
    border_title: Option<String>,
    size: Option<Size>,
    min: Option<u16>,
    max: Option<u16>,
    padding: Option<[u16; 4]>,
    gap: Option<u16>,
    grid_columns: Option<Vec<Size>>,
    grid_rows: Option<Vec<Size>>,
    hidden: Option<bool>,
    dock: Option<Dock>,
}

/// What a sheet decides about a node's layout.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Layout {
    pub size: Option<Size>,
    pub min: Option<u16>,
    pub max: Option<u16>,
    /// Top, right, bottom, left.
    pub padding: [u16; 4],
    pub border: Option<(BoxKind, Style)>,
    /// `border: none` was said, which takes a panel's own border away.
    pub no_border: bool,
    pub border_title: Option<String>,
    pub hidden: bool,
    pub dock: Option<Dock>,
    pub gap: Option<u16>,
    pub grid_columns: Option<Vec<Size>>,
    pub grid_rows: Option<Vec<Size>>,
}

impl Layout {
    /// Cells the border and padding take: top, right, bottom, left.
    pub(crate) fn insets(&self) -> [u16; 4] {
        let edge = u16::from(self.border.is_some());
        let [t, r, b, l] = self.padding;
        [t + edge, r + edge, b + edge, l + edge]
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
        Parser::new(css).sheet()
    }

    /// Whether it has no rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Whether a rule with colours or a text style could ever match the
    /// last node of `chain`, and whether such a rule asks about the focus
    /// and the pointer: what the node must draw again for.
    pub(crate) fn watches(&self, chain: &[Element]) -> (bool, bool, bool) {
        let mut out = (false, false, false);
        for rule in &self.rules {
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
    /// the last node of `chain`.
    pub(crate) fn layout(&self, chain: &[Element], vars: Vars) -> Layout {
        let mut out = Layout::default();
        let mut border: Option<Option<(BoxKind, Option<Paint>)>> = None;
        for rule in &self.rules {
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
            out.hidden = d.hidden.unwrap_or(out.hidden);
            out.dock = d.dock.or(out.dock);
            out.gap = d.gap.or(out.gap);
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
    /// in `states`, if any do.
    pub(crate) fn style(&self, chain: &[Element], states: &States, vars: Vars) -> Option<Style> {
        let mut color = None;
        let mut background = None;
        let mut text: Option<Style> = None;
        let mut any = false;
        for rule in &self.rules {
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

    /// Skip spaces and comments.
    fn skip(&mut self) -> Result<(), SheetError> {
        loop {
            let rest = self.rest();
            let trimmed = rest.trim_start();
            self.at += rest.len() - trimmed.len();
            if let Some(comment) = trimmed.strip_prefix("/*") {
                match comment.find("*/") {
                    Some(end) => self.at += end + 4,
                    None => return Err(self.error(self.at, "a comment that never ends")),
                }
            } else {
                return Ok(());
            }
        }
    }

    fn sheet(mut self) -> Result<Stylesheet, SheetError> {
        let mut rules: Vec<(Rule, (usize, usize, usize), usize)> = Vec::new();
        loop {
            self.skip()?;
            if self.rest().is_empty() {
                break;
            }
            let start = self.at;
            let Some(open) = self.rest().find('{') else {
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
            let Some(close) = self.rest().find('}') else {
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
                    },
                    specificity,
                    order,
                ));
            }
        }
        rules.sort_by_key(|(_, specificity, order)| (*specificity, *order));
        Ok(Stylesheet {
            rules: rules.into_iter().map(|(rule, _, _)| rule).collect(),
        })
    }

    fn decls(&self, start: usize, body: &str) -> Result<Decls, SheetError> {
        let mut decls = Decls::default();
        let mut offset = 0;
        for part in body.split(';') {
            let here = start + offset;
            offset += part.len() + 1;
            // Comments inside a rule.
            let mut clean = String::new();
            let mut rest = part;
            while let Some(open) = rest.find("/*") {
                clean.push_str(&rest[..open]);
                match rest[open + 2..].find("*/") {
                    Some(end) => rest = &rest[open + 2 + end + 2..],
                    None => return Err(self.error(here, "a comment that never ends")),
                }
            }
            clean.push_str(rest);
            let line = clean.trim();
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

fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
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
    let bad = || format!("`{value}` is not a size (3, 50%, 2fr or auto)");
    if value == "auto" {
        Ok(Size::Auto)
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
                Some("round") => BoxKind::Round,
                Some("square" | "solid") => BoxKind::Square,
                Some("heavy") => BoxKind::Heavy,
                Some("double") => BoxKind::Double,
                Some("ascii") => BoxKind::Ascii,
                _ => {
                    return Err(format!(
                        "`{value}`: a border is none, round, square, heavy, double or ascii, then a colour"
                    ))
                }
            };
            let color = words.next().map(paint).transpose()?;
            if let Some(extra) = words.next() {
                return Err(format!("`{extra}`: a border takes a box and one colour"));
            }
            decls.border = Some(Some((kind, color)));
        }
        "border-title" => {
            let title = value.trim_matches(|c| c == '"' || c == '\'');
            decls.border_title = Some(title.to_string());
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
        _ => return Err(format!("unknown property `{property}`")),
    }
    Ok(())
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
        }
    }

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
        let style = |chain: &[Element]| sheet.style(chain, &states, &no_vars).unwrap();
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
        let layout = sheet.layout(&full, &no_vars);
        assert_eq!(layout.size, Some(Size::Fixed(3)));
        let focus = [full[0].id, full[2].id];
        let states = States {
            focus: &focus,
            hover: &[],
        };
        assert!(sheet.style(&full, &states, &no_vars).is_some());
        let states = States {
            focus: &[],
            hover: &[],
        };
        assert!(sheet.style(&full, &states, &no_vars).is_none());
    }

    #[test]
    fn values_parse_and_errors_say_where() {
        let sheet = Stylesheet::parse(
            "/* top */ grid { grid-columns: 1fr 20 50%; gap: 1; padding: 1 2; border: heavy $accent; dock: top; display: none; }",
        )
        .unwrap();
        let accent = |name: &str| (name == "accent").then(|| Style::parse("bold magenta").unwrap());
        let layout = sheet.layout(&[element("grid", None, &[])], &accent);
        assert_eq!(
            layout.grid_columns,
            Some(vec![Size::Flex(1), Size::Fixed(20), Size::Percent(50)])
        );
        assert_eq!(layout.padding, [1, 2, 1, 2]);
        assert_eq!(layout.insets(), [2, 3, 2, 3]);
        assert_eq!(
            layout.border,
            Some((BoxKind::Heavy, Style::parse("bold magenta").unwrap()))
        );
        assert!(layout.hidden);
        assert_eq!(layout.dock, Some(Dock::Top));
        for (css, message) in [
            (
                "label { size: big; }",
                "`big` is not a size (3, 50%, 2fr or auto)",
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
}
