use alloc::collections::BTreeMap;
use core::fmt;

use midenc_hir::{
    diagnostics::{DiagnosticsHandler, Report, Severity, Spanned},
    *,
};

use super::Rule;

/// This error is produced when type checking the IR for function or module
#[derive(Debug, thiserror::Error)]
pub enum TypeError {
    /// The number of arguments given does not match what is expected by the instruction
    #[error("expected {expected} arguments, but {actual} are given")]
    IncorrectArgumentCount { expected: usize, actual: usize },
    /// The number of results produced does not match what is expected from the instruction
    #[error("expected {expected} results, but {actual} are produced")]
    IncorrectResultCount { expected: usize, actual: usize },
    /// One of the arguments is not of the correct type
    #[error("expected argument of {expected} type at index {index}, got {actual}")]
    IncorrectArgumentType {
        expected: TypePattern,
        actual: Type,
        index: usize,
    },
    /// One of the results is not of the correct type
    #[error("expected result of {expected} type at index {index}, got {actual}")]
    InvalidResultType {
        expected: TypePattern,
        actual: Type,
        index: usize,
    },
    /// An attempt was made to cast from a larger integer type to a smaller one via widening cast,
    /// e.g. `zext`
    #[error("expected result to be an integral type larger than {expected}, but got {actual}")]
    InvalidWideningCast { expected: Type, actual: Type },
    /// An attempt was made to cast from a smaller integer type to a larger one via narrowing cast,
    /// e.g. `trunc`
    #[error("expected result to be an integral type smaller than {expected}, but got {actual}")]
    InvalidNarrowingCast { expected: Type, actual: Type },
    /// The arguments of an instruction were supposed to be the same type, but at least one differs
    /// from the controlling type
    #[error(
        "expected arguments to be the same type ({expected}), but argument at index {index} is \
         {actual}"
    )]
    MatchingArgumentTypeViolation {
        expected: Type,
        actual: Type,
        index: usize,
    },
    /// The result type of an instruction was supposed to be the same as the arguments, but it
    /// wasn't
    #[error("expected result to be the same type ({expected}) as the arguments, but got {actual}")]
    MatchingResultTypeViolation { expected: Type, actual: Type },
}

