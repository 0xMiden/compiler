//! Test fixtures: assemble a small MASM library with the real assembler so the model is
//! exercised against a genuine package manifest rather than a hand-built one.

use alloc::{string::ToString, sync::Arc};

use miden_assembly_syntax::{
    ModuleParser,
    debuginfo::{DefaultSourceManager, SourceLanguage, SourceManager, Uri},
};
use miden_mast_package::Package;

/// A library covering every classification and lowering row the model must handle:
/// a felt identity (direct return), a struct result (out-pointer), a struct parameter
/// (flattened), a word round-trip (four felts each way), a narrowed scalar, an element-space
/// pointer parameter, a procedure with no signature (skipped), a role procedure, a constant,
/// and two type exports.
pub(crate) const FIXTURE_SOURCE: &str = r#"
pub type Pair = struct { a: felt, b: felt }
pub type Tag = u16

pub const LIMIT = 1024

pub proc id(x: felt) -> felt
    nop
end

pub proc make_pair(a: felt, b: felt) -> Pair
    nop
end

pub proc take_pair(p: Pair)
    drop drop
end

pub proc hash(w: word) -> word
    nop
end

pub proc tag_of(x: felt) -> Tag
    nop
end

pub proc write(p: ptr<u32, addrspace(felt)>, n: u32)
    drop drop
end

pub proc untyped
    nop
end

@account_procedure
pub proc role(x: felt) -> felt
    nop
end
"#;

/// Assemble `source` as the root module `root_path` of a library package named `name`.
pub(crate) fn assemble_fixture(name: &str, root_path: &str, source: &str) -> Arc<Package> {
    let source_manager: Arc<dyn SourceManager> = Arc::new(DefaultSourceManager::default());
    let uri = Uri::from(root_path.to_string().into_boxed_str());
    let source_file = source_manager.load(SourceLanguage::Masm, uri, source.to_string());
    let path = miden_assembly_syntax::ast::Path::new(root_path);
    let root = ModuleParser::new(None)
        .parse(Some(path), source_file, source_manager.clone())
        .unwrap_or_else(|err| panic!("fixture must parse: {err}"));
    let package = miden_assembly::Assembler::new(source_manager)
        .assemble_library(
            name,
            root,
            core::iter::empty::<alloc::boxed::Box<miden_assembly_syntax::ast::Module>>(),
        )
        .unwrap_or_else(|err| panic!("fixture must assemble: {err}"));
    Arc::from(package)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fixture_assembles_and_exports_its_procedures() {
        let package = assemble_fixture("fixture", "fixture", FIXTURE_SOURCE);
        assert!(package.is_library());
        let names: alloc::vec::Vec<_> =
            package.manifest.exports().map(|export| export.path().to_string()).collect();
        for expected in [
            "id",
            "make_pair",
            "take_pair",
            "hash",
            "tag_of",
            "write",
            "untyped",
            "role",
            "LIMIT",
            "Pair",
            "Tag",
        ] {
            assert!(
                names.iter().any(|name| name.ends_with(expected)),
                "missing export `{expected}` in {names:?}"
            );
        }
    }
}
