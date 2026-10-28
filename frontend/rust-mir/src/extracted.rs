//! Holds the MIR subset that the frontend supports, as plain owned data.
//!
//! The extraction phase builds these values inside the rustc callback, and the translation phase
//! reads them on the caller thread. Thus every type here is [Send] and refers to no rustc data.

use midenc_hir::{Immediate, Type};

/// The index of a MIR local. MIR numbers the return local `_0` and the argument locals
/// `_1..=arg_count` first.
pub(crate) type Local = usize;

/// The MIR return local `_0`.
pub(crate) const RETURN_LOCAL: Local = 0;

/// The functions extracted from the analyzed crate.
#[derive(Debug)]
pub(crate) struct ExtractedCrate {
    /// The extracted functions, in the order of the crate items.
    pub functions: Vec<ExtractedFunction>,
}

/// One function body with a single basic block that ends with `return`.
#[derive(Debug)]
pub(crate) struct ExtractedFunction {
    /// The Miden IR symbol of the function. It is unique in the crate.
    pub symbol: String,
    /// The types of the parameters.
    pub params: Vec<Type>,
    /// The result type, or `None` if the function returns the unit type.
    pub result: Option<Type>,
    /// The type of every MIR local, indexed by the local. A local of the unit type has `None`.
    pub locals: Vec<Option<Type>>,
    /// The assignments of the basic block, in program order.
    pub statements: Vec<Assign>,
}

/// An assignment of an rvalue to a whole local.
#[derive(Debug)]
pub(crate) struct Assign {
    /// The local that receives the value.
    pub dest: Local,
    /// The value that is assigned.
    pub value: Rvalue,
}

/// A MIR rvalue.
#[derive(Debug)]
pub(crate) enum Rvalue {
    /// The value of an operand.
    Use(Operand),
    /// The wrapping sum of two operands.
    AddWrapping(Operand, Operand),
}

/// A MIR operand.
///
/// MIR `Copy` and `Move` become the same operand, because the Miden IR does not track who owns
/// the value afterwards.
#[derive(Debug)]
pub(crate) enum Operand {
    /// The value of a whole local.
    Local(Local),
    /// A scalar constant.
    Constant(Immediate),
}

// The extracted crate crosses from the rustc thread to the caller thread.
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<ExtractedCrate>();
};
