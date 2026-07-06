//! Temporary suppression of known-harmless log records emitted by dependencies of the Miden
//! compiler binaries.

use log::{Log, Metadata, Record};

/// The `log` target of the suppressed [`SUPPRESSED_DEPENDENCY_ERROR_MESSAGES`] records.
const SUPPRESSED_DEPENDENCY_ERRORS_TARGET: &str = "miden_core::mast::serialization";

/// Error records emitted by the `miden-core` dependency when deserializing Miden packages whose
/// MAST includes node hashes and debug info (as the packages produced by the compiler do). They
/// are harmless: validation simply recomputes and checks the hashes.
///
/// TODO: Remove this suppression once the compiler migrates to the Miden VM release that no
/// longer emits these records (see https://github.com/0xMiden/compiler/issues/1211).
const SUPPRESSED_DEPENDENCY_ERROR_MESSAGES: [&str; 2] = [
    "UntrustedMastForest expected HASHLESS input; supplied artifact includes wire node hashes, \
     and validation will recompute them and require them to match",
    "UntrustedMastForest expected STRIPPED input; supplied artifact includes DebugInfo and other \
     optional payloads over the wire",
];

/// Returns `true` if the record is one of the suppressed dependency errors.
fn is_suppressed(record: &Record<'_>) -> bool {
    record.level() == log::Level::Error
        && record.target() == SUPPRESSED_DEPENDENCY_ERRORS_TARGET
        && SUPPRESSED_DEPENDENCY_ERROR_MESSAGES.contains(&record.args().to_string().as_str())
}

/// A [`Log`] adapter that drops the known-harmless dependency error records and forwards
/// everything else to the wrapped logger.
pub struct SuppressKnownDependencyErrors<L>(L);

impl<L: Log> SuppressKnownDependencyErrors<L> {
    /// Wraps `logger`, dropping the known-harmless dependency error records.
    pub fn new(logger: L) -> Self {
        Self(logger)
    }
}

impl<L: Log> Log for SuppressKnownDependencyErrors<L> {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.0.enabled(metadata)
    }

    fn log(&self, record: &Record<'_>) {
        if !is_suppressed(record) {
            self.0.log(record);
        }
    }

    fn flush(&self) {
        self.0.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The suppressed messages as single-line literals copied verbatim from the `miden-core`
    /// sources, so they can be compared against the dependency with a simple grep.
    #[rustfmt::skip]
    const VERBATIM_MESSAGES: [&str; 2] = [
        "UntrustedMastForest expected HASHLESS input; supplied artifact includes wire node hashes, and validation will recompute them and require them to match",
        "UntrustedMastForest expected STRIPPED input; supplied artifact includes DebugInfo and other optional payloads over the wire",
    ];

    /// Returns `true` if a record with the given level, target and message is suppressed.
    fn is_suppressed(level: log::Level, target: &str, message: &str) -> bool {
        super::is_suppressed(
            &Record::builder()
                .level(level)
                .target(target)
                .args(format_args!("{message}"))
                .build(),
        )
    }

    /// Verifies that the multi-line message constants match the `miden-core` sources exactly.
    #[test]
    fn suppressed_messages_match_miden_core_sources() {
        assert_eq!(SUPPRESSED_DEPENDENCY_ERROR_MESSAGES, VERBATIM_MESSAGES);
    }

    #[test]
    fn suppresses_known_dependency_errors() {
        for message in VERBATIM_MESSAGES {
            assert!(is_suppressed(log::Level::Error, "miden_core::mast::serialization", message));
        }
    }

    #[test]
    fn passes_through_other_records() {
        // A different message from the same module at the same level is not suppressed.
        assert!(!is_suppressed(
            log::Level::Error,
            "miden_core::mast::serialization",
            "failed to deserialize MAST forest",
        ));
        // The known messages coming from another module are not suppressed.
        assert!(!is_suppressed(log::Level::Error, "other_crate::module", VERBATIM_MESSAGES[1]));
    }
}
