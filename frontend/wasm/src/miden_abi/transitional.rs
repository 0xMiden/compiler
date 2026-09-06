//! Transitional signatures for procedures the package manifests cannot yet describe the way the
//! SDK binds them.
//!
//! Every procedure the SDK binds has a typed signature in the core 0.35 and protocol 0.17
//! manifests; what remains here is where the hand-written SDK wrapper and the manifest disagree
//! about the Wasm-side shape, plus one kept for its effects. All temporary (census of 2026-10-02):
//!
//! - 60 procedures whose manifest signature uses a narrow integer, a boolean or an enum
//!   (`u8`/`u16`/`u32`/`i1`) where the SDK wrapper passes or reads a `felt`.
//! - `::miden::core::mem::pipe_double_words_to_memory`, whose stub signature agrees with its
//!   manifest signature. It stays for the effects its hand table attached (see [`effects`]): a
//!   procedure resolved from a manifest carries none.
//!
//! All 61 are deleted when the SDK's sys crates are regenerated from the manifests
//! (sub-project 3): generated wrappers pass the manifest's types.
//!
//! Entries are typed exactly as the deleted hand tables typed them, on the Wasm side, and are
//! consulted *before* the linked packages so the SDK's current contract keeps working. The
//! signatures use the `Fast` convention so the rule set lowers them like any package export.
//!
//! The three `::miden::core::mem::pipe_*` entries also keep the memory and advice effects their
//! hand table attached, since they are compiler-owned data and a lint depends on them: see
//! [`effects`].

use alloc::{sync::Arc, vec::Vec};

use midenc_hir::{
    CallConv, FunctionType, SmallVec, Type,
    effects::{AdviceEffect, AdviceMapResource, AdviceStackResource, MemoryEffect},
    smallvec,
};
use midenc_hir_symbol::sync::LazyLock;
use midenc_package_interface::{PackageInterface, ProcedureClass, ProcedureItem, lower_signature};
use midenc_session::miden_assembly_syntax::ast::Path;

use crate::intrinsics::IntrinsicEffect;

/// The transitional interface, built once.
pub(crate) fn interface() -> &'static PackageInterface {
    static INTERFACE: LazyLock<PackageInterface> = LazyLock::new(build);
    &INTERFACE
}

fn build() -> PackageInterface {
    use miden_mast_package::{PackageId, TargetType, Version, Word};
    use midenc_session::miden_assembly_syntax::ast::AttributeSet;

    let procedures = TRANSITIONAL
        .iter()
        .map(|(path, params, results)| {
            let signature =
                FunctionType::new(CallConv::Fast, params.iter().cloned(), results.iter().cloned());
            let lowered = lower_signature(&signature)
                .unwrap_or_else(|err| panic!("transitional entry {path} must lower: {err}"));
            let path: Arc<Path> = Arc::from(Path::new(path).to_path_buf().into_boxed_path());
            ProcedureItem {
                path,
                digest: Word::default(),
                signature: Some(signature),
                attributes: AttributeSet::default(),
                class: ProcedureClass::Bindable(lowered),
            }
        })
        .collect::<Vec<_>>();

    PackageInterface {
        name: PackageId::from("midenc-transitional-signatures"),
        version: Version::new(0, 0, 0),
        kind: TargetType::Library,
        digest: Word::default(),
        procedures,
        types: Vec::new(),
        constants: Vec::new(),
        modules: Vec::new(),
    }
}

/// The memory and advice effects the deleted hand table
/// (`miden_abi::stdlib::mem::function_effects`) attached to the three `::miden::core::mem`
/// procedures, kept for the transitional entries only.
///
/// These three are compiler-owned data in the same sense the intrinsic tables are, and the
/// advice-taint lint (`-Zlint`) reads them off the callee's declaration to see that a value piped
/// out of the advice provider is unconstrained. A procedure resolved from a real package manifest
/// carries no effects: a manifest has no way to declare them, and treating an export's effects as
/// unknown is the intended end state (spec §7).
///
/// `path` is the callee's MASM path, e.g. `::miden::core::mem::pipe_words_to_memory`; a leading
/// `::` is ignored the same way [`PackageInterface::procedure`](
/// midenc_package_interface::PackageInterface::procedure) ignores it when resolving, so both an
/// absolute and a relative spelling of a matching path are recognized. Any other path yields no
/// effects.
pub(crate) fn effects(path: &Path) -> SmallVec<[IntrinsicEffect; 2]> {
    let memory_write = || IntrinsicEffect::Memory {
        effect: MemoryEffect::Write,
        result: None,
        argument: None,
    };
    let advice_read = |resource: Box<dyn midenc_hir::effects::Resource>| IntrinsicEffect::Advice {
        effect: AdviceEffect::Read,
        resource,
        result: None,
        argument: None,
    };
    match path.to_relative().to_string().as_str() {
        "miden::core::mem::pipe_words_to_memory"
        | "miden::core::mem::pipe_double_words_to_memory" => {
            smallvec![advice_read(Box::new(AdviceStackResource)), memory_write()]
        }
        "miden::core::mem::pipe_preimage_to_memory" => {
            smallvec![advice_read(Box::new(AdviceMapResource)), memory_write()]
        }
        _ => smallvec![],
    }
}

