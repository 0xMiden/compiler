//! The interface a Miden package presents to other languages.
//!
//! Built from a package's manifest and nothing else: which procedures can be invoked with
//! `exec` from foreign code, what their Wasm C ABI shape is under the compiler's one lowering
//! rule set, which exports carry a protocol role, and which are skipped and why. The Wasm
//! frontend and the binding generators both consume this crate, so they cannot disagree about a
//! procedure.

#![deny(warnings)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![no_std]

extern crate alloc;

pub mod abi;
pub mod model;
pub mod resolve;

/// Test support: assemble a fixture library with the real assembler.
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use self::{
    abi::{
        FieldStep, Flattened, LoweredSignature, MAX_STACK_ELEMENTS, ReturnArea, ReturnSlot,
        ReturnStrategy, UnsupportedSignature, WasmParam, WasmScalar, flatten_type, lower_signature,
    },
    model::{
        ConstantItem, PackageInterface, ProcedureClass, ProcedureItem, ROLE_ATTRIBUTES, Role,
        SkipReason, TypeItem,
    },
    resolve::{ExportResolver, ResolvedProcedure},
};
