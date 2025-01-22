use alloc::borrow::Cow;

/// Configuration for the WASM translation.
#[derive(Debug)]
pub struct WasmTranslationConfig {
    /// The source file name.
    /// This is used as a fallback for module/component name if it's not parsed from the Wasm
    /// binary, and an override name is not specified
    pub source_name: Cow<'static, str>,

    /// If specified, overrides the module/component name with the one specified
    pub override_name: Option<Cow<'static, str>>,

    /// Whether or not to generate native DWARF debug information.
    pub generate_native_debuginfo: bool,

    /// Whether or not to retain DWARF sections in compiled modules.
    pub parse_wasm_debuginfo: bool,
}

impl Default for WasmTranslationConfig {
    fn default() -> Self {
        Self {
            source_name: Cow::Borrowed("noname"),
            override_name: None,
            generate_native_debuginfo: false,
            parse_wasm_debuginfo: true,
        }
    }
}
