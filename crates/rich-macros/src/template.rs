//! `richf!`, `markup!`, `style!` and `theme_key!`.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{bracketed, Expr, Ident, LitStr, Token};

use crate::markup as checks;

/// Marks where a placeholder sits in the markup being checked. A private-use
/// character, so it cannot collide with a tag or text a template means.
const MARK: char = '\u{E000}';

/// `keys["a", "b"],` before a template.
fn parse_keys(input: ParseStream<'_>) -> syn::Result<Vec<String>> {
    if !(input.peek(Ident) && input.peek2(syn::token::Bracket)) {
        return Ok(Vec::new());
    }
    let ident: Ident = input.parse()?;
    if ident != "keys" {
        return Err(syn::Error::new(
            ident.span(),
            "expected `keys[...]` or a string literal",
        ));
    }
    let content;
    bracketed!(content in input);
    let keys = Punctuated::<LitStr, Token![,]>::parse_terminated(&content)?;
    input.parse::<Token![,]>()?;
    Ok(keys.into_iter().map(|key| key.value()).collect())
}

enum Arg {
    Positional(Expr),
    Named(Ident, Expr),
}

struct Richf {
    keys: Vec<String>,
    template: LitStr,
    args: Vec<Arg>,
}

impl Parse for Richf {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let keys = parse_keys(input)?;
        let template: LitStr = input.parse()?;
        let mut args = Vec::new();
        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            if input.peek(Ident) && input.peek2(Token![=]) && !input.peek2(Token![==]) {
                let name: Ident = input.parse()?;
                input.parse::<Token![=]>()?;
                args.push(Arg::Named(name, input.parse()?));
            } else {
                args.push(Arg::Positional(input.parse()?));
            }
        }
        Ok(Richf {
            keys,
            template,
            args,
        })
    }
}

/// Which argument a placeholder names.
#[derive(Clone, Debug, PartialEq)]
enum ArgRef {
    Index(usize),
    Name(String),
}

enum Piece {
    Literal(String),
    Placeholder { arg: ArgRef, spec: String },
}

/// Split a `format!` template into literal markup and placeholders.
fn pieces(template: &str) -> Result<Vec<Piece>, String> {
    let mut out = Vec::new();
    let mut literal = String::new();
    let mut next = 0;
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                literal.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                literal.push('}');
            }
            '}' => return Err("unmatched `}` in template; write `}}` for a literal brace".into()),
            '{' => {
                let mut inner = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) => inner.push(c),
                        None => {
                            return Err(
                                "unclosed `{` in template; write `{{` for a literal brace".into()
                            )
                        }
                    }
                }
                let (name, spec) = inner.split_once(':').unwrap_or((&inner, ""));
                let name = name.trim();
                let arg = if name.is_empty() {
                    next += 1;
                    ArgRef::Index(next - 1)
                } else if let Ok(index) = name.parse() {
                    ArgRef::Index(index)
                } else if name
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
                    && name.chars().all(|c| c.is_alphanumeric() || c == '_')
                {
                    ArgRef::Name(name.to_string())
                } else {
                    return Err(format!("invalid placeholder `{{{inner}}}`"));
                };
                if spec.contains(".*") {
                    return Err("`.*` precision is not supported; name it: `{x:.prec$}`".into());
                }
                out.push(Piece::Literal(std::mem::take(&mut literal)));
                out.push(Piece::Placeholder {
                    arg,
                    spec: spec.to_string(),
                });
            }
            c => literal.push(c),
        }
    }
    out.push(Piece::Literal(literal));
    Ok(out)
}

/// `name$` and `N$` references inside a format spec.
fn spec_references(spec: &str) -> Vec<ArgRef> {
    let mut out = Vec::new();
    let mut token = String::new();
    for c in spec.chars() {
        if c.is_alphanumeric() || c == '_' {
            token.push(c);
            continue;
        }
        if c == '$' && !token.is_empty() {
            out.push(match token.parse() {
                Ok(index) => ArgRef::Index(index),
                Err(_) => ArgRef::Name(token.clone()),
            });
        }
        token.clear();
    }
    out
}

