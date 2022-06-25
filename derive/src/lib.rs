#![recursion_limit = "128"]

extern crate proc_macro;

mod function;
mod mock;
mod utils;

use proc_macro::TokenStream;

#[proc_macro_derive(Mock, attributes(mock))]
pub fn mock_derive(input: TokenStream) -> TokenStream {
    mock::impl_mock(input)
}

#[proc_macro_attribute]
pub fn mock(attr: TokenStream, item: TokenStream) -> TokenStream {
    function::wrap(attr, item)
}