const TRANSITIONAL: &[(&str, &[Type], &[Type])] = &[
    // ::miden::core::mem
    (
        "::miden::core::mem::pipe_double_words_to_memory",
        &[
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::I32,
            Type::I32,
        ],
        &[
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::I32,
        ],
    ),
    (
        "::miden::core::mem::pipe_preimage_to_memory",
        &[Type::Felt, Type::I32, Type::Felt, Type::Felt, Type::Felt, Type::Felt],
        &[Type::I32],
    ),
    (
        "::miden::core::mem::pipe_words_to_memory",
        &[Type::Felt, Type::I32],
        &[
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::I32,
        ],
    ),
    // ::miden::protocol::active_account
    ("::miden::protocol::active_account::get_num_procedures", &[], &[Type::Felt]),
    (
        "::miden::protocol::active_account::get_procedure_root",
        &[Type::Felt],
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt],
    ),
    (
        "::miden::protocol::active_account::has_asset",
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt],
        &[Type::Felt],
    ),
    (
        "::miden::protocol::active_account::has_procedure",
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt],
        &[Type::Felt],
    ),
    (
        "::miden::protocol::active_account::has_storage_slot",
        &[Type::Felt, Type::Felt],
        &[Type::Felt],
    ),
    // ::miden::protocol::active_note
    (
        "::miden::protocol::active_note::find_attachment",
        &[Type::Felt],
        &[Type::Felt, Type::Felt],
    ),
    (
        "::miden::protocol::active_note::get_asset",
        &[Type::Felt],
        &[
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
        ],
    ),
    ("::miden::protocol::active_note::get_initial_num_assets", &[], &[Type::Felt]),
    ("::miden::protocol::active_note::is_private", &[], &[Type::Felt]),
    ("::miden::protocol::active_note::is_public", &[], &[Type::Felt]),
    (
        "::miden::protocol::active_note::write_attachment_to_memory",
        &[Type::I32, Type::Felt],
        &[Type::I32],
    ),
    // ::miden::protocol::asset
    (
        "::miden::protocol::asset::id_into_composition",
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt],
        &[Type::Felt],
    ),
    // ::miden::protocol::input_note
    (
        "::miden::protocol::input_note::find_attachment",
        &[Type::Felt, Type::Felt],
        &[Type::Felt, Type::Felt],
    ),
    (
        "::miden::protocol::input_note::get_asset",
        &[Type::Felt, Type::Felt],
        &[
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
        ],
    ),
    (
        "::miden::protocol::input_note::get_attachments_commitment",
        &[Type::Felt],
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt],
    ),
    (
        "::miden::protocol::input_note::get_initial_assets",
        &[Type::I32, Type::Felt],
        &[Type::I32],
    ),
    (
        "::miden::protocol::input_note::get_initial_assets_info",
        &[Type::Felt],
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt, Type::Felt],
    ),
    (
        "::miden::protocol::input_note::get_initial_num_assets",
        &[Type::Felt],
        &[Type::Felt],
    ),
    (
        "::miden::protocol::input_note::get_metadata",
        &[Type::Felt],
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt],
    ),
    (
        "::miden::protocol::input_note::get_note_id",
        &[Type::Felt],
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt],
    ),
    (
        "::miden::protocol::input_note::get_recipient",
        &[Type::Felt],
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt],
    ),
    (
        "::miden::protocol::input_note::get_script_root",
        &[Type::Felt],
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt],
    ),
    (
        "::miden::protocol::input_note::get_sender",
        &[Type::Felt],
        &[Type::Felt, Type::Felt],
    ),
    (
        "::miden::protocol::input_note::get_serial_number",
        &[Type::Felt],
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt],
    ),
    (
        "::miden::protocol::input_note::get_storage_info",
        &[Type::Felt],
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt, Type::Felt],
    ),
    (
        "::miden::protocol::input_note::remove_asset",
        &[
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
        ],
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt],
    ),
    (
        "::miden::protocol::input_note::write_attachment_commitments_to_memory",
        &[Type::I32, Type::Felt],
        &[Type::I32],
    ),
    (
        "::miden::protocol::input_note::write_attachment_to_memory",
        &[Type::I32, Type::Felt, Type::Felt],
        &[Type::I32],
    ),
    // ::miden::protocol::native_account
    (
        "::miden::protocol::native_account::has_initial_asset",
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt],
        &[Type::Felt],
    ),
    ("::miden::protocol::native_account::has_state_changed", &[], &[Type::Felt]),
    (
        "::miden::protocol::native_account::was_procedure_called",
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt],
        &[Type::Felt],
    ),
    // ::miden::protocol::note
    (
        "::miden::protocol::note::find_attachment_idx",
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt, Type::Felt],
        &[Type::Felt, Type::Felt],
    ),
    (
        "::miden::protocol::note::metadata_into_note_type",
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt],
        &[Type::Felt],
    ),
    (
        "::miden::protocol::note::metadata_into_tag",
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt],
        &[Type::Felt],
    ),
    // ::miden::protocol::output_note
    (
        "::miden::protocol::output_note::add_asset",
        &[
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
            Type::Felt,
        ],
        &[],
    ),
    (
        "::miden::protocol::output_note::add_attachment",
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt, Type::Felt, Type::Felt],
        &[],
    ),
    (
        "::miden::protocol::output_note::add_attachment_from_memory",
        &[Type::Felt, Type::I32, Type::I32, Type::Felt],
        &[],
    ),
    (
        "::miden::protocol::output_note::add_word_attachment",
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt, Type::Felt, Type::Felt],
        &[],
    ),
    (
        "::miden::protocol::output_note::compute_note_id",
        &[Type::Felt],
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt],
    ),
    (
        "::miden::protocol::output_note::create",
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt, Type::Felt, Type::Felt],
        &[Type::Felt],
    ),
    (
        "::miden::protocol::output_note::find_attachment",
        &[Type::Felt, Type::Felt],
        &[Type::Felt, Type::Felt],
    ),
    (
        "::miden::protocol::output_note::get_assets",
        &[Type::I32, Type::Felt],
        &[Type::I32],
    ),
    (
        "::miden::protocol::output_note::get_assets_info",
        &[Type::Felt],
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt, Type::Felt],
    ),
    (
        "::miden::protocol::output_note::get_attachments_commitment",
        &[Type::Felt],
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt],
    ),
    (
        "::miden::protocol::output_note::get_metadata",
        &[Type::Felt],
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt],
    ),
    (
        "::miden::protocol::output_note::get_recipient",
        &[Type::Felt],
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt],
    ),
    ("::miden::protocol::output_note::is_sealed", &[Type::Felt], &[Type::Felt]),
    ("::miden::protocol::output_note::seal", &[Type::Felt], &[]),
    (
        "::miden::protocol::output_note::write_attachment_commitments_to_memory",
        &[Type::I32, Type::Felt],
        &[Type::I32],
    ),
    (
        "::miden::protocol::output_note::write_attachment_to_memory",
        &[Type::I32, Type::Felt, Type::Felt],
        &[Type::I32],
    ),
    // ::miden::protocol::tx
    (
        "::miden::protocol::tx::compute_fee",
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt, Type::Felt],
        &[Type::Felt],
    ),
    (
        "::miden::protocol::tx::get_block_commitment",
        &[Type::Felt],
        &[Type::Felt, Type::Felt, Type::Felt, Type::Felt],
    ),
    ("::miden::protocol::tx::get_block_timestamp", &[], &[Type::Felt]),
    ("::miden::protocol::tx::get_expiration_block_delta", &[], &[Type::Felt]),
    ("::miden::protocol::tx::get_num_input_notes", &[], &[Type::Felt]),
    ("::miden::protocol::tx::get_num_output_notes", &[], &[Type::Felt]),
    ("::miden::protocol::tx::get_reference_block_number", &[], &[Type::Felt]),
    ("::miden::protocol::tx::update_expiration_block_delta", &[Type::Felt], &[]),
];

