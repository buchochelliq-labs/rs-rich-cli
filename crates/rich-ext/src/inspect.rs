//! A report on a Rust value, laid out as upstream's `rich.inspect` lays out
//! its report on a Python object (`rich/_inspect.py`, #796).
//!
//! Python's `inspect` walks a live object with `dir()` and `getattr()`.
//! Rust has no runtime reflection, so the facts it shows are collected at
//! compile time instead: `#[derive(Inspect)]` (the `macros` feature) writes
//! an [`Inspectable`] impl with the type's name, doc comment, fields (name,
//! type as written, doc comment, visibility and value) and, for an enum, its
//! variants; `#[rich_ext::inspect]` on an inherent `impl` block records its
//! methods' signatures and doc comments ([`InspectMethods`]). The report is
//! core renderables: a fitted [`Panel`] titled with the type, the doc
//! comment, the value in a green panel, and a grid [`Table`] of `name =
//! value` rows. DIVERGENCES §19 lists what differs from upstream.
//!
//! ```
//! # #[cfg(feature = "macros")] {
//! use rich_ext::Inspect;
//!
//! /// A point in the plane.
//! #[derive(Inspect)]
//! pub struct Point {
//!     pub x: i32,
//!     pub y: f64,
//!     secret: u8,
//! }
//!
//! let point = Point { x: 1, y: 2.5, secret: 3 };
//! let console = rich::Console::builder().width(40).build();
//! let report = console.render_to_string(&Inspect::new(&point));
//! assert!(report.contains("A point in the plane."));
//! assert!(report.contains("x = 1"));
//! assert!(!report.contains("secret ="));
//! # }
//! ```

use std::borrow::Cow;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use rich::cells::cell_len;
use rich::console::{Console, ConsoleOptions, Justify};
use rich::containers::Renderables;
use rich::highlighter::ReprHighlighter;
use rich::measure::Measurement;
use rich::protocol::{Highlighter, Renderable};
use rich::segment::Segment;
use rich::style::StyleType;
use rich::table::{Cell, ColumnOptions, Table};
use rich::{Panel, Text};

/// A value [`Inspect`] can report on. `#[derive(Inspect)]` writes it; a
/// hand-written impl is the way to inspect a type the derive cannot see.
pub trait Inspectable {
    /// The type: its name, doc comment and, for an enum, its variants.
    fn inspect_type(&self) -> TypeInfo;

    /// The value: its fields (for an enum, the current variant's), each
    /// nested report at most `depth` levels deep.
    fn inspect_record(&self, depth: usize) -> Record;

    /// The methods recorded by `#[rich_ext::inspect]` on an `impl` block.
    /// The derive returns them when the type is marked `#[inspect(methods)]`.
    fn inspect_methods(&self) -> &'static [Method] {
        &[]
    }
}

/// The methods `#[rich_ext::inspect]` recorded from a type's inherent `impl`
/// block. One block per type: a second would be a second impl of this trait.
#[diagnostic::on_unimplemented(
    message = "`{Self}` has no recorded methods",
    note = "put `#[rich_ext::inspect]` on an inherent `impl` block of `{Self}`, or remove `#[inspect(methods)]`"
)]
pub trait InspectMethods {
    /// The methods, in source order.
    const METHODS: &'static [Method];
}

/// A field shown as a nested report (`#[inspect(nested)]`): any
/// [`Inspectable`], and an `Option` of one.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be inspected as a nested report",
    note = "derive `Inspect` for it, or drop `#[inspect(nested)]` to show the field's `Debug`"
)]
pub trait InspectField {
    /// The field's value, nested at most `depth` levels.
    fn inspect_field(&self, depth: usize) -> FieldValue;
}

impl<T: Inspectable + ?Sized> InspectField for T {
    fn inspect_field(&self, depth: usize) -> FieldValue {
        let ty = self.inspect_type();
        if depth == 0 {
            let record = self.inspect_record(0);
            let name = repr_name(&ty, &record);
            return FieldValue::Elided(match record.shape {
                Shape::Named => format!("{name} {{ .. }}"),
                Shape::Tuple => format!("{name}(..)"),
                Shape::Unit => name.to_string(),
            });
        }
        FieldValue::Nested(Box::new(Nested {
            ty,
            record: self.inspect_record(depth - 1),
        }))
    }
}

