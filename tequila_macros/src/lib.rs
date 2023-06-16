#![allow(unreachable_code)]

use std::sync::OnceLock;

use config::TequilaConfig;
use proc_macro::TokenStream;
use proc_macro2::Ident;
use proc_macro_error::{
    abort_call_site, emit_call_site_warning, emit_error, proc_macro_error, set_dummy,
};
use syn::{
    spanned::Spanned,
    Item, LitStr, Meta, MetaList, Path, Type, TypePath,
    __private::{quote::quote, Span},
};

mod config;

const TEQUILA_URL: &str = "https://tequila.epfl.ch/cgi-bin/tequila/";

static CONFIG: OnceLock<Option<TequilaConfig>> = OnceLock::new();

#[derive(Debug)]
struct Field {
    name: Option<String>,
    attribute: String,
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

fn get_config() -> &'static Option<TequilaConfig> {
    if let Some(cfg) = CONFIG.get() {
        cfg
    } else {
        let config = TequilaConfig::fetch(TEQUILA_URL.into());

        if let Err(e) = &config {
            emit_call_site_warning!("Could not fetch Tequila's server configuration: {:?}", e);
        }

        CONFIG.get_or_init(|| config.ok())
    }
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

    // Create a dummy implementation in case the macro fails, to avoid the error: "<type> does not implement FromTequilaAttributes"
    let id = struct_.ident;
    set_dummy(quote! {
        impl ::tequila::FromTequilaAttributes for #id {
            fn from_tequila_attributes(attributes: ::std::collections::HashMap<String, String>) -> Result<Self, ::tequila::TequilaError> {
                unimplemented!();
            }

            fn wished_attributes() -> Vec<String> {
                unimplemented!();
            }

            fn requested_attributes() -> Vec<String> {
                unimplemented!();
            }
        }
    });

    // Get the no_check attibutes on the structure, which disables the verification with the server's configuration
    let mut check_config = true;
    if let Some(attr) = struct_.attrs.iter().find(|a| a.path().is_ident("tequila")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("no_check") {
                check_config = false;
                Ok(())
            } else {
                Err(meta.error("unrecognized attribute"))
            }
        })
        .unwrap_or_else(|e| emit_error!(attr, e.to_string()))
    }

    // Generate the required metadata for all fields
    let fields = struct_
        .fields
        .iter()
        .filter_map(|f| {
            // Get the key which this field will take its value from
            let mut key = f.ident.as_ref().map(|id| id.to_string());
            let mut key_span = f.span();
            if let Some(attr) = f.attrs.iter().find(|a| a.path().is_ident("tequila")) {
                if let Ok(val) = attr.parse_args::<LitStr>() {
                    key_span = val.span();
                    key = Some(val.value());
                } else {
                    match &attr.meta {
                        Meta::List(MetaList { tokens, .. }) => {
                            emit_error!(tokens.span(), "Expected string litteral")
                        }
                        o => {
                            emit_error!(
                                o,
                                "Should be a list containing exactly one string litteral"
                            )
                        }
                    }
                    return None;
                }
            }

            // If required, check the key with the server's configuration
            if check_config {
                if let (Some(key), Some(config)) = (&key, get_config()) {
                    if !config.attributes.contains(key) {
                        emit_error!(
                            key_span,
                            "Invalid attribute \"{}\"", key;
                            note = "Available attributes are [{}]", config.attributes.join(", ")
                        )
                    }
                }
            }

            Some(Field {
                name: f.ident.as_ref().map(|i| i.to_string()),
                attribute: key.unwrap_or_default(),
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
                        emit_error!(t, "Unsupported type");
                        FieldType::Other
                    }
                },
            })
        })
        .collect::<Vec<_>>();

    let wished_attributes = fields
        .iter()
        .filter_map(|f| {
            matches!(f._type, FieldType::Option | FieldType::Vec).then(|| {
                let f = f.attribute.clone();
                quote! {
                    #f.into(),
                }
            })
        })
        .fold(proc_macro2::TokenStream::new(), |mut acc, ts| {
            acc.extend(ts);
            acc
        });

    let requested_attributes = fields
        .iter()
        .filter_map(|f| {
            matches!(f._type, FieldType::Other).then(|| {
                let f = f.attribute.clone();
                quote! {
                    #f.into(),
                }
            })
        })
        .fold(proc_macro2::TokenStream::new(), |mut acc, ts| {
            acc.extend(ts);
            acc
        });

    // In order to check all fields before returning an error (thus indicating all missing fields in the error message),
    // we store all values in temporary variables, and the list of missing fields in the Vec missing
    // Temp variables are named f{field_number}, we cannot use their names since they may be anonym
    let field_variables = fields.iter().enumerate().map(|(i, f)| {
        let Field { attribute: key, _type ,..} = f;
        let name = Ident::new(&format!("f{i}"), Span::call_site());
        let key = Ident::new(&key, Span::call_site());
        let key_str = key.to_string();

        match _type {
            FieldType::Other => quote!{
                let #name = attributes.get(&#key_str.to_string()).cloned();
                if #name.is_none() { missing.push(#key_str.to_string()) }
            },
            FieldType::Option => quote!{
                let #name = attributes.get(&#key_str.to_string()).unwrap_or_default();
            },
            FieldType::Vec => quote! {
                let #name = attributes.get(&#key_str.to_string()).unwrap_or_default().split(',').collect();
            },
        }
    })
    .fold(proc_macro2:: TokenStream::new(), |mut acc, ts| {acc.extend(ts); acc});

    // Generate the list of assignation inside the Self {..} construct
    let field_assignations = fields
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let Field {
                _type,
                name: field_name,
                ..
            } = f;
            let name = Ident::new(&format!("f{i}"), Span::call_site());

            let value = match f._type {
                FieldType::Other => quote!(#name.unwrap()),
                _ => quote!(#name),
            };

            if let Some(name) = field_name {
                let name = Ident::new(name, Span::call_site());
                quote!(#name: #value,)
            } else {
                quote!(#value,)
            }
        })
        .fold(proc_macro2::TokenStream::new(), |mut acc, ts| {
            acc.extend(ts);
            acc
        });

    // For constructing the resulting struct, we need to differenciate three cases: named fields (Self {..}), anonym fields (Self(..)) and no fields (Self)
    let instanciation = match struct_.fields.iter().next() {
        Some(syn::Field { ident: Some(_), .. }) => quote!(Self { #field_assignations }),
        Some(syn::Field { ident: None, .. }) => quote!(Self (#field_assignations)),
        None => quote!(Self),
    };

    // Constructs the trait implementation
    quote! {
        impl ::tequila::FromTequilaAttributes for #id {
            fn from_tequila_attributes(attributes: ::std::collections::HashMap<String, String>) -> Result<Self, ::tequila::TequilaError> {
                let mut missing: ::std::vec::Vec<::std::string::String> = vec![];
                #field_variables
                if missing.is_empty() {
                    Ok(#instanciation)
                } else {
                    Err(::tequila::TequilaError::MissingAttributes(missing))
                }
            }

            fn wished_attributes() -> Vec<String> {
                vec![#wished_attributes]
            }

            fn requested_attributes() -> Vec<String> {
                vec![#requested_attributes]
            }
        }
    }.into()
}
