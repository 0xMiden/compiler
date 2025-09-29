use super::script::{expand as expand_script, ScriptConfig};

const TX_SCRIPT_EXPORT: &str = "miden:base/transaction-script@1.0.0";
const TX_SCRIPT_GUEST: &str = "self::bindings::exports::miden::base::transaction_script::Guest";

/// Expands the `#[tx_script]` attribute, wiring default transaction script bindings.
pub fn expand(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    expand_script(
        attr,
        item,
        ScriptConfig {
            export_interface: TX_SCRIPT_EXPORT,
            guest_trait_path: TX_SCRIPT_GUEST,
        },
    )
}
