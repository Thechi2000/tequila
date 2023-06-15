#![allow(unreachable_code)]

use std::sync::OnceLock;

use config::TequilaConfig;
use proc_macro::TokenStream;
use proc_macro2::Ident;
use proc_macro_error::{abort_call_site, emit_error, proc_macro_error};
use syn::{
    spanned::Spanned,
    Item, LitStr, Meta, MetaList, Path, Type, TypePath,
    __private::{quote::quote, Span},
    parse_macro_input,
    punctuated::Punctuated,
    token::Comma,
};

mod config;

const TEQUILA_URL: &str = "https://tequila.epfl.ch/cgi-bin/tequila/";

static CONFIG: OnceLock<Option<TequilaConfig>> = OnceLock::new();

#[derive(Debug)]
struct Field {
    name: Option<String>,
    attribute: (String, Span),
    _type: FieldType,
}

#[derive(Debug)]
enum FieldType {
    Other,
    Option,
    Vec,
}

const OPTION_PATHES: [(&str, bool); 3] = [
    ("Option", false),
    ("std|option|Option", true),
    ("core|option|Option", true),
];
const VEC_PATHES: [(&str, bool); 3] = [
    ("Vec", false),
    ("std|vec|Vec|", true),
    ("alloc|vec|Vec", true),
];

fn compare_pathes(pathes: &[(&str, bool)], path: &Path) -> bool {
    pathes.iter().any(|p| {
        let split = p.0.split('|').collect::<Vec<_>>();
        split.len() == path.segments.len()
            && split
                .iter()
                .zip(path.segments.iter())
                .all(|(a, b)| *a == b.ident.to_string())
            && (path.leading_colon.is_some() || !p.1)
    })
}

/// Derives the `FromTequilaAttributes` trait. The fields of type `Option` or `Vec` are considered optional.
/// 
/// You may set the key of the value that the field should take using the `#[tequila("key")]` attribute. If no such attribute is present, the key will default to the field's name
#[proc_macro_error]
#[proc_macro_derive(FromTequilaAttributes, attributes(tequila))]
pub fn derive_from_tequila_attributes(ts: TokenStream) -> TokenStream {
    let Ok(Item::Struct(struct_))= syn::parse::<Item>(ts) else {
        abort_call_site!("FromTequilaAttributes can only be used on structs")
    };
    let mut error: bool = false;

    let fields = struct_
        .fields
        .iter()
        .map(|f| Field {
            name: f.ident.as_ref().map(|i| i.to_string()),
            attribute: f
                .attrs
                .iter()
                .find_map(|a| match &a.meta {
                    Meta::List(MetaList { path, tokens, .. }) => {
                        if path.is_ident("tequila") {
                            Some((
                                syn::parse::<LitStr>(tokens.clone().into())
                                    .unwrap_or_else(|_| {
                                        error = true;
                                        emit_error!(tokens, "Must be a string litteral");
                                        LitStr::new("", tokens.span())
                                    })
                                    .value(),
                                tokens.span(),
                            ))
                        } else {
                            None
                        }
                    }
                    _ => None,
                })
                .unwrap_or_else(|| {
                    (
                        f.ident.as_ref().map(|i| i.to_string()).unwrap_or_else(|| {
                            error = true;
                            emit_error!(f, "Unqualified fields must be named");
                            "".into()
                        }),
                        f.ident.span(),
                    )
                }),
            _type: match &f.ty {
                Type::Path(TypePath { path, .. }) => {
                    if compare_pathes(&OPTION_PATHES, path) {
                        FieldType::Option
                    } else if compare_pathes(&VEC_PATHES, path) {
                        FieldType::Vec
                    } else {
                        FieldType::Other
                    }
                }
                t => {
                    error = true;
                    emit_error!(t, "Unsupported type");
                    FieldType::Other
                }
            },
        })
        .collect::<Vec<_>>();

    if error {
        return TokenStream::new();
    }

    let check = {
        let meta = struct_.attrs.iter().find_map(|m| match &m.meta {
            Meta::List(MetaList { path, tokens, .. }) => path.is_ident("tequila").then_some(tokens),
            _ => None,
        });

        if let Some(meta) = meta {
            let meta: proc_macro::TokenStream = meta.clone().into();
            let input = parse_macro_input!(meta with Punctuated<Path, Comma>::parse_terminated);
            !input.iter().any(|id| id.is_ident("no_check"))
        } else {
            true
        }
    };

    if check {
        if let Some(cfg) = CONFIG.get_or_init(|| TequilaConfig::fetch(TEQUILA_URL.into()).ok()) {
            fields.iter().for_each(|f| {
                if !cfg.attributes.contains(&f.attribute.0) {
                    error = true;
                    emit_error!(
                        f.attribute.1,
                        "Invalid attribute \"{}\"", f.attribute.0;
                        note = "Available attributes are [{}]", cfg.attributes.join(", ")
                    )
                }
            })
        }
    }

    if error {
        return TokenStream::new();
    }

    let requested_attributes = fields
        .iter()
        .filter_map(|f| {
            matches!(f._type, FieldType::Option | FieldType::Vec).then(|| {
                let f = f.attribute.0.clone();
                quote! {
                    #f.into(),
                }
            })
        })
        .fold(proc_macro2::TokenStream::new(), |mut acc, ts| {
            acc.extend(ts);
            acc
        });
    let required_attributes = fields
        .iter()
        .filter_map(|f| {
            matches!(f._type, FieldType::Other).then(|| {
                let f = f.attribute.0.clone();
                quote! {
                    #f.into(),
                }
            })
        })
        .fold(proc_macro2::TokenStream::new(), |mut acc, ts| {
            acc.extend(ts);
            acc
        });

    let f = fields.into_iter().enumerate().map(|(i, f)| {
        let Field { name, attribute: (key, _), _type } = f;
        let name = Ident::new(name.unwrap_or_else(|| i.to_string()).as_str(), Span::call_site());
        let key = Ident::new(&key, Span::call_site());
        let key_str = key.to_string();

        match _type {
            FieldType::Other => quote!{
                #name: attributes.get(&#key_str.to_string()).ok_or(::tequila::TequilaError::MissingAttribute(#key_str.into()))?.clone(),
            },
            FieldType::Option => quote!{
                #name: attributes.get(&#key_str.to_string()).unwrap_or_default(),
            },
            FieldType::Vec => quote! {
                #name: attributes.get(&#key_str.to_string()).unwrap_or_default().split(',').collect(),
            },
        }
    })
    .fold(proc_macro2:: TokenStream::new(), |mut acc, ts| {acc.extend(ts); acc});

    let id = struct_.ident;
    quote! {
        impl ::tequila::FromTequilaAttributes for #id {
            fn from_tequila_attributes(attributes: ::std::collections::HashMap<String, String>) -> Result<Self, ::tequila::TequilaError> {
                Ok(Self {
                    #f
                })
            }

            fn requested_attributes() -> Vec<String> {
                vec![#requested_attributes]
            }

            fn required_attributes() -> Vec<String> {
                vec![#required_attributes]
            }
        }
    }.into()
}
