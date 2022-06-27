//! Procedural macros for [`mimicry`].
//!
//! See macro docs for examples of usage.
//!
//! [`mimicry`]: https://docs.rs/mimicry/

#![recursion_limit = "128"]
// Linter settings.
#![warn(missing_debug_implementations, missing_docs, bare_trait_objects)]
#![warn(clippy::all, clippy::pedantic)]
#![allow(clippy::must_use_candidate, clippy::module_name_repetitions)]

extern crate proc_macro;

mod function;
mod mock_impl;
mod util;

use proc_macro::TokenStream;

/// Derives the `Mock` trait for a type, allowing to use it as a state for mocking.
///
/// # Container attributes
///
/// Container attributes are placed in a `#[mock(...)]` attribute on a struct / enum.
///
/// ## `shared`
///
/// Signals to use the [`Shared`] wrapper for the mock state; by default,
/// the [`ThreadLocal`] wrapper is used. Can be specified as `#[mock(shared)]` or
/// `#[mock(shared = true)]`.
///
/// # Examples
///
/// See [`ThreadLocal`] and [`Shared`] docs for examples of usage.
///
/// [`Shared`]: https://docs.rs/mimicry/latest/mimicry/struct.Shared.html
/// [`ThreadLocal`]: https://docs.rs/mimicry/latest/mimicry/struct.ThreadLocal.html
#[proc_macro_derive(Mock, attributes(mock))]
pub fn mock_derive(input: TokenStream) -> TokenStream {
    mock_impl::impl_mock(input)
}

/// Injects mocking logic into a function / method.
///
/// You may want to use this attribute conditionally, e.g.,
/// behind a `#[cfg_attr(test, _)]` wrapper.
///
/// # Attributes
///
/// Attributes are specified according to standard Rust conventions:
/// `#[mock(attr1 = "value1", ...)]`.
///
/// ## `using`
///
/// Specifies a [path] string to the mock state. The path can point to the type of the mock state
/// (e.g., `"mocks::State"`); in this case, the mock impl is an inherent function of the state
/// with the same name as the mocked function / method. Alternatively, a path can specify
/// the function name as well (e.g., `"mocks::State::mock_something"`); this is useful in case
/// of name collision. The choice of these 2 options is auto-detected based on the last segment
/// in the path: if it starts with an uppercase letter, it is considered a mock state type;
/// otherwise, it is considered a type + function.
///
/// # Supported items
///
/// The attribute can be used on functions / methods. Pretty much all signatures are supported
/// (e.g., generic functions, non-`'static` args, return types with dependent / elided lifetime,
/// etc. `const` functions are not supported.
///
/// [path]: syn::Path
#[proc_macro_attribute]
pub fn mock(attr: TokenStream, item: TokenStream) -> TokenStream {
    function::wrap(attr, item)
}