#[cfg(test)]
mod tests {
    use core::str::FromStr;

    use midenc_hir::{FunctionIdent, SymbolPath, Type};
    use midenc_package_interface::ReturnStrategy;

    use super::*;

    /// The three `mem` procedures keep the effects the deleted hand table attached, and nothing
    /// else gets any: see [`effects`].
    #[test]
    fn the_mem_procedures_keep_the_effects_the_hand_table_attached() {
        for (name, resource) in [
            ("pipe_words_to_memory", "advice-stack"),
            ("pipe_double_words_to_memory", "advice-stack"),
            ("pipe_preimage_to_memory", "advice-map"),
        ] {
            let path = alloc::format!("::miden::core::mem::{name}");
            let effects = effects(Path::new(&path));
            assert_eq!(effects.len(), 2, "{name}");
            match &effects[0] {
                IntrinsicEffect::Advice {
                    effect,
                    resource: declared,
                    ..
                } => {
                    assert_eq!(*effect, AdviceEffect::Read, "{name}");
                    assert_eq!(declared.name(), resource, "{name}");
                }
                _ => panic!("{name}: expected an advice read as the first effect"),
            }
            assert!(
                matches!(
                    &effects[1],
                    IntrinsicEffect::Memory {
                        effect: MemoryEffect::Write,
                        ..
                    }
                ),
                "{name}"
            );
        }

        // Everything else, transitional or not, carries no effects.
        assert!(effects(Path::new("::miden::core::mem::pipe_words_to_memory_v2")).is_empty());
        assert!(effects(Path::new("::miden::core::collections::smt::get")).is_empty());
        assert!(effects(Path::new("::miden::protocol::tx::get_block_timestamp")).is_empty());
    }

