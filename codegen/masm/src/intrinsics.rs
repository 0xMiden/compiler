use miden_assembly::{
    LibraryPath,
    ast::{Module, ModuleKind},
};
use midenc_session::diagnostics::{PrintDiagnostic, SourceLanguage, SourceManager, Uri};

pub const I32_INTRINSICS_MODULE_NAME: &str = "intrinsics::i32";
pub const I64_INTRINSICS_MODULE_NAME: &str = "intrinsics::i64";
pub const I128_INTRINSICS_MODULE_NAME: &str = "intrinsics::i128";
pub const MEM_INTRINSICS_MODULE_NAME: &str = "intrinsics::mem";
pub const CRYPTO_INTRINSICS_MODULE_NAME: &str = "intrinsics::crypto";
pub const ADVICE_INTRINSICS_MODULE_NAME: &str = "intrinsics::advice";

pub const INTRINSICS_MODULE_NAMES: [&str; 6] = [
    I32_INTRINSICS_MODULE_NAME,
    I64_INTRINSICS_MODULE_NAME,
    I128_INTRINSICS_MODULE_NAME,
    MEM_INTRINSICS_MODULE_NAME,
    CRYPTO_INTRINSICS_MODULE_NAME,
    ADVICE_INTRINSICS_MODULE_NAME,
];

const I32_INTRINSICS: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/intrinsics/i32.masm"));
const I64_INTRINSICS: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/intrinsics/i64.masm"));
const I128_INTRINSICS: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/intrinsics/i128.masm"));
const MEM_INTRINSICS: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/intrinsics/mem.masm"));
const CRYPTO_INTRINSICS: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/intrinsics/crypto.masm"));
const ADVICE_INTRINSICS: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/intrinsics/advice.masm"));

/// This is a mapping of intrinsics module name to the raw MASM source for that module
const INTRINSICS: [(&str, &str, &str); 6] = [
    (
        I32_INTRINSICS_MODULE_NAME,
        I32_INTRINSICS,
        concat!(env!("CARGO_MANIFEST_DIR"), "/intrinsics/i32.masm"),
    ),
    (
        I64_INTRINSICS_MODULE_NAME,
        I64_INTRINSICS,
        concat!(env!("CARGO_MANIFEST_DIR"), "/intrinsics/i64.masm"),
    ),
    (
        I128_INTRINSICS_MODULE_NAME,
        I128_INTRINSICS,
        concat!(env!("CARGO_MANIFEST_DIR"), "/intrinsics/i128.masm"),
    ),
    (
        MEM_INTRINSICS_MODULE_NAME,
        MEM_INTRINSICS,
        concat!(env!("CARGO_MANIFEST_DIR"), "/intrinsics/mem.masm"),
    ),
    (
        CRYPTO_INTRINSICS_MODULE_NAME,
        CRYPTO_INTRINSICS,
        concat!(env!("CARGO_MANIFEST_DIR"), "/intrinsics/crypto.masm"),
    ),
    (
        ADVICE_INTRINSICS_MODULE_NAME,
        ADVICE_INTRINSICS,
        concat!(env!("CARGO_MANIFEST_DIR"), "/intrinsics/advice.masm"),
    ),
];

/// This helper loads the named module from the set of intrinsics modules defined in this crate.
///
/// Expects the fully-qualified name to be given, e.g. `intrinsics::mem`
pub fn load<N: AsRef<str>>(name: N, source_manager: &dyn SourceManager) -> Option<Box<Module>> {
    let name = name.as_ref();
    let (name, source, filename) = INTRINSICS.iter().copied().find(|(n, ..)| *n == name)?;
    let filename = Uri::new(filename);
    let source_file = source_manager.load(SourceLanguage::Masm, filename, source.to_string());
    let path = LibraryPath::new(name).expect("invalid module name");
    match Module::parse(path, ModuleKind::Library, source_file.clone()) {
        Ok(module) => Some(module),
        Err(err) => {
            let err = PrintDiagnostic::new(err);
            panic!("failed to parse intrinsic module: {err}");
        }
    }
}
