#![recursion_limit = "128"]

extern crate proc_macro;

mod function;
mod mock_impl;
mod util;

use proc_macro::TokenStream;

#[proc_macro_derive(Mock, attributes(mock))]
pub fn mock_derive(input: TokenStream) -> TokenStream {
    mock_impl::impl_mock(input)
}

#[proc_macro_attribute]
pub fn mock(attr: TokenStream, item: TokenStream) -> TokenStream {
    function::wrap(attr, item)
}