impl<T: InspectField> InspectField for Option<T> {
    fn inspect_field(&self, depth: usize) -> FieldValue {
        match self {
            None => FieldValue::debug(&None::<()>),
            Some(value) => value.inspect_field(depth),
        }
    }
}

macro_rules! delegate_inspectable {
    ($($ty:ty),*) => {$(
        impl<T: Inspectable + ?Sized> Inspectable for $ty {
            fn inspect_type(&self) -> TypeInfo {
                (**self).inspect_type()
            }
            fn inspect_record(&self, depth: usize) -> Record {
                (**self).inspect_record(depth)
            }
            fn inspect_methods(&self) -> &'static [Method] {
                (**self).inspect_methods()
            }
        }
    )*};
}

delegate_inspectable!(&T, &mut T, Box<T>, Rc<T>, Arc<T>);

/// What kind of type is inspected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TypeKind {
    /// A struct (named, tuple or unit).
    Struct,
    /// An enum.
    Enum,
}

impl TypeKind {
    fn keyword(self) -> &'static str {
        match self {
            TypeKind::Struct => "struct",
            TypeKind::Enum => "enum",
        }
    }
}

/// The shape of a struct or variant's fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// `{ a: A, b: B }`.
    Named,
    /// `(A, B)`.
    Tuple,
    /// No fields.
    Unit,
}

/// An enum variant, as declared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Variant {
    /// Its name.
    pub name: &'static str,
    /// Its doc comment, as the compiler gives it (uncleaned).
    pub doc: &'static str,
    /// The shape of its fields.
    pub shape: Shape,
}

impl Variant {
    /// A variant with no doc comment.
    pub const fn new(name: &'static str, shape: Shape) -> Self {
        Variant {
            name,
            doc: "",
            shape,
        }
    }

    /// With its doc comment.
    #[must_use]
    pub const fn doc(mut self, doc: &'static str) -> Self {
        self.doc = doc;
        self
    }
}

/// A type, as `#[derive(Inspect)]` saw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TypeInfo {
    /// The full name, from [`std::any::type_name`]: `my_crate::Point<i32>`.
    pub name: &'static str,
    /// The name as declared (`Point`), which `Debug` output starts with.
    pub ident: &'static str,
    /// Struct or enum.
    pub kind: TypeKind,
    /// Its doc comment, as the compiler gives it (uncleaned).
    pub doc: &'static str,
    /// An enum's variants, in declaration order.
    pub variants: &'static [Variant],
}

impl TypeInfo {
    /// A type with no doc comment or variants.
    pub fn new(name: &'static str, ident: &'static str, kind: TypeKind) -> Self {
        TypeInfo {
            name,
            ident,
            kind,
            doc: "",
            variants: &[],
        }
    }

    /// With its doc comment.
    #[must_use]
    pub fn doc(mut self, doc: &'static str) -> Self {
        self.doc = doc;
        self
    }

    /// With an enum's variants.
    #[must_use]
    pub fn variants(mut self, variants: &'static [Variant]) -> Self {
        self.variants = variants;
        self
    }
}

/// A value: its fields, and for an enum which variant it is.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Record {
    /// The index of the current variant in [`TypeInfo::variants`].
    pub variant: Option<usize>,
    /// The shape of the fields.
    pub shape: Shape,
    /// The fields, in declaration order, without skipped ones.
    pub fields: Vec<Field>,
    /// Whether fields were skipped (`#[inspect(skip)]`); the value is then
    /// written `Name { a: 1, .. }`, as `finish_non_exhaustive` writes it.
    pub skipped: bool,
}

impl Record {
    /// A struct's record.
    pub fn new(shape: Shape, fields: Vec<Field>) -> Self {
        Record {
            variant: None,
            shape,
            fields,
            skipped: false,
        }
    }

    /// For an enum, the index of the current variant.
    #[must_use]
    pub fn variant(mut self, index: usize) -> Self {
        self.variant = Some(index);
        self
    }

    /// Mark that fields were skipped.
    #[must_use]
    pub fn skipped(mut self, skipped: bool) -> Self {
        self.skipped = skipped;
        self
    }
}

/// A field of a value.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Field {
    /// Its name, or its index in a tuple struct or variant.
    pub name: Cow<'static, str>,
    /// Its type as written in the source: `Vec<String>`.
    pub ty: &'static str,
    /// Its doc comment, as the compiler gives it (uncleaned).
    pub doc: &'static str,
    /// Whether it is `pub` (an enum variant's fields always are).
    pub public: bool,
    /// Its value.
    pub value: FieldValue,
}

