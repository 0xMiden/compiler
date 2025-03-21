use alloc::borrow::Cow;

/// Configuration for the WASM translation.
pub struct WasmTranslationConfig {
    /// The source file name.
    /// This is used as a fallback for module/component name if it's not parsed from the Wasm
    /// binary, and an override name is not specified
    pub source_name: Cow<'static, str>,

    /// If specified, overrides the module/component name with the one specified
    pub override_name: Option<Cow<'static, str>>,

    /// The HIR world in which to translate any components/modules
    pub world: Option<midenc_hir::dialects::builtin::WorldRef>,

    /// Whether or not to generate native DWARF debug information.
    pub generate_native_debuginfo: bool,

    /// Whether or not to retain DWARF sections in compiled modules.
    pub parse_wasm_debuginfo: bool,
}

impl core::fmt::Debug for WasmTranslationConfig {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let world = if self.world.is_some() { "Some" } else { "None" };
        f.debug_struct("WasmTranslationConfig")
            .field("source_name", &self.source_name)
            .field("override_name", &self.override_name)
            .field("world", &world)
            .field("generate_native_debuginfo", &self.generate_native_debuginfo)
            .field("parse_wasm_debuginfo", &self.parse_wasm_debuginfo)
            .finish()
    }
}

impl Default for WasmTranslationConfig {
    fn default() -> Self {
        Self {
            source_name: Cow::Borrowed("noname"),
            override_name: None,
            world: None,
            generate_native_debuginfo: false,
            parse_wasm_debuginfo: true,
        }
    }
}
