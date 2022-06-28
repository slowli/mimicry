//! Derivation of `Mock` trait.

use darling::{FromDeriveInput, FromMeta};
use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::{parse_quote, spanned::Spanned, DeriveInput, GenericParam, Generics, Ident};

use crate::utils::find_meta_attrs;

#[derive(Debug, Default, FromMeta)]
struct MockAttrs {
    #[darling(default)]
    shared: bool,
    #[darling(rename = "mut", default)]
    mutable: bool,
}

#[derive(Debug)]
struct Mock {
    generics: Generics,
    ident: Ident,
    shared: bool,
    mutable: bool,
}

impl Mock {
    fn impl_mock(&self) -> impl ToTokens {
        let ident = &self.ident;
        let base = if self.mutable {
            quote!(mimicry::Mut<Self>)
        } else {
            quote!(Self)
        };
        let wrapper = if self.shared {
            quote!(mimicry::Shared)
        } else {
            quote!(mimicry::ThreadLocal)
        };

        let (impl_generics, ty_generics, where_clause) = self.generics.split_for_impl();
        let mut where_clause = where_clause.cloned().unwrap_or_else(|| parse_quote!(where));
        where_clause
            .predicates
            .push(parse_quote!(#wrapper<#base>: Send + Sync));

        // `static` requires an exact type.
        let shared_ty = if self.mutable {
            quote!(#wrapper<mimicry::Mut<#ident #ty_generics>>)
        } else {
            quote!(#wrapper<#ident #ty_generics>)
        };

        quote! {
            impl #impl_generics mimicry::Mock for #ident #ty_generics #where_clause {
                type Base = #base;
                type Shared = #wrapper<Self::Base>;

                fn instance() -> &'static mimicry::Static<Self::Shared> {
                    static SHARED: mimicry::Static<#shared_ty> = mimicry::Static::new();
                    &SHARED
                }
            }
        }
    }
}

impl FromDeriveInput for Mock {
    fn from_derive_input(input: &DeriveInput) -> darling::Result<Self> {
        let attrs = find_meta_attrs("mock", None, &input.attrs).map_or_else(
            || Ok(MockAttrs::default()),
            |meta| MockAttrs::from_nested_meta(&meta),
        )?;

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
            mutable: attrs.mutable,
        })
    }
}

impl ToTokens for Mock {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let mock_impl = self.impl_mock();
        tokens.extend(quote!(#mock_impl));
    }
}

pub(crate) fn impl_mock(input: TokenStream) -> TokenStream {
    let input: DeriveInput = syn::parse(input).unwrap();
    let trait_impl = match Mock::from_derive_input(&input) {
        Ok(trait_impl) => trait_impl,
        Err(err) => return err.write_errors().into(),
    };
    let tokens = quote!(#trait_impl);
    tokens.into()
}
