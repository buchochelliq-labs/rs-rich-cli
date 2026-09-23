//! `#[derive(Rich)]`: a `RichRecord` and a `Renderable` impl.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields, LitInt, LitStr};

#[derive(Default)]
struct TypeOptions {
    title: Option<LitStr>,
    panel: bool,
    table: bool,
}

struct FieldOptions {
    skip: bool,
    label: Option<LitStr>,
    style: Option<LitStr>,
    display: bool,
    format: Option<LitStr>,
    justify: Option<LitStr>,
    order: i64,
}

fn type_options(input: &DeriveInput) -> syn::Result<TypeOptions> {
    let mut options = TypeOptions::default();
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("rich")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("title") {
                options.title = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("panel") {
                options.panel = true;
            } else if meta.path.is_ident("table") {
                options.table = true;
            } else {
                return Err(meta.error("expected `title = \"…\"`, `panel` or `table`"));
            }
            Ok(())
        })?;
    }
    if options.panel && options.table {
        return Err(syn::Error::new(
            input.ident.span(),
            "choose one of `panel` and `table`",
        ));
    }
    Ok(options)
}

fn field_options(field: &syn::Field) -> syn::Result<FieldOptions> {
    let mut options = FieldOptions {
        skip: false,
        label: None,
        style: None,
        display: false,
        format: None,
        justify: None,
        order: 0,
    };
    for attr in field.attrs.iter().filter(|a| a.path().is_ident("rich")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip") {
                options.skip = true;
            } else if meta.path.is_ident("display") {
                options.display = true;
            } else if meta.path.is_ident("label") {
                options.label = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("style") {
                let style: LitStr = meta.value()?.parse()?;
                if let Err(error) = rich::Style::parse(&style.value()) {
                    return Err(syn::Error::new(
                        style.span(),
                        format!("invalid style: {error}"),
                    ));
                }
                options.style = Some(style);
            } else if meta.path.is_ident("format") {
                options.format = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("justify") {
                let justify: LitStr = meta.value()?.parse()?;
                if !matches!(justify.value().as_str(), "left" | "center" | "right") {
                    return Err(syn::Error::new(
                        justify.span(),
                        "expected \"left\", \"center\" or \"right\"",
                    ));
                }
                options.justify = Some(justify);
            } else if meta.path.is_ident("order") {
                let order: LitInt = meta.value()?.parse()?;
                options.order = order.base10_parse()?;
            } else {
                return Err(meta.error(
                    "expected `skip`, `label`, `style`, `display`, `format`, `justify` or `order`",
                ));
            }
            Ok(())
        })?;
    }
    if options.display && options.format.is_some() {
        return Err(syn::Error::new(
            field.span(),
            "use either `display` or `format`",
        ));
    }
    Ok(options)
}

/// Text-like types (`String`, `str`, `char`, `Cow<str>`, and references to
/// them) show with `Display`, unquoted; everything else with `Debug`.
fn is_text(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Reference(reference) => is_text(&reference.elem),
        syn::Type::Path(path) => path.path.segments.last().is_some_and(|segment| {
            matches!(
                segment.ident.to_string().as_str(),
                "String" | "str" | "char" | "Cow"
            )
        }),
        _ => false,
    }
}

