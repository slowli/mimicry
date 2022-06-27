//! Mocked function attribute.

use darling::FromMeta;
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{quote, quote_spanned, ToTokens};
use syn::{
    parse::Parser, punctuated::Punctuated, spanned::Spanned, token::Comma, FnArg, Ident, Item,
    ItemFn, ItemImpl, NestedMeta, Pat, PatIdent, Path, Signature,
};

use std::mem;

use crate::utils::{find_meta_attrs, receiver_span};

#[derive(Debug, FromMeta)]
struct FunctionAttrs {
    using: Path,
    rename: Option<String>,
}

impl FunctionAttrs {
    fn parse(attr: TokenStream) -> darling::Result<Self> {
        let meta = Punctuated::<NestedMeta, Comma>::parse_terminated.parse(attr)?;
        let meta: Vec<_> = meta.into_iter().collect();
        Self::from_list(&meta)
    }

    fn rename(spec: &str, ident: &Ident) -> Ident {
        let ident_string = ident.to_string();
        let ident_string = spec.replace("{}", &ident_string);
        Ident::new(&ident_string, ident.span())
    }
}

#[derive(Debug)]
pub struct FunctionWrapper {
    state: Path,
    mock_fn: Ident,
    function: ItemFn,
    receiver: Option<Span>,
    arg_patterns: Vec<Pat>,
    args: Vec<Ident>,
}

impl FunctionWrapper {
    fn can_process(signature: &Signature) -> darling::Result<()> {
        if let Some(const_token) = &signature.constness {
            let message = "const functions are not supported";
            return Err(darling::Error::custom(message).with_span(const_token));
        }
        Ok(())
    }

    // TODO: support async fns if feasible
    fn new(attrs: FunctionAttrs, mut function: ItemFn) -> darling::Result<Self> {
        Self::can_process(&function.sig)?;

        let mut state = attrs.using;
        let mock_fn = Self::split_off_function(&mut state).unwrap_or_else(|| {
            if let Some(spec) = &attrs.rename {
                FunctionAttrs::rename(spec, &function.sig.ident)
            } else {
                function.sig.ident.clone()
            }
        });
        let receiver = function.sig.inputs.first().and_then(receiver_span);
        let (arg_patterns, args) = Self::take_arg_patterns(receiver.is_some(), &mut function.sig);

        Ok(Self {
            state,
            mock_fn,
            function,
            receiver,
            arg_patterns,
            args,
        })
    }

    fn split_off_function(path: &mut Path) -> Option<Ident> {
        let last_segment = path.segments.last()?.ident.to_string();
        if last_segment.starts_with(|ch: char| ch.is_ascii_uppercase()) {
            // Last segment looks like type ident
            None
        } else {
            let last_segment = path.segments.pop().unwrap().into_value();
            // Remove trailing `::`.
            if let Some(pair) = path.segments.pop() {
                path.segments.push_value(pair.into_value());
            }
            Some(last_segment.ident)
        }
    }

    fn take_arg_patterns(skip_receiver: bool, sig: &mut Signature) -> (Vec<Pat>, Vec<Ident>) {
        let iter = sig
            .inputs
            .iter_mut()
            .enumerate()
            .skip(usize::from(skip_receiver));
        let iter = iter.map(|(i, arg)| {
            let span = arg.span();
            if let FnArg::Typed(pat_type) = arg {
                let ident = Ident::new(&format!("__arg{}", i), span);
                let simple_pat = Box::new(Pat::Ident(PatIdent {
                    attrs: vec![],
                    by_ref: None,
                    mutability: None,
                    ident: ident.clone(),
                    subpat: None,
                }));
                let original_pat = *mem::replace(&mut pat_type.pat, simple_pat);
                (original_pat, ident)
            } else {
                unreachable!() // filtered out previously
            }
        });
        iter.unzip()
    }

    fn wrap(&self, logic: impl ToTokens) -> impl ToTokens {
        let attrs = &self.function.attrs;
        let vis = &self.function.vis;
        let statements = &self.function.block.stmts;
        let signature = &self.function.sig;
        let arg_patterns = &self.arg_patterns;
        let args = &self.args;

        quote! {
            #(#attrs)*
            #vis #signature {
                #logic
                let (#(#arg_patterns,)*) = (#(#args,)*);
                #(#statements)*
            }
        }
    }

