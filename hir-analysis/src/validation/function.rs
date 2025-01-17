use midenc_hir::{
    diagnostics::{DiagnosticsHandler, Report, Severity, Spanned},
    *,
};

use super::{BlockValidator, DefsDominateUses, NamingConventions, Rule, TypeCheck};
use crate::{ControlFlowGraph, DominatorTree};

/// This validation rule ensures that function-local invariants are upheld:
///
/// * A function may not be empty
/// * All blocks in the function body must be valid
/// * All uses of values must be dominated by their definitions
/// * All value uses must type check, i.e. branching to a block with a value
///   of a different type than declared by the block parameter is invalid.
pub struct FunctionValidator {
    in_kernel_module: bool,
}
impl FunctionValidator {
    pub fn new(in_kernel_module: bool) -> Self {
        Self { in_kernel_module }
    }
}
impl Rule<Function> for FunctionValidator {
    fn validate(
        &mut self,
        function: &Function,
        diagnostics: &DiagnosticsHandler,
    ) -> Result<(), Report> {
        // Validate the function declaration
        let mut rules = NamingConventions.chain(CoherentSignature::new(self.in_kernel_module));
        rules.validate(function, diagnostics)?;

        // Ensure basic integrity of the function body
        let mut rules = BlockValidator::new(&function.dfg, function.id.span());
        for (_, block) in function.dfg.blocks() {
            rules.validate(block, diagnostics)?;
        }

        // Construct control flow and dominator tree analyses
        let cfg = ControlFlowGraph::with_function(function);
        let domtree = DominatorTree::with_function(function, &cfg);

        // Verify value usage
        let mut rules = DefsDominateUses::new(&function.dfg, &domtree)
            .chain(TypeCheck::new(&function.signature, &function.dfg));
        for (_, block) in function.dfg.blocks() {
            rules.validate(block, diagnostics)?;
        }

        Ok(())
    }
}

/// This validation rule ensures that a [Signature] is coherent
///
/// A signature is coherent if:
///
/// 1. The linkage is valid for functions
/// 2. The calling convention is valid in the context the function is defined in
/// 3. The ABI of its parameters matches the calling convention
/// 4. The ABI of the parameters and results are coherent, e.g. there are no signed integer
///    parameters which are specified as being zero-extended, there are no results if an sret
///    parameter is present, etc.
struct CoherentSignature {
    in_kernel_module: bool,
}
impl CoherentSignature {
    pub fn new(in_kernel_module: bool) -> Self {
        Self { in_kernel_module }
    }
}

