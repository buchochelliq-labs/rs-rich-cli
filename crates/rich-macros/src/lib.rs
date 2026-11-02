//! Compile-time checked markup and derive macros for the `rich` Rust port.
//!
//! Use these through `rs-rich-ext`'s `macros` feature: it re-exports them and
//! supplies the runtime they expand to (`rich_ext::__private`, `rich_ext::derive`).
//!
//! Markup and styles are checked with the core's own parsers, so a literal the
//! macro accepts is one the runtime accepts, with one deliberate addition: the
//! macros also reject unknown style names and unclosed tags, which the runtime
//! renders silently as no-ops.

// A builder method that returns `Self` is `#[must_use]`: dropping its
// result drops the value (CONTRIBUTING.md, "API conventions").
#![warn(clippy::return_self_not_must_use)]

use proc_macro::TokenStream;

mod derive;
mod inspect;
mod markup;
mod template;

/// A [`Text`](https://docs.rs/rs-rich/latest/rich/text/struct.Text.html)
/// from checked markup with `format!`-style placeholders.
///
/// ```ignore
/// let text = richf!("[bold]{name}[/] has {count:>3} items", count = items.len());
/// ```
///
/// Compile errors: an unbalanced, mismatched or unclosed tag; a tag that is
/// neither a style (`bold red on blue`) nor a default theme key
/// (`repr.number`); a malformed placeholder (the name must be an
/// identifier); an unused argument. Declare custom
/// theme keys first: `richf!(keys["app.title"], "[app.title]{name}[/]")`.
///
/// Values are escaped, so `[` in user data is never read as markup, and a
/// value cannot reach the template's own tags (a trailing `\` does not escape
/// the next one). A `[` before a placeholder that a `]` in the value could
/// complete as a tag is a compile error. A
/// placeholder inside a tag (`[{color}]`) is inserted as markup instead, and
/// that tag is checked at run time only.
#[proc_macro]
pub fn richf(input: TokenStream) -> TokenStream {
    template::richf(input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Check a markup literal at compile time and return it as `&'static str`.
/// Accepts `keys[...]` before the literal, as [`richf!`] does.
#[proc_macro]
pub fn markup(input: TokenStream) -> TokenStream {
    template::markup(input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// A `Style` parsed at compile time: `style!("bold red on white")`.
#[proc_macro]
pub fn style(input: TokenStream) -> TokenStream {
    template::style(input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// A theme key checked against the default theme: `theme_key!("repr.number")`
/// expands to the `&'static str`.
#[proc_macro]
pub fn theme_key(input: TokenStream) -> TokenStream {
    template::theme_key(input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Render a struct or enum through `rich_ext::derive`.
///
/// Type attributes: `#[rich(title = "…")]`, `#[rich(panel)]` (bordered, titled),
/// `#[rich(table)]` (a one-row table). Field attributes: `skip`,
/// `label = "…"`, `style = "…"` (checked), `display` (use `Display`, not
/// `Debug`; the default for `String`, `str`, `char` and `Cow` fields),
/// `format = "{:.2}"`, `justify = "left" | "center" | "right"`, `order = N`.
#[proc_macro_derive(Rich, attributes(rich))]
pub fn derive_rich(input: TokenStream) -> TokenStream {
    derive::rich(syn::parse_macro_input!(input as syn::DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Record what `rich_ext::inspect` reports on a struct or enum: the type's
/// name and doc comment, each field's name, type as written, doc comment,
/// visibility and value, and an enum's variants.
///
/// Field values are their `Debug` output, so every field's type must
/// implement `Debug` unless it is marked. Field attributes:
/// `#[inspect(skip)]` (left out; the value reads `Name { a: 1, .. }`),
/// `#[inspect(with = path)]` (formatted by `path`, a function with
/// `Debug::fmt`'s signature) and `#[inspect(nested)]` (a nested report of
/// a field that is itself `Inspectable`, or an `Option` of one).
///
/// Type attribute: `#[inspect(methods)]` lists the methods recorded by
/// [`macro@inspect`] on the type's inherent `impl` block.
///
/// A generic parameter used in a field's type gets a `Debug` bound on that
/// type (`InspectField` for a nested field). Unions are refused.
#[proc_macro_derive(Inspect, attributes(inspect))]
pub fn derive_inspect(input: TokenStream) -> TokenStream {
    inspect::derive(syn::parse_macro_input!(input as syn::DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Record an inherent `impl` block's methods for `rich_ext::inspect`: each
/// `fn`'s name, signature as written, doc comment, and whether it is `pub`
/// or `async`. Mark the type `#[inspect(methods)]` to list them; one block
/// per type. `#[inspect(skip)]` on a method leaves it out.
#[proc_macro_attribute]
pub fn inspect(args: TokenStream, item: TokenStream) -> TokenStream {
    inspect::methods(args.into(), syn::parse_macro_input!(item as syn::ItemImpl))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
