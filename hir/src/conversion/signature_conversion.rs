use alloc::{format, string::ToString, vec::Vec};

use smallvec::SmallVec;

use super::TypeConverter;
use crate::{
    BlockRef, CallOpInterface, CallableOpInterface, OperationRef, RegionBranchOpInterface,
    RegionBranchTerminatorOpInterface, RegionSuccessorInfo, Report, SuccessorOperand,
    SuccessorOperands, Type, Value, ValueRef, dialects::builtin::attributes::Signature,
    traits::BranchOpInterface,
};

/// Describes how a block/callable signature maps original inputs to converted inputs.
#[derive(Debug)]
pub struct SignatureConversion {
    converted_types: SmallVec<[Type; 4]>,
    mappings: Vec<InputMapping>,
}

impl SignatureConversion {
    pub fn new() -> Self {
        Self {
            converted_types: SmallVec::new(),
            mappings: Vec::new(),
        }
    }

    pub fn for_block_1_to_1(
        block: BlockRef,
        type_converter: &TypeConverter,
    ) -> Result<Self, Report> {
        let arguments = block
            .borrow()
            .arguments()
            .iter()
            .copied()
            .map(|arg| arg as ValueRef)
            .collect::<SmallVec<[ValueRef; 4]>>();

        let mut conversion = Self::new();
        for (index, argument) in arguments.into_iter().enumerate() {
            conversion.keep_input(index, type_converter.convert_value_1_to_1(argument)?);
        }
        Ok(conversion)
    }

    #[inline]
    pub fn converted_types(&self) -> &[Type] {
        &self.converted_types
    }

    #[inline]
    pub fn mappings(&self) -> &[InputMapping] {
        &self.mappings
    }

    pub fn keep_input(&mut self, old_index: usize, new_type: Type) -> &mut Self {
        let new_index = self.converted_types.len();
        self.converted_types.push(new_type);
        self.mappings.push(InputMapping::Keep {
            old_index,
            new_index,
            new_count: 1,
        });
        self
    }

    pub fn replace_input(
        &mut self,
        old_index: usize,
        values: impl IntoIterator<Item = ValueRef>,
    ) -> &mut Self {
        self.mappings.push(InputMapping::Replace {
            old_index,
            values: values.into_iter().collect(),
        });
        self
    }

    pub fn drop_input(&mut self, old_index: usize) -> &mut Self {
        self.mappings.push(InputMapping::Drop { old_index });
        self
    }

    /// Apply this conversion to block argument types in place.
    ///
    /// This is intentionally limited to 1:1 conversions. 1:N and drop conversions need a
    /// replacement block plus successor rewrites, which should be implemented when a concrete
    /// lowering needs it.
    pub fn apply_to_block_arguments_1_to_1(&self, block: BlockRef) -> Result<bool, Report> {
        let arguments = block.borrow().arguments().iter().copied().collect::<SmallVec<[_; 4]>>();
        if arguments.len() != self.mappings.len() {
            return Err(Report::msg(format!(
                "cannot apply signature conversion to block: expected {} input mappings for the \
                 block arguments, got {}",
                arguments.len(),
                self.mappings.len()
            )));
        }
        if arguments.len() != self.converted_types.len() {
            return Err(Report::msg(format!(
                "cannot apply signature conversion to block: expected {} converted inputs for the \
                 block arguments, got {}",
                arguments.len(),
                self.converted_types.len()
            )));
        }

        for (expected_old_index, mapping) in self.mappings.iter().enumerate() {
            match mapping {
                InputMapping::Keep {
                    old_index,
                    new_index,
                    new_count,
                } if *old_index == expected_old_index
                    && *new_index == expected_old_index
                    && *new_count == 1 => {}
                InputMapping::Keep { .. } => {
                    return Err(Report::msg(
                        "cannot apply non-identity input ordering to block arguments in place",
                    ));
                }
                InputMapping::Replace { .. } | InputMapping::Drop { .. } => {
                    return Err(Report::msg(
                        "cannot apply replace/drop input mapping to block arguments in place",
                    ));
                }
            }
        }

        let mut changed = false;
        for (mut argument, ty) in arguments.into_iter().zip(self.converted_types.iter()) {
            if argument.borrow().ty() != ty {
                argument.borrow_mut().set_type(ty.clone());
                changed = true;
            }
        }
        Ok(changed)
    }
}

