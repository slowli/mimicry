//! `CallReal` trait derivation.

use darling::{FromDeriveInput, FromMeta};
use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::{Data, DataStruct, DeriveInput, Field, Fields, Generics, Ident, Index, Type, TypePath};

use crate::utils::find_meta_attrs;

#[derive(Debug)]
enum FieldIdent {
    Named(Ident),
    Unnamed(Index),
}

impl FieldIdent {
    fn new(idx: usize, field: &Field) -> Self {
        field
            .ident
            .clone()
            .map_or_else(|| Self::Unnamed(idx.into()), Self::Named)
    }
}

impl ToTokens for FieldIdent {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        match self {
            Self::Named(id) => id.to_tokens(tokens),
            Self::Unnamed(idx) => idx.to_tokens(tokens),
        }
    }
}

#[derive(Debug, Default, FromMeta)]
struct FieldAttrs {
    #[darling(default)]
    switch: Option<()>,
}

#[derive(Debug)]
struct CallReal {
    generics: Generics,
    ident: Ident,
    switch_field: FieldIdent,
}

impl CallReal {
    fn detect_switch_field(fields: &Fields) -> darling::Result<FieldIdent> {
        let tagged_fields = fields.iter().enumerate().filter_map(|(i, field)| {
            let attr = find_meta_attrs("mock", None, &field.attrs);
            let attr = attr
                .as_ref()
                .and_then(|meta| FieldAttrs::from_nested_meta(meta).ok())
                .unwrap_or_default();
            attr.switch.map(|()| (i, field))
        });
        let tagged_fields: Vec<_> = tagged_fields.take(2).collect();
        match tagged_fields.as_slice() {
            [] => { /* No explicitly tagged fields; continue. */ }
            [(idx, field)] => return Ok(FieldIdent::new(*idx, field)),
            [_, (_, field), ..] => {
                let message = "Multiple `#[mock(switch)]` attrs; there should be no more than one";
                return Err(darling::Error::custom(message).with_span(field));
            }
        }

        let implicit_fields = fields.iter().enumerate().filter_map(|(i, field)| {
            if Self::is_switch(&field.ty) {
                Some((i, field))
            } else {
                None
            }
        });
        let implicit_fields: Vec<_> = implicit_fields.take(2).collect();
        match implicit_fields.as_slice() {
            [] => {
                let message = "No fields of `RealCallSwitch` type. Please add such a field, \
                    or, if it's present, mark it with `#[mock(switch)]` attr";
                Err(darling::Error::custom(message).with_span(fields))
            }
            [(idx, field)] => Ok(FieldIdent::new(*idx, field)),
            [_, (_, field), ..] => {
                let message = "Multiple fields with `RealCallSwitch` type. \
                    Mark the expected one with `#[mock(switch)]` attr";
                Err(darling::Error::custom(message).with_span(field))
            }
        }
    }

    fn is_switch(ty: &Type) -> bool {
        if let Type::Path(TypePath { path, .. }) = ty {
            path.segments
                .last()
                .map_or(false, |segment| segment.ident == "RealCallSwitch")
        } else {
            false
        }
    }

    fn impl_call_real(&self) -> impl ToTokens {
        let ident = &self.ident;
        let field = &self.switch_field;
        let (impl_generics, ty_generics, where_clause) = self.generics.split_for_impl();

        quote! {
            impl #impl_generics mimicry::CallReal for #ident #ty_generics #where_clause {
                fn access_switch<R>(&self, action: impl FnOnce(&RealCallSwitch) -> R) -> R {
                    action(&self.#field)
                }
            }
        }
    }
}

impl FromDeriveInput for CallReal {
    fn from_derive_input(input: &DeriveInput) -> darling::Result<Self> {
        let fields = if let Data::Struct(DataStruct { fields, .. }) = &input.data {
            fields
        } else {
            let message = "can only derive `CallReal` for structs";
            return Err(darling::Error::custom(message).with_span(input));
        };

        let switch_field = Self::detect_switch_field(fields)?;
        Ok(Self {
            generics: input.generics.clone(),
            ident: input.ident.clone(),
            switch_field,
        })
    }
}

impl ToTokens for CallReal {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let call_real_impl = self.impl_call_real();
        tokens.extend(quote!(#call_real_impl));
    }
}

pub(crate) fn impl_call_real(input: TokenStream) -> TokenStream {
    let input: DeriveInput = syn::parse(input).unwrap();
    let trait_impl = match CallReal::from_derive_input(&input) {
        Ok(trait_impl) => trait_impl,
        Err(err) => return err.write_errors().into(),
    };
    let tokens = quote!(#trait_impl);
    tokens.into()
}