impl Rule<Function> for CoherentSignature {
    fn validate(
        &mut self,
        function: &Function,
        diagnostics: &DiagnosticsHandler,
    ) -> Result<(), Report> {
        let span = function.id.span();

        // 1
        let linkage = function.signature.linkage;
        if !matches!(linkage, Linkage::External | Linkage::Internal) {
            return Err(diagnostics
                .diagnostic(Severity::Error)
                .with_message("invalid function signature")
                .with_primary_label(
                    span,
                    format!(
                        "the signature of this function specifies '{linkage}' linkage, but only \
                         'external' or 'internal' are valid"
                    ),
                )
                .into_report());
        }

        // 2
        let cc = function.signature.cc;
        let is_kernel_function = matches!(cc, CallConv::Kernel);
        if self.in_kernel_module {
            let is_public = function.signature.is_public();
            if is_public && !is_kernel_function {
                return Err(diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid function signature")
                    .with_primary_label(
                        span,
                        format!(
                            "the '{cc}' calling convention may only be used with 'internal' \
                             linkage in kernel modules",
                        ),
                    )
                    .with_help(
                        "This function is declared with 'external' linkage in a kernel module, so \
                         it must use the 'kernel' calling convention",
                    )
                    .into_report());
            } else if !is_public && is_kernel_function {
                return Err(diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid function signature")
                    .with_primary_label(
                        span,
                        "the 'kernel' calling convention may only be used with 'external' linkage",
                    )
                    .with_help(
                        "This function has 'internal' linkage, so it must either be made \
                         'external', or a different calling convention must be used",
                    )
                    .into_report());
            }
        } else if is_kernel_function {
            return Err(diagnostics
                .diagnostic(Severity::Error)
                .with_message("invalid function signature")
                .with_primary_label(
                    span,
                    "the 'kernel' calling convention may only be used in kernel modules",
                )
                .with_help(
                    "Kernel functions may only be declared in kernel modules, so you must either \
                     change the module type, or change the calling convention of this function",
                )
                .into_report());
        }

        // 3
        // * sret parameters may not be used with kernel calling convention
        // * pointer-typed parameters/results may not be used with kernel calling convention
        // * parameters larger than 8 bytes must be passed by reference with fast/C calling
        //   conventions
        // * results larger than 8 bytes require the use of an sret parameter with fast/C calling
        //   conventions
        // * total size of all parameters when laid out on the operand stack may not exceed 64 bytes
        //   (16 field elements)
        //
        // 4
        // * parameter count and types must be consistent between the signature and the entry block
        // * only sret parameter is permitted, and it must be the first parameter when present
        // * the sret attribute may not be applied to results
        // * sret parameters imply no results
        // * signed integer values may not be combined with zero-extension
        // * non-integer values may not be combined with argument extension
        let mut sret_count = 0;
        let mut effective_stack_usage = 0;
        let params = function.dfg.block_args(function.dfg.entry_block());
        if params.len() != function.signature.arity() {
            return Err(diagnostics
                .diagnostic(Severity::Error)
                .with_message("invalid function signature")
                .with_primary_label(
                    span,
                    "function signature and entry block have different arities",
                )
                .with_help(
                    "This happens if the signature or entry block are modified without updating \
                     the other, make sure the number and types of all parameters are the same in \
                     both the signature and the entry block",
                )
                .into_report());
        }
        for (i, param) in function.signature.params.iter().enumerate() {
            let is_first = i == 0;
            let value = params[i];
            let span = function.dfg.value_span(value);
            let param_ty = &param.ty;
            let value_ty = function.dfg.value_type(value);

            if param_ty != value_ty {
                return Err(diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid function signature")
                    .with_primary_label(
                        span,
                        "parameter type mismatch between signature and entry block",
                    )
                    .with_help(format!(
                        "The function declares this parameter as having type {param_ty}, but the \
                         actual type is {value_ty}"
                    ))
                    .into_report());
            }

            let is_integer = param_ty.is_integer();
            let is_signed_integer = param_ty.is_signed_integer();
            match param.extension {
                ArgumentExtension::Zext if is_signed_integer => {
                    return Err(diagnostics
                        .diagnostic(Severity::Error)
                        .with_message("invalid function signature")
                        .with_primary_label(
                            span,
                            "signed integer parameters may not be combined with zero-extension",
                        )
                        .with_help(
                            "Zero-extending a signed-integer loses the signedness, you should use \
                             signed-extension instead",
                        )
                        .into_report());
                }
                ArgumentExtension::Sext | ArgumentExtension::Zext if !is_integer => {
                    return Err(diagnostics
                        .diagnostic(Severity::Error)
                        .with_message("invalid function signature")
                        .with_primary_label(
                            span,
                            "non-integer parameters may not be combined with argument extension \
                             attributes",
                        )
                        .with_help(
                            "Argument extension has no meaning for types other than integers",
                        )
                        .into_report());
                }
                _ => (),
            }

            let is_pointer = param_ty.is_pointer();
            let is_sret = param.purpose == ArgumentPurpose::StructReturn;
            if is_sret {
                sret_count += 1;
            }

            if is_kernel_function && (is_sret || is_pointer) {
                return Err(diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid function signature")
                    .with_primary_label(
                        span,
                        "functions using the 'kernel' calling convention may not use sret or \
                         pointer-typed parameters",
                    )
                    .with_help(
                        "Kernel functions are invoked in a different memory context, so they may \
                         not pass or return values by reference",
                    )
                    .into_report());
            }

            if !is_kernel_function {
                if is_sret {
                    if sret_count > 1 || !is_first {
                        return Err(diagnostics
                            .diagnostic(Severity::Error)
                            .with_message("invalid function signature")
                            .with_primary_label(
                                span,
                                "a function may only have a single sret parameter, and it must be \
                                 the first parameter",
                            )
                            .with_help(
                                "The sret parameter type is used to return a large value from a \
                                 function, but it may only be used for functions with a single \
                                 return value",
                            )
                            .into_report());
                    }
                    if !is_pointer {
                        return Err(diagnostics
                            .diagnostic(Severity::Error)
                            .with_message("invalid function signature")
                            .with_primary_label(
                                span,
                                format!(
                                    "sret parameters must be pointer-typed, but got {param_ty}"
                                ),
                            )
                            .with_help(format!(
                                "Did you mean to define this parameter with type {}?",
                                &Type::Ptr(Box::new(param_ty.clone()))
                            ))
                            .into_report());
                    }

                    if !function.signature.results.is_empty() {
                        return Err(diagnostics
                            .diagnostic(Severity::Error)
                            .with_message("invalid function signature")
                            .with_primary_label(
                                span,
                                "functions with an sret parameter must have no results",
                            )
                            .with_help(
                                "An sret parameter is used in place of normal return values, but \
                                 this function uses both, which is not valid. You should remove \
                                 the results from the function signature.",
                            )
                            .into_report());
                    }
                }

                let size_in_bytes = param_ty.size_in_bytes();
                if !is_pointer && size_in_bytes > 8 {
                    return Err(diagnostics
                        .diagnostic(Severity::Error)
                        .with_message("invalid function signature")
                        .with_primary_label(
                            span,
                            "this parameter type is too large to pass by value",
                        )
                        .with_help(format!(
                            "This parameter has type {param_ty}, you must refactor this function \
                             to pass it by reference instead"
                        ))
                        .into_report());
                }
            }

            effective_stack_usage +=
                param_ty.clone().to_raw_parts().map(|parts| parts.len()).unwrap_or(0);
        }

        if effective_stack_usage > 16 {
            return Err(diagnostics
                .diagnostic(Severity::Error)
                .with_message("invalid function signature")
                .with_primary_label(span, "this function has a signature with too many parameters")
                .with_help(
                    "Due to the constraints of the Miden VM, all function parameters must fit on \
                     the operand stack, which is 16 elements (each of which is effectively 4 \
                     bytes, a maximum of 64 bytes). The layout of the parameter list of this \
                     function requires more than this limit. You should either remove parameters, \
                     or combine some of them into a struct which is then passed by reference.",
                )
                .into_report());
        }

        for (i, result) in function.signature.results.iter().enumerate() {
            if result.purpose == ArgumentPurpose::StructReturn {
                return Err(diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid function signature")
                    .with_primary_label(
                        span,
                        "the sret attribute is only permitted on function parameters",
                    )
                    .into_report());
            }

            if result.extension != ArgumentExtension::None {
                return Err(diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid function signature")
                    .with_primary_label(
                        span,
                        "the argument extension attributes are only permitted on function \
                         parameters",
                    )
                    .into_report());
            }

            let size_in_bytes = result.ty.size_in_bytes();
            if !result.ty.is_pointer() && size_in_bytes > 8 {
                return Err(diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid function signature")
                    .with_primary_label(
                        function.id.span(),
                        "This function specifies a result type which is too large to pass by value",
                    )
                    .with_help(format!(
                        "The parameter at index {} has type {}, you must refactor this function \
                         to pass it by reference instead",
                        i, &result.ty
                    ))
                    .into_report());
            }
        }

        Ok(())
    }
}