/// This validation rule type checks a block to catch any type violations by instructions in that
/// block
pub struct TypeCheck<'a> {
    signature: &'a Signature,
    dfg: &'a DataFlowGraph,
}
impl<'a> TypeCheck<'a> {
    pub fn new(signature: &'a Signature, dfg: &'a DataFlowGraph) -> Self {
        Self { signature, dfg }
    }
}
impl Rule<BlockData> for TypeCheck<'_> {
    fn validate(
        &mut self,
        block_data: &BlockData,
        diagnostics: &DiagnosticsHandler,
    ) -> Result<(), Report> {
        // Traverse the block, checking each instruction in turn
        for node in block_data.insts.iter() {
            let span = node.span();
            let opcode = node.opcode();
            let results = self.dfg.inst_results(node.key);
            let typechecker = InstTypeChecker::new(diagnostics, self.dfg, node)?;

            match node.as_ref() {
                Instruction::UnaryOp(UnaryOp { arg, .. }) => match opcode {
                    Opcode::ImmI1
                    | Opcode::ImmU8
                    | Opcode::ImmI8
                    | Opcode::ImmU16
                    | Opcode::ImmI16
                    | Opcode::ImmU32
                    | Opcode::ImmI32
                    | Opcode::ImmU64
                    | Opcode::ImmI64
                    | Opcode::ImmFelt
                    | Opcode::ImmF64 => {
                        return Err(diagnostics
                            .diagnostic(Severity::Error)
                            .with_message("invalid instruction")
                            .with_primary_label(
                                span,
                                format!(
                                    "immediate opcode '{opcode}' cannot be used with \
                                     non-immediate argument"
                                ),
                            )
                            .into_report());
                    }
                    _ => {
                        typechecker.check(&[*arg], results)?;
                    }
                },
                Instruction::UnaryOpImm(UnaryOpImm { imm, .. }) => match opcode {
                    Opcode::PtrToInt => {
                        return Err(diagnostics
                            .diagnostic(Severity::Error)
                            .with_message("invalid instruction")
                            .with_primary_label(
                                span,
                                format!("'{opcode}' cannot be used with an immediate value"),
                            )
                            .into_report());
                    }
                    _ => {
                        typechecker.check_immediate(&[], imm, results)?;
                    }
                },
                Instruction::Load(LoadOp { ref ty, addr, .. }) => {
                    if ty.size_in_felts() > 4 {
                        return Err(diagnostics
                            .diagnostic(Severity::Error)
                            .with_message("invalid instruction")
                            .with_primary_label(
                                span,
                                format!(
                                    "cannot load a value of type {ty} on the stack, as it is \
                                     larger than 16 bytes"
                                ),
                            )
                            .into_report());
                    }
                    typechecker.check(&[*addr], results)?;
                }
                Instruction::BinaryOpImm(BinaryOpImm { imm, arg, .. }) => {
                    typechecker.check_immediate(&[*arg], imm, results)?;
                }
                Instruction::PrimOpImm(PrimOpImm { imm, args, .. }) => {
                    let args = args.as_slice(&self.dfg.value_lists);
                    typechecker.check_immediate(args, imm, results)?;
                }
                Instruction::LocalVar(LocalVarOp { op, local, args }) => {
                    let args = args.as_slice(&self.dfg.value_lists);
                    match op {
                        Opcode::Store => {
                            let expected_ty = self.dfg.local_type(*local);
                            let actual_ty = self.dfg.value_type(args[0]);
                            if actual_ty != expected_ty {
                                return Err(diagnostics
                                    .diagnostic(Severity::Error)
                                    .with_message("type error")
                                    .with_primary_label(
                                        span,
                                        format!(
                                            "local type is {expected_ty}, but argument is \
                                             {actual_ty}"
                                        ),
                                    )
                                    .into_report());
                            }
                            typechecker.check(args, results)?;
                        }
                        Opcode::Load => {
                            if !args.is_empty() {
                                return Err(diagnostics
                                    .diagnostic(Severity::Error)
                                    .with_message("invalid instruction")
                                    .with_primary_label(
                                        span,
                                        "local.load does not accept any arguments",
                                    )
                                    .into_report());
                            }
                            if results.len() != 1 {
                                return Err(diagnostics
                                    .diagnostic(Severity::Error)
                                    .with_message("invalid instruction")
                                    .with_primary_label(
                                        span,
                                        "local.load should have exactly one result",
                                    )
                                    .into_report());
                            }
                            let local_ty = self.dfg.local_type(*local);
                            if local_ty.size_in_felts() > 4 {
                                return Err(diagnostics
                                    .diagnostic(Severity::Error)
                                    .with_message("invalid instruction")
                                    .with_primary_label(
                                        span,
                                        format!(
                                            "cannot load a value of type {local_ty} on the stack, \
                                             as it is larger than 16 bytes"
                                        ),
                                    )
                                    .into_report());
                            }
                            let result_ty = self.dfg.value_type(results[0]);
                            if local_ty != result_ty {
                                return Err(diagnostics
                                    .diagnostic(Severity::Error)
                                    .with_message("type error")
                                    .with_primary_label(
                                        span,
                                        format!(
                                            "local type is {local_ty}, but result of load is \
                                             {result_ty}"
                                        ),
                                    )
                                    .into_report());
                            }
                        }
                        opcode => {
                            return Err(diagnostics
                                .diagnostic(Severity::Error)
                                .with_message("invalid instruction")
                                .with_primary_label(
                                    span,
                                    format!(
                                        "opcode '{opcode}' cannot be used with local variables"
                                    ),
                                )
                                .into_report());
                        }
                    }
                }
                Instruction::GlobalValue(_)
                | Instruction::BinaryOp(_)
                | Instruction::PrimOp(_)
                | Instruction::Test(_)
                | Instruction::InlineAsm(_)
                | Instruction::Call(_) => {
                    let args = node.arguments(&self.dfg.value_lists);
                    typechecker.check(args, results)?;
                }
                Instruction::Ret(Ret { ref args, .. }) => {
                    let args = args.as_slice(&self.dfg.value_lists);
                    if args.len() != self.signature.results.len() {
                        return Err(diagnostics
                            .diagnostic(Severity::Error)
                            .with_message("invalid instruction")
                            .with_primary_label(
                                span,
                                format!(
                                    "the function signature states that {} results should be \
                                     returned, but {} were given",
                                    self.signature.results.len(),
                                    args.len()
                                ),
                            )
                            .into_report());
                    }
                    for (index, (expected, arg)) in
                        self.signature.results.iter().zip(args.iter().copied()).enumerate()
                    {
                        let actual = self.dfg.value_type(arg);
                        if actual != &expected.ty {
                            return Err(diagnostics
                                .diagnostic(Severity::Error)
                                .with_message("type error")
                                .with_primary_label(
                                    span,
                                    format!(
                                        "result at index {index} is {actual}, but function \
                                         signature expects {}",
                                        &expected.ty
                                    ),
                                )
                                .into_report());
                        }
                    }
                }
                Instruction::RetImm(RetImm { ref arg, .. }) => {
                    if self.signature.results.len() != 1 {
                        return Err(diagnostics
                            .diagnostic(Severity::Error)
                            .with_message("invalid instruction")
                            .with_primary_label(
                                span,
                                format!(
                                    "the function signature states that {} results should be \
                                     returned, but {} were given",
                                    self.signature.results.len(),
                                    1
                                ),
                            )
                            .into_report());
                    }
                    let expected = &self.signature.results[0].ty;
                    let actual = arg.ty();
                    if &actual != expected {
                        return Err(diagnostics
                            .diagnostic(Severity::Error)
                            .with_message("type error")
                            .with_primary_label(
                                span,
                                format!(
                                    "result is {actual}, but function signature expects {expected}"
                                ),
                            )
                            .into_report());
                    }
                }
                Instruction::Br(Br {
                    successor:
                        Successor {
                            destination,
                            ref args,
                        },
                    ..
                }) => {
                    let successor = *destination;
                    let expected = self.dfg.block_args(successor);
                    let args = args.as_slice(&self.dfg.value_lists);
                    if args.len() != expected.len() {
                        return Err(diagnostics
                            .diagnostic(Severity::Error)
                            .with_message("invalid instruction")
                            .with_primary_label(
                                span,
                                format!(
                                    "{successor} expects {} arguments, but is being given {}",
                                    expected.len(),
                                    args.len()
                                ),
                            )
                            .into_report());
                    }
                    for (index, (param, arg)) in
                        expected.iter().copied().zip(args.iter().copied()).enumerate()
                    {
                        let expected = self.dfg.value_type(param);
                        let actual = self.dfg.value_type(arg);
                        if actual != expected {
                            return Err(diagnostics
                                .diagnostic(Severity::Error)
                                .with_message("type error")
                                .with_primary_label(
                                    span,
                                    format!(
                                        "{successor} argument at index {index} is expected to be \
                                         {expected}, but got {actual}"
                                    ),
                                )
                                .into_report());
                        }
                    }
                }
                Instruction::CondBr(CondBr {
                    cond,
                    ref then_dest,
                    ref else_dest,
                    ..
                }) => {
                    typechecker.check(&[*cond], results)?;

                    for successor in [then_dest, else_dest].into_iter() {
                        let expected = self.dfg.block_args(successor.destination);
                        let args = successor.args.as_slice(&self.dfg.value_lists);
                        if args.len() != expected.len() {
                            return Err(diagnostics
                                .diagnostic(Severity::Error)
                                .with_message("invalid instruction")
                                .with_primary_label(
                                    span,
                                    format!(
                                        "{successor} expects {} arguments, but is being given {}",
                                        expected.len(),
                                        args.len(),
                                        successor = successor.destination,
                                    ),
                                )
                                .into_report());
                        }
                        for (index, (param, arg)) in
                            expected.iter().copied().zip(args.iter().copied()).enumerate()
                        {
                            let expected = self.dfg.value_type(param);
                            let actual = self.dfg.value_type(arg);
                            if actual != expected {
                                return Err(diagnostics
                                    .diagnostic(Severity::Error)
                                    .with_message("type error")
                                    .with_primary_label(
                                        span,
                                        format!(
                                            "{successor} argument at index {index} is expected to \
                                             be {expected}, but got {actual}",
                                            successor = successor.destination
                                        ),
                                    )
                                    .into_report());
                            }
                        }
                    }
                }
                Instruction::Switch(Switch {
                    arg,
                    arms,
                    default: fallback,
                    ..
                }) => {
                    typechecker.check(&[*arg], results)?;

                    let mut seen = BTreeMap::<u32, usize>::default();
                    for (i, arm) in arms.iter().enumerate() {
                        if let Some(prev) = seen.insert(arm.value, i) {
                            return Err(diagnostics
                                .diagnostic(Severity::Error)
                                .with_message("invalid instruction")
                                .with_primary_label(
                                    span,
                                    format!(
                                        "all arms of a 'switch' must have a unique discriminant, \
                                         but the arm at index {i} has the same discriminant as \
                                         the arm at {prev}"
                                    ),
                                )
                                .into_report());
                        }
                    }

                    for (i, successor) in arms
                        .iter()
                        .map(|arm| &arm.successor)
                        .chain(core::iter::once(fallback))
                        .enumerate()
                    {
                        let expected = self.dfg.block_args(successor.destination);
                        let args = successor.args.as_slice(&self.dfg.value_lists);
                        if args.len() != expected.len() {
                            return Err(diagnostics
                                .diagnostic(Severity::Error)
                                .with_message("invalid instruction")
                                .with_primary_label(
                                    span,
                                    format!(
                                        "the destination for the arm at index {i}, {successor}, \
                                         expects {} arguments, but is being given {}",
                                        expected.len(),
                                        args.len(),
                                        successor = successor.destination,
                                    ),
                                )
                                .into_report());
                        }
                        for (index, (param, arg)) in
                            expected.iter().copied().zip(args.iter().copied()).enumerate()
                        {
                            let expected = self.dfg.value_type(param);
                            let actual = self.dfg.value_type(arg);
                            if actual != expected {
                                return Err(diagnostics
                                    .diagnostic(Severity::Error)
                                    .with_message("type error")
                                    .with_primary_label(
                                        span,
                                        format!(
                                            "invalid switch arm at index {i}: {successor} \
                                             argument at index {index} is expected to be \
                                             {expected}, but got {actual}",
                                            successor = successor.destination
                                        ),
                                    )
                                    .into_report());
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

/// This type represents a match pattern over kinds of types.
///
/// This is quite useful in the type checker, as otherwise we would have to handle many
/// type combinations for each instruction.
#[derive(Debug, PartialEq, Eq)]
pub enum TypePattern {
    /// Matches any type
    Any,
    /// Matches any integer type
    Int,
    /// Matches any unsigned integer type
    Uint,
    /// Matches any signed integer type
    #[allow(dead_code)]
    Sint,
    /// Matches any pointer type
    Pointer,
    /// Matches any primitive numeric or pointer type
    Primitive,
    /// Matches a specific type
    Exact(Type),
}
impl TypePattern {
    /// Returns true if this pattern matches `ty`
    pub fn matches(&self, ty: &Type) -> bool {
        match self {
            Self::Any => true,
            Self::Int => ty.is_integer(),
            Self::Uint => ty.is_unsigned_integer(),
            Self::Sint => ty.is_signed_integer(),
            Self::Pointer => ty.is_pointer(),
            Self::Primitive => ty.is_numeric() || ty.is_pointer(),
            Self::Exact(expected) => expected.eq(ty),
        }
    }
}
impl From<Type> for TypePattern {
    #[inline(always)]
    fn from(ty: Type) -> Self {
        Self::Exact(ty)
    }
}
impl fmt::Display for TypePattern {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Any => f.write_str("any"),
            Self::Int => f.write_str("integer"),
            Self::Uint => f.write_str("unsigned integer"),
            Self::Sint => f.write_str("signed integer"),
            Self::Pointer => f.write_str("pointer"),
            Self::Primitive => f.write_str("primitive"),
            Self::Exact(ty) => write!(f, "{ty}"),
        }
    }
}

/// This type represents kinds of instructions in terms of their argument and result types.
///
/// Each instruction kind represents a category of instructions with similar semantics.
pub enum InstPattern {
    /// The instruction matches if it has no arguments or results
    Empty,
    /// The instruction matches if it has one argument and one result, both of the given type
    Unary(TypePattern),
    /// The instruction matches if it has one argument of the given type and no results
    UnaryNoResult(TypePattern),
    /// The instruction matches if it has one argument of the first type and one result of the
    /// second type
    ///
    /// This is used to represent things like `inttoptr` or `ptrtoint` which map one type to
    /// another
    UnaryMap(TypePattern, TypePattern),
    /// The instruction matches if it has one argument of integral type, and one result of a larger
    /// integral type
    UnaryWideningCast(TypePattern, TypePattern),
    /// The instruction matches if it has one argument of integral type, and one result of a
    /// smaller integral type
    UnaryNarrowingCast(TypePattern, TypePattern),
    /// The instruction matches if it has two arguments of the given type, and one result which is
    /// the same type as the first argument
    Binary(TypePattern, TypePattern),
    /// The instruction matches if it has two arguments and one result, all of the same type
    BinaryMatching(TypePattern),
    /// The instruction matches if it has two arguments of the same type, and no results
    BinaryMatchingNoResult(TypePattern),
    /// The instruction matches if it has two arguments of the same type, and returns a boolean
    BinaryPredicate(TypePattern),
    /// The instruction matches if its first argument matches the first type, with two more
    /// arguments and one result matching the second type
    ///
    /// This is used to model instructions like `select`
    TernaryMatching(TypePattern, TypePattern),
    /// The instruction matches if it has the exact number of arguments and results given, each
    /// corresponding to the given type
    Exact(Vec<TypePattern>, Vec<TypePattern>),
    /// The instruction matches any number of arguments and results, of any type
    Any,
}
impl InstPattern {
    /// Evaluate this pattern against the given arguments and results
    pub fn into_match(
        self,
        dfg: &DataFlowGraph,
        args: &[Value],
        results: &[Value],
    ) -> Result<(), TypeError> {
        match self {
            Self::Empty => {
                if !args.is_empty() {
                    return Err(TypeError::IncorrectArgumentCount {
                        expected: 0,
                        actual: args.len(),
                    });
                }
                if !results.is_empty() {
                    return Err(TypeError::IncorrectResultCount {
                        expected: 0,
                        actual: args.len(),
                    });
                }
                Ok(())
            }
            Self::Unary(_)
            | Self::UnaryMap(..)
            | Self::UnaryWideningCast(..)
            | Self::UnaryNarrowingCast(..) => {
                if args.len() != 1 {
                    return Err(TypeError::IncorrectArgumentCount {
                        expected: 1,
                        actual: args.len(),
                    });
                }
                if results.len() != 1 {
                    return Err(TypeError::IncorrectResultCount {
                        expected: 1,
                        actual: results.len(),
                    });
                }
                let actual_in = dfg.value_type(args[0]);
                let actual_out = dfg.value_type(results[0]);
                self.into_unary_match(actual_in, Some(actual_out))
            }
            Self::UnaryNoResult(_) => {
                if args.len() != 1 {
                    return Err(TypeError::IncorrectArgumentCount {
                        expected: 1,
                        actual: args.len(),
                    });
                }
                if !results.is_empty() {
                    return Err(TypeError::IncorrectResultCount {
                        expected: 0,
                        actual: results.len(),
                    });
                }
                let actual = dfg.value_type(args[0]);
                self.into_unary_match(actual, None)
            }
            Self::Binary(..) | Self::BinaryMatching(_) | Self::BinaryPredicate(_) => {
                if args.len() != 2 {
                    return Err(TypeError::IncorrectArgumentCount {
                        expected: 2,
                        actual: args.len(),
                    });
                }
                if results.len() != 1 {
                    return Err(TypeError::IncorrectResultCount {
                        expected: 1,
                        actual: results.len(),
                    });
                }
                let lhs = dfg.value_type(args[0]);
                let rhs = dfg.value_type(args[1]);
                let result = dfg.value_type(results[0]);
                self.into_binary_match(lhs, rhs, Some(result))
            }
            Self::BinaryMatchingNoResult(_) => {
                if args.len() != 2 {
                    return Err(TypeError::IncorrectArgumentCount {
                        expected: 2,
                        actual: args.len(),
                    });
                }
                if !results.is_empty() {
                    return Err(TypeError::IncorrectResultCount {
                        expected: 0,
                        actual: results.len(),
                    });
                }
                let lhs = dfg.value_type(args[0]);
                let rhs = dfg.value_type(args[1]);
                self.into_binary_match(lhs, rhs, None)
            }
            Self::TernaryMatching(..) => {
                if args.len() != 3 {
                    return Err(TypeError::IncorrectArgumentCount {
                        expected: 3,
                        actual: args.len(),
                    });
                }
                if results.len() != 1 {
                    return Err(TypeError::IncorrectResultCount {
                        expected: 1,
                        actual: results.len(),
                    });
                }
                let cond = dfg.value_type(args[0]);
                let lhs = dfg.value_type(args[1]);
                let rhs = dfg.value_type(args[2]);
                let result = dfg.value_type(results[0]);
                self.into_ternary_match(cond, lhs, rhs, result)
            }
            Self::Exact(expected_args, expected_results) => {
                if args.len() != expected_args.len() {
                    return Err(TypeError::IncorrectArgumentCount {
                        expected: expected_args.len(),
                        actual: args.len(),
                    });
                }
                if results.len() != expected_results.len() {
                    return Err(TypeError::IncorrectResultCount {
                        expected: expected_results.len(),
                        actual: results.len(),
                    });
                }
                for (index, (expected, arg)) in
                    expected_args.into_iter().zip(args.iter().copied()).enumerate()
                {
                    let actual = dfg.value_type(arg);
                    if !expected.matches(actual) {
                        return Err(TypeError::IncorrectArgumentType {
                            expected,
                            actual: actual.clone(),
                            index,
                        });
                    }
                }
                for (index, (expected, result)) in
                    expected_results.into_iter().zip(results.iter().copied()).enumerate()
                {
                    let actual = dfg.value_type(result);
                    if !expected.matches(actual) {
                        return Err(TypeError::InvalidResultType {
                            expected,
                            actual: actual.clone(),
                            index,
                        });
                    }
                }

                Ok(())
            }
            Self::Any => Ok(()),
        }
    }

    /// Evaluate this pattern against the given arguments (including an immediate argument) and
    /// results
    pub fn into_match_with_immediate(
        self,
        dfg: &DataFlowGraph,
        args: &[Value],
        imm: &Immediate,
        results: &[Value],
    ) -> Result<(), TypeError> {
        match self {
            Self::Empty => panic!("invalid empty pattern for instruction with immediate argument"),
            Self::Unary(_)
            | Self::UnaryMap(..)
            | Self::UnaryWideningCast(..)
            | Self::UnaryNarrowingCast(..) => {
                if !args.is_empty() {
                    return Err(TypeError::IncorrectArgumentCount {
                        expected: 1,
                        actual: args.len() + 1,
                    });
                }
                if results.len() != 1 {
                    return Err(TypeError::IncorrectResultCount {
                        expected: 1,
                        actual: results.len(),
                    });
                }
                let actual_in = imm.ty();
                let actual_out = dfg.value_type(results[0]);
                self.into_unary_match(&actual_in, Some(actual_out))
            }
            Self::UnaryNoResult(_) => {
                if !args.is_empty() {
                    return Err(TypeError::IncorrectArgumentCount {
                        expected: 1,
                        actual: args.len() + 1,
                    });
                }
                if !results.is_empty() {
                    return Err(TypeError::IncorrectResultCount {
                        expected: 0,
                        actual: results.len(),
                    });
                }
                let actual = imm.ty();
                self.into_unary_match(&actual, None)
            }
            Self::Binary(..) | Self::BinaryMatching(_) | Self::BinaryPredicate(_) => {
                if args.len() != 1 {
                    return Err(TypeError::IncorrectArgumentCount {
                        expected: 2,
                        actual: args.len() + 1,
                    });
                }
                if results.len() != 1 {
                    return Err(TypeError::IncorrectResultCount {
                        expected: 1,
                        actual: results.len(),
                    });
                }
                let lhs = dfg.value_type(args[0]);
                let rhs = imm.ty();
                let result = dfg.value_type(results[0]);
                self.into_binary_match(lhs, &rhs, Some(result))
            }
            Self::BinaryMatchingNoResult(_) => {
                if args.len() != 1 {
                    return Err(TypeError::IncorrectArgumentCount {
                        expected: 2,
                        actual: args.len() + 1,
                    });
                }
                if !results.is_empty() {
                    return Err(TypeError::IncorrectResultCount {
                        expected: 0,
                        actual: results.len(),
                    });
                }
                let lhs = dfg.value_type(args[0]);
                let rhs = imm.ty();
                self.into_binary_match(lhs, &rhs, None)
            }
            Self::TernaryMatching(..) => {
                if args.len() != 2 {
                    return Err(TypeError::IncorrectArgumentCount {
                        expected: 3,
                        actual: args.len() + 1,
                    });
                }
                if results.len() != 1 {
                    return Err(TypeError::IncorrectResultCount {
                        expected: 1,
                        actual: results.len(),
                    });
                }
                let cond = dfg.value_type(args[0]);
                let lhs = dfg.value_type(args[1]);
                let rhs = imm.ty();
                let result = dfg.value_type(results[0]);
                self.into_ternary_match(cond, lhs, &rhs, result)
            }
            Self::Exact(expected_args, expected_results) => {
                if args.len() != expected_args.len() {
                    return Err(TypeError::IncorrectArgumentCount {
                        expected: expected_args.len(),
                        actual: args.len(),
                    });
                }
                if results.len() != expected_results.len() {
                    return Err(TypeError::IncorrectResultCount {
                        expected: expected_results.len(),
                        actual: results.len(),
                    });
                }
                for (index, (expected, arg)) in
                    expected_args.into_iter().zip(args.iter().copied()).enumerate()
                {
                    let actual = dfg.value_type(arg);
                    if !expected.matches(actual) {
                        return Err(TypeError::IncorrectArgumentType {
                            expected,
                            actual: actual.clone(),
                            index,
                        });
                    }
                }
                for (index, (expected, result)) in
                    expected_results.into_iter().zip(results.iter().copied()).enumerate()
                {
                    let actual = dfg.value_type(result);
                    if !expected.matches(actual) {
                        return Err(TypeError::InvalidResultType {
                            expected,
                            actual: actual.clone(),
                            index,
                        });
                    }
                }

                Ok(())
            }
            Self::Any => Ok(()),
        }
    }

    fn into_unary_match(
        self,
        actual_in: &Type,
        actual_out: Option<&Type>,
    ) -> Result<(), TypeError> {
        match self {
            Self::Unary(expected) | Self::UnaryNoResult(expected) => {
                if !expected.matches(actual_in) {
                    return Err(TypeError::IncorrectArgumentType {
                        expected,
                        actual: actual_in.clone(),
                        index: 0,
                    });
                }
                if let Some(actual_out) = actual_out {
                    if actual_in != actual_out {
                        return Err(TypeError::MatchingResultTypeViolation {
                            expected: actual_in.clone(),
                            actual: actual_out.clone(),
                        });
                    }
                }
            }
            Self::UnaryMap(expected_in, expected_out) => {
                if !expected_in.matches(actual_in) {
                    return Err(TypeError::IncorrectArgumentType {
                        expected: expected_in,
                        actual: actual_in.clone(),
                        index: 0,
                    });
                }
                let actual_out = actual_out.expect("expected result type");
                if !expected_out.matches(actual_out) {
                    return Err(TypeError::InvalidResultType {
                        expected: expected_out,
                        actual: actual_out.clone(),
                        index: 0,
                    });
                }
            }
            Self::UnaryWideningCast(expected_in, expected_out) => {
                if !expected_in.matches(actual_in) {
                    return Err(TypeError::IncorrectArgumentType {
                        expected: expected_in,
                        actual: actual_in.clone(),
                        index: 0,
                    });
                }
                let actual_out = actual_out.expect("expected result type");
                if !expected_out.matches(actual_out) {
                    return Err(TypeError::InvalidResultType {
                        expected: expected_out,
                        actual: actual_out.clone(),
                        index: 0,
                    });
                }
                if actual_in.size_in_bits() > actual_out.size_in_bits() {
                    return Err(TypeError::InvalidWideningCast {
                        expected: actual_in.clone(),
                        actual: actual_out.clone(),
                    });
                }
            }
            Self::UnaryNarrowingCast(expected_in, expected_out) => {
                if !expected_in.matches(actual_in) {
                    return Err(TypeError::IncorrectArgumentType {
                        expected: expected_in,
                        actual: actual_in.clone(),
                        index: 0,
                    });
                }
                let actual_out = actual_out.expect("expected result type");
                if !expected_out.matches(actual_out) {
                    return Err(TypeError::InvalidResultType {
                        expected: expected_out,
                        actual: actual_out.clone(),
                        index: 0,
                    });
                }
                if actual_in.size_in_bits() < actual_out.size_in_bits() {
                    return Err(TypeError::InvalidNarrowingCast {
                        expected: actual_in.clone(),
                        actual: actual_out.clone(),
                    });
                }
            }
            Self::Empty
            | Self::Binary(..)
            | Self::BinaryMatching(_)
            | Self::BinaryMatchingNoResult(_)
            | Self::BinaryPredicate(_)
            | Self::TernaryMatching(..)
            | Self::Exact(..)
            | Self::Any => unreachable!(),
        }

        Ok(())
    }

    fn into_binary_match(
        self,
        lhs: &Type,
        rhs: &Type,
        result: Option<&Type>,
    ) -> Result<(), TypeError> {
        match self {
            Self::Binary(expected_lhs, expected_rhs) => {
                if !expected_lhs.matches(lhs) {
                    return Err(TypeError::IncorrectArgumentType {
                        expected: expected_lhs,
                        actual: lhs.clone(),
                        index: 0,
                    });
                }
                if !expected_rhs.matches(rhs) {
                    return Err(TypeError::IncorrectArgumentType {
                        expected: expected_rhs,
                        actual: rhs.clone(),
                        index: 1,
                    });
                }
                let result = result.expect("expected result type");
                if lhs != result {
                    return Err(TypeError::MatchingResultTypeViolation {
                        expected: lhs.clone(),
                        actual: result.clone(),
                    });
                }
            }
            Self::BinaryMatching(expected) | Self::BinaryMatchingNoResult(expected) => {
                if !expected.matches(lhs) {
                    return Err(TypeError::IncorrectArgumentType {
                        expected,
                        actual: lhs.clone(),
                        index: 0,
                    });
                }
                if lhs != rhs {
                    return Err(TypeError::MatchingArgumentTypeViolation {
                        expected: lhs.clone(),
                        actual: rhs.clone(),
                        index: 1,
                    });
                }
                if let Some(result) = result {
                    if lhs != result {
                        return Err(TypeError::MatchingResultTypeViolation {
                            expected: lhs.clone(),
                            actual: result.clone(),
                        });
                    }
                }
            }
            Self::BinaryPredicate(expected) => {
                if !expected.matches(lhs) {
                    return Err(TypeError::IncorrectArgumentType {
                        expected,
                        actual: lhs.clone(),
                        index: 0,
                    });
                }
                if lhs != rhs {
                    return Err(TypeError::MatchingArgumentTypeViolation {
                        expected: lhs.clone(),
                        actual: rhs.clone(),
                        index: 1,
                    });
                }
                let result = result.expect("expected result type");
                let expected = Type::I1;
                if result != &expected {
                    return Err(TypeError::MatchingResultTypeViolation {
                        expected,
                        actual: result.clone(),
                    });
                }
            }
            Self::Empty
            | Self::Unary(_)
            | Self::UnaryNoResult(_)
            | Self::UnaryMap(..)
            | Self::UnaryWideningCast(..)
            | Self::UnaryNarrowingCast(..)
            | Self::TernaryMatching(..)
            | Self::Exact(..)
            | Self::Any => unreachable!(),
        }

        Ok(())
    }

    fn into_ternary_match(
        self,
        cond: &Type,
        lhs: &Type,
        rhs: &Type,
        result: &Type,
    ) -> Result<(), TypeError> {
        match self {
            Self::TernaryMatching(expected_cond, expected_inout) => {
                if !expected_cond.matches(cond) {
                    return Err(TypeError::IncorrectArgumentType {
                        expected: expected_cond,
                        actual: cond.clone(),
                        index: 0,
                    });
                }
                if !expected_inout.matches(lhs) {
                    return Err(TypeError::IncorrectArgumentType {
                        expected: expected_inout,
                        actual: lhs.clone(),
                        index: 1,
                    });
                }
                if lhs != rhs {
                    return Err(TypeError::IncorrectArgumentType {
                        expected: lhs.clone().into(),
                        actual: rhs.clone(),
                        index: 2,
                    });
                }
                if lhs != result {
                    return Err(TypeError::MatchingResultTypeViolation {
                        expected: lhs.clone(),
                        actual: result.clone(),
                    });
                }
            }
            Self::Empty
            | Self::Unary(_)
            | Self::UnaryNoResult(_)
            | Self::UnaryMap(..)
            | Self::UnaryWideningCast(..)
            | Self::UnaryNarrowingCast(..)
            | Self::Binary(..)
            | Self::BinaryMatching(_)
            | Self::BinaryMatchingNoResult(_)
            | Self::BinaryPredicate(_)
            | Self::Exact(..)
            | Self::Any => unreachable!(),
        }

        Ok(())
    }
}

/// This type plays the role of type checking instructions.
///
/// It is separate from the [TypeCheck] rule itself to factor out
/// all the instruction-related boilerplate.
struct InstTypeChecker<'a> {
    diagnostics: &'a DiagnosticsHandler,
    dfg: &'a DataFlowGraph,
    span: SourceSpan,
    pattern: InstPattern,
}
impl<'a> InstTypeChecker<'a> {
    /// Create a new instance of the type checker for the instruction represented by `node`.
    pub fn new(
        diagnostics: &'a DiagnosticsHandler,
        dfg: &'a DataFlowGraph,
        node: &InstNode,
    ) -> Result<Self, Report> {
        let span = node.span();
        let opcode = node.opcode();
        let is_local_op = matches!(&*node.data, Instruction::LocalVar(_));
        let pattern = match opcode {
            Opcode::Assert | Opcode::Assertz => InstPattern::UnaryNoResult(Type::I1.into()),
            Opcode::AssertEq => InstPattern::BinaryMatchingNoResult(Type::I1.into()),
            Opcode::ImmI1 => InstPattern::Unary(Type::I1.into()),
            Opcode::ImmU8 => InstPattern::Unary(Type::U8.into()),
            Opcode::ImmI8 => InstPattern::Unary(Type::I8.into()),
            Opcode::ImmU16 => InstPattern::Unary(Type::U16.into()),
            Opcode::ImmI16 => InstPattern::Unary(Type::I16.into()),
            Opcode::ImmU32 => InstPattern::Unary(Type::U32.into()),
            Opcode::ImmI32 => InstPattern::Unary(Type::I32.into()),
            Opcode::ImmU64 => InstPattern::Unary(Type::U64.into()),
            Opcode::ImmI64 => InstPattern::Unary(Type::I64.into()),
            Opcode::ImmU128 => InstPattern::Unary(Type::U128.into()),
            Opcode::ImmI128 => InstPattern::Unary(Type::I128.into()),
            Opcode::ImmFelt => InstPattern::Unary(Type::Felt.into()),
            Opcode::ImmF64 => InstPattern::Unary(Type::F64.into()),
            Opcode::Alloca => InstPattern::Exact(vec![], vec![TypePattern::Pointer]),
            Opcode::MemGrow => InstPattern::Exact(vec![Type::U32.into()], vec![Type::I32.into()]),
            Opcode::MemSize => InstPattern::Exact(vec![], vec![Type::U32.into()]),
            opcode @ Opcode::GlobalValue => match node.as_ref() {
                Instruction::GlobalValue(GlobalValueOp { global, .. }) => {
                    match dfg.global_value(*global) {
                        GlobalValueData::Symbol { .. } | GlobalValueData::IAddImm { .. } => {
                            InstPattern::Exact(vec![], vec![TypePattern::Pointer])
                        }
                        GlobalValueData::Load { ref ty, .. } => {
                            InstPattern::Exact(vec![], vec![ty.clone().into()])
                        }
                    }
                }
                inst => panic!("invalid opcode '{opcode}' for {inst:#?}"),
            },
            Opcode::Load if is_local_op => InstPattern::Unary(TypePattern::Any),
            Opcode::Load => InstPattern::UnaryMap(TypePattern::Pointer, TypePattern::Any),
            Opcode::Store if is_local_op => InstPattern::UnaryNoResult(TypePattern::Any),
            Opcode::Store => {
                InstPattern::Exact(vec![TypePattern::Pointer, TypePattern::Any], vec![])
            }
            Opcode::MemSet => InstPattern::Exact(
                vec![TypePattern::Pointer, Type::U32.into(), TypePattern::Any],
                vec![],
            ),
            Opcode::MemCpy => InstPattern::Exact(
                vec![TypePattern::Pointer, TypePattern::Pointer, Type::U32.into()],
                vec![],
            ),
            Opcode::PtrToInt => InstPattern::UnaryMap(TypePattern::Pointer, TypePattern::Int),
            Opcode::IntToPtr => InstPattern::UnaryMap(TypePattern::Uint, TypePattern::Pointer),
            Opcode::Bitcast => InstPattern::UnaryMap(TypePattern::Int, TypePattern::Int),
            Opcode::Cast => InstPattern::UnaryMap(TypePattern::Int, TypePattern::Int),
            Opcode::Trunc => InstPattern::UnaryNarrowingCast(TypePattern::Int, TypePattern::Int),
            Opcode::Zext => InstPattern::UnaryWideningCast(TypePattern::Int, TypePattern::Uint),
            Opcode::Sext => InstPattern::UnaryWideningCast(TypePattern::Int, TypePattern::Int),
            Opcode::Test => InstPattern::UnaryMap(TypePattern::Int, Type::I1.into()),
            Opcode::Select => InstPattern::TernaryMatching(Type::I1.into(), TypePattern::Primitive),
            Opcode::Add
            | Opcode::Sub
            | Opcode::Mul
            | Opcode::Div
            | Opcode::Mod
            | Opcode::DivMod
            | Opcode::Band
            | Opcode::Bor
            | Opcode::Bxor => InstPattern::BinaryMatching(TypePattern::Int),
            Opcode::Exp | Opcode::Shl | Opcode::Shr | Opcode::Rotl | Opcode::Rotr => {
                InstPattern::Binary(TypePattern::Int, TypePattern::Uint)
            }
            Opcode::Neg
            | Opcode::Inv
            | Opcode::Incr
            | Opcode::Ilog2
            | Opcode::Pow2
            | Opcode::Bnot
            | Opcode::Popcnt
            | Opcode::Clz
            | Opcode::Ctz
            | Opcode::Clo
            | Opcode::Cto => InstPattern::Unary(TypePattern::Int),
            Opcode::Not => InstPattern::Unary(Type::I1.into()),
            Opcode::And | Opcode::Or | Opcode::Xor => InstPattern::BinaryMatching(Type::I1.into()),
            Opcode::Eq | Opcode::Neq => InstPattern::BinaryPredicate(TypePattern::Primitive),
            Opcode::Gt | Opcode::Gte | Opcode::Lt | Opcode::Lte => {
                InstPattern::BinaryPredicate(TypePattern::Int)
            }
            Opcode::IsOdd => InstPattern::Exact(vec![TypePattern::Int], vec![Type::I1.into()]),
            Opcode::Min | Opcode::Max => InstPattern::BinaryMatching(TypePattern::Int),
            Opcode::Exec | Opcode::Call | Opcode::Syscall => match node.as_ref() {
                Instruction::Call(Call { ref callee, .. }) => {
                    if let Some(import) = dfg.get_import(callee) {
                        let args = import
                            .signature
                            .params
                            .iter()
                            .map(|p| TypePattern::Exact(p.ty.clone()))
                            .collect();
                        let results = import
                            .signature
                            .results
                            .iter()
                            .map(|p| TypePattern::Exact(p.ty.clone()))
                            .collect();
                        InstPattern::Exact(args, results)
                    } else {
                        return Err(diagnostics
                            .diagnostic(Severity::Error)
                            .with_message("invalid instruction")
                            .with_primary_label(
                                span,
                                format!("no signature is available for {callee}"),
                            )
                            .with_help(
                                "Make sure you import functions before building calls to them.",
                            )
                            .into_report());
                    }
                }
                inst => panic!("invalid opcode '{opcode}' for {inst:#?}"),
            },
            Opcode::Br => InstPattern::Any,
            Opcode::CondBr => InstPattern::Exact(vec![Type::I1.into()], vec![]),
            Opcode::Switch => InstPattern::Exact(vec![Type::U32.into()], vec![]),
            Opcode::Ret => InstPattern::Any,
            Opcode::Unreachable => InstPattern::Empty,
            Opcode::InlineAsm => InstPattern::Any,
            Opcode::Spill | Opcode::Reload => InstPattern::Any,
        };
        Ok(Self {
            diagnostics,
            dfg,
            span: node.span(),
            pattern,
        })
    }

    /// Checks that the given `operands` and `results` match the types represented by this
    /// [InstTypeChecker]
    pub fn check(self, operands: &[Value], results: &[Value]) -> Result<(), Report> {
        let diagnostics = self.diagnostics;
        let dfg = self.dfg;
        match self.pattern.into_match(dfg, operands, results) {
            Ok(_) => Ok(()),
            Err(err) => Err(diagnostics
                .diagnostic(Severity::Error)
                .with_message("type error")
                .with_primary_label(self.span, err)
                .into_report()),
        }
    }

    /// Checks that the given `operands` (with immediate) and `results` match the types represented
    /// by this [InstTypeChecker]
    pub fn check_immediate(
        self,
        operands: &[Value],
        imm: &Immediate,
        results: &[Value],
    ) -> Result<(), Report> {
        let diagnostics = self.diagnostics;
        let dfg = self.dfg;
        match self.pattern.into_match_with_immediate(dfg, operands, imm, results) {
            Ok(_) => Ok(()),
            Err(err) => Err(diagnostics
                .diagnostic(Severity::Error)
                .with_message("type error")
                .with_primary_label(self.span, err)
                .into_report()),
        }
    }
}