impl Field {
    /// A private field with no doc comment.
    pub fn new(name: impl Into<Cow<'static, str>>, ty: &'static str, value: FieldValue) -> Self {
        Field {
            name: name.into(),
            ty,
            doc: "",
            public: false,
            value,
        }
    }

    /// With its doc comment.
    #[must_use]
    pub fn doc(mut self, doc: &'static str) -> Self {
        self.doc = doc;
        self
    }

    /// Whether it is `pub`.
    #[must_use]
    pub fn public(mut self, public: bool) -> Self {
        self.public = public;
        self
    }
}

/// A field's value.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum FieldValue {
    /// Its `Debug` output, on one line (`{:?}`) and expanded (`{:#?}`).
    Debug {
        /// `{:?}`.
        compact: String,
        /// `{:#?}`.
        expanded: String,
    },
    /// A nested report (`#[inspect(nested)]`).
    Nested(Box<Nested>),
    /// A nested report cut at the depth limit: `Node { .. }`.
    Elided(String),
}

impl FieldValue {
    /// The value's `Debug` output.
    pub fn debug<T: fmt::Debug + ?Sized>(value: &T) -> Self {
        FieldValue::Debug {
            compact: format!("{value:?}"),
            expanded: format!("{value:#?}"),
        }
    }

    /// The value formatted by `with`, a function with `Debug::fmt`'s
    /// signature (`#[inspect(with = path)]`).
    pub fn debug_with<T: ?Sized>(
        value: &T,
        with: fn(&T, &mut fmt::Formatter<'_>) -> fmt::Result,
    ) -> Self {
        struct With<'a, T: ?Sized>(&'a T, fn(&T, &mut fmt::Formatter<'_>) -> fmt::Result);
        impl<T: ?Sized> fmt::Debug for With<'_, T> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                (self.1)(self.0, f)
            }
        }
        FieldValue::debug(&With(value, with))
    }
}

/// A nested report: a field's own type and record.
#[derive(Clone, Debug, PartialEq)]
pub struct Nested {
    /// The field's type.
    pub ty: TypeInfo,
    /// The field's value.
    pub record: Record,
}

/// A method recorded by `#[rich_ext::inspect]` on an `impl` block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Method {
    /// Its name.
    pub name: &'static str,
    /// What comes before the name: `pub const unsafe fn`.
    pub prefix: &'static str,
    /// What follows the name: generics, parameters, return type and where
    /// clause, as written (`(&self, scale: f64) -> f64`).
    pub signature: &'static str,
    /// Its doc comment, as the compiler gives it (uncleaned).
    pub doc: &'static str,
    /// Whether it is `pub`.
    pub public: bool,
    /// Whether it is `async`.
    pub is_async: bool,
}

impl Method {
    /// A private, synchronous method named `name` with the given prefix and
    /// signature and no doc comment.
    pub const fn new(name: &'static str, prefix: &'static str, signature: &'static str) -> Self {
        Method {
            name,
            prefix,
            signature,
            doc: "",
            public: false,
            is_async: false,
        }
    }

    /// With its doc comment.
    #[must_use]
    pub const fn doc(mut self, doc: &'static str) -> Self {
        self.doc = doc;
        self
    }

    /// Whether it is `pub`.
    #[must_use]
    pub const fn public(mut self, public: bool) -> Self {
        self.public = public;
        self
    }

    /// Whether it is `async`.
    #[must_use]
    pub const fn is_async(mut self, is_async: bool) -> Self {
        self.is_async = is_async;
        self
    }
}

/// The options of a report, named as upstream's `inspect` names them. The
/// default is upstream's `inspect()` default: docs, value and sorted public
/// fields; no methods, private fields or full help.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct InspectOptions {
    /// The panel's title, in place of the type (console markup).
    pub title: Option<String>,
    /// The source text of the inspected expression, shown before the type
    /// in the title (`rich_inspect!` sets it).
    pub label: Option<String>,
    /// Full doc comments rather than their first paragraph (implies `docs`).
    pub help: bool,
    /// List the methods recorded by `#[rich_ext::inspect]`.
    pub methods: bool,
    /// Show doc comments: the type's, the fields' and the methods'.
    pub docs: bool,
    /// Show private (non-`pub`) fields and methods.
    pub private: bool,
    /// Sort fields and methods by name (upstream's default); otherwise they
    /// are in declaration order.
    pub sort: bool,
    /// Show the value in a panel.
    pub value: bool,
    /// Show each field's type, as written: `x: i32 = 1`.
    pub types: bool,
    /// For an enum, list every variant, not only the current one.
    pub variants: bool,
    /// How deep `#[inspect(nested)]` reports go.
    pub max_depth: usize,
}