pub fn richf(input: TokenStream) -> syn::Result<TokenStream> {
    let Richf {
        keys,
        template,
        args,
    } = syn::parse2(input)?;
    let span = template.span();
    let error = |message: String| syn::Error::new(span, message);
    let pieces = pieces(&template.value()).map_err(error)?;

    // Check the markup with each placeholder standing in as MARK, and find the
    // placeholders that sit inside a tag: those are inserted as markup.
    let mut checked = String::new();
    let mut offsets = Vec::new();
    for piece in &pieces {
        match piece {
            Piece::Literal(text) => checked.push_str(text),
            Piece::Placeholder { .. } => {
                // A letter first, so `[{color}]` still reads as a tag.
                checked.push('x');
                offsets.push(checked.len());
                checked.push(MARK);
            }
        }
    }
    checks::check(&checked, &keys, Some(MARK)).map_err(error)?;
    let tags = checks::tags(&checked);
    let in_tag: Vec<bool> = offsets
        .iter()
        .map(|&at| tags.iter().any(|tag| tag.start < at && at < tag.end))
        .collect();

    // Bind every argument once, by reference, in order.
    let mut bindings = Vec::new();
    let mut positional = Vec::new();
    let mut named: Vec<(String, Ident)> = Vec::new();
    for (index, arg) in args.iter().enumerate() {
        let binding = format_ident!("__rich_arg{}", index);
        match arg {
            Arg::Positional(expr) => {
                if !named.is_empty() {
                    return Err(syn::Error::new_spanned(
                        expr,
                        "positional arguments must come before named ones",
                    ));
                }
                bindings.push(quote!(let #binding = &(#expr);));
                positional.push(binding);
            }
            Arg::Named(name, expr) => {
                bindings.push(quote!(let #binding = &(#expr);));
                named.push((name.to_string(), binding));
            }
        }
    }
    let mut used_positional = vec![false; positional.len()];
    let mut used_named = vec![false; named.len()];
    let mut resolve = |arg: &ArgRef| -> syn::Result<TokenStream> {
        match arg {
            ArgRef::Index(index) => match positional.get(*index) {
                Some(binding) => {
                    used_positional[*index] = true;
                    Ok(quote!(#binding))
                }
                None => Err(error(format!("placeholder {index} has no argument"))),
            },
            ArgRef::Name(name) => match named.iter().position(|(n, _)| n == name) {
                Some(at) => {
                    used_named[at] = true;
                    let binding = &named[at].1;
                    Ok(quote!(#binding))
                }
                // Captured from scope, as `format!` does.
                None => {
                    let ident = Ident::new(name, Span::call_site());
                    Ok(quote_spanned!(span=> &#ident))
                }
            },
        }
    };

    let mut pushes = Vec::new();
    let mut placeholder = 0;
    for piece in &pieces {
        match piece {
            Piece::Literal(text) if text.is_empty() => {}
            Piece::Literal(text) => pushes.push(quote!(__rich_markup.push_str(#text);)),
            Piece::Placeholder { arg, spec } => {
                let value = resolve(arg)?;
                let mut extra = Vec::new();
                for reference in spec_references(spec) {
                    let name = match &reference {
                        ArgRef::Index(index) => format_ident!("__rich_ref{}", index),
                        ArgRef::Name(name) => Ident::new(name, Span::call_site()),
                    };
                    let bound = resolve(&reference)?;
                    extra.push(quote!(#name = *#bound));
                }
                let spec = spec_rewrite(spec);
                let format = format!("{{:{spec}}}");
                let formatted = quote!(::std::format!(#format, #value #(, #extra)*));
                pushes.push(if in_tag[placeholder] {
                    quote!(__rich_markup.push_str(&#formatted);)
                } else {
                    quote!(__rich_markup.push_str(
                        &::rich_ext::__private::rich::markup::escape(&#formatted)
                    );)
                });
                placeholder += 1;
            }
        }
    }
    if let Some(index) = used_positional.iter().position(|used| !used) {
        return Err(syn::Error::new(
            span,
            format!("argument {index} is never used"),
        ));
    }
    if let Some(at) = used_named.iter().position(|used| !used) {
        return Err(syn::Error::new(
            span,
            format!("named argument `{}` is never used", named[at].0),
        ));
    }
    let dynamic = in_tag.iter().any(|dynamic| *dynamic);
    let build = if dynamic {
        // A placeholder inside a tag was only checked at run time.
        quote!(
            ::rich_ext::__private::rich::Text::from_markup(&__rich_markup)
                .unwrap_or_else(|_| ::rich_ext::__private::rich::Text::new(__rich_markup.clone()))
        )
    } else {
        quote!(
            ::rich_ext::__private::rich::Text::from_markup(&__rich_markup)
                .expect("markup checked by richf!")
        )
    };
    Ok(quote!({
        #(#bindings)*
        let mut __rich_markup = ::std::string::String::new();
        #(#pushes)*
        #build
    }))
}

/// Rename `N$` references to the `__rich_refN` names they are passed as.
fn spec_rewrite(spec: &str) -> String {
    let mut out = String::new();
    let mut token = String::new();
    for c in spec.chars() {
        if c.is_alphanumeric() || c == '_' {
            token.push(c);
            continue;
        }
        if c == '$' && !token.is_empty() && token.chars().all(|c| c.is_ascii_digit()) {
            out.push_str("__rich_ref");
        }
        out.push_str(&token);
        token.clear();
        out.push(c);
    }
    out.push_str(&token);
    out
}

struct Checked {
    keys: Vec<String>,
    literal: LitStr,
}

impl Parse for Checked {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let keys = parse_keys(input)?;
        let literal = input.parse()?;
        let _ = input.parse::<Option<Token![,]>>()?;
        Ok(Checked { keys, literal })
    }
}

pub fn markup(input: TokenStream) -> syn::Result<TokenStream> {
    let Checked { keys, literal } = syn::parse2(input)?;
    checks::check(&literal.value(), &keys, None)
        .map_err(|message| syn::Error::new(literal.span(), message))?;
    Ok(quote!(#literal))
}

pub fn style(input: TokenStream) -> syn::Result<TokenStream> {
    let literal: LitStr = syn::parse2(input)?;
    if let Err(error) = rich::Style::parse(&literal.value()) {
        return Err(syn::Error::new(
            literal.span(),
            format!("invalid style: {error}"),
        ));
    }
    Ok(
        quote!(::rich_ext::__private::rich::Style::parse(#literal).expect("style checked by style!")),
    )
}

pub fn theme_key(input: TokenStream) -> syn::Result<TokenStream> {
    let literal: LitStr = syn::parse2(input)?;
    let key = literal.value();
    if !rich::theme::DEFAULT_STYLES
        .iter()
        .any(|(name, _)| *name == key)
    {
        let near: Vec<&str> = rich::theme::DEFAULT_STYLES
            .iter()
            .map(|(name, _)| *name)
            .filter(|name| name.split('.').next() == key.split('.').next())
            .take(5)
            .collect();
        let hint = if near.is_empty() {
            String::new()
        } else {
            format!("; keys in that group include {}", near.join(", "))
        };
        return Err(syn::Error::new(
            literal.span(),
            format!("`{key}` is not a default theme key{hint}"),
        ));
    }
    Ok(quote!(#literal))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_split_into_literals_and_placeholders() {
        let split = pieces("a {{b}} {} {name:>4} {0}").unwrap();
        let args: Vec<_> = split
            .iter()
            .filter_map(|piece| match piece {
                Piece::Placeholder { arg, spec } => Some((arg.clone(), spec.clone())),
                Piece::Literal(_) => None,
            })
            .collect();
        assert_eq!(
            args,
            [
                (ArgRef::Index(0), String::new()),
                (ArgRef::Name("name".into()), ">4".into()),
                (ArgRef::Index(0), String::new()),
            ]
        );
        assert!(pieces("a }").is_err());
        assert!(pieces("{x").is_err());
        assert!(pieces("{a b}").is_err());
    }

    #[test]
    fn spec_references_and_rewrites() {
        assert_eq!(
            spec_references(">width$.1$"),
            [ArgRef::Name("width".into()), ArgRef::Index(1)]
        );
        assert_eq!(spec_rewrite(">width$.1$"), ">width$.__rich_ref1$");
    }
}
