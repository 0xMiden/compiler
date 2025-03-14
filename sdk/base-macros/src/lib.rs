extern crate proc_macro;

/// Account component
#[proc_macro_attribute]
pub fn component(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    // TODO: implement
    item
}
