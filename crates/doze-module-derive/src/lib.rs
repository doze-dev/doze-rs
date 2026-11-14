//! `#[derive(Spec)]`: a module's config type gives both its schema (sent in the manifest, validated by the core,
//! documented by the registry) and its decoder, so the two cannot drift (docs/port/modules/sdk.md).
//!
//! Field attributes, all inside `#[spec(...)]`: `doc = "…"`, `default = <bool | number | "string">`, `since = N`,
//! `until = N` (engine majors), `rename = "hcl_name"`, `block` (a `Vec<T>` of nested blocks, `T: Spec`), `label` (this
//! nested block type's label; a `String`), `key` (an argument that is part of the block's identity with its labels: two
//! blocks are the same only if their labels and keys are; mariadb's `user "app"` on two hosts are two accounts).

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::{format_ident, quote};
use syn::{
    Data, DeriveInput, Expr, ExprLit, Fields, GenericArgument, Lit, PathArguments, Type,
    parse_macro_input,
};

#[proc_macro_derive(Spec, attributes(spec))]
pub fn derive_spec(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(&input) {
        Ok(t) => t.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

#[derive(Default)]
struct FieldSpec {
    name: Option<String>,
    doc: String,
    default: Option<Tokens>,
    since: Option<u32>,
    until: Option<u32>,
    block: bool,
    label: bool,
    key: bool,
}

fn int(e: &Expr) -> syn::Result<u32> {
    match e {
        Expr::Lit(ExprLit {
            lit: Lit::Int(i), ..
        }) => i.base10_parse(),
        _ => Err(syn::Error::new_spanned(
            e,
            "expected a whole number (an engine major)",
        )),
    }
}

fn default_value(e: &Expr) -> syn::Result<Tokens> {
    let v = quote! { ::doze_module::proto::msg::value::Value };
    Ok(match e {
        Expr::Lit(ExprLit {
            lit: Lit::Bool(b), ..
        }) => quote! { #v::Bool(#b) },
        Expr::Lit(ExprLit {
            lit: Lit::Int(i), ..
        }) => {
            let s = i.base10_digits().to_string();
            quote! { #v::Number(#s.to_string()) }
        }
        Expr::Lit(ExprLit {
            lit: Lit::Float(f), ..
        }) => {
            let s = f.base10_digits().to_string();
            quote! { #v::Number(#s.to_string()) }
        }
        Expr::Lit(ExprLit {
            lit: Lit::Str(s), ..
        }) => quote! { #v::String(#s.to_string()) },
        _ => {
            return Err(syn::Error::new_spanned(
                e,
                "a default is a bool, a number or a string literal",
            ));
        }
    })
}

fn parse_field(attrs: &[syn::Attribute]) -> syn::Result<FieldSpec> {
    let mut f = FieldSpec::default();
    for attr in attrs.iter().filter(|a| a.path().is_ident("spec")) {
        attr.parse_nested_meta(|m| {
            let key = m.path.get_ident().map(ToString::to_string).unwrap_or_default();
            match key.as_str() {
                "block" => f.block = true,
                "label" => f.label = true,
                "key" => f.key = true,
                "doc" => f.doc = m.value()?.parse::<syn::LitStr>()?.value(),
                "rename" => f.name = Some(m.value()?.parse::<syn::LitStr>()?.value()),
                "default" => f.default = Some(default_value(&m.value()?.parse::<Expr>()?)?),
                "since" => f.since = Some(int(&m.value()?.parse::<Expr>()?)?),
                "until" => f.until = Some(int(&m.value()?.parse::<Expr>()?)?),
                _ => return Err(m.error("unknown spec attribute; expected doc, default, since, until, rename, block, label or key")),
            }
            Ok(())
        })?;
    }
    Ok(f)
}

/// `T` of a `Vec<T>` (block fields).
fn vec_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(p) = ty else { return None };
    let last = p.path.segments.last()?;
    if last.ident != "Vec" {
        return None;
    }
    let PathArguments::AngleBracketed(a) = &last.arguments else {
        return None;
    };
    match a.args.first()? {
        GenericArgument::Type(t) => Some(t),
        _ => None,
    }
}

fn opt_u32(v: Option<u32>) -> Tokens {
    match v {
        Some(n) => quote! { ::core::option::Option::Some(#n) },
        None => quote! { ::core::option::Option::None },
    }
}

fn opt_string(v: Option<u32>) -> Tokens {
    match v {
        Some(n) => {
            let s = n.to_string();
            quote! { ::core::option::Option::Some(#s.to_string()) }
        }
        None => quote! { ::core::option::Option::None },
    }
}

/// What one field contributes to the generated impl.
#[derive(Default)]
struct Parts {
    attrs: Vec<Tokens>,
    blocks: Vec<Tokens>,
    labels: Vec<Tokens>,
    keys: Vec<Tokens>,
    reads: Vec<Tokens>,
    builds: Vec<Tokens>,
    names: Vec<String>,
}

fn attr_tokens(m: &Tokens, name: &str, ty: &Type, f: &FieldSpec, default: &Tokens) -> Tokens {
    let doc = &f.doc;
    let (since_s, until_s) = (opt_string(f.since), opt_string(f.until));
    quote! {
        #m::proto::msg::hello::Attr {
            name: #name.to_string(),
            ty: <#ty as #m::spec::Field>::ty(),
            required: <#ty as #m::spec::Field>::absent().is_none() && #default.is_none(),
            default: #default,
            doc: #doc.to_string(),
            since: #since_s,
            until: #until_s,
        }
    }
}

fn add_field(p: &mut Parts, m: &Tokens, field: &syn::Field) -> syn::Result<()> {
    let ident = field
        .ident
        .as_ref()
        .ok_or_else(|| syn::Error::new_spanned(field, "unnamed field"))?;
    let f = parse_field(&field.attrs)?;
    let name = f.name.clone().unwrap_or_else(|| ident.to_string());
    let ty = &field.ty;
    let var = format_ident!("__f_{}", ident);
    p.names.push(name.clone());
    p.builds.push(quote! { #ident: #var? });
    if f.block {
        let inner = vec_inner(ty)
            .ok_or_else(|| syn::Error::new_spanned(ty, "a block field is a Vec<T> with T: Spec"))?;
        let doc = &f.doc;
        p.blocks
            .push(quote! { #m::spec::block_schema::<#inner>(#name, #doc) });
        p.reads
            .push(quote! { let #var = cx.blocks::<#inner>(map, #name); });
        return Ok(());
    }
    let default = match &f.default {
        Some(d) => quote! { ::core::option::Option::Some(#d) },
        None => quote! { ::core::option::Option::<#m::proto::msg::value::Value>::None },
    };
    let (since, until) = (opt_u32(f.since), opt_u32(f.until));
    p.reads
        .push(quote! { let #var = cx.field::<#ty>(map, #name, #default, (#since, #until)); });
    if f.label {
        p.labels.push(quote! { #name.to_string() });
    } else {
        if f.key {
            p.keys.push(quote! { #name.to_string() });
        }
        p.attrs.push(attr_tokens(m, &name, ty, &f, &default));
    }
    Ok(())
}

fn expand(input: &DeriveInput) -> syn::Result<Tokens> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            input,
            "Spec is derived for structs",
        ));
    };
    let Fields::Named(named) = &data.fields else {
        return Err(syn::Error::new_spanned(input, "Spec needs named fields"));
    };
    let m = quote! { ::doze_module };
    let mut p = Parts::default();
    for field in &named.named {
        add_field(&mut p, &m, field)?;
    }
    let Parts {
        attrs,
        blocks,
        labels,
        keys,
        reads,
        builds,
        names,
    } = p;
    let ident = &input.ident;
    Ok(quote! {
        impl #m::spec::Spec for #ident {
            fn schema() -> #m::proto::msg::hello::Schema {
                #m::proto::msg::hello::Schema { attrs: vec![#(#attrs),*], blocks: vec![#(#blocks),*] }
            }
            fn labels() -> ::std::vec::Vec<::std::string::String> {
                vec![#(#labels),*]
            }
            fn identity() -> ::std::vec::Vec<::std::string::String> {
                vec![#(#labels,)* #(#keys),*]
            }
            fn decode(node: &#m::proto::msg::value::Node, cx: &mut #m::spec::Decoding) -> ::core::option::Option<Self> {
                let map = cx.map(node, "the block")?;
                cx.only(map, &[#(#names),*]);
                #(#reads)*
                ::core::option::Option::Some(Self { #(#builds),* })
            }
        }
    })
}
