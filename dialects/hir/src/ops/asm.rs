use midenc_hir::{
    derive::{EffectOpInterface, operation},
    dialects::builtin::attributes::{SignatureAttr, StringAttr},
    effects::*,
    print::AsmPrinter,
    traits::*,
    *,
};

use crate::HirDialect;

/// A typed MASM stack transformation. The signature describes the source boundary, never
/// the authority or ABI of procedures invoked by its body. Memory and advice are conservative.
#[derive(EffectOpInterface)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, AdviceEffectOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
#[effects(
    MemoryEffect(MemoryEffect::Read, MemoryEffect::Write),
    AdviceEffect(
        AdviceEffect::Read,
        AdviceEffect::Write,
        AdviceEffect::Allocate,
        AdviceEffect::Free
    )
)]
pub struct InlineAsm {
    #[attr(hidden)]
    body: StringAttr,
    #[attr(hidden)]
    signature: SignatureAttr,
    #[operands]
    arguments: AnyType,
}

impl InlineAsm {
    /// Check the exact declared source tuple without repairing result or argument types.
    pub fn validate_signature(&self) -> Result<(), Report> {
        let signature = self.get_signature();
        super::invoke::verify_arguments_against_signature(
            "hir.inline_asm",
            self.arguments(),
            &signature,
        )?;
        if self.num_results() != signature.results().len()
            || self
                .results()
                .iter()
                .zip(signature.results())
                .any(|(result, abi)| result.borrow().ty() != &abi.ty)
        {
            return Err(Report::msg("invalid hir.inline_asm: results disagree with signature"));
        }
        Ok(())
    }
}

impl InferTypeOpInterface for InlineAsm {
    fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
        if self.op.results.is_empty() {
            let signature = self.signature.borrow();
            super::invoke::infer_results_from_signature(&mut self.op, &signature, context)?;
        }
        self.validate_signature()
    }
}

impl Verify<dyn InferTypeOpInterface> for InlineAsm {
    fn verify(&self, _: &Context) -> Result<(), Report> {
        self.validate_signature()
    }
}

impl OpPrinter for InlineAsm {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        printer.print_space();
        printer.print_string(self.get_body().as_str());
        printer.print_space();
        super::invoke::print_arguments_and_signature(printer, self.arguments(), &self.signature());
        super::invoke::print_optional_attributes(printer, &self.op);
    }
}

impl OpParser for InlineAsm {
    fn parse(state: &mut OperationState, parser: &mut dyn OpAsmParser<'_>) -> ParseResult {
        let text = parser.parse_string()?.into_inner();
        let body = parser.context_rc().create_attribute::<StringAttr, _>(text);
        state.add_attribute("body", body);
        let (arguments, types) = super::invoke::parse_arguments_and_signature(state, parser)?;
        parser.parse_optional_attribute_dict_with_keyword(&mut state.attrs)?;
        let mut values = SmallVec::default();
        parser.resolve_operands(state.span, &arguments, &types, &mut values)?;
        state.operands.push(values);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use midenc_hir::{
        dialects::builtin::{BuiltinOpBuilder, attributes::Signature},
        testing::Test,
    };

    use super::*;
    use crate::HirOpBuilder;

    #[test]
    fn inline_assembly_builder_and_verifier_enforce_exact_source_tuple() {
        let mut test = Test::new("inline_contract", &[Type::Felt], &[]);
        let context = test.context_rc();
        let signature = Signature::new(&context, [Type::Felt], [Type::U32]);
        let mut builder = test.function_builder();
        let input = builder.entry_block().borrow().arguments()[0] as ValueRef;
        let mut asm = builder.inline_asm("", signature, [input], SourceSpan::UNKNOWN).unwrap();
        builder.ret(None, SourceSpan::UNKNOWN).unwrap();
        assert_eq!(asm.borrow().results()[0].borrow().ty(), &Type::U32);
        assert!(asm.borrow().validate_signature().is_ok());
        asm.borrow_mut().results_mut()[0].borrow_mut().set_type(Type::Felt);
        assert!(asm.borrow().validate_signature().is_err());
        assert!(asm.borrow().as_operation().recursively_verify().is_err());
        let wrong = Signature::new(&context, [Type::U32], []);
        assert!(builder.inline_asm("", wrong, [input], SourceSpan::UNKNOWN).is_err());
    }
}
