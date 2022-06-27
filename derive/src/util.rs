//! Misc utils.

#![allow(dead_code)] // FIXME

use proc_macro2::Span;
use syn::{
    spanned::Spanned,
    visit_mut::{self, VisitMut},
    Attribute, FnArg, Lifetime, NestedMeta, Pat, PatType, Signature,
};

pub(crate) fn find_meta_attrs(name: &str, args: &[Attribute]) -> Option<NestedMeta> {
    args.as_ref()
        .iter()
        .filter_map(|a| a.parse_meta().ok())
        .find(|m| m.path().is_ident(name))
        .map(NestedMeta::from)
}

#[derive(Debug)]
enum LifetimeSpecifier {
    Check(bool),
    Set(Lifetime),
}

impl VisitMut for LifetimeSpecifier {
    fn visit_lifetime_mut(&mut self, lifetime: &mut Lifetime) {
        if lifetime.ident == "_" {
            match self {
                Self::Set(new_lifetime) => {
                    *lifetime = new_lifetime.clone();
                }
                Self::Check(is_used) => {
                    *is_used = true;
                }
            }
        }
    }

    fn visit_parenthesized_generic_arguments_mut(
        &mut self,
        _: &mut syn::ParenthesizedGenericArguments,
    ) {
        // Do not recurse into `impl Fn{Mut,Once}(..) -> .., since they have separate
        // lifetime elision scope.
    }

    fn visit_type_bare_fn_mut(&mut self, _: &mut syn::TypeBareFn) {
        // Do not recurse into bare functions since they have separate lifetime elision scope.
    }

    fn visit_type_reference_mut(&mut self, ty_ref: &mut syn::TypeReference) {
        if ty_ref.lifetime.is_none() {
            match self {
                Self::Set(new_lifetime) => {
                    ty_ref.lifetime = Some(new_lifetime.clone());
                }
                Self::Check(is_used) => {
                    *is_used = true;
                }
            }
        }
        // Recurse to visit embedded lifetimes (e.g., in `&mut Cow<'_, [u8]>`).
        visit_mut::visit_type_reference_mut(self, ty_ref);
    }
}

#[derive(Debug, Default)]
struct LifetimeScanner {
    lifetime: Option<Lifetime>,
    duplicate_span: Option<Span>,
}

impl LifetimeScanner {
    fn insert_lifetime(&mut self, lifetime: &Lifetime) {
        if self.duplicate_span.is_some() {
            return; // Already found a duplicate.
        }

        if let Some(existing_lifetime) = &self.lifetime {
            if lifetime != existing_lifetime {
                self.duplicate_span = Some(lifetime.span());
            }
        } else {
            self.lifetime = Some(lifetime.clone());
        }
    }

    fn create_lifetime(&mut self, from: &Lifetime) -> Option<Lifetime> {
        if self.duplicate_span.is_some() {
            return None;
        } else if self.lifetime.is_some() {
            self.duplicate_span = Some(from.span());
            return None;
        }

        let new_lifetime = Lifetime::new("'__elided", from.span());
        self.lifetime = Some(new_lifetime.clone());
        Some(new_lifetime)
    }
}

impl VisitMut for LifetimeScanner {
    fn visit_lifetime_mut(&mut self, lifetime: &mut Lifetime) {
        if lifetime.ident == "_" {
            if let Some(new_lifetime) = self.create_lifetime(lifetime) {
                *lifetime = new_lifetime;
            }
        } else if lifetime.ident != "static" {
            self.insert_lifetime(lifetime);
        }
    }

    fn visit_parenthesized_generic_arguments_mut(
        &mut self,
        _: &mut syn::ParenthesizedGenericArguments,
    ) {
        // Do not recurse into `impl Fn{Mut,Once}(..) -> .., since they have separate
        // lifetime elision scope.
    }

