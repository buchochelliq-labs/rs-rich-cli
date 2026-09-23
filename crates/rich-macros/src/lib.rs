//! Compile-time checked markup and derive macros for the `rich` Rust port.
//!
//! Use these through `rs-rich-ext`'s `macros` feature: it re-exports them and
//! supplies the runtime they expand to (`rich_ext::__private`, `rich_ext::derive`).
//!
//! Markup and styles are checked with the core's own parsers, so a literal the
//! macro accepts is one the runtime accepts, with one deliberate addition: the
//! macros also reject unknown style names and unclosed tags, which the runtime
//! renders silently as no-ops.

use proc_macro::TokenStream;

mod derive;
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
/// (`repr.number`); a malformed placeholder; an unused argument. Declare custom
/// theme keys first: `richf!(keys["app.title"], "[app.title]{name}[/]")`.
///
/// Values are escaped, so `[` in user data is never read as markup. A
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
