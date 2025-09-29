use super::script::{expand as expand_script, ScriptConfig};

const NOTE_SCRIPT_EXPORT: &str = "miden:base/note-script@1.0.0";
const NOTE_SCRIPT_GUEST: &str = "self::bindings::exports::miden::base::note_script::Guest";

/// Expands the `#[note_script]` attribute, wiring default note script bindings.
pub fn expand(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    expand_script(
        attr,
        item,
        ScriptConfig {
            export_interface: NOTE_SCRIPT_EXPORT,
            guest_trait_path: NOTE_SCRIPT_GUEST,
        },
    )
}