    fn visit_receiver_mut(&mut self, recv: &mut syn::Receiver) {
        let span = recv.span();
        if let Some((_, maybe_lifetime)) = &mut recv.reference {
            if maybe_lifetime.is_none() {
                *maybe_lifetime = Some(Lifetime::new("'_", span));
            }
        }
        visit_mut::visit_receiver_mut(self, recv);
    }

    fn visit_type_bare_fn_mut(&mut self, _: &mut syn::TypeBareFn) {
        // Do not recurse into bare functions since they have separate lifetime elision scope.
    }

    fn visit_type_reference_mut(&mut self, ty_ref: &mut syn::TypeReference) {
        if ty_ref.lifetime.is_none() {
            ty_ref.lifetime = Some(Lifetime::new("'_", ty_ref.span()));
            // ^ This lifetime will be processed later in `visit_lifetime_mut`
        }
        visit_mut::visit_type_reference_mut(self, ty_ref);
    }
}

pub(crate) fn receiver_span(arg: &FnArg) -> Option<Span> {
    match arg {
        FnArg::Receiver(receiver) => Some(receiver.self_token.span()),
        FnArg::Typed(PatType { pat, .. }) => {
            if let Pat::Ident(pat_ident) = pat.as_ref() {
                if pat_ident.ident == "self" {
                    return Some(pat_ident.ident.span());
                }
            }
            None
        }
    }
}

fn get_elided_lifetime<'a>(
    args: impl IntoIterator<Item = &'a mut FnArg>,
) -> darling::Result<Option<Lifetime>> {
    let mut scanner = LifetimeScanner::default();
    for arg in args {
        scanner.visit_fn_arg_mut(arg);
        if let Some(span) = &scanner.duplicate_span {
            let message = "Cannot elide lifetimes: there are multiple possible sources";
            return Err(darling::Error::custom(message).with_span(span));
        }
    }
    Ok(scanner.lifetime)
}

