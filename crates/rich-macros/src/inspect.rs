//! `#[derive(Inspect)]` and `#[inspect]` on an `impl` block: the facts
//! `rich_ext::inspect` reports, collected at compile time (#796).

use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree};
use quote::{ToTokens, format_ident, quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{Attribute, Data, DeriveInput, Expr, Fields, ImplItem, ItemImpl, Visibility};

const FIELD_ATTRS: &str = "expected `skip`, `nested` or `with = path`";

/// A field's `#[inspect(…)]` options.
#[derive(Default)]
struct FieldOptions {
    skip: bool,
    nested: bool,
    with: Option<syn::Path>,
}

fn field_options(field: &syn::Field) -> syn::Result<FieldOptions> {
    let mut options = FieldOptions::default();
    for attr in field.attrs.iter().filter(|a| a.path().is_ident("inspect")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip") {
                options.skip = true;
            } else if meta.path.is_ident("nested") {
                options.nested = true;
            } else if meta.path.is_ident("with") {
                options.with = Some(meta.value()?.parse()?);
            } else {
                return Err(meta.error(FIELD_ATTRS));
            }
            Ok(())
        })?;
    }
    let chosen = usize::from(options.skip)
        + usize::from(options.nested)
        + usize::from(options.with.is_some());
    if chosen > 1 {
        return Err(syn::Error::new(
            field.span(),
            "use one of `skip`, `nested` and `with`",
        ));
    }
    Ok(options)
}

/// The type's `#[inspect(…)]` options: only `methods`.
fn type_methods(input: &DeriveInput) -> syn::Result<bool> {
    let mut methods = false;
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("inspect")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("methods") {
                methods = true;
                Ok(())
            } else {
                Err(meta.error(
                    "expected `methods` (the methods recorded by `#[inspect]` on an impl block)",
                ))
            }
        })?;
    }
    Ok(methods)
}

