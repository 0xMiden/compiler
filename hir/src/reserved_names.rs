//! Names of the procedures the backend generates in the modules of a component.
//!
//! No function a frontend defines may take one of these names in a module the backend lowers.

/// The generated procedure each module uses to fill the function-table slots whose callees it
/// defines.
///
/// One procedure per module, rather than one for the component, because `procref` on a private
/// procedure is only legal within its defining module — and a callee's visibility is its
/// author's decision, not something initialization gets to widen. A module's procedure also
/// invokes the procedures of the modules nested within it, so the component's `init` only has
/// to reach the top-level ones.
pub const FUNCTION_TABLE_INIT_PROC: &str = "__init_function_table";

/// The private canonical-ABI entry body generated in the root module of an executable, for
/// dispatch after `main` has already initialized the component.
pub const EXECUTABLE_ENTRYPOINT_WITHOUT_INIT_PROC: &str = "__midenc_entrypoint_without_init";

/// Every name in this module.
pub const RESERVED_PROCEDURE_NAMES: &[&str] =
    &[FUNCTION_TABLE_INIT_PROC, EXECUTABLE_ENTRYPOINT_WITHOUT_INIT_PROC];