impl Default for InspectOptions {
    fn default() -> Self {
        InspectOptions {
            title: None,
            label: None,
            help: false,
            methods: false,
            docs: true,
            private: false,
            sort: true,
            value: true,
            types: false,
            variants: false,
            max_depth: 3,
        }
    }
}

macro_rules! option_setters {
    ($($(#[$doc:meta])* $name:ident),* $(,)?) => {$(
        $(#[$doc])*
        #[must_use]
        pub fn $name(mut self, $name: bool) -> Self {
            self.options_mut().$name = $name;
            self
        }
    )*};
}

macro_rules! builder {
    () => {
        option_setters!(
            /// Full doc comments rather than their first paragraph.
            help,
            /// List the methods recorded by `#[rich_ext::inspect]`.
            methods,
            /// Show doc comments.
            docs,
            /// Show private (non-`pub`) fields and methods.
            private,
            /// Sort fields and methods by name.
            sort,
            /// Show the value in a panel.
            value,
            /// Show each field's type.
            types,
            /// For an enum, list every variant.
            variants,
        );

        /// Upstream's `all`: when true, turn on `methods`, `private`,
        /// `types` and `variants` (false changes nothing, as upstream's
        /// `all=False` does).
        #[must_use]
        pub fn all(mut self, all: bool) -> Self {
            if all {
                let options = self.options_mut();
                options.methods = true;
                options.private = true;
                options.types = true;
                options.variants = true;
            }
            self
        }

        /// The panel's title (console markup), in place of the type.
        #[must_use]
        pub fn title(mut self, title: impl Into<String>) -> Self {
            self.options_mut().title = Some(title.into());
            self
        }

        /// The inspected expression's source text, shown before the type.
        #[must_use]
        pub fn label(mut self, label: impl Into<String>) -> Self {
            self.options_mut().label = Some(label.into());
            self
        }

        /// How deep `#[inspect(nested)]` reports go.
        #[must_use]
        pub fn max_depth(mut self, max_depth: usize) -> Self {
            self.options_mut().max_depth = max_depth;
            self
        }
    };
}

impl InspectOptions {
    /// Upstream's defaults.
    pub fn new() -> Self {
        InspectOptions::default()
    }

    fn options_mut(&mut self) -> &mut InspectOptions {
        self
    }

    builder!();
}

/// A renderable report on a value. Mirrors `rich._inspect.Inspect`.
pub struct Inspect<'a> {
    value: &'a dyn Inspectable,
    options: InspectOptions,
}

impl<'a> Inspect<'a> {
    /// A report on `value` with upstream's default options.
    pub fn new(value: &'a dyn Inspectable) -> Self {
        Inspect {
            value,
            options: InspectOptions::default(),
        }
    }

    /// A report with `options`.
    #[must_use]
    pub fn options(mut self, options: InspectOptions) -> Self {
        self.options = options;
        self
    }

    fn options_mut(&mut self) -> &mut InspectOptions {
        &mut self.options
    }

    builder!();

    fn panel(&self) -> Panel {
        let options = &self.options;
        let ty = self.value.inspect_type();
        let record = self.value.inspect_record(options.max_depth);
        let methods = self.value.inspect_methods();
        let title = match &options.title {
            Some(title) => Text::from_markup(title).unwrap_or_else(|_| Text::new(title.clone())),
            None => {
                let mut title = String::new();
                if let Some(label) = &options.label {
                    title.push_str(label);
                    title.push_str(": ");
                }
                title.push_str(&type_title(&ty));
                highlighted(Text::new(title))
            }
        };
        let parts = render_parts(&ty, &record, methods, options, true);
        Panel::fit(Box::new(Renderables::new(parts)))
            .title_as_text(title)
            .border_style("scope.border")
            .padding((0, 1, 0, 1))
    }
}

impl Renderable for Inspect<'_> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.panel().rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.panel().measure(console, options)
    }
}