    fn fallback_logic(&self) -> impl ToTokens {
        let recv = self
            .receiver
            .as_ref()
            .map(|receiver| quote_spanned!(*receiver=> self,));
        let args = &self.args;
        let state = &self.state;
        let mock_fn = &self.mock_fn;

        quote! {
            std::thread_local! {
                static __FALLBACK: mimicry::FallbackSwitch = mimicry::FallbackSwitch::default();
            }

            if !__FALLBACK.with(mimicry::FallbackSwitch::is_active) {
                let instance = <#state as mimicry::Mock>::instance();
                if let Some(mock_ref) = mimicry::GetMock::get(instance) {
                    return __FALLBACK.with(|fallback| {
                        mimicry::CallMock::call_mock(
                            mock_ref,
                            fallback,
                            |cx| #state::#mock_fn(cx, #recv #(#args,)*),
                        )
                    });
                }
            }
        }
    }
}

impl ToTokens for FunctionWrapper {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let wrapper = self.wrap(self.fallback_logic());
        tokens.extend(quote!(#wrapper));
    }
}

#[derive(Debug)]
struct ImplWrapper {
    block: ItemImpl,
}

impl ImplWrapper {
    fn new(mut attrs: FunctionAttrs, mut block: ItemImpl) -> darling::Result<Self> {
        let maybe_fn = FunctionWrapper::split_off_function(&mut attrs.using);
        if maybe_fn.is_some() {
            let message = "function specification is not supported for impl blocks; \
                 use the `rename` attr instead, such as \
                 `#[mock(using = \"Mock\", rename = \"mock_{}\")]";
            return Err(darling::Error::custom(message).with_span(&attrs.using));
        }

        let path = &attrs.using;
        let path_string = quote!(#path).to_string();
        let rename = attrs.rename.as_deref();
        for item in &mut block.items {
            if let syn::ImplItem::Method(method) = item {
                if FunctionWrapper::can_process(&method.sig).is_ok()
                    && find_meta_attrs("mock", Some("mimicry"), &method.attrs).is_none()
                {
                    Self::add_attr(method, &path_string, rename);
                }
            }
        }
        Ok(Self { block })
    }

    fn add_attr(method: &mut syn::ImplItemMethod, path_str: &str, rename: Option<&str>) {
        let rename = rename.map(|spec| quote!(, rename = #spec));
        method.attrs.push(syn::parse_quote! {
            #[mimicry::mock(using = #path_str #rename)]
        });
    }
}

impl ToTokens for ImplWrapper {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let block = &self.block;
        tokens.extend(quote!(#block));
    }
}

pub(crate) fn wrap(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attrs = match FunctionAttrs::parse(attr) {
        Ok(attrs) => attrs,
        Err(err) => return err.write_errors().into(),
    };
    let tokens = match syn::parse(item) {
        Ok(Item::Fn(function)) => {
            FunctionWrapper::new(attrs, function).map(|wrapper| quote!(#wrapper))
        }
        Ok(Item::Impl(impl_block)) => {
            ImplWrapper::new(attrs, impl_block).map(|wrapper| quote!(#wrapper))
        }
        Ok(item) => {
            let message = "Item is not supported; use `#[mock] on functions";
            Err(darling::Error::custom(message).with_span(&item))
        }
        Err(err) => return err.into_compile_error().into(),
    };

    match tokens {
        Ok(tokens) => tokens.into(),
        Err(err) => err.write_errors().into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitting_off_function() {
        let mut path: Path = syn::parse_quote!(TestMock);
        let function = FunctionWrapper::split_off_function(&mut path);
        assert!(function.is_none());
        assert_eq!(path, syn::parse_quote!(TestMock));

        let mut path: Path = syn::parse_quote!(super::TestMock);
        let function = FunctionWrapper::split_off_function(&mut path);
        assert!(function.is_none());
        assert_eq!(path, syn::parse_quote!(super::TestMock));

        let mut path: Path = syn::parse_quote!(super::TestMock::mock_test);
        let function = FunctionWrapper::split_off_function(&mut path);
        assert_eq!(function.unwrap(), "mock_test");
        assert_eq!(path, syn::parse_quote!(super::TestMock));
    }

    #[test]
    fn transforming_args() {
        let mut signature: Signature = syn::parse_quote! {
            fn test(
                mut this: Vec<u8>,
                &reference: &u8,
                [.., tail]: &[u8],
                Point { x, .. }: &Point,
            ) -> &str
        };
        let (arg_patterns, args) = FunctionWrapper::take_arg_patterns(false, &mut signature);

        assert_eq!(
            args.iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["__arg0", "__arg1", "__arg2", "__arg3"]
        );
        let expected_sig: Signature = syn::parse_quote! {
            fn test(__arg0: Vec<u8>, __arg1: &u8, __arg2: &[u8], __arg3: &Point,) -> &str
        };
        assert_eq!(signature, expected_sig);

        let arg_patterns: Pat = syn::parse_quote!((#(#arg_patterns,)*));
        let expected_patterns: Pat = syn::parse_quote! {
            (mut this, &reference, [.., tail], Point { x, .. },)
        };
        assert_eq!(arg_patterns, expected_patterns);
    }

    #[test]
    fn simple_wrapper() {
        let attrs = FunctionAttrs {
            using: syn::parse_quote!(TestMock),
            rename: None,
        };
        let function: ItemFn = syn::parse_quote! {
            fn test(
                mut this: Vec<u8>,
                [.., tail]: &[u8],
                Point { x, .. }: &mut Point,
            ) -> &str {
                this + tail;
                x.to_string()
            }
        };
        let wrapper = FunctionWrapper::new(attrs, function).unwrap();
        let wrapper = wrapper.wrap(quote!());
        let wrapper: ItemFn = syn::parse_quote!(#wrapper);

        let expected: ItemFn = syn::parse_quote! {
            fn test(__arg0: Vec<u8>, __arg1: &[u8], __arg2: &mut Point,) -> &str {
                let (mut this, [.., tail], Point { x, .. },) = (__arg0, __arg1, __arg2,);
                this + tail;
                x.to_string()
            }
        };
        assert_eq!(wrapper, expected, "{}", quote!(#wrapper));
    }

    #[test]
    fn error_on_const_fn() {
        let attrs = FunctionAttrs {
            using: syn::parse_quote!(TestMock),
            rename: None,
        };
        let function: ItemFn = syn::parse_quote! {
            const fn test(x: u8, y: u8) -> u8 { x + y }
        };

        let err = FunctionWrapper::new(attrs, function)
            .unwrap_err()
            .to_string();
        assert!(err.contains("const functions"), "{}", err);
    }

    #[test]
    fn defining_fallback_flag() {
        let attrs = FunctionAttrs {
            using: syn::parse_quote!(TestMock),
            rename: None,
        };
        let function: ItemFn = syn::parse_quote! {
            fn test(x: u8, y: u8) -> u16 { x + y }
        };
        let wrapper = FunctionWrapper::new(attrs, function).unwrap();
        let fallback_logic = wrapper.fallback_logic();
        let fallback_flag: syn::Block = syn::parse_quote!({ #fallback_logic });

        #[rustfmt::skip] // formatting removes the necessary trailing comma
        let expected: syn::Block = syn::parse_quote!({
            std::thread_local! {
                static __FALLBACK: mimicry::FallbackSwitch = mimicry::FallbackSwitch::default();
            }

            if !__FALLBACK.with(mimicry::FallbackSwitch::is_active) {
                let instance = <TestMock as mimicry::Mock>::instance();
                if let Some(mock_ref) = mimicry::GetMock::get(instance) {
                    return __FALLBACK.with(|fallback| {
                        mimicry::CallMock::call_mock(
                            mock_ref,
                            fallback,
                            |cx| TestMock::test(cx, __arg0, __arg1,),
                        )
                    });
                }
            }
        });
        assert_eq!(fallback_flag, expected, "{}", quote!(#fallback_flag));
    }

    #[test]
    fn wrapping_impl_block() {
        let attrs = FunctionAttrs {
            using: syn::parse_quote!(TestMock),
            rename: None,
        };
        let block: ItemImpl = syn::parse_quote! {
            impl Test {
                const CONST: usize = 0;

                fn test(&self) -> usize { Self::CONST }

                #[mock(using = "OtherMock")]
                fn other() -> String { String::new() }
            }
        };

        let wrapper = ImplWrapper::new(attrs, block).unwrap();
        let expected: ItemImpl = syn::parse_quote! {
            impl Test {
                const CONST: usize = 0;

                #[mimicry::mock(using = "TestMock")]
                fn test(&self) -> usize { Self::CONST }

                #[mock(using = "OtherMock")]
                fn other() -> String { String::new() }
            }
        };
        assert_eq!(wrapper.block, expected, "{}", quote!(#wrapper));
    }

    #[test]
    fn wrapping_impl_block_errors() {
        let attrs = FunctionAttrs {
            using: syn::parse_quote!(TestMock::test),
            rename: None,
        };
        let block: ItemImpl = syn::parse_quote! {
            impl Test {
                fn test(&self) -> usize { Self::CONST }
            }
        };

        let err = ImplWrapper::new(attrs, block).unwrap_err().to_string();
        assert!(
            err.contains("function specification is not supported"),
            "{}",
            err
        );
    }
}
