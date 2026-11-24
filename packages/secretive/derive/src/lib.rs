//! `#[derive(Duplicate)]`, for a type that holds a secret: its
//! `duplicate()` copies each field, a field marked `#[duplicate]` (one
//! holding a secret) by its own `duplicate()`, every other by `Clone`. A
//! type holding a secret has no `Clone`, so a copy of it is always said.
//! The trait it implements, `Duplicate`, must be in scope where it's
//! derived (`use link_secretive::Duplicate;` brings both).

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, parse_macro_input};

#[proc_macro_derive(Duplicate, attributes(duplicate))]
pub fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let body = match &input.data {
        Data::Struct(data) => {
            let (pattern, built) = fields(&data.fields);
            quote! { let #name #pattern = self; #name #built }
        }
        Data::Enum(data) => {
            let arms = data.variants.iter().map(|variant| {
                let v = &variant.ident;
                let (pattern, built) = fields(&variant.fields);
                quote! { #name::#v #pattern => #name::#v #built }
            });
            quote! { match self { #(#arms,)* } }
        }
        Data::Union(_) => {
            return syn::Error::new_spanned(name, "Duplicate is for structs and enums")
                .to_compile_error()
                .into();
        }
    };
    quote! {
        impl #impl_generics Duplicate for #name #ty_generics #where_clause {
            fn duplicate(&self) -> Self {
                #body
            }
        }
    }
    .into()
}

/// The pattern binding each field by reference, and the value built of
/// their copies.
fn fields(fields: &Fields) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let copy = |field: &syn::Field, bound: &syn::Ident| {
        if field.attrs.iter().any(|a| a.path().is_ident("duplicate")) {
            quote! { Duplicate::duplicate(#bound) }
        } else {
            quote! { ::core::clone::Clone::clone(#bound) }
        }
    };
    match fields {
        Fields::Named(named) => {
            let names: Vec<_> = named
                .named
                .iter()
                .map(|f| f.ident.clone().unwrap())
                .collect();
            let copies = named.named.iter().zip(&names).map(|(f, n)| {
                let c = copy(f, n);
                quote! { #n: #c }
            });
            (quote! { { #(#names,)* } }, quote! { { #(#copies,)* } })
        }
        Fields::Unnamed(unnamed) => {
            let names: Vec<_> = (0..unnamed.unnamed.len())
                .map(|i| format_ident!("f{i}"))
                .collect();
            let copies = unnamed.unnamed.iter().zip(&names).map(|(f, n)| copy(f, n));
            (quote! { ( #(#names,)* ) }, quote! { ( #(#copies,)* ) })
        }
        Fields::Unit => (quote! {}, quote! {}),
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