/// Print a report on `value` to standard output. Upstream's `rich.inspect`.
pub fn inspect(value: &dyn Inspectable, options: InspectOptions) {
    Console::new().print(&Inspect::new(value).options(options));
}

/// Print a report on a value to standard output, its source text in the
/// title. Options follow as `name = value`: `help`, `methods`, `docs`,
/// `private`, `sort`, `value`, `types`, `variants`, `all`, `title`,
/// `max_depth`.
///
/// ```
/// # #[cfg(feature = "macros")] {
/// #[derive(rich_ext::Inspect)]
/// struct Config {
///     pub retries: u32,
/// }
/// let config = Config { retries: 3 };
/// rich_ext::rich_inspect!(config, private = true);
/// # }
/// ```
#[macro_export]
macro_rules! rich_inspect {
    ($value:expr $(, $key:ident = $option:expr)* $(,)?) => {
        $crate::inspect::inspect(
            &$value,
            $crate::inspect::InspectOptions::new()
                .label(::std::stringify!($value))
                $(.$key($option))*,
        )
    };
}

/// `<struct 'demo::Point'>`, after upstream's `<class '__main__.Point'>`.
fn type_title(ty: &TypeInfo) -> String {
    format!("<{} '{}'>", ty.kind.keyword(), ty.name)
}

fn highlighted(mut text: Text) -> Text {
    ReprHighlighter::new().highlight(&mut text);
    text
}

/// Python's `inspect.cleandoc(…).strip()` over a doc comment: the common
/// indentation (every doc comment line starts with a space) is removed, and
/// leading and trailing blank lines are dropped.
pub fn clean_doc(doc: &str) -> String {
    let doc = doc.replace('\t', "        ");
    let indent = doc
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    let lines: Vec<&str> = doc
        .lines()
        .map(|line| line.get(indent..).unwrap_or("").trim_end())
        .collect();
    lines.join("\n").trim().to_string()
}

/// Upstream's `_get_formatted_doc`: the cleaned doc comment, its first
/// paragraph unless `help`, with control codes escaped; `None` when empty.
fn formatted_doc(doc: &str, options: &InspectOptions) -> Option<String> {
    let mut doc = clean_doc(doc);
    if doc.is_empty() {
        return None;
    }
    if !options.help
        && let Some((first, _)) = doc.split_once("\n\n")
    {
        doc = first.to_string();
    }
    Some(escape_control_codes(&doc))
}

/// Upstream's `escape_control_codes`: BEL, BS, VT, FF and CR as escapes.
fn escape_control_codes(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\x07' => escaped.push_str("\\a"),
            '\x08' => escaped.push_str("\\b"),
            '\x0b' => escaped.push_str("\\v"),
            '\x0c' => escaped.push_str("\\f"),
            '\r' => escaped.push_str("\\r"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// The name `Debug` output starts with: the variant's for an enum.
fn repr_name(ty: &TypeInfo, record: &Record) -> &'static str {
    match record.variant.and_then(|index| ty.variants.get(index)) {
        Some(variant) => variant.name,
        None => ty.ident,
    }
}

/// The value as `#[derive(Debug)]` would write it, from the recorded fields.
struct Repr<'a> {
    name: &'a str,
    record: &'a Record,
}

struct RawValue<'a>(&'a FieldValue);

impl fmt::Debug for RawValue<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            FieldValue::Debug { compact, expanded } => {
                f.write_str(if f.alternate() { expanded } else { compact })
            }
            FieldValue::Nested(nested) => fmt::Debug::fmt(
                &Repr {
                    name: repr_name(&nested.ty, &nested.record),
                    record: &nested.record,
                },
                f,
            ),
            FieldValue::Elided(text) => f.write_str(text),
        }
    }
}

impl fmt::Debug for Repr<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.record.shape {
            Shape::Unit => f.write_str(self.name),
            Shape::Named => {
                let mut out = f.debug_struct(self.name);
                for field in &self.record.fields {
                    out.field(&field.name, &RawValue(&field.value));
                }
                if self.record.skipped {
                    out.finish_non_exhaustive()
                } else {
                    out.finish()
                }
            }
            Shape::Tuple => {
                let mut out = f.debug_tuple(self.name);
                for field in &self.record.fields {
                    out.field(&RawValue(&field.value));
                }
                if self.record.skipped {
                    out.finish_non_exhaustive()
                } else {
                    out.finish()
                }
            }
        }
    }
}

