//! The rule for the Miden procedure paths carried in the `@external-id` of component functions
//! (e.g. `miden::counter_contract::counter_contract::get_count`).
//!
//! The compiler enforces it when it names component functions, and the SDK macros when they read
//! the `@external-id`s of a dependency, so both accept exactly the same paths.

use core::fmt;

/// Why a string is not a Miden procedure path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcedurePathError;

impl fmt::Display for ProcedurePathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "is not a Miden procedure path with a module and a function name (a leading `::` is \
             optional) whose segments are ASCII letters, digits and `_`",
        )
    }
}

/// Checks that `path` is a Miden procedure path and returns its module path and function name.
///
/// A procedure path is an optional leading `::` followed by at least two `::`-separated segments
/// of ASCII letters, digits and `_`, the last of which names the function.
pub fn validate_procedure_path(path: &str) -> Result<(&str, &str), ProcedurePathError> {
    let path = path.strip_prefix("::").unwrap_or(path);
    let is_bare_identifier = |segment: &str| {
        !segment.is_empty() && segment.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    };
    if !path.split("::").all(is_bare_identifier) {
        return Err(ProcedurePathError);
    }
    path.rsplit_once("::").ok_or(ProcedurePathError)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_procedure_paths_into_module_and_function() {
        assert_eq!(
            validate_procedure_path("miden::counter::counter::get_count"),
            Ok(("miden::counter::counter", "get_count"))
        );
        assert_eq!(validate_procedure_path("::acme::Add_2"), Ok(("acme", "Add_2")));
    }

    #[test]
    fn rejects_paths_that_are_not_procedure_paths() {
        for path in [
            "",
            "::",
            "get_count",
            "::get_count",
            "miden::",
            "miden::::get",
            "miden::x::\"first\"",
            "miden::x::get-count",
            "miden:x/get",
            ":::miden::x",
        ] {
            assert_eq!(validate_procedure_path(path), Err(ProcedurePathError), "`{path}`");
        }
    }
}
