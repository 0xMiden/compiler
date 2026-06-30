//! Cycle-count helpers for mock-chain integration tests.
//!
//! These helpers are intended for snapshot-style assertions using `midenc_expect_test::expect!`.

use miden_protocol::transaction::TransactionMeasurements;

/// Returns the measured prologue cycles as a string.
pub(crate) fn prologue_cycles(measurements: &TransactionMeasurements) -> &str {
    cycles_str(measurements.prologue)
}

/// Returns the measured transaction-script processing cycles as a string.
pub(crate) fn tx_script_processing_cycles(measurements: &TransactionMeasurements) -> &str {
    cycles_str(measurements.tx_script_processing)
}

/// Returns the measured authentication-procedure cycles as a string.
pub(crate) fn auth_procedure_cycles(measurements: &TransactionMeasurements) -> &str {
    cycles_str(measurements.auth_procedure)
}

/// Returns the measured note-execution cycles for a transaction with exactly one input note.
pub(crate) fn single_note_cycles(measurements: &TransactionMeasurements) -> &str {
    let [(_, num_cycles)] = measurements.note_execution.as_slice() else {
        panic!(
            "expected exactly one note-execution measurement, found {}",
            measurements.note_execution.len()
        );
    };

    cycles_str(*num_cycles)
}

fn cycles_str(cycles: usize) -> &'static str {
    Box::leak(cycles.to_string().into_boxed_str())
}