impl Default for SignatureConversion {
    fn default() -> Self {
        Self::new()
    }
}

/// Mapping from one original input to the converted signature.
#[derive(Clone, Debug)]
pub enum InputMapping {
    Keep {
        old_index: usize,
        new_index: usize,
        new_count: usize,
    },
    Replace {
        old_index: usize,
        values: SmallVec<[ValueRef; 2]>,
    },
    Drop {
        old_index: usize,
    },
}

/// Convert a builtin ABI signature 1:1, preserving ABI metadata on each parameter/result.
pub fn convert_signature_1_to_1(
    signature: &Signature,
    type_converter: &TypeConverter,
) -> Result<Signature, Report> {
    let mut converted = signature.clone();
    for param in converted.params.iter_mut() {
        param.ty = type_converter.convert_type_1_to_1(&param.ty)?;
    }
    for result in converted.results.iter_mut() {
        result.ty = type_converter.convert_type_1_to_1(&result.ty)?;
    }
    Ok(converted)
}

/// Return the converted callable signature for `op`, if it implements `CallableOpInterface`.
pub fn converted_callable_signature_1_to_1(
    op: OperationRef,
    type_converter: &TypeConverter,
) -> Result<Option<Signature>, Report> {
    let op = op.borrow();
    let Some(callable) = op.as_trait::<dyn CallableOpInterface>() else {
        return Ok(None);
    };
    convert_signature_1_to_1(&callable.signature(), type_converter).map(Some)
}

/// Resolve a call-like operation and return the converted callee signature.
///
/// This helper intentionally does not rewrite call operands, result values, or callee attributes.
/// Different dialects store those pieces differently, so concrete conversion patterns should use
/// this to drive operation-specific rewrites.
pub fn converted_resolved_call_signature_1_to_1(
    op: OperationRef,
    type_converter: &TypeConverter,
) -> Result<Option<Signature>, Report> {
    let op = op.borrow();
    let Some(call) = op.as_trait::<dyn CallOpInterface>() else {
        return Ok(None);
    };
    let signature = resolve_call_signature(op.name().to_string(), call)?;
    convert_signature_1_to_1(&signature, type_converter).map(Some)
}

/// Verify that a call-like operation's operands/results match its resolved callable signature.
pub fn verify_call_signature_operands_and_results(op: OperationRef) -> Result<(), Report> {
    let op = op.borrow();
    let Some(call) = op.as_trait::<dyn CallOpInterface>() else {
        return Ok(());
    };

    let op_name = op.name().to_string();
    let signature = resolve_call_signature(op_name.clone(), call)?;

    let arguments = call.arguments();
    if arguments.len() != signature.params().len() {
        return Err(Report::msg(format!(
            "operation '{op_name}' has {} call operands, but the resolved callee expects {}",
            arguments.len(),
            signature.params().len()
        )));
    }
    for (index, (argument, param)) in arguments.iter().zip(signature.params()).enumerate() {
        let argument_ty = argument.borrow().ty();
        if argument_ty != param.ty {
            return Err(Report::msg(format!(
                "operation '{op_name}' passes operand {index} of type '{argument_ty}', but the \
                 resolved callee expects '{}'",
                param.ty
            )));
        }
    }

    let results = signature.results();
    if op.num_results() != results.len() {
        return Err(Report::msg(format!(
            "operation '{op_name}' has {} results, but the resolved callee returns {}",
            op.num_results(),
            results.len()
        )));
    }
    for (index, (result, expected)) in op.results().iter().zip(results.iter()).enumerate() {
        let result_ty = result.borrow().ty().clone();
        if result_ty != expected.ty {
            return Err(Report::msg(format!(
                "operation '{op_name}' result {index} has type '{result_ty}', but the resolved \
                 callee returns '{}'",
                expected.ty
            )));
        }
    }

    Ok(())
}