/// A `Debug` value: on one line where it fits, expanded where it does not
/// (upstream's `Pretty` fits a repr to the width the same way), then a doc.
struct Fit {
    compact: String,
    expanded: String,
    doc: Option<String>,
    indent_guides: bool,
}

impl Fit {
    fn text(&self, width: usize, ascii_only: bool) -> Text {
        let fits = !self.compact.contains('\n') && cell_len(&self.compact) <= width;
        let value = if fits { &self.compact } else { &self.expanded };
        let mut text = highlighted(Text::new(value.clone()));
        if self.indent_guides && !ascii_only {
            text = text.with_indent_guides(Some(4), "│", "repr.indent");
        }
        if let Some(doc) = &self.doc {
            text.append(if doc.contains('\n') { "\n" } else { " " }, None);
            text = text.append_text(&doc_text(doc));
        }
        text
    }
}

fn doc_text(doc: &str) -> Text {
    let mut text = highlighted(Text::new(doc));
    let length = text.plain().chars().count();
    text.stylize("inspect.doc", 0, length);
    text
}

impl Renderable for Fit {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.text(options.max_width, options.ascii_only())
            .rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.text(usize::MAX, true).measure(console, options)
    }
}

/// An empty line between parts (upstream yields `""`).
struct Blank;

impl Renderable for Blank {
    fn rich_render(&self, _console: &Console, _options: &ConsoleOptions) -> Vec<Segment> {
        vec![Segment::new("", None)]
    }

    fn measure(&self, _console: &Console, _options: &ConsoleOptions) -> Measurement {
        Measurement::new(0, 0)
    }
}

/// Upstream's `Panel(Pretty(obj), border_style="inspect.value.border")`.
struct ValuePanel(Arc<Fit>);

struct Shared(Arc<Fit>);

impl Renderable for Shared {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.0.rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.0.measure(console, options)
    }
}

impl ValuePanel {
    fn panel(&self) -> Panel {
        Panel::new(Box::new(Shared(self.0.clone()))).border_style("inspect.value.border")
    }
}

impl Renderable for ValuePanel {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.panel().rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.panel().measure(console, options)
    }
}

/// A nested report, in its own fitted panel: the attribute table only.
struct NestedView {
    nested: Nested,
    options: InspectOptions,
}

impl NestedView {
    fn panel(&self) -> Panel {
        let parts = render_parts(
            &self.nested.ty,
            &self.nested.record,
            &[],
            &self.options,
            false,
        );
        Panel::fit(Box::new(Renderables::new(parts)))
            .title_as_text(highlighted(Text::new(type_title(&self.nested.ty))))
            .border_style("scope.border")
            .padding((0, 1, 0, 1))
    }
}

impl Renderable for NestedView {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.panel().rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.panel().measure(console, options)
    }
}

type Part = Arc<dyn Renderable + Send + Sync>;

/// Sort key: upstream's `key.strip("_").lower()`, with tuple indices in
/// numeric order.
fn sort_key(name: &str) -> (usize, String) {
    match name.parse::<usize>() {
        Ok(index) => (index, String::new()),
        Err(_) => (usize::MAX, name.trim_matches('_').to_lowercase()),
    }
}

fn key_text(name: &str, ty: Option<&str>) -> Text {
    let mut key = Text::styled(name.to_string(), "inspect.attr");
    if let Some(ty) = ty {
        key.append(": ", None);
        key = key.append_text(&highlighted(Text::new(ty)));
    }
    key.append(" =", Some(StyleType::Name("inspect.equals".to_string())));
    key
}

