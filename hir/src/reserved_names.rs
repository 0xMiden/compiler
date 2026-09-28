//! Names of the procedures the backend generates in the modules of a component.
//!
//! No function a frontend defines may take one of [`RESERVED_PROCEDURE_NAMES`] in a module the
//! backend lowers.

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

/// The component initializer codegen emits into a component's root module, next to the exports;
/// `<namespace>::init` is therefore reserved and no export may use it.
pub const COMPONENT_INIT_PROCEDURE: &str = "init";

/// Every name in this module except [`COMPONENT_INIT_PROCEDURE`].
///
/// The initializer only exists in the root module, where the procedures a frontend defines are the
/// exports, which the frontend already keeps off that name; core functions live in nested modules,
/// so a core function named `init` collides with nothing.
pub const RESERVED_PROCEDURE_NAMES: &[&str] =
    &[FUNCTION_TABLE_INIT_PROC, EXECUTABLE_ENTRYPOINT_WITHOUT_INIT_PROC];
