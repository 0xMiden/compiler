use crate::script::ScriptConfig;

extern crate proc_macro;

mod account_component_metadata;
mod component_macro;
mod export_type;
mod generate;
mod script;
mod types;
mod util;

#[proc_macro]
pub fn generate(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    generate::expand(input)
}

#[proc_macro_attribute]
pub fn note_script(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    script::expand(
        attr,
        item,
        ScriptConfig {
            export_interface: "miden:base/note-script@1.0.0",
            guest_trait_path: "self::bindings::exports::miden::base::note_script::Guest",
        },
    )
}

#[proc_macro_attribute]
pub fn tx_script(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    script::expand(
        attr,
        item,
        ScriptConfig {
            export_interface: "miden:base/transaction-script@1.0.0",
            guest_trait_path: "self::bindings::exports::miden::base::transaction_script::Guest",
        },
    )
}

#[proc_macro_attribute]
pub fn component(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    component_macro::component(attr, item)
}

#[proc_macro_attribute]
pub fn export_type(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    export_type::expand(attr, item)
}