/// Upstream's `_render`: the doc, the value panel and the attribute table.
fn render_parts(
    ty: &TypeInfo,
    record: &Record,
    methods: &[Method],
    options: &InspectOptions,
    top: bool,
) -> Vec<Part> {
    let mut parts: Vec<Part> = Vec::new();
    let docs = options.docs || options.help;
    if top
        && docs
        && let Some(doc) = formatted_doc(ty.doc, options)
    {
        parts.push(Arc::new(highlighted(Text::styled(doc, "inspect.help"))));
        parts.push(Arc::new(Blank));
    }
    if top && options.value {
        let repr = Repr {
            name: repr_name(ty, record),
            record,
        };
        let value = Fit {
            compact: format!("{repr:?}"),
            expanded: format!("{repr:#?}"),
            doc: None,
            indent_guides: true,
        };
        parts.push(Arc::new(ValuePanel(Arc::new(value))));
        parts.push(Arc::new(Blank));
    }

    let mut table = Table::grid().padding(0, 1, 0, 1).expand(false);
    table.add_column_with(
        Text::new(""),
        ColumnOptions {
            justify: Justify::Right,
            ..ColumnOptions::default()
        },
    );
    table.add_column_with(Text::new(""), ColumnOptions::default());
    let mut rows = 0;
    let mut hidden = 0;

    // An enum's variant comes first, unsorted: it says which fields follow.
    if let Some(variant) = record.variant.and_then(|index| ty.variants.get(index)) {
        let mut value = highlighted(Text::new(variant.name));
        if docs && let Some(doc) = formatted_doc(variant.doc, options) {
            value.append(if doc.contains('\n') { "\n" } else { " " }, None);
            value = value.append_text(&doc_text(&doc));
        }
        table.add_row_cells(vec![
            Cell::Text(key_text("variant", None)),
            Cell::Text(value),
        ]);
        rows += 1;
        if options.variants {
            let names: Vec<&str> = ty.variants.iter().map(|variant| variant.name).collect();
            table.add_row_cells(vec![
                Cell::Text(key_text("variants", None)),
                Cell::Text(highlighted(Text::new(names.join(", ")))),
            ]);
            rows += 1;
        }
    }

    let mut fields: Vec<&Field> = record.fields.iter().collect();
    if options.sort {
        fields.sort_by_cached_key(|field| sort_key(&field.name));
    }
    for field in fields {
        if !field.public && !options.private {
            hidden += 1;
            continue;
        }
        let doc = if docs {
            formatted_doc(field.doc, options)
        } else {
            None
        };
        let cell: Cell = match &field.value {
            FieldValue::Debug { compact, expanded } => Cell::Renderable(Arc::new(Fit {
                compact: compact.clone(),
                expanded: expanded.clone(),
                doc,
                indent_guides: false,
            })),
            FieldValue::Elided(text) => Cell::Renderable(Arc::new(Fit {
                compact: text.clone(),
                expanded: text.clone(),
                doc,
                indent_guides: false,
            })),
            FieldValue::Nested(nested) => Cell::Renderable(Arc::new(NestedView {
                nested: (**nested).clone(),
                options: options.clone(),
            })),
        };
        let ty = options.types.then_some(field.ty);
        table.add_row_cells(vec![Cell::Text(key_text(&field.name, ty)), cell]);
        rows += 1;
    }

    if options.methods {
        let mut methods: Vec<&Method> = methods.iter().collect();
        if options.sort {
            methods.sort_by_cached_key(|method| sort_key(method.name));
        }
        for method in methods {
            if !method.public && !options.private {
                hidden += 1;
                continue;
            }
            table.add_row_cells(vec![
                Cell::Text(key_text(method.name, None)),
                Cell::Text(signature_text(method, docs, options)),
            ]);
            rows += 1;
        }
    } else {
        hidden += methods.len();
    }

    if rows > 0 {
        parts.push(Arc::new(table));
    } else if hidden > 0 && top {
        let message = format!(
            "[b cyan]{hidden}[/][i] attribute(s) not shown.[/i] Run \
             [b][magenta]Inspect[/]::[not b]all[/][/b] for options."
        );
        parts.push(Arc::new(
            Text::from_markup(&message).unwrap_or_else(|_| Text::new(message)),
        ));
    }
    parts
}

/// Upstream's `_get_signature` and the doc after it: `pub fn` styled as
/// upstream's `def`, the name as `inspect.callable`, the rest highlighted,
/// then `:` and the doc's first paragraph, dim.
fn signature_text(method: &Method, docs: bool, options: &InspectOptions) -> Text {
    let prefix_style = if method.is_async {
        "inspect.async_def"
    } else {
        "inspect.def"
    };
    let mut text = Text::styled(format!("{} ", method.prefix), prefix_style);
    text.append(
        method.name,
        Some(StyleType::Name("inspect.callable".to_string())),
    );
    text = text.append_text(&highlighted(Text::new(format!("{}:", method.signature))));
    if docs && let Some(doc) = formatted_doc(method.doc, options) {
        text.append(if doc.contains('\n') { "\n" } else { " " }, None);
        text = text.append_text(&doc_text(&doc));
    }
    text
}
