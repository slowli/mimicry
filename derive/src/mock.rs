//! Derivation of `Mock` trait.

use darling::{FromDeriveInput, FromMeta};
use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::{parse_quote, spanned::Spanned, DeriveInput, GenericParam, Generics, Ident};

use crate::utils::find_meta_attrs;

#[derive(Debug, Default, FromMeta)]
struct MockAttrs {
    shared: bool,
}

#[derive(Debug)]
struct Mock {
    generics: Generics,
    ident: Ident,
    shared: bool,
}

impl Mock {
    fn impl_mock(&self) -> impl ToTokens {
        let ident = &self.ident;
        let wrapper = if self.shared {
            quote!(mock::Shared)
        } else {
            quote!(mock::ThreadLocal)
        };

        let (impl_generics, ty_generics, where_clause) = self.generics.split_for_impl();
        let shared_ty = quote!(#wrapper<#ident #ty_generics>);
        let mut where_clause = where_clause.cloned().unwrap_or_else(|| parse_quote!(where));
        where_clause
            .predicates
            .push(parse_quote!(#wrapper<Self>: Send + Sync));

        quote! {
            impl #impl_generics mock::Mock for #ident #ty_generics #where_clause {
                type Shared = #wrapper<Self>;

                fn instance() -> &'static mock::Static<Self::Shared> {
                    static SHARED: mock::Static<#shared_ty> = mock::Static::new();
                    &SHARED
                }
            }
        }
    }
}

impl FromDeriveInput for Mock {
    fn from_derive_input(input: &DeriveInput) -> darling::Result<Self> {
        let attrs = find_meta_attrs("mock", &input.attrs)
            .map(|meta| MockAttrs::from_nested_meta(&meta))
            .unwrap_or_else(|| Ok(MockAttrs::default()))?;

        let mut params = input.generics.params.iter();
        let lifetime_span = params.find_map(|param| {
            if matches!(param, GenericParam::Lifetime(_)) {
                Some(param.span())
            } else {
                None
            }
        });
        if let Some(span) = lifetime_span {
            let message = "Mock states with lifetimes are not supported";
            return Err(darling::Error::custom(message).with_span(&span));
        }

        Ok(Self {
            generics: input.generics.clone(),
            ident: input.ident.clone(),
            shared: attrs.shared,
        })
    }
}

impl ToTokens for Mock {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let mock_impl = self.impl_mock();
        tokens.extend(quote! {
            #mock_impl
        })
    }
}

pub(crate) fn impl_mock(input: TokenStream) -> TokenStream {
    let input: DeriveInput = syn::parse(input).unwrap();
    let interface = match Mock::from_derive_input(&input) {
        Ok(interface) => interface,
        Err(err) => return err.write_errors().into(),
    };
    let tokens = quote!(#interface);
    tokens.into()
}