pub(crate) fn specify_lifetime(sig: &mut Signature) -> darling::Result<()> {
    let mut specifier = LifetimeSpecifier::Check(false);
    specifier.visit_return_type_mut(&mut sig.output);
    if matches!(specifier, LifetimeSpecifier::Check(true)) {
        // The output type of the function contains an elided lifetime; need to find
        // where to borrow it from.

        // First, check the first argument, which is the only one that can be the receiver.
        let first_arg = if let Some(arg) = sig.inputs.first_mut() {
            arg
        } else {
            let message = "Nowhere to elide the lifetime from";
            return Err(darling::Error::custom(message).with_span(sig));
        };

        let lifetime = if receiver_span(first_arg).is_some() {
            get_elided_lifetime([first_arg])?
        } else {
            get_elided_lifetime(&mut sig.inputs)?
        };
        let lifetime = lifetime.ok_or_else(|| {
            let message = "Nowhere to elide the lifetime from";
            darling::Error::custom(message).with_span(sig)
        })?;

        if lifetime.ident == "__elided" {
            if sig.generics.lt_token.is_none() {
                sig.generics.lt_token = Some(syn::parse_quote!(<));
            }
            if sig.generics.gt_token.is_none() {
                sig.generics.gt_token = Some(syn::parse_quote!(>));
            }
            sig.generics.params.insert(0, syn::parse_quote!(#lifetime));
        }

        let mut specifier = LifetimeSpecifier::Set(lifetime);
        specifier.visit_return_type_mut(&mut sig.output);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use syn::ReturnType;

    use super::*;

    #[test]
    fn specifying_lifetimes() {
        let mut specifier = LifetimeSpecifier::Set(syn::parse_quote!('elided));
        let mut return_type: ReturnType = syn::parse_quote!(-> &str);
        specifier.visit_return_type_mut(&mut return_type);
        let expected: ReturnType = syn::parse_quote!(-> &'elided str);
        assert_eq!(return_type, expected);

        let mut return_type: ReturnType = syn::parse_quote! {
            -> Option<(&str, usize, &mut Cow<'_, str>)>
        };
        specifier.visit_return_type_mut(&mut return_type);
        let expected: ReturnType = syn::parse_quote! {
            -> Option<(&'elided str, usize, &'elided mut Cow<'elided, str>)>
        };
        assert_eq!(return_type, expected);

        let mut return_type: ReturnType = syn::parse_quote!(-> Result<&str, &'static str>);
        specifier.visit_return_type_mut(&mut return_type);
        let expected: ReturnType = syn::parse_quote! {
            -> Result<&'elided str, &'static str>
        };
        assert_eq!(return_type, expected);

        let mut return_type: ReturnType = syn::parse_quote! {
            -> impl Iterator<Item = &str> + '_
        };
        specifier.visit_return_type_mut(&mut return_type);
        let expected: ReturnType = syn::parse_quote! {
            -> impl Iterator<Item = &'elided str> + 'elided
        };
        assert_eq!(return_type, expected);
    }

    #[test]
    fn specifying_lifetimes_with_fn_types() {
        let mut specifier = LifetimeSpecifier::Set(syn::parse_quote!('elided));
        let mut return_type: ReturnType = syn::parse_quote! {
            -> (&str, fn(&str) -> &[u8], Cow<'_, str>)
        };
        specifier.visit_return_type_mut(&mut return_type);
        let expected: ReturnType = syn::parse_quote! {
            -> (&'elided str, fn(&str) -> &[u8], Cow<'elided, str>)
        };
        assert_eq!(return_type, expected);

        let mut return_type: ReturnType = syn::parse_quote! {
            -> (impl FnMut(&str) -> &str, &[u8])
        };
        specifier.visit_return_type_mut(&mut return_type);
        let expected: ReturnType = syn::parse_quote! {
            -> (impl FnMut(&str) -> &str, &'elided [u8])
        };
        assert_eq!(return_type, expected);
    }

    #[test]
    fn getting_elided_lifetime_for_receiver() {
        let mut arg: FnArg = syn::parse_quote!(&mut self);
        let lifetime = get_elided_lifetime([&mut arg]).unwrap();
        assert_eq!(lifetime.unwrap().ident, "__elided");
        assert_eq!(arg, syn::parse_quote!(&'__elided mut self));

        let mut arg: FnArg = syn::parse_quote!(self: Pin<&mut Self>);
        let lifetime = get_elided_lifetime([&mut arg]).unwrap();
        assert_eq!(lifetime.unwrap().ident, "__elided");
        assert_eq!(arg, syn::parse_quote!(self: Pin<&'__elided mut Self>));

        let mut arg: FnArg = syn::parse_quote!(&'s self);
        let lifetime = get_elided_lifetime([&mut arg]).unwrap();
        assert_eq!(lifetime.unwrap().ident, "s");
        assert_eq!(arg, syn::parse_quote!(&'s self));
    }

    #[test]
    fn getting_elided_lifetime_for_multiple_args() {
        let mut sig: Signature = syn::parse_quote!(fn test(_: u8, s: &str));
        let lifetime = get_elided_lifetime(&mut sig.inputs).unwrap();
        assert_eq!(lifetime.unwrap().ident, "__elided");
        assert_eq!(sig, syn::parse_quote!(fn test(_: u8, s: &'__elided str)));

        let mut sig: Signature = syn::parse_quote!(fn test<'a>(_: u8, s: &'a str));
        let lifetime = get_elided_lifetime(&mut sig.inputs).unwrap();
        assert_eq!(lifetime.unwrap().ident, "a");
        assert_eq!(sig, syn::parse_quote!(fn test<'a>(_: u8, s: &'a str)));

        let mut sig: Signature = syn::parse_quote!(fn test<'a>(_: u8, s: &'a Cow<'a, str>));
        let lifetime = get_elided_lifetime(&mut sig.inputs).unwrap();
        assert_eq!(lifetime.unwrap().ident, "a");
        assert_eq!(
            sig,
            syn::parse_quote!(fn test<'a>(_: u8, s: &'a Cow<'a, str>))
        );
    }

    #[test]
    fn elided_lifetime_with_static_lifetime() {
        let mut sig: Signature = syn::parse_quote!(fn test(_: u8, s: &'static str));
        let lifetime = get_elided_lifetime(&mut sig.inputs).unwrap();
        assert!(lifetime.is_none());

        let mut sig: Signature = syn::parse_quote!(fn test(&'static self));
        let lifetime = get_elided_lifetime(&mut sig.inputs).unwrap();
        assert!(lifetime.is_none());

        let mut sig: Signature = syn::parse_quote! {
            fn test(s: &'static str, cow: Cow<'_, [u8]>)
        };
        let lifetime = get_elided_lifetime(&mut sig.inputs).unwrap();
        assert_eq!(lifetime.unwrap().ident, "__elided");
        let expected: Signature = syn::parse_quote! {
            fn test(s: &'static str, cow: Cow<'__elided, [u8]>)
        };
        assert_eq!(sig, expected);

        let mut sig: Signature = syn::parse_quote! {
            fn test(s: &'a str, cow: Cow<'static, [u8]>)
        };
        let expected = sig.clone();
        let lifetime = get_elided_lifetime(&mut sig.inputs).unwrap();
        assert_eq!(lifetime.unwrap().ident, "a");
        assert_eq!(sig, expected);
    }

    #[test]
    fn getting_elided_lifetime_errors() {
        let mut sig: Signature = syn::parse_quote!(fn test(x: &str, y: &str));
        let err = get_elided_lifetime(&mut sig.inputs)
            .unwrap_err()
            .to_string();
        assert!(err.contains("multiple possible sources"), "{}", err);

        let mut sig: Signature = syn::parse_quote!(fn test(cow: &mut Cow<'_, str>));
        let err = get_elided_lifetime(&mut sig.inputs)
            .unwrap_err()
            .to_string();
        assert!(err.contains("multiple possible sources"), "{}", err);
    }

    #[test]
    fn specifying_lifetime_with_receiver() {
        let mut sig: Signature = syn::parse_quote! {
            fn test(&self, y: &str) -> &[u8]
        };
        specify_lifetime(&mut sig).unwrap();
        let expected: Signature = syn::parse_quote! {
            fn test<'__elided>(&'__elided self, y: &str) -> &'__elided [u8]
        };
        assert_eq!(sig, expected, "{}", quote::quote!(#sig));

        let mut sig: Signature = syn::parse_quote! {
            fn test(self: Pin<&mut Self>, y: &str) -> &[u8]
        };
        specify_lifetime(&mut sig).unwrap();
        let expected: Signature = syn::parse_quote! {
            fn test<'__elided>(self: Pin<&'__elided mut Self>, y: &str) -> &'__elided [u8]
        };
        assert_eq!(sig, expected);
    }

    #[test]
    fn specifying_lifetime_without_receiver() {
        let mut sig: Signature = syn::parse_quote! {
            fn test(x: usize, y: &str) -> &[u8]
        };
        specify_lifetime(&mut sig).unwrap();
        let expected: Signature = syn::parse_quote! {
            fn test<'__elided>(x: usize, y: &'__elided str) -> &'__elided [u8]
        };
        assert_eq!(sig, expected);

        let mut sig: Signature = syn::parse_quote! {
            fn test<'a>(cow: &'a Cow<'a, str>) -> (&[u8], impl Iterator<Item = &u8>)
        };
        specify_lifetime(&mut sig).unwrap();
        let expected: Signature = syn::parse_quote! {
            fn test<'a>(cow: &'a Cow<'a, str>) -> (&'a [u8], impl Iterator<Item = &'a u8>)
        };
        assert_eq!(sig, expected);
    }
}