/// The pattern binding a variant's or struct's fields, and the `Field`
/// expressions built from the bindings.
fn fields(fields: &Fields) -> syn::Result<(TokenStream, Vec<TokenStream>)> {
    let mut entries: Vec<(i64, usize, TokenStream)> = Vec::new();
    let mut pattern = Vec::new();
    for (index, field) in fields.iter().enumerate() {
        let options = field_options(field)?;
        let binding = format_ident!("__rich_field{}", index);
        match &field.ident {
            Some(ident) => pattern.push(quote!(#ident: #binding)),
            None => pattern.push(quote!(#binding)),
        }
        if options.skip {
            continue;
        }
        let label = match (&options.label, &field.ident) {
            (Some(label), _) => label.value(),
            (None, Some(ident)) => ident.to_string().trim_start_matches("r#").to_string(),
            (None, None) => index.to_string(),
        };
        let display = options.display || is_text(&field.ty);
        let format = match (&options.format, display) {
            (Some(format), _) => quote!(#format),
            (None, true) => quote!("{}"),
            (None, false) => quote!("{:?}"),
        };
        let style = match &options.style {
            Some(style) => quote!(::std::option::Option::Some(
                ::rich_ext::__private::rich::Style::parse(#style).expect("style checked by derive(Rich)")
            )),
            None => quote!(::std::option::Option::None),
        };
        let justify = match options.justify.as_ref().map(LitStr::value).as_deref() {
            Some("center") => quote!(::rich_ext::__private::rich::Justify::Center),
            Some("right") => quote!(::rich_ext::__private::rich::Justify::Right),
            _ => quote!(::rich_ext::__private::rich::Justify::Left),
        };
        let highlight = options.format.is_none() && !display && options.style.is_none();
        entries.push((
            options.order,
            index,
            quote!(::rich_ext::derive::Field {
                label: ::std::string::String::from(#label),
                value: ::std::format!(#format, #binding),
                style: #style,
                justify: #justify,
                highlight: #highlight,
            }),
        ));
    }
    entries.sort_by_key(|(order, index, _)| (*order, *index));
    let pattern = match fields {
        Fields::Named(_) => quote!({ #(#pattern,)* }),
        Fields::Unnamed(_) => quote!(( #(#pattern,)* )),
        Fields::Unit => quote!(),
    };
    Ok((
        pattern,
        entries.into_iter().map(|(_, _, entry)| entry).collect(),
    ))
}

pub fn rich(input: DeriveInput) -> syn::Result<TokenStream> {
    let options = type_options(&input)?;
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let presentation = if options.panel {
        quote!(::rich_ext::derive::Presentation::Panel)
    } else if options.table {
        quote!(::rich_ext::derive::Presentation::Table)
    } else {
        quote!(::rich_ext::derive::Presentation::Fields)
    };
    let title_override = options.title.as_ref().map(LitStr::value);

    let arms = match &input.data {
        Data::Struct(data) => {
            let (pattern, entries) = fields(&data.fields)?;
            let title = match &title_override {
                Some(title) => {
                    quote!(::std::option::Option::Some(::std::string::String::from(#title)))
                }
                None if options.panel => {
                    let name = name.to_string();
                    quote!(::std::option::Option::Some(::std::string::String::from(#name)))
                }
                None => quote!(::std::option::Option::None),
            };
            vec![quote!(Self #pattern => (#title, ::std::vec![#(#entries),*]))]
        }
        Data::Enum(data) => {
            let mut arms = Vec::new();
            for variant in &data.variants {
                let ident = &variant.ident;
                let (pattern, entries) = fields(&variant.fields)?;
                let title = title_override.clone().unwrap_or_else(|| ident.to_string());
                arms.push(quote!(Self::#ident #pattern => (
                    ::std::option::Option::Some(::std::string::String::from(#title)),
                    ::std::vec![#(#entries),*]
                )));
            }
            if arms.is_empty() {
                return Err(syn::Error::new(
                    name.span(),
                    "cannot derive Rich for an empty enum",
                ));
            }
            arms
        }
        Data::Union(_) => {
            return Err(syn::Error::new(
                name.span(),
                "cannot derive Rich for a union",
            ))
        }
    };

    Ok(quote! {
        impl #impl_generics ::rich_ext::derive::RichRecord for #name #ty_generics #where_clause {
            #[allow(unused_variables)]
            fn rich_record(&self) -> (
                ::std::option::Option<::std::string::String>,
                ::std::vec::Vec<::rich_ext::derive::Field>,
            ) {
                match self {
                    #(#arms,)*
                }
            }

            fn rich_presentation(&self) -> ::rich_ext::derive::Presentation {
                #presentation
            }
        }

        impl #impl_generics ::rich_ext::__private::rich::Renderable for #name #ty_generics #where_clause {
            fn rich_render(
                &self,
                console: &::rich_ext::__private::rich::Console,
                options: &::rich_ext::__private::rich::ConsoleOptions,
            ) -> ::std::vec::Vec<::rich_ext::__private::rich::Segment> {
                ::rich_ext::derive::render(self, console, options)
            }
        }
    })
}