/// Verify that a branch operation's successor operand lists still match destination block arity.
pub fn verify_branch_successor_operand_arities(op: OperationRef) -> Result<(), Report> {
    let op = op.borrow();
    let Some(branch) = op.as_trait::<dyn BranchOpInterface>() else {
        return Ok(());
    };

    for successor_index in 0..op.num_successors() {
        let successor = op.successor(successor_index);
        let destination = successor.successor();
        let destination_arguments = destination
            .borrow()
            .arguments()
            .iter()
            .copied()
            .map(|arg| arg as ValueRef)
            .collect::<SmallVec<[ValueRef; 4]>>();
        let operands = branch.get_successor_operands(successor_index);
        verify_successor_operands_against_inputs(
            op.name().to_string(),
            format!("successor {successor_index}"),
            &operands,
            destination_arguments.iter().copied(),
            |lhs, rhs| branch.are_types_compatible(lhs, rhs),
        )?;
    }

    Ok(())
}

/// Verify entry successor operands for a region-branching operation.
pub fn verify_region_branch_successor_operand_arities(op: OperationRef) -> Result<(), Report> {
    let op = op.borrow();
    let Some(region_branch) = op.as_trait::<dyn RegionBranchOpInterface>() else {
        return Ok(());
    };

    let operand_constants = unknown_operand_constants(op.num_operands());
    for successor in region_branch.get_entry_successor_regions(&operand_constants) {
        let point = *successor.branch_point();
        let operands = region_branch.get_entry_successor_operands(point);
        verify_successor_operands_against_inputs(
            op.name().to_string(),
            format!("region successor {point}"),
            &operands,
            successor.successor_inputs().iter(),
            |lhs, rhs| region_branch.are_types_compatible(lhs, rhs),
        )?;
    }

    Ok(())
}

/// Verify region successor operands forwarded by a region-branch terminator operation.
pub fn verify_region_branch_terminator_successor_operand_arities(
    op: OperationRef,
) -> Result<(), Report> {
    let op = op.borrow();
    let Some(terminator) = op.as_trait::<dyn RegionBranchTerminatorOpInterface>() else {
        return Ok(());
    };

    let parent_op_ref = op.parent_region().and_then(|region| region.parent()).ok_or_else(|| {
        Report::msg(format!(
            "operation '{}' implements RegionBranchTerminatorOpInterface but has no parent region \
             branch operation",
            op.name()
        ))
    })?;

    let op_name = op.name().to_string();
    let operand_constants = unknown_operand_constants(op.num_operands());
    for successor in terminator.get_successor_regions(&operand_constants) {
        let point = successor.successor();
        let inputs = region_successor_inputs(&successor)?;
        let operands = terminator.get_successor_operands(point);
        let parent = parent_op_ref.borrow();
        let region_branch = parent.as_trait::<dyn RegionBranchOpInterface>().ok_or_else(|| {
            Report::msg(format!(
                "operation '{op_name}' has parent operation '{}' that does not implement \
                 RegionBranchOpInterface",
                parent.name()
            ))
        })?;
        verify_successor_operands_against_inputs(
            op_name.clone(),
            format!("region terminator successor {point}"),
            &operands,
            inputs.iter().copied(),
            |lhs, rhs| region_branch.are_types_compatible(lhs, rhs),
        )?;
    }

    Ok(())
}

fn unknown_operand_constants(num_operands: usize) -> Vec<Option<crate::AttributeRef>> {
    core::iter::repeat_with(|| None).take(num_operands).collect()
}

fn resolve_call_signature(
    op_name: alloc::string::String,
    call: &dyn CallOpInterface,
) -> Result<Signature, Report> {
    let callee = call.callable_for_callee();
    let Some(resolved) = call.resolve() else {
        return Err(Report::msg(format!(
            "operation '{op_name}' references unresolved callee '{callee}'"
        )));
    };
    let Some(callable) = resolved.as_trait_ref::<dyn CallableOpInterface>() else {
        return Err(Report::msg(format!(
            "operation '{op_name}' resolved callee '{callee}', but it is not callable"
        )));
    };
    Ok(callable.borrow().signature())
}

