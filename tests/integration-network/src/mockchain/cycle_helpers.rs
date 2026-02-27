//! Cycle-count helpers for mock-chain integration tests.
//!
//! These helpers are intended for snapshot-style assertions using `midenc_expect_test::expect!`.

use miden_protocol::{note::NoteId, transaction::TransactionMeasurements};

/// Returns the measured prologue cycles as a string.
pub(super) fn prologue_cycles(measurements: &TransactionMeasurements) -> &str {
    cycles_str(measurements.prologue)
}

/// Returns the measured transaction-script processing cycles as a string.
pub(super) fn tx_script_processing_cycles(measurements: &TransactionMeasurements) -> &str {
    cycles_str(measurements.tx_script_processing)
}

/// Returns the measured authentication-procedure cycles as a string.
pub(super) fn auth_procedure_cycles(measurements: &TransactionMeasurements) -> &str {
    cycles_str(measurements.auth_procedure)
}

/// Returns the measured note-execution cycles for `note_id` as a string.
pub(super) fn note_cycles(measurements: &TransactionMeasurements, note_id: NoteId) -> &str {
    let (_, num_cycles) = measurements
        .note_execution
        .iter()
        .find(|(executed_note_id, _)| executed_note_id == &note_id)
        .unwrap_or_else(|| {
            panic!("No note-execution measurement found for note id {}", note_id.to_hex())
        });

    cycles_str(*num_cycles)
}

fn cycles_str(cycles: usize) -> &'static str {
    Box::leak(cycles.to_string().into_boxed_str())
}