    /// The real lowering renders a callee path through
    /// [`SymbolPath::to_library_path`](midenc_hir::SymbolPath::to_library_path), which keeps the
    /// leading `::`; `effects` must recognize that rendering, not just the relative spelling used
    /// directly above.
    #[test]
    fn effects_recognizes_the_path_rendering_the_real_lowering_uses() {
        let id = FunctionIdent::from_str("miden::core::mem::pipe_words_to_memory").unwrap();
        let library_path = SymbolPath::from_masm_function_id(id).to_library_path();
        assert_eq!(effects(&library_path).len(), 2);
    }

    #[test]
    fn the_table_has_exactly_the_61_transitional_entries_all_bindable() {
        let iface = interface();
        assert_eq!(iface.bindable().count(), 61);
        assert_eq!(iface.skipped().count(), 0);
        assert_eq!(TRANSITIONAL.len(), 61);
    }

    #[test]
    fn entries_lower_to_the_shapes_the_hand_tables_recorded() {
        let iface = interface();
        // input_note::get_recipient: a note index in, one word out through an out pointer. The
        // SDK passes the index as a felt where the manifest says `u16`; the table keeps felt.
        let (_, lowered) = iface
            .bindable()
            .find(|(p, _)| {
                p.name() == "get_recipient" && p.namespace().to_string().ends_with("input_note")
            })
            .unwrap();
        assert_eq!(lowered.stub_signature().params(), &[Type::Felt, Type::I32]);
        assert!(lowered.stub_signature().results().is_empty());
        assert_eq!(lowered.import_signature().params(), &[Type::Felt]);
        // pipe_preimage_to_memory: single i32 result returned directly
        let (_, lowered) =
            iface.bindable().find(|(p, _)| p.name() == "pipe_preimage_to_memory").unwrap();
        assert_eq!(lowered.ret, ReturnStrategy::Direct(midenc_package_interface::WasmScalar::I32));
        assert_eq!(
            lowered.import_signature().params(),
            &[Type::Felt, Type::I32, Type::Felt, Type::Felt, Type::Felt, Type::Felt]
        );
        // get_reference_block_number: the SDK reads a felt where the manifest says u32; the
        // table keeps felt
        let (_, lowered) = iface
            .bindable()
            .find(|(p, _)| p.name() == "get_reference_block_number")
            .unwrap();
        assert_eq!(lowered.stub_signature().results(), &[Type::Felt]);
    }

    #[test]
    fn every_entry_is_a_fully_qualified_absolute_path_with_no_duplicates() {
        let mut seen = std::collections::BTreeSet::new();
        for (path, ..) in TRANSITIONAL {
            assert!(path.starts_with("::miden::"), "{path}");
            assert!(seen.insert(*path), "duplicate {path}");
        }
    }
}