fn region_successor_inputs(
    successor: &RegionSuccessorInfo,
) -> Result<SmallVec<[ValueRef; 4]>, Report> {
    match successor {
        RegionSuccessorInfo::Entering(region) => {
            let region = region.borrow();
            let Some(entry) = region.entry_block_ref() else {
                return Err(Report::msg(
                    "region branch successor points to an empty region with no entry block",
                ));
            };
            Ok(entry
                .borrow()
                .arguments()
                .iter()
                .map(|arg| arg.borrow().as_value_ref())
                .collect())
        }
        RegionSuccessorInfo::Returning(values) => Ok(values.iter().copied().collect()),
    }
}

fn verify_successor_operands_against_inputs(
    op_name: alloc::string::String,
    edge: alloc::string::String,
    operands: &impl SuccessorOperands,
    inputs: impl IntoIterator<Item = ValueRef>,
    are_types_compatible: impl Fn(&Type, &Type) -> bool,
) -> Result<(), Report> {
    let inputs = inputs.into_iter().collect::<SmallVec<[ValueRef; 4]>>();
    if operands.len() != inputs.len() {
        return Err(Report::msg(format!(
            "operation '{op_name}' has {} operands for {edge}, but the destination expects {}",
            operands.len(),
            inputs.len()
        )));
    }

    let forwarded = operands.forwarded();
    for (index, input) in inputs.into_iter().enumerate() {
        let successor_operand = if index < operands.num_produced() {
            SuccessorOperand::Produced
        } else {
            let forwarded_index = index - operands.num_produced();
            let Some(operand) = forwarded.get(forwarded_index) else {
                return Err(Report::msg(format!(
                    "operation '{op_name}' is missing forwarded operand {forwarded_index} for \
                     {edge}"
                )));
            };
            SuccessorOperand::Forwarded(operand.borrow().as_value_ref())
        };

        let SuccessorOperand::Forwarded(value) = successor_operand else {
            continue;
        };
        let value_ty = value.borrow().ty().clone();
        let input_ty = input.borrow().ty().clone();
        if !are_types_compatible(&value_ty, &input_ty) {
            return Err(Report::msg(format!(
                "operation '{op_name}' forwards operand {index} of type '{value_ty}' to {edge}, \
                 but the destination expects '{input_ty}'"
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use alloc::{format, vec};

    use super::*;
    use crate::{
        AttributeRef, BlockRef, Builder, BuilderExt, Context, Op, RegionBranchPoint, RegionKind,
        RegionKindInterface, RegionSuccessorIter, SourceSpan, SuccessorOperandRange,
        SuccessorOperandRangeMut, Type,
        derive::{EffectOpInterface, operation},
        dialects::{builtin::attributes::Signature, test::TestDialect},
        effects::MemoryEffectOpInterface,
        smallvec,
        testing::Test,
        traits::{AnyType, Terminator},
    };

    #[derive(EffectOpInterface)]
    #[operation(
        dialect = TestDialect,
        traits(Terminator),
        implements(BranchOpInterface, MemoryEffectOpInterface)
    )]
    pub struct SignatureTestBr {
        #[successor]
        target: Successor,
    }

    impl BranchOpInterface for SignatureTestBr {}

    #[derive(EffectOpInterface)]
    #[operation(
        dialect = TestDialect,
        implements(RegionBranchOpInterface, RegionKindInterface, MemoryEffectOpInterface)
    )]
    pub struct SignatureTestRegionBranch {
        #[region]
        body: Region,
    }

    impl RegionBranchOpInterface for SignatureTestRegionBranch {
        fn get_successor_regions(&self, point: RegionBranchPoint) -> RegionSuccessorIter<'_> {
            match point {
                RegionBranchPoint::Parent => RegionSuccessorIter::new(
                    self.as_operation(),
                    [RegionSuccessorInfo::Entering(self.body().as_region_ref())],
                ),
                RegionBranchPoint::Child(_) => RegionSuccessorIter::new(
                    self.as_operation(),
                    [RegionSuccessorInfo::Returning(SmallVec::new())],
                ),
            }
        }
    }

    impl RegionKindInterface for SignatureTestRegionBranch {
        fn kind(&self) -> RegionKind {
            RegionKind::SSA
        }
    }

    #[derive(EffectOpInterface)]
    #[operation(
        dialect = TestDialect,
        traits(Terminator),
        implements(RegionBranchTerminatorOpInterface, MemoryEffectOpInterface)
    )]
    pub struct SignatureTestRegionYield {
        #[operands]
        yielded: AnyType,
    }

    impl RegionBranchTerminatorOpInterface for SignatureTestRegionYield {
        fn get_successor_operands(&self, _point: RegionBranchPoint) -> SuccessorOperandRange<'_> {
            SuccessorOperandRange::forward(self.yielded())
        }

        fn get_mutable_successor_operands(
            &mut self,
            _point: RegionBranchPoint,
        ) -> SuccessorOperandRangeMut<'_> {
            SuccessorOperandRangeMut::forward(self.yielded_mut())
        }

        fn get_successor_regions(
            &self,
            _operands: &[Option<AttributeRef>],
        ) -> SmallVec<[RegionSuccessorInfo; 2]> {
            smallvec![RegionSuccessorInfo::Returning(SmallVec::new())]
        }
    }

    fn u32_to_i32_converter() -> TypeConverter {
        let mut converter = TypeConverter::new();
        converter.add_conversion(|ty| {
            if ty == &Type::U32 {
                Some(super::super::TypeConversion::One(Type::I32))
            } else {
                None
            }
        });
        converter
    }

    #[test]
    fn applies_1_to_1_block_argument_conversion_in_place() {
        let test = Test::new(
            "applies_1_to_1_block_argument_conversion_in_place",
            &[Type::U32, Type::I32],
            &[],
        );
        let block = test.entry_block();

        let conversion = SignatureConversion::for_block_1_to_1(block, &u32_to_i32_converter())
            .expect("signature conversion failed");

        assert!(conversion.apply_to_block_arguments_1_to_1(block).unwrap());
        let types = block.borrow().argument_types().collect::<Vec<_>>();
        assert_eq!(types, vec![Type::I32, Type::I32]);
    }

    #[test]
    fn applies_1_to_1_non_entry_block_argument_conversion_in_place() {
        let mut test =
            Test::new("applies_1_to_1_non_entry_block_argument_conversion_in_place", &[], &[]);
        let block = {
            let mut builder = test.function_builder();
            let block = builder.create_block();
            builder.append_block_param(block, Type::U32, SourceSpan::UNKNOWN);
            block
        };

        let conversion = SignatureConversion::for_block_1_to_1(block, &u32_to_i32_converter())
            .expect("signature conversion failed");

        assert!(conversion.apply_to_block_arguments_1_to_1(block).unwrap());
        let types = block.borrow().argument_types().collect::<Vec<_>>();
        assert_eq!(types, vec![Type::I32]);
    }

    #[test]
    fn rejects_non_1_to_1_block_argument_conversion() {
        let test = Test::new("rejects_non_1_to_1_block_argument_conversion", &[Type::U32], &[]);
        let block = test.entry_block();
        let mut converter = TypeConverter::new();
        converter.add_conversion(|ty| {
            if ty == &Type::U32 {
                Some(super::super::TypeConversion::Drop)
            } else {
                None
            }
        });

        let err = SignatureConversion::for_block_1_to_1(block, &converter).unwrap_err();
        assert!(format!("{err}").contains("1:1"));
    }

    #[test]
    fn converts_builtin_signature_1_to_1() {
        let context = alloc::rc::Rc::new(Context::default());
        let signature = Signature::new(&context, [Type::U32], [Type::U32]);

        let converted = convert_signature_1_to_1(&signature, &u32_to_i32_converter()).unwrap();

        assert_eq!(converted.params()[0].ty, Type::I32);
        assert_eq!(converted.results()[0].ty, Type::I32);
        assert_eq!(converted.calling_convention(), signature.calling_convention());
    }

    #[test]
    fn converts_callable_signature_1_to_1() {
        let test = Test::new("converts_callable_signature_1_to_1", &[Type::U32], &[Type::U32]);

        let converted = converted_callable_signature_1_to_1(
            test.function().as_operation_ref(),
            &u32_to_i32_converter(),
        )
        .unwrap()
        .expect("function should implement CallableOpInterface");

        assert_eq!(converted.params()[0].ty, Type::I32);
        assert_eq!(converted.results()[0].ty, Type::I32);
    }

    #[test]
    fn reports_branch_successor_operand_arity_mismatch() {
        let mut test =
            Test::new("reports_branch_successor_operand_arity_mismatch", &[Type::U32], &[]);
        let op = {
            let mut builder = test.function_builder();
            let dest = builder.create_block();
            builder.append_block_param(dest, Type::U32, SourceSpan::UNKNOWN);
            let op_builder = builder
                .builder_mut()
                .create::<SignatureTestBr, (BlockRef, Vec<ValueRef>)>(SourceSpan::UNKNOWN);
            op_builder(dest, Vec::new()).unwrap()
        };

        let err = verify_branch_successor_operand_arities(op.as_operation_ref()).unwrap_err();
        assert!(format!("{err}").contains("destination expects 1"));
    }

    #[test]
    fn accepts_matching_branch_successor_operands() {
        let mut test = Test::new("accepts_matching_branch_successor_operands", &[Type::U32], &[]);
        let op = {
            let mut builder = test.function_builder();
            let entry = builder.entry_block();
            let arg = entry.borrow().arguments()[0] as ValueRef;
            let dest: BlockRef = builder.create_block();
            builder.append_block_param(dest, Type::U32, SourceSpan::UNKNOWN);
            let op_builder = builder
                .builder_mut()
                .create::<SignatureTestBr, (BlockRef, [ValueRef; 1])>(SourceSpan::UNKNOWN);
            op_builder(dest, [arg]).unwrap()
        };

        verify_branch_successor_operand_arities(op.as_operation_ref()).unwrap();
    }

    #[test]
    fn reports_region_branch_entry_successor_arity_mismatch() {
        let mut test = Test::new("reports_region_branch_entry_successor_arity_mismatch", &[], &[]);
        let op = {
            let mut builder = test.function_builder();
            let op_builder = builder
                .builder_mut()
                .create::<SignatureTestRegionBranch, ()>(SourceSpan::UNKNOWN);
            let op = op_builder().unwrap();
            let region = op.borrow().body().as_region_ref();
            builder.builder_mut().create_block(region, None, &[Type::U32]);
            op
        };

        let err =
            verify_region_branch_successor_operand_arities(op.as_operation_ref()).unwrap_err();
        assert!(format!("{err}").contains("destination expects 1"));
    }

    #[test]
    fn reports_region_branch_terminator_successor_arity_mismatch() {
        let mut test = Test::new(
            "reports_region_branch_terminator_successor_arity_mismatch",
            &[Type::U32],
            &[],
        );
        let terminator = {
            let mut builder = test.function_builder();
            let entry = builder.entry_block();
            let arg = entry.borrow().arguments()[0] as ValueRef;
            let op_builder = builder
                .builder_mut()
                .create::<SignatureTestRegionBranch, ()>(SourceSpan::UNKNOWN);
            let op = op_builder().unwrap();
            let region = op.borrow().body().as_region_ref();
            let body = builder.builder_mut().create_block(region, None, &[]);
            builder.builder_mut().set_insertion_point_to_end(body);
            let yield_builder = builder
                .builder_mut()
                .create::<SignatureTestRegionYield, ([ValueRef; 1],)>(SourceSpan::UNKNOWN);
            yield_builder([arg]).unwrap()
        };

        let err = verify_region_branch_terminator_successor_operand_arities(
            terminator.as_operation_ref(),
        )
        .unwrap_err();
        assert!(format!("{err}").contains("destination expects 0"));
    }
}