/// The doc comment, as a `&'static str` expression: the `#[doc]` values
/// joined by newlines (`concat!`, so `#[doc = include_str!(…)]` works too).
fn doc_expr(attrs: &[Attribute]) -> TokenStream {
    let parts: Vec<&Expr> = attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .filter_map(|attr| match &attr.meta {
            syn::Meta::NameValue(value) => Some(&value.value),
            _ => None,
        })
        .collect();
    match parts.as_slice() {
        [] => quote!(""),
        [first, rest @ ..] => quote!(::std::concat!(#first #(, "\n", #rest)*)),
    }
}

/// Whether `ty` names one of the type parameters (and so needs a bound).
fn mentions(tokens: TokenStream, params: &[String]) -> bool {
    tokens.into_iter().any(|tree| match tree {
        TokenTree::Ident(ident) => params.iter().any(|param| ident == param),
        TokenTree::Group(group) => mentions(group.stream(), params),
        _ => false,
    })
}

/// The pattern binding a struct's or variant's fields, the `Field`
/// expressions built from the bindings, whether any were skipped, and the
/// where-clause predicates the field types need.
struct Bound {
    pattern: TokenStream,
    fields: Vec<TokenStream>,
    skipped: bool,
    shape: TokenStream,
}

fn bind(
    own: &syn::Ident,
    fields: &Fields,
    always_public: bool,
    params: &[String],
    predicates: &mut Vec<TokenStream>,
) -> syn::Result<Bound> {
    let mut pattern = Vec::new();
    let mut entries = Vec::new();
    let mut skipped = false;
    for (index, field) in fields.iter().enumerate() {
        let options = field_options(field)?;
        let binding = format_ident!("__inspect_field{}", index);
        if options.skip {
            skipped = true;
            if let Some(ident) = &field.ident {
                pattern.push(quote!(#ident: _));
            } else {
                pattern.push(quote!(_));
            }
            continue;
        }
        match &field.ident {
            Some(ident) => pattern.push(quote!(#ident: #binding)),
            None => pattern.push(quote!(#binding)),
        }
        let name = match &field.ident {
            Some(ident) => ident.to_string().trim_start_matches("r#").to_string(),
            None => index.to_string(),
        };
        let ty = &field.ty;
        let ty_text = tokens_to_string(ty.to_token_stream());
        let generic = mentions(ty.to_token_stream(), params);
        let span = ty.span();
        let value = if let Some(with) = &options.with {
            quote_spanned!(span=> ::rich_ext::inspect::FieldValue::debug_with(#binding, #with))
        } else if options.nested {
            // A recursive field (`Option<Box<Tree<T>>>`) would make the
            // bound prove itself; the impl's own bounds already cover it.
            if generic && !mentions(ty.to_token_stream(), &[own.to_string(), "Self".into()]) {
                predicates.push(quote!(#ty: ::rich_ext::inspect::InspectField));
            }
            quote_spanned!(span=> ::rich_ext::inspect::InspectField::inspect_field(#binding, __inspect_depth))
        } else {
            if generic {
                predicates.push(quote!(#ty: ::std::fmt::Debug));
            }
            quote_spanned!(span=> ::rich_ext::inspect::FieldValue::debug::<#ty>(#binding))
        };
        let doc = doc_expr(&field.attrs);
        let public = always_public || matches!(field.vis, Visibility::Public(_));
        entries.push(quote! {
            ::rich_ext::inspect::Field::new(#name, #ty_text, #value)
                .doc(#doc)
                .public(#public)
        });
    }
    let (pattern, shape) = match fields {
        Fields::Named(_) => (
            quote!({ #(#pattern,)* }),
            quote!(::rich_ext::inspect::Shape::Named),
        ),
        Fields::Unnamed(_) => (
            quote!(( #(#pattern,)* )),
            quote!(::rich_ext::inspect::Shape::Tuple),
        ),
        Fields::Unit => (quote!(), quote!(::rich_ext::inspect::Shape::Unit)),
    };
    Ok(Bound {
        pattern,
        fields: entries,
        skipped,
        shape,
    })
}

pub fn derive(input: DeriveInput) -> syn::Result<TokenStream> {
    let methods = type_methods(&input)?;
    let name = &input.ident;
    let ident = name.to_string().trim_start_matches("r#").to_string();
    let params: Vec<String> = input
        .generics
        .type_params()
        .map(|param| param.ident.to_string())
        .collect();
    let mut predicates = Vec::new();
    let doc = doc_expr(&input.attrs);

    let (kind, variants, arms, heads) = match &input.data {
        Data::Struct(data) => {
            let bound = bind(name, &data.fields, false, &params, &mut predicates)?;
            let Bound {
                pattern,
                fields,
                skipped,
                shape,
            } = bound;
            (
                quote!(::rich_ext::inspect::TypeKind::Struct),
                quote!(),
                vec![quote! {
                    Self #pattern => ::rich_ext::inspect::Record::new(#shape, ::std::vec![#(#fields),*])
                        .skipped(#skipped)
                }],
                vec![quote! {
                    Self { .. } => ::rich_ext::inspect::Record::new(#shape, ::std::vec::Vec::new())
                        .skipped(#skipped)
                }],
            )
        }
        Data::Enum(data) => {
            if data.variants.is_empty() {
                return Err(syn::Error::new(
                    name.span(),
                    "cannot derive Inspect for an enum with no variants: it has no values",
                ));
            }
            let mut arms = Vec::new();
            let mut heads = Vec::new();
            let mut variants = Vec::new();
            for (index, variant) in data.variants.iter().enumerate() {
                let variant_ident = &variant.ident;
                let variant_name = variant_ident
                    .to_string()
                    .trim_start_matches("r#")
                    .to_string();
                let Bound {
                    pattern,
                    fields,
                    skipped,
                    shape,
                } = bind(name, &variant.fields, true, &params, &mut predicates)?;
                let variant_doc = doc_expr(&variant.attrs);
                variants.push(quote! {
                    ::rich_ext::inspect::Variant::new(#variant_name, #shape).doc(#variant_doc)
                });
                arms.push(quote! {
                    Self::#variant_ident #pattern => ::rich_ext::inspect::Record::new(
                        #shape,
                        ::std::vec![#(#fields),*],
                    )
                    .variant(#index)
                    .skipped(#skipped)
                });
                heads.push(quote! {
                    Self::#variant_ident { .. } => ::rich_ext::inspect::Record::new(
                        #shape,
                        ::std::vec::Vec::new(),
                    )
                    .variant(#index)
                    .skipped(#skipped)
                });
            }
            (
                quote!(::rich_ext::inspect::TypeKind::Enum),
                quote! {
                    .variants({
                        const VARIANTS: &[::rich_ext::inspect::Variant] = &[#(#variants),*];
                        VARIANTS
                    })
                },
                arms,
                heads,
            )
        }
        Data::Union(data) => {
            return Err(syn::Error::new(
                data.union_token.span(),
                "cannot derive Inspect for a union: which field holds the value is not known",
            ));
        }
    };

    if methods {
        // The impl exists where the methods' impl does, whatever its bounds.
        predicates.push(quote!(Self: ::rich_ext::inspect::InspectMethods));
    }
    let mut generics = input.generics.clone();
    if !predicates.is_empty() {
        let where_clause = generics.make_where_clause();
        for predicate in predicates {
            where_clause.predicates.push(syn::parse2(predicate)?);
        }
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let methods = methods.then(|| {
        quote! {
            fn inspect_methods(&self) -> &'static [::rich_ext::inspect::Method] {
                <Self as ::rich_ext::inspect::InspectMethods>::METHODS
            }
        }
    });

    Ok(quote! {
        impl #impl_generics ::rich_ext::inspect::Inspectable for #name #ty_generics #where_clause {
            fn inspect_type(&self) -> ::rich_ext::inspect::TypeInfo {
                ::rich_ext::inspect::TypeInfo::new(
                    ::std::any::type_name::<Self>(),
                    #ident,
                    #kind,
                )
                .doc(#doc)
                #variants
            }

            #[allow(unused_variables)]
            fn inspect_record(&self, __inspect_depth: usize) -> ::rich_ext::inspect::Record {
                match self {
                    #(#arms,)*
                }
            }

            fn inspect_head(&self) -> ::rich_ext::inspect::Record {
                match self {
                    #(#heads,)*
                }
            }

            #methods
        }
    })
}

/// `#[inspect]` on an inherent `impl` block: the block unchanged (less any
/// `#[inspect(skip)]` on its methods) and an `InspectMethods` impl.
pub fn methods(args: TokenStream, mut item: ItemImpl) -> syn::Result<TokenStream> {
    if !args.is_empty() {
        return Err(syn::Error::new(
            args.span(),
            "`#[inspect]` on an impl block takes no arguments",
        ));
    }
    if let Some((path, _)) = &item.trait_ {
        return Err(syn::Error::new(
            path.span(),
            "`#[inspect]` records an inherent impl's methods; a trait impl's are not supported",
        ));
    }
    let mut methods = Vec::new();
    for impl_item in &mut item.items {
        let ImplItem::Fn(function) = impl_item else {
            continue;
        };
        let mut skip = false;
        let mut error = None;
        function.attrs.retain(|attr| {
            if !attr.path().is_ident("inspect") {
                return true;
            }
            let parsed = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("skip") {
                    skip = true;
                    Ok(())
                } else {
                    Err(meta.error("expected `skip`"))
                }
            });
            if let Err(parsed) = parsed {
                error = Some(parsed);
            }
            false
        });
        if let Some(error) = error {
            return Err(error);
        }
        if skip {
            continue;
        }
        let sig = &function.sig;
        let mut prefix = Vec::new();
        let vis = tokens_to_string(function.vis.to_token_stream());
        if !vis.is_empty() {
            prefix.push(vis);
        }
        if sig.constness.is_some() {
            prefix.push("const".to_string());
        }
        if sig.asyncness.is_some() {
            prefix.push("async".to_string());
        }
        if matches!(sig.safety, syn::Safety::Unsafe(_)) {
            prefix.push("unsafe".to_string());
        }
        if let Some(abi) = &sig.abi {
            prefix.push(tokens_to_string(abi.to_token_stream()));
        }
        prefix.push("fn".to_string());
        let prefix = prefix.join(" ");
        let name = sig.ident.to_string().trim_start_matches("r#").to_string();
        let mut inputs = sig.inputs.clone();
        for input in &mut inputs {
            match input {
                syn::FnArg::Receiver(receiver) => receiver.attrs.clear(),
                syn::FnArg::Typed(typed) => typed.attrs.clear(),
            }
        }
        let params = &sig.generics.params;
        let generics = if params.is_empty() {
            quote!()
        } else {
            quote!(<#params>)
        };
        let output = &sig.output;
        let where_clause = &sig.generics.where_clause;
        let variadic = sig.variadic.as_ref().map(|_| quote!(, ...));
        let signature =
            tokens_to_string(quote!(#generics (#inputs #variadic) #output #where_clause));
        let doc = doc_expr(&function.attrs);
        let public = matches!(function.vis, Visibility::Public(_));
        let is_async = sig.asyncness.is_some();
        methods.push(quote! {
            ::rich_ext::inspect::Method::new(#name, #prefix, #signature)
                .doc(#doc)
                .public(#public)
                .is_async(#is_async)
        });
    }
    let self_ty = &item.self_ty;
    let (impl_generics, _, where_clause) = item.generics.split_for_impl();
    Ok(quote! {
        #item

        impl #impl_generics ::rich_ext::inspect::InspectMethods for #self_ty #where_clause {
            const METHODS: &'static [::rich_ext::inspect::Method] = &[#(#methods),*];
        }
    })
}

/// A token of a type or signature, for [`tokens_to_string`].
#[derive(Clone, PartialEq)]
enum Token {
    /// An identifier, keyword, lifetime (`'a`) or literal.
    Word(String),
    /// Joined punctuation: `->`, `::`, `&`.
    Op(String),
    Open(char),
    Close(char),
}

fn flatten(tokens: TokenStream, out: &mut Vec<Token>) {
    let mut pending = String::new();
    for tree in tokens {
        match tree {
            TokenTree::Punct(punct) => {
                pending.push(punct.as_char());
                if punct.as_char() == '\'' {
                    continue;
                }
                if punct.spacing() == Spacing::Alone {
                    out.push(Token::Op(std::mem::take(&mut pending)));
                }
                continue;
            }
            TokenTree::Ident(ident) if pending == "'" => {
                out.push(Token::Word(format!("'{ident}")));
                pending.clear();
                continue;
            }
            _ => {}
        }
        if !pending.is_empty() {
            out.push(Token::Op(std::mem::take(&mut pending)));
        }
        match tree {
            TokenTree::Ident(ident) => out.push(Token::Word(ident.to_string())),
            TokenTree::Literal(literal) => out.push(Token::Word(literal.to_string())),
            TokenTree::Group(group) => {
                let (open, close) = match group.delimiter() {
                    Delimiter::Parenthesis => (Some('('), Some(')')),
                    Delimiter::Bracket => (Some('['), Some(']')),
                    Delimiter::Brace => (Some('{'), Some('}')),
                    Delimiter::None => (None, None),
                };
                if let Some(open) = open {
                    out.push(Token::Open(open));
                }
                flatten(group.stream(), out);
                if let Some(close) = close {
                    out.push(Token::Close(close));
                }
            }
            TokenTree::Punct(_) => unreachable!(),
        }
    }
    if !pending.is_empty() {
        out.push(Token::Op(pending));
    }
}

fn is_op(token: &Token, ops: &[&str]) -> bool {
    matches!(token, Token::Op(op) if ops.contains(&op.as_str()))
}

fn closes_generics(token: &Token) -> bool {
    matches!(token, Token::Op(op) if op.chars().all(|c| c == '>'))
}

/// Whether a space goes between two tokens of a type or signature, so the
/// text reads as rustfmt writes it: `fn f<T: Into<u8>>(&self, x: T) -> u8`.
fn space_between(prev: &Token, next: &Token) -> bool {
    if matches!(prev, Token::Open(_)) || matches!(next, Token::Close(_)) {
        return false;
    }
    if is_op(next, &[",", ";", ":"]) || closes_generics(next) {
        return false;
    }
    if is_op(prev, &["&", "&&", "*", "::", "<", "?", "#", "$"]) {
        return false;
    }
    if is_op(next, &["::"]) {
        return !matches!(prev, Token::Word(_) | Token::Close(_)) && !closes_generics(prev);
    }
    if is_op(next, &["<"]) {
        return !matches!(prev, Token::Word(_));
    }
    if matches!(next, Token::Open('(' | '[')) {
        // `Fn(u8)` and `pub(crate)`, but `&'a [u8]`, `mut (a, b)`, `dyn (…)`.
        return match prev {
            Token::Word(word) => {
                word.starts_with('\'')
                    || matches!(
                        word.as_str(),
                        "mut" | "dyn" | "impl" | "const" | "where" | "as" | "ref" | "in"
                    )
            }
            // `fn f<T>(…)`.
            prev => !closes_generics(prev),
        };
    }
    true
}

/// A type or signature as rustfmt would write it. Token streams print with
/// spaces between every token (`Vec < u8 >`), which reads badly in a report.
pub fn tokens_to_string(tokens: TokenStream) -> String {
    let mut flat = Vec::new();
    flatten(tokens, &mut flat);
    let mut out = String::new();
    let mut prev: Option<&Token> = None;
    for token in &flat {
        if let Some(prev) = prev
            && space_between(prev, token)
        {
            out.push(' ');
        }
        match token {
            Token::Word(word) | Token::Op(word) => out.push_str(word),
            Token::Open(c) | Token::Close(c) => out.push(*c),
        }
        prev = Some(token);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::tokens_to_string;
    use quote::quote;

    #[test]
    fn types_and_signatures_read_as_written() {
        let cases = [
            (quote!(Vec<String>), "Vec<String>"),
            (quote!(Option<Box<Node>>), "Option<Box<Node>>"),
            (quote!(&'a mut [u8; 4]), "&'a mut [u8; 4]"),
            (
                quote!(::std::collections::HashMap<K, V>),
                "::std::collections::HashMap<K, V>",
            ),
            (quote!((u8, u16)), "(u8, u16)"),
            (quote!(*const T), "*const T"),
            (
                quote!(Box<dyn Fn(u8) -> u8 + Send>),
                "Box<dyn Fn(u8) -> u8 + Send>",
            ),
            (
                quote!(<T: Into<f64>>(&self, scale: T) -> f64 where T: Copy),
                "<T: Into<f64>>(&self, scale: T) -> f64 where T: Copy",
            ),
            (
                quote!((self: Box<Self>, mut items: Vec<T>)),
                "(self: Box<Self>, mut items: Vec<T>)",
            ),
            (quote!(<T: ?Sized>(value: &T)), "<T: ?Sized>(value: &T)"),
            (quote!(pub(crate)), "pub(crate)"),
            (quote!(() -> !), "() -> !"),
            (quote!(extern "C"), "extern \"C\""),
            (quote!(impl Iterator<Item = u8>), "impl Iterator<Item = u8>"),
        ];
        for (tokens, expected) in cases {
            assert_eq!(tokens_to_string(tokens), expected);
        }
    }
}
