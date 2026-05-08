use std::{collections::BTreeMap, rc::Rc};

use miden_assembly_syntax::{
    Felt, Path as MasmPath,
    ast::{Block, Immediate, Instruction, InvocationTarget, Module, Op, Procedure},
    debuginfo::{SourceSpan, Spanned},
    parser::{IntValue, PushValue},
};
use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_dialect_scf::StructuredControlFlowOpBuilder;
use midenc_hir::{
    AsSymbolRef, BlockRef, Builder, Context, Ident, Op as HirOp, OpBuilder, OperationRef,
    ProgramPoint, SymbolTable, Type, ValueRef, Visibility,
    dialects::builtin::{
        BuiltinOpBuilder, FunctionBuilder, FunctionRef,
        attributes::{LocalVariable, Signature},
    },
};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    DisassembledModule, DisassemblerConfig, ExternalSignatureMap, Result, error, infer, signatures,
};

pub(crate) fn lift_module(
    module: &Module,
    config: &DisassemblerConfig,
    external_signatures: &ExternalSignatureMap,
    context: Rc<Context>,
) -> Result<DisassembledModule> {
    let mut builder = OpBuilder::new(context.clone());
    let mut hir_module = builder.create_module(Ident::with_empty_span(
        midenc_hir::interner::Symbol::intern(module.name()),
    ))?;
    ensure_op_region(&context, &mut *hir_module.borrow_mut());
    let body = hir_module.borrow().body().as_region_ref();
    let body_block = builder.create_block(body, None, &[]);
    builder.set_insertion_point_to_end(body_block);

    let mut signatures = FxHashMap::<String, Signature>::default();
    for procedure in module.procedures() {
        let Some(signature) = procedure.signature() else {
            continue;
        };
        let signature = signatures::convert_signature(&context, module, signature)?;
        signatures.insert(procedure.name().as_str().to_owned(), signature);
    }
    let external_signatures = convert_external_signatures(&context, external_signatures)?;

    if !config.infer_missing_signatures {
        if let Some(procedure) = module
            .procedures()
            .find(|procedure| !signatures.contains_key(procedure.name().as_str()))
        {
            return Err(error::error(format!(
                "procedure '{}' is missing a signature",
                procedure.name()
            )));
        }
    }

    reject_recursive_calls(module)?;

    if config.infer_missing_signatures {
        for name in callee_first_order(module)? {
            if signatures.contains_key(name.as_str()) {
                continue;
            }
            let procedure = module
                .procedures()
                .find(|procedure| procedure.name().as_str() == name)
                .expect("callee-first order must contain local procedures only");
            let signature =
                infer::infer_signature(&context, procedure, &signatures, &external_signatures)?;
            signatures.insert(name, signature);
        }
    }

    let mut external_functions = FxHashMap::<String, FunctionRef>::default();
    for (index, (path, signature)) in external_signatures.iter().enumerate() {
        let function = builder.create_function(
            Ident::with_empty_span(midenc_hir::interner::Symbol::intern(&external_symbol_name(
                index, path,
            ))),
            Visibility::Public,
            signature.clone(),
        )?;
        hir_module
            .borrow_mut()
            .insert_new(function.as_symbol_ref(), ProgramPoint::default());
        external_functions.insert(path.clone(), function);
    }

    let mut functions = FxHashMap::<String, FunctionRef>::default();
    for procedure in module.procedures() {
        let signature = signatures.get(procedure.name().as_str()).cloned().ok_or_else(|| {
            error::error(format!("procedure '{}' is missing a signature", procedure.name()))
        })?;
        let visibility = if procedure.visibility().is_public() {
            Visibility::Public
        } else {
            Visibility::Private
        };
        let mut function = builder.create_function(
            Ident::with_empty_span(midenc_hir::interner::Symbol::intern(procedure.name().as_str())),
            visibility,
            signature.clone(),
        )?;
        ensure_op_region(&context, &mut *function.borrow_mut());
        hir_module
            .borrow_mut()
            .insert_new(function.as_symbol_ref(), ProgramPoint::default());
        functions.insert(procedure.name().as_str().to_owned(), function);
    }

    for procedure in module.procedures() {
        let function = *functions.get(procedure.name().as_str()).unwrap();
        let signature = signatures.get(procedure.name().as_str()).unwrap().clone();
        let mut function_builder = FunctionBuilder::new(function, &mut builder);
        let mut lifter =
            ProcedureLifter::new(procedure, signature, &functions, &external_functions);
        lifter.lift(&mut function_builder)?;
    }

    Ok(DisassembledModule {
        context,
        module: hir_module,
    })
}

fn convert_external_signatures(
    context: &Rc<Context>,
    external_signatures: &ExternalSignatureMap,
) -> Result<FxHashMap<String, Signature>> {
    external_signatures
        .iter()
        .map(|(path, signature)| {
            let path = normalize_external_path(path)?;
            let signature = signatures::convert_hir_function_type(context, signature);
            Ok((path, signature))
        })
        .collect()
}

fn normalize_external_path(path: &str) -> Result<String> {
    let path = path
        .parse::<miden_assembly_syntax::PathBuf>()
        .map_err(|err| error::error(format!("invalid external MASM path '{path}': {err}")))?;
    Ok(path.as_path().to_absolute().to_string())
}

fn invocation_path_key(path: &MasmPath) -> String {
    path.to_absolute().to_string()
}

fn external_symbol_name(index: usize, path: &str) -> String {
    let mut name = format!("__masm_external_{index}");
    for ch in path.chars() {
        if ch.is_ascii_alphanumeric() {
            name.push(ch);
        } else {
            name.push('_');
        }
    }
    name
}

fn ensure_op_region(context: &Rc<Context>, op: &mut dyn HirOp) {
    if op.num_regions() == 0 {
        let region = context.create_region();
        op.as_operation_mut().regions_mut().push_back(region);
    }
}

fn reject_recursive_calls(module: &Module) -> Result<()> {
    let graph = local_call_graph(module);
    let mut states = FxHashMap::<String, VisitState>::default();
    let mut stack = Vec::<String>::new();
    for name in graph.keys() {
        reject_recursive_calls_from(name, &graph, &mut states, &mut stack)?;
    }
    Ok(())
}

fn local_call_graph(module: &Module) -> FxHashMap<String, Vec<String>> {
    let local_names: FxHashSet<_> = module
        .procedures()
        .map(|procedure| procedure.name().as_str().to_owned())
        .collect();
    let mut graph = FxHashMap::<String, Vec<String>>::default();

    for procedure in module.procedures() {
        let mut callees = Vec::new();
        for target in procedure.invoked() {
            let InvocationTarget::Symbol(name) = &target.target else {
                continue;
            };
            if local_names.contains(name.as_str()) {
                callees.push(name.as_str().to_owned());
            }
        }
        graph.insert(procedure.name().as_str().to_owned(), callees);
    }

    graph
}

fn reject_recursive_calls_from(
    name: &str,
    graph: &FxHashMap<String, Vec<String>>,
    states: &mut FxHashMap<String, VisitState>,
    stack: &mut Vec<String>,
) -> Result<()> {
    match states.get(name).copied() {
        Some(VisitState::Done) => return Ok(()),
        Some(VisitState::Visiting) => {
            let cycle_start = stack.iter().position(|entry| entry == name).unwrap_or(0);
            let mut cycle = stack[cycle_start..].to_vec();
            cycle.push(name.to_owned());
            return Err(error::error(format!(
                "recursive MASM procedure calls are not supported: {}",
                cycle.join(" -> ")
            )));
        }
        None => (),
    }

    states.insert(name.to_owned(), VisitState::Visiting);
    stack.push(name.to_owned());
    if let Some(callees) = graph.get(name) {
        for callee in callees {
            reject_recursive_calls_from(callee, graph, states, stack)?;
        }
    }
    stack.pop();
    states.insert(name.to_owned(), VisitState::Done);
    Ok(())
}

fn callee_first_order(module: &Module) -> Result<Vec<String>> {
    let graph = local_call_graph(module);
    let mut states = FxHashMap::<String, VisitState>::default();
    let mut stack = Vec::<String>::new();
    let mut order = Vec::<String>::new();
    for procedure in module.procedures() {
        callee_first_order_from(
            procedure.name().as_str(),
            &graph,
            &mut states,
            &mut stack,
            &mut order,
        )?;
    }
    Ok(order)
}

fn callee_first_order_from(
    name: &str,
    graph: &FxHashMap<String, Vec<String>>,
    states: &mut FxHashMap<String, VisitState>,
    stack: &mut Vec<String>,
    order: &mut Vec<String>,
) -> Result<()> {
    match states.get(name).copied() {
        Some(VisitState::Done) => return Ok(()),
        Some(VisitState::Visiting) => {
            let cycle_start = stack.iter().position(|entry| entry == name).unwrap_or(0);
            let mut cycle = stack[cycle_start..].to_vec();
            cycle.push(name.to_owned());
            return Err(error::error(format!(
                "recursive MASM procedure calls are not supported: {}",
                cycle.join(" -> ")
            )));
        }
        None => (),
    }

    states.insert(name.to_owned(), VisitState::Visiting);
    stack.push(name.to_owned());
    if let Some(callees) = graph.get(name) {
        for callee in callees {
            callee_first_order_from(callee, graph, states, stack, order)?;
        }
    }
    stack.pop();
    states.insert(name.to_owned(), VisitState::Done);
    order.push(name.to_owned());
    Ok(())
}

#[derive(Clone, Copy)]
enum VisitState {
    Visiting,
    Done,
}

#[derive(Clone, Copy)]
struct StackValue {
    value: ValueRef,
    #[allow(dead_code)]
    span: SourceSpan,
}

struct ProcedureLifter<'a> {
    procedure: &'a Procedure,
    signature: Signature,
    functions: &'a FxHashMap<String, FunctionRef>,
    external_functions: &'a FxHashMap<String, FunctionRef>,
    locals: BTreeMap<u16, LocalVariable>,
    stack: Vec<StackValue>,
}

impl<'a> ProcedureLifter<'a> {
    fn new(
        procedure: &'a Procedure,
        signature: Signature,
        functions: &'a FxHashMap<String, FunctionRef>,
        external_functions: &'a FxHashMap<String, FunctionRef>,
    ) -> Self {
        Self {
            procedure,
            signature,
            functions,
            external_functions,
            locals: BTreeMap::new(),
            stack: Vec::new(),
        }
    }

    fn lift(&mut self, builder: &mut FunctionBuilder<'_, OpBuilder>) -> Result<()> {
        self.initialize_locals(builder);
        self.initialize_stack(builder);
        self.lift_block(self.procedure.body(), builder)?;
        let results = self.pop_results(builder, self.procedure.span())?;
        if !self.stack.is_empty() {
            return Err(error::error(format!(
                "procedure '{}' leaves {} extra value(s) on the stack",
                self.procedure.name(),
                self.stack.len()
            )));
        }
        builder.ret(results, self.procedure.span())?;
        Ok(())
    }

    fn initialize_locals(&mut self, builder: &mut FunctionBuilder<'_, OpBuilder>) {
        for id in 0..self.procedure.num_locals() {
            let local = builder.alloc_local(Type::Felt);
            self.locals.insert(id, local);
        }
    }

    fn initialize_stack(&mut self, builder: &mut FunctionBuilder<'_, OpBuilder>) {
        self.stack = builder
            .entry_block()
            .borrow()
            .arguments()
            .iter()
            .rev()
            .map(|arg| StackValue {
                value: *arg as ValueRef,
                span: arg.borrow().span(),
            })
            .collect();
    }

    fn lift_block(
        &mut self,
        block: &Block,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
    ) -> Result<()> {
        for op in block.iter() {
            match op {
                Op::Inst(inst) => self.lift_instruction(inst.inner(), inst.span(), builder)?,
                Op::If {
                    span,
                    then_blk,
                    else_blk,
                } => self.lift_if(then_blk, else_blk, *span, builder)?,
                Op::While { span, body } => self.lift_while(body, *span, builder)?,
                Op::Repeat { count, body, .. } => {
                    let count = immediate_u32(count)?;
                    for _ in 0..count {
                        self.lift_block(body, builder)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn lift_instruction(
        &mut self,
        inst: &Instruction,
        span: SourceSpan,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
    ) -> Result<()> {
        use Instruction::*;

        match inst {
            Nop => Ok(()),
            Drop => self.drop_n(1, span),
            DropW => self.drop_n(4, span),
            PadW => {
                for _ in 0..4 {
                    self.push_value(builder.felt(Felt::ZERO, span), span);
                }
                Ok(())
            }
            Dup0 => self.dup(0, span),
            Dup1 => self.dup(1, span),
            Dup2 => self.dup(2, span),
            Dup3 => self.dup(3, span),
            Dup4 => self.dup(4, span),
            Dup5 => self.dup(5, span),
            Dup6 => self.dup(6, span),
            Dup7 => self.dup(7, span),
            Dup8 => self.dup(8, span),
            Dup9 => self.dup(9, span),
            Dup10 => self.dup(10, span),
            Dup11 => self.dup(11, span),
            Dup12 => self.dup(12, span),
            Dup13 => self.dup(13, span),
            Dup14 => self.dup(14, span),
            Dup15 => self.dup(15, span),
            DupW0 => self.dup_word(0, span),
            DupW1 => self.dup_word(1, span),
            DupW2 => self.dup_word(2, span),
            DupW3 => self.dup_word(3, span),
            Swap1 => self.swap(1, span),
            Swap2 => self.swap(2, span),
            Swap3 => self.swap(3, span),
            Swap4 => self.swap(4, span),
            Swap5 => self.swap(5, span),
            Swap6 => self.swap(6, span),
            Swap7 => self.swap(7, span),
            Swap8 => self.swap(8, span),
            Swap9 => self.swap(9, span),
            Swap10 => self.swap(10, span),
            Swap11 => self.swap(11, span),
            Swap12 => self.swap(12, span),
            Swap13 => self.swap(13, span),
            Swap14 => self.swap(14, span),
            Swap15 => self.swap(15, span),
            SwapW1 => self.swap_word(1, span),
            SwapW2 => self.swap_word(2, span),
            SwapW3 => self.swap_word(3, span),
            SwapDw => self.swap_double_word(span),
            MovUp2 => self.movup(2, span),
            MovUp3 => self.movup(3, span),
            MovUp4 => self.movup(4, span),
            MovUp5 => self.movup(5, span),
            MovUp6 => self.movup(6, span),
            MovUp7 => self.movup(7, span),
            MovUp8 => self.movup(8, span),
            MovUp9 => self.movup(9, span),
            MovUp10 => self.movup(10, span),
            MovUp11 => self.movup(11, span),
            MovUp12 => self.movup(12, span),
            MovUp13 => self.movup(13, span),
            MovUp14 => self.movup(14, span),
            MovUp15 => self.movup(15, span),
            MovUpW2 => self.movup_word(2, span),
            MovUpW3 => self.movup_word(3, span),
            MovDn2 => self.movdn(2, span),
            MovDn3 => self.movdn(3, span),
            MovDn4 => self.movdn(4, span),
            MovDn5 => self.movdn(5, span),
            MovDn6 => self.movdn(6, span),
            MovDn7 => self.movdn(7, span),
            MovDn8 => self.movdn(8, span),
            MovDn9 => self.movdn(9, span),
            MovDn10 => self.movdn(10, span),
            MovDn11 => self.movdn(11, span),
            MovDn12 => self.movdn(12, span),
            MovDn13 => self.movdn(13, span),
            MovDn14 => self.movdn(14, span),
            MovDn15 => self.movdn(15, span),
            MovDnW2 => self.movdn_word(2, span),
            MovDnW3 => self.movdn_word(3, span),
            Reversew => self.reverse_word(span),
            Reversedw => self.reverse_double_word(span),
            Push(value) => self.push_immediate(immediate_value(value)?, span, builder),
            PushSlice(value, range) => {
                self.push_word_slice(immediate_value(value)?, range, span, builder)
            }
            PushFeltList(values) => {
                for value in values {
                    self.push_value(builder.felt(*value, span), span);
                }
                Ok(())
            }
            U32WrappingAdd => {
                self.binary_with_type(builder, Type::U32, span, |builder, lhs, rhs, span| {
                    builder.add_wrapping(lhs, rhs, span)
                })
            }
            U32WrappingAddImm(value) => {
                self.u32_binary_imm(builder, value, span, |builder, lhs, rhs, span| {
                    builder.add_wrapping(lhs, rhs, span)
                })
            }
            U32OverflowingAdd => {
                self.u32_overflowing_binary(builder, span, |builder, lhs, rhs, span| {
                    builder.add_overflowing(lhs, rhs, span)
                })
            }
            U32OverflowingAddImm(value) => {
                self.u32_overflowing_binary_imm(builder, value, span, |builder, lhs, rhs, span| {
                    builder.add_overflowing(lhs, rhs, span)
                })
            }
            U32WrappingSub => {
                self.binary_with_type(builder, Type::U32, span, |builder, lhs, rhs, span| {
                    builder.sub_wrapping(lhs, rhs, span)
                })
            }
            U32WrappingSubImm(value) => {
                self.u32_binary_imm(builder, value, span, |builder, lhs, rhs, span| {
                    builder.sub_wrapping(lhs, rhs, span)
                })
            }
            U32OverflowingSub => {
                self.u32_overflowing_binary(builder, span, |builder, lhs, rhs, span| {
                    builder.sub_overflowing(lhs, rhs, span)
                })
            }
            U32OverflowingSubImm(value) => {
                self.u32_overflowing_binary_imm(builder, value, span, |builder, lhs, rhs, span| {
                    builder.sub_overflowing(lhs, rhs, span)
                })
            }
            U32WrappingMul => {
                self.binary_with_type(builder, Type::U32, span, |builder, lhs, rhs, span| {
                    builder.mul_wrapping(lhs, rhs, span)
                })
            }
            U32WrappingMulImm(value) => {
                self.u32_binary_imm(builder, value, span, |builder, lhs, rhs, span| {
                    builder.mul_wrapping(lhs, rhs, span)
                })
            }
            U32Div => self.binary_with_type(builder, Type::U32, span, |builder, lhs, rhs, span| {
                builder.div(lhs, rhs, span)
            }),
            U32DivImm(value) => {
                self.u32_binary_imm(builder, value, span, |builder, lhs, rhs, span| {
                    builder.div(lhs, rhs, span)
                })
            }
            U32Mod => self.binary_with_type(builder, Type::U32, span, |builder, lhs, rhs, span| {
                builder.r#mod(lhs, rhs, span)
            }),
            U32ModImm(value) => {
                self.u32_binary_imm(builder, value, span, |builder, lhs, rhs, span| {
                    builder.r#mod(lhs, rhs, span)
                })
            }
            U32DivMod => {
                let (lhs, rhs) = self.pop_binary(span)?;
                let lhs = self.cast(builder, lhs.value, Type::U32, span)?;
                let rhs = self.cast(builder, rhs.value, Type::U32, span)?;
                let (quotient, remainder) = builder.divmod(lhs, rhs, span)?;
                self.push_value(quotient, span);
                self.push_value(remainder, span);
                Ok(())
            }
            U32DivModImm(value) => {
                let lhs = self.pop(span)?;
                let lhs = self.cast(builder, lhs.value, Type::U32, span)?;
                let rhs = builder.u32(immediate_value(value)?, span);
                let (quotient, remainder) = builder.divmod(lhs, rhs, span)?;
                self.push_value(quotient, span);
                self.push_value(remainder, span);
                Ok(())
            }
            U32And => self.binary_with_type(builder, Type::U32, span, |builder, lhs, rhs, span| {
                builder.band(lhs, rhs, span)
            }),
            U32Or => self.binary_with_type(builder, Type::U32, span, |builder, lhs, rhs, span| {
                builder.bor(lhs, rhs, span)
            }),
            U32Xor => self.binary_with_type(builder, Type::U32, span, |builder, lhs, rhs, span| {
                builder.bxor(lhs, rhs, span)
            }),
            U32Not => self.unary_with_type(builder, Type::U32, span, |builder, value, span| {
                builder.bnot(value, span)
            }),
            U32Shr => self.binary_with_type(builder, Type::U32, span, |builder, lhs, rhs, span| {
                builder.shr(lhs, rhs, span)
            }),
            U32ShrImm(value) => self.u32_binary_const(
                builder,
                immediate_value(value)? as u32,
                span,
                |builder, lhs, rhs, span| builder.shr(lhs, rhs, span),
            ),
            U32Shl => self.binary_with_type(builder, Type::U32, span, |builder, lhs, rhs, span| {
                builder.shl(lhs, rhs, span)
            }),
            U32ShlImm(value) => self.u32_binary_const(
                builder,
                immediate_value(value)? as u32,
                span,
                |builder, lhs, rhs, span| builder.shl(lhs, rhs, span),
            ),
            U32Rotr => {
                self.binary_with_type(builder, Type::U32, span, |builder, lhs, rhs, span| {
                    builder.rotr(lhs, rhs, span)
                })
            }
            U32RotrImm(value) => self.u32_binary_const(
                builder,
                immediate_value(value)? as u32,
                span,
                |builder, lhs, rhs, span| builder.rotr(lhs, rhs, span),
            ),
            U32Rotl => {
                self.binary_with_type(builder, Type::U32, span, |builder, lhs, rhs, span| {
                    builder.rotl(lhs, rhs, span)
                })
            }
            U32RotlImm(value) => self.u32_binary_const(
                builder,
                immediate_value(value)? as u32,
                span,
                |builder, lhs, rhs, span| builder.rotl(lhs, rhs, span),
            ),
            U32Lt => self.binary_with_type(builder, Type::U32, span, |builder, lhs, rhs, span| {
                builder.lt(lhs, rhs, span)
            }),
            U32Lte => self.binary_with_type(builder, Type::U32, span, |builder, lhs, rhs, span| {
                builder.lte(lhs, rhs, span)
            }),
            U32Gt => self.binary_with_type(builder, Type::U32, span, |builder, lhs, rhs, span| {
                builder.gt(lhs, rhs, span)
            }),
            U32Gte => self.binary_with_type(builder, Type::U32, span, |builder, lhs, rhs, span| {
                builder.gte(lhs, rhs, span)
            }),
            U32Min => self.binary_with_type(builder, Type::U32, span, |builder, lhs, rhs, span| {
                builder.min(lhs, rhs, span)
            }),
            U32Max => self.binary_with_type(builder, Type::U32, span, |builder, lhs, rhs, span| {
                builder.max(lhs, rhs, span)
            }),
            U32Popcnt => self.unary_with_type(builder, Type::U32, span, |builder, value, span| {
                builder.popcnt(value, span)
            }),
            U32Ctz => self.unary_with_type(builder, Type::U32, span, |builder, value, span| {
                builder.ctz(value, span)
            }),
            U32Clz => self.unary_with_type(builder, Type::U32, span, |builder, value, span| {
                builder.clz(value, span)
            }),
            U32Clo => self.unary_with_type(builder, Type::U32, span, |builder, value, span| {
                builder.clo(value, span)
            }),
            U32Cto => self.unary_with_type(builder, Type::U32, span, |builder, value, span| {
                builder.cto(value, span)
            }),
            U32Cast | U32Assert => self.u32_assert_n(1, span, builder),
            U32AssertWithError(_) => unsupported_instruction(inst, span),
            U32Assert2 => self.u32_assert_n(2, span, builder),
            U32Assert2WithError(_) => unsupported_instruction(inst, span),
            U32AssertW => self.u32_assert_n(4, span, builder),
            U32AssertWWithError(_) => unsupported_instruction(inst, span),
            U32Test => self.u32_test(span, builder),
            U32TestW => self.u32_testw(span, builder),
            U32Split => self.u32_split(span, builder),
            CSwap => self.conditional_swap(1, span, builder),
            CSwapW => self.conditional_swap(4, span, builder),
            CDrop => self.conditional_drop(1, span, builder),
            CDropW => self.conditional_drop(4, span, builder),
            Assert => {
                let value = self.pop(span)?;
                builder.assert(value.value, span)?;
                Ok(())
            }
            AssertWithError(_) => unsupported_instruction(inst, span),
            Assertz => {
                let value = self.pop(span)?;
                builder.assertz(value.value, span)?;
                Ok(())
            }
            AssertzWithError(_) => unsupported_instruction(inst, span),
            AssertEq => {
                let (lhs, rhs) = self.pop_binary(span)?;
                builder.assert_eq(lhs.value, rhs.value, span)?;
                Ok(())
            }
            AssertEqWithError(_) => unsupported_instruction(inst, span),
            AssertEqw => self.assert_eq_word(span, builder),
            AssertEqwWithError(_) => unsupported_instruction(inst, span),
            LocLoad(id) => {
                let local = self.local(immediate_value(id)?, span)?;
                let value = builder.load_local(local, span)?;
                self.push_value(value, span);
                Ok(())
            }
            LocStore(id) => {
                let local = self.local(immediate_value(id)?, span)?;
                let value = self.pop(span)?;
                let value = self.cast(builder, value.value, local.ty(), span)?;
                builder.store_local(local, value, span)?;
                Ok(())
            }
            Exec(target) => self.invoke(builder, target, span, InvokeKind::Exec),
            Call(target) => self.invoke(builder, target, span, InvokeKind::Call),
            SysCall(target) => self.invoke(builder, target, span, InvokeKind::Syscall),
            Add => self.binary_with_type(builder, Type::Felt, span, |builder, lhs, rhs, span| {
                builder.add(lhs, rhs, span)
            }),
            AddImm(value) => {
                self.felt_binary_imm(builder, value, span, |builder, lhs, rhs, span| {
                    builder.add(lhs, rhs, span)
                })
            }
            Sub => self.binary_with_type(builder, Type::Felt, span, |builder, lhs, rhs, span| {
                builder.sub(lhs, rhs, span)
            }),
            SubImm(value) => {
                self.felt_binary_imm(builder, value, span, |builder, lhs, rhs, span| {
                    builder.sub(lhs, rhs, span)
                })
            }
            Mul => self.binary_with_type(builder, Type::Felt, span, |builder, lhs, rhs, span| {
                builder.mul(lhs, rhs, span)
            }),
            MulImm(value) => {
                self.felt_binary_imm(builder, value, span, |builder, lhs, rhs, span| {
                    builder.mul(lhs, rhs, span)
                })
            }
            Div => self.binary_with_type(builder, Type::Felt, span, |builder, lhs, rhs, span| {
                builder.div(lhs, rhs, span)
            }),
            DivImm(value) => {
                self.felt_binary_imm(builder, value, span, |builder, lhs, rhs, span| {
                    builder.div(lhs, rhs, span)
                })
            }
            Neg => self.unary_with_type(builder, Type::Felt, span, |builder, value, span| {
                builder.neg(value, span)
            }),
            ILog2 => self.unary_with_type(builder, Type::Felt, span, |builder, value, span| {
                builder.ilog2(value, span)
            }),
            Inv => self.unary_with_type(builder, Type::Felt, span, |builder, value, span| {
                builder.inv(value, span)
            }),
            Incr => self.unary_with_type(builder, Type::Felt, span, |builder, value, span| {
                builder.incr(value, span)
            }),
            Pow2 => self.unary_with_type(builder, Type::Felt, span, |builder, value, span| {
                builder.pow2(value, span)
            }),
            Exp | ExpBitLength(_) => {
                self.binary_with_type(builder, Type::Felt, span, |builder, lhs, rhs, span| {
                    builder.exp(lhs, rhs, span)
                })
            }
            ExpImm(value) => {
                self.felt_binary_imm(builder, value, span, |builder, lhs, rhs, span| {
                    builder.exp(lhs, rhs, span)
                })
            }
            Not => self.unary_with_type(builder, Type::I1, span, |builder, value, span| {
                builder.not(value, span)
            }),
            And => self.binary_with_type(builder, Type::I1, span, |builder, lhs, rhs, span| {
                builder.and(lhs, rhs, span)
            }),
            Or => self.binary_with_type(builder, Type::I1, span, |builder, lhs, rhs, span| {
                builder.or(lhs, rhs, span)
            }),
            Xor => self.binary_with_type(builder, Type::I1, span, |builder, lhs, rhs, span| {
                builder.xor(lhs, rhs, span)
            }),
            Eq => self.binary_with_type(builder, Type::Felt, span, |builder, lhs, rhs, span| {
                builder.eq(lhs, rhs, span)
            }),
            EqImm(value) => {
                self.felt_binary_imm(builder, value, span, |builder, lhs, rhs, span| {
                    builder.eq(lhs, rhs, span)
                })
            }
            Eqw => self.eq_word(span, builder),
            Neq => self.binary_with_type(builder, Type::Felt, span, |builder, lhs, rhs, span| {
                builder.neq(lhs, rhs, span)
            }),
            NeqImm(value) => {
                self.felt_binary_imm(builder, value, span, |builder, lhs, rhs, span| {
                    builder.neq(lhs, rhs, span)
                })
            }
            Lt => self.binary_with_type(builder, Type::Felt, span, |builder, lhs, rhs, span| {
                builder.lt(lhs, rhs, span)
            }),
            Lte => self.binary_with_type(builder, Type::Felt, span, |builder, lhs, rhs, span| {
                builder.lte(lhs, rhs, span)
            }),
            Gt => self.binary_with_type(builder, Type::Felt, span, |builder, lhs, rhs, span| {
                builder.gt(lhs, rhs, span)
            }),
            Gte => self.binary_with_type(builder, Type::Felt, span, |builder, lhs, rhs, span| {
                builder.gte(lhs, rhs, span)
            }),
            IsOdd => self.unary_with_type(builder, Type::Felt, span, |builder, value, span| {
                builder.is_odd(value, span)
            }),
            _ => unsupported_instruction(inst, span),
        }
    }

    fn lift_if(
        &mut self,
        then_blk: &Block,
        else_blk: &Block,
        span: SourceSpan,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
    ) -> Result<()> {
        let cond = self.pop(span)?;
        let cond = self.cast(builder, cond.value, Type::I1, span)?;
        let input_stack = self.stack.clone();

        let if_op = builder.r#if(cond, &[], span)?;
        let if_ref = if_op.as_operation_ref();
        builder.builder_mut().set_insertion_point_after(if_ref);

        let then_region = { if_op.borrow().then_body().as_region_ref() };
        let then_block = builder.create_block_in_region(then_region);
        builder.switch_to_block(then_block);
        self.stack = input_stack.clone();
        self.lift_block(then_blk, builder)?;
        let then_stack = self.stack.clone();

        let else_region = { if_op.borrow().else_body().as_region_ref() };
        let else_block = builder.create_block_in_region(else_region);
        builder.switch_to_block(else_block);
        self.stack = input_stack;
        self.lift_block(else_blk, builder)?;
        let else_stack = self.stack.clone();

        if then_stack.len() != else_stack.len() {
            return Err(error::error(format!(
                "if branches leave different stack depths at {span:?}: then={}, else={}",
                then_stack.len(),
                else_stack.len()
            )));
        }

        let result_types = stack_types(&then_stack);
        append_results(builder, if_ref, &result_types, span);

        builder.switch_to_block(then_block);
        let yielded = self.cast_stack_to_types(builder, &then_stack, &result_types, span)?;
        builder.r#yield(yielded, span)?;

        builder.switch_to_block(else_block);
        let yielded = self.cast_stack_to_types(builder, &else_stack, &result_types, span)?;
        builder.r#yield(yielded, span)?;

        builder.builder_mut().set_insertion_point_after(if_ref);
        self.stack = op_results_as_stack(if_ref, span);
        Ok(())
    }

    fn lift_while(
        &mut self,
        body: &Block,
        span: SourceSpan,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
    ) -> Result<()> {
        self.require_depth(0, span)?;

        let init_stack = self.stack.clone();
        let init_types = stack_types(&init_stack);
        let result_types = init_types[..init_types.len() - 1].to_vec();
        let inits = init_stack.iter().map(|value| value.value);

        let while_op = builder.r#while(inits, &result_types, span)?;
        let while_ref = while_op.as_operation_ref();
        builder.builder_mut().set_insertion_point_after(while_ref);

        let before_block =
            { while_op.borrow().before().entry_block_ref().expect("scf.while before block") };
        builder.switch_to_block(before_block);
        self.stack = stack_from_block_args(before_block);
        let cond = self.pop(span)?;
        let cond = self.cast(builder, cond.value, Type::I1, span)?;
        let forwarded =
            self.cast_stack_to_types(builder, &self.stack.clone(), &result_types, span)?;
        builder.condition(cond, forwarded, span)?;

        let after_block =
            { while_op.borrow().after().entry_block_ref().expect("scf.while after block") };
        builder.switch_to_block(after_block);
        self.stack = stack_from_block_args(after_block);
        self.lift_block(body, builder)?;

        if self.stack.len() != init_types.len() {
            return Err(error::error(format!(
                "while body must leave {} value(s) for the next iteration at {span:?}, but left {}",
                init_types.len(),
                self.stack.len()
            )));
        }

        let yielded = self.cast_stack_to_types(builder, &self.stack.clone(), &init_types, span)?;
        builder.r#yield(yielded, span)?;

        builder.builder_mut().set_insertion_point_after(while_ref);
        self.stack = op_results_as_stack(while_ref, span);
        Ok(())
    }

    fn push_immediate(
        &mut self,
        value: PushValue,
        span: SourceSpan,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
    ) -> Result<()> {
        match value {
            PushValue::Int(IntValue::U8(value)) => {
                self.push_value(builder.u8(value, span), span);
            }
            PushValue::Int(IntValue::U16(value)) => {
                self.push_value(builder.u16(value, span), span);
            }
            PushValue::Int(IntValue::U32(value)) => {
                self.push_value(builder.u32(value, span), span);
            }
            PushValue::Int(IntValue::Felt(value)) => {
                self.push_value(builder.felt(value, span), span);
            }
            PushValue::Word(value) => self.push_word(value, span, builder),
        }
        Ok(())
    }

    fn push_word(
        &mut self,
        value: miden_assembly_syntax::parser::WordValue,
        span: SourceSpan,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
    ) {
        for value in value.0 {
            self.push_value(builder.felt(value, span), span);
        }
    }

    fn push_word_slice(
        &mut self,
        value: miden_assembly_syntax::parser::WordValue,
        range: &std::ops::Range<usize>,
        span: SourceSpan,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
    ) -> Result<()> {
        let Some(values) = value.0.get(range.clone()) else {
            return Err(error::error(format!(
                "invalid push word slice range {:?} at {span:?}",
                range
            )));
        };
        for value in values {
            self.push_value(builder.felt(*value, span), span);
        }
        Ok(())
    }

    fn invoke(
        &mut self,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
        target: &InvocationTarget,
        span: SourceSpan,
        kind: InvokeKind,
    ) -> Result<()> {
        let function = self.resolve_local_target(target)?;
        let signature = function.borrow().get_signature().clone();
        let mut args = Vec::with_capacity(signature.arity());
        for param in signature.params().iter() {
            let arg = self.pop(span)?;
            args.push(self.cast(builder, arg.value, param.ty.clone(), span)?);
        }

        let results: Vec<_> = match kind {
            InvokeKind::Exec => builder
                .exec(function, signature, args, span)?
                .borrow()
                .results()
                .iter()
                .map(|result| result.borrow().as_value_ref())
                .collect(),
            InvokeKind::Call => builder
                .call(function, signature, args, span)?
                .borrow()
                .results()
                .iter()
                .map(|result| result.borrow().as_value_ref())
                .collect(),
            InvokeKind::Syscall => builder
                .syscall(function, signature, args, span)?
                .borrow()
                .results()
                .iter()
                .map(|result| result.borrow().as_value_ref())
                .collect(),
        };
        for result in results.into_iter().rev() {
            self.push_value(result, span);
        }
        Ok(())
    }

    fn pop_results(
        &mut self,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
        span: SourceSpan,
    ) -> Result<Vec<ValueRef>> {
        let result_types: Vec<_> =
            self.signature.results().iter().map(|result| result.ty.clone()).collect();
        let mut results = Vec::with_capacity(result_types.len());
        for result_ty in result_types {
            let value = self.pop(span)?;
            results.push(self.cast(builder, value.value, result_ty, span)?);
        }
        Ok(results)
    }

    fn binary_with_type<F>(
        &mut self,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
        ty: Type,
        span: SourceSpan,
        f: F,
    ) -> Result<()>
    where
        F: FnOnce(
            &mut FunctionBuilder<'_, OpBuilder>,
            ValueRef,
            ValueRef,
            SourceSpan,
        ) -> Result<ValueRef>,
    {
        let (lhs, rhs) = self.pop_binary(span)?;
        let lhs = self.cast(builder, lhs.value, ty.clone(), span)?;
        let rhs = self.cast(builder, rhs.value, ty, span)?;
        let result = f(builder, lhs, rhs, span)?;
        self.push_value(result, span);
        Ok(())
    }

    fn felt_binary_imm<F>(
        &mut self,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
        immediate: &Immediate<Felt>,
        span: SourceSpan,
        f: F,
    ) -> Result<()>
    where
        F: FnOnce(
            &mut FunctionBuilder<'_, OpBuilder>,
            ValueRef,
            ValueRef,
            SourceSpan,
        ) -> Result<ValueRef>,
    {
        let lhs = self.pop(span)?;
        let lhs = self.cast(builder, lhs.value, Type::Felt, span)?;
        let rhs = builder.felt(immediate_value(immediate)?, span);
        let result = f(builder, lhs, rhs, span)?;
        self.push_value(result, span);
        Ok(())
    }

    fn u32_binary_imm<F>(
        &mut self,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
        immediate: &Immediate<u32>,
        span: SourceSpan,
        f: F,
    ) -> Result<()>
    where
        F: FnOnce(
            &mut FunctionBuilder<'_, OpBuilder>,
            ValueRef,
            ValueRef,
            SourceSpan,
        ) -> Result<ValueRef>,
    {
        self.u32_binary_const(builder, immediate_value(immediate)?, span, f)
    }

    fn u32_binary_const<F>(
        &mut self,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
        immediate: u32,
        span: SourceSpan,
        f: F,
    ) -> Result<()>
    where
        F: FnOnce(
            &mut FunctionBuilder<'_, OpBuilder>,
            ValueRef,
            ValueRef,
            SourceSpan,
        ) -> Result<ValueRef>,
    {
        let lhs = self.pop(span)?;
        let lhs = self.cast(builder, lhs.value, Type::U32, span)?;
        let rhs = builder.u32(immediate, span);
        let result = f(builder, lhs, rhs, span)?;
        self.push_value(result, span);
        Ok(())
    }

    fn u32_overflowing_binary<F>(
        &mut self,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
        span: SourceSpan,
        f: F,
    ) -> Result<()>
    where
        F: FnOnce(
            &mut FunctionBuilder<'_, OpBuilder>,
            ValueRef,
            ValueRef,
            SourceSpan,
        ) -> Result<(ValueRef, ValueRef)>,
    {
        let (lhs, rhs) = self.pop_binary(span)?;
        let lhs = self.cast(builder, lhs.value, Type::U32, span)?;
        let rhs = self.cast(builder, rhs.value, Type::U32, span)?;
        let (overflowed, result) = f(builder, lhs, rhs, span)?;
        self.push_value(result, span);
        self.push_value(overflowed, span);
        Ok(())
    }

    fn u32_overflowing_binary_imm<F>(
        &mut self,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
        immediate: &Immediate<u32>,
        span: SourceSpan,
        f: F,
    ) -> Result<()>
    where
        F: FnOnce(
            &mut FunctionBuilder<'_, OpBuilder>,
            ValueRef,
            ValueRef,
            SourceSpan,
        ) -> Result<(ValueRef, ValueRef)>,
    {
        let lhs = self.pop(span)?;
        let lhs = self.cast(builder, lhs.value, Type::U32, span)?;
        let rhs = builder.u32(immediate_value(immediate)?, span);
        let (overflowed, result) = f(builder, lhs, rhs, span)?;
        self.push_value(result, span);
        self.push_value(overflowed, span);
        Ok(())
    }

    fn unary_with_type<F>(
        &mut self,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
        ty: Type,
        span: SourceSpan,
        f: F,
    ) -> Result<()>
    where
        F: FnOnce(&mut FunctionBuilder<'_, OpBuilder>, ValueRef, SourceSpan) -> Result<ValueRef>,
    {
        let value = self.pop(span)?;
        let value = self.cast(builder, value.value, ty, span)?;
        let result = f(builder, value, span)?;
        self.push_value(result, span);
        Ok(())
    }

    fn eq_word(
        &mut self,
        span: SourceSpan,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
    ) -> Result<()> {
        let rhs = self.pop_word(span)?;
        let lhs = self.pop_word(span)?;
        let mut result = None;
        for (lhs, rhs) in lhs.into_iter().zip(rhs.into_iter()) {
            let lhs = self.cast(builder, lhs.value, Type::Felt, span)?;
            let rhs = self.cast(builder, rhs.value, Type::Felt, span)?;
            let comparison = builder.eq(lhs, rhs, span)?;
            result = Some(match result {
                Some(result) => builder.and(result, comparison, span)?,
                None => comparison,
            });
        }
        let result = result.ok_or_else(|| {
            error::error(format!("word equality requires word operands at {span:?}"))
        })?;
        self.push_value(result, span);
        Ok(())
    }

    fn assert_eq_word(
        &mut self,
        span: SourceSpan,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
    ) -> Result<()> {
        let rhs = self.pop_word(span)?;
        let lhs = self.pop_word(span)?;
        for (lhs, rhs) in lhs.into_iter().zip(rhs.into_iter()) {
            let lhs = self.cast(builder, lhs.value, Type::Felt, span)?;
            let rhs = self.cast(builder, rhs.value, Type::Felt, span)?;
            builder.assert_eq(lhs, rhs, span)?;
        }
        Ok(())
    }

    fn conditional_drop(
        &mut self,
        chunk_len: usize,
        span: SourceSpan,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
    ) -> Result<()> {
        let cond = self.pop_condition(span, builder)?;
        let if_true = self.pop_chunk(chunk_len, span)?;
        let if_false = self.pop_chunk(chunk_len, span)?;
        for (if_false, if_true) in if_false.into_iter().zip(if_true.into_iter()) {
            let result_ty = if_false.value.borrow().ty().clone();
            let selected =
                self.select_as_type(builder, cond, if_true.value, if_false.value, result_ty, span)?;
            self.push_value(selected, span);
        }
        Ok(())
    }

    fn conditional_swap(
        &mut self,
        chunk_len: usize,
        span: SourceSpan,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
    ) -> Result<()> {
        let cond = self.pop_condition(span, builder)?;
        let if_true = self.pop_chunk(chunk_len, span)?;
        let if_false = self.pop_chunk(chunk_len, span)?;

        let mut lower = Vec::with_capacity(chunk_len);
        let mut upper = Vec::with_capacity(chunk_len);
        for (if_false, if_true) in if_false.into_iter().zip(if_true.into_iter()) {
            let lower_ty = if_false.value.borrow().ty().clone();
            let upper_ty = if_true.value.borrow().ty().clone();
            lower.push(self.select_as_type(
                builder,
                cond,
                if_true.value,
                if_false.value,
                lower_ty,
                span,
            )?);
            upper.push(self.select_as_type(
                builder,
                cond,
                if_false.value,
                if_true.value,
                upper_ty,
                span,
            )?);
        }

        for value in lower {
            self.push_value(value, span);
        }
        for value in upper {
            self.push_value(value, span);
        }
        Ok(())
    }

    fn u32_assert_n(
        &mut self,
        n: usize,
        span: SourceSpan,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
    ) -> Result<()> {
        self.require_depth(n - 1, span)?;
        let start = self.stack.len() - n;
        for index in start..self.stack.len() {
            let value = self.stack[index].value;
            self.stack[index].value = self.cast(builder, value, Type::U32, span)?;
        }
        Ok(())
    }

    fn u32_test(
        &mut self,
        span: SourceSpan,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
    ) -> Result<()> {
        self.require_depth(0, span)?;
        let value = self.stack.last().unwrap().value;
        let in_range = self.u32_range_check(value, span, builder)?;
        self.push_value(in_range, span);
        Ok(())
    }

    fn u32_testw(
        &mut self,
        span: SourceSpan,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
    ) -> Result<()> {
        self.require_depth(3, span)?;
        let start = self.stack.len() - 4;
        let values: Vec<_> = self.stack[start..].iter().map(|value| value.value).collect();
        let mut result = None;
        for value in values {
            let in_range = self.u32_range_check(value, span, builder)?;
            result = Some(match result {
                Some(result) => builder.and(result, in_range, span)?,
                None => in_range,
            });
        }
        let result = result
            .ok_or_else(|| error::error(format!("u32testw requires word operands at {span:?}")))?;
        self.push_value(result, span);
        Ok(())
    }

    fn u32_split(
        &mut self,
        span: SourceSpan,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
    ) -> Result<()> {
        let value = self.pop(span)?;
        let value = self.cast(builder, value.value, Type::U64, span)?;
        let (high, low) = builder.split2(value, Type::U32, span)?;
        self.push_value(high, span);
        self.push_value(low, span);
        Ok(())
    }

    fn u32_range_check(
        &mut self,
        value: ValueRef,
        span: SourceSpan,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
    ) -> Result<ValueRef> {
        let value = self.cast(builder, value, Type::U64, span)?;
        let (high, _low) = builder.split2(value, Type::U32, span)?;
        let zero = builder.u32(0, span);
        builder.eq(high, zero, span).map_err(Into::into)
    }

    fn pop_condition(
        &mut self,
        span: SourceSpan,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
    ) -> Result<ValueRef> {
        let cond = self.pop(span)?;
        self.cast(builder, cond.value, Type::I1, span)
    }

    fn select_as_type(
        &mut self,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
        cond: ValueRef,
        if_true: ValueRef,
        if_false: ValueRef,
        result_ty: Type,
        span: SourceSpan,
    ) -> Result<ValueRef> {
        let if_true = self.cast(builder, if_true, result_ty.clone(), span)?;
        let if_false = self.cast(builder, if_false, result_ty, span)?;
        builder.select(cond, if_true, if_false, span).map_err(Into::into)
    }

    fn cast_stack_to_types(
        &mut self,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
        stack: &[StackValue],
        types: &[Type],
        span: SourceSpan,
    ) -> Result<Vec<ValueRef>> {
        if stack.len() != types.len() {
            return Err(error::error(format!(
                "cannot cast stack of depth {} to {} type(s) at {span:?}",
                stack.len(),
                types.len()
            )));
        }

        stack
            .iter()
            .zip(types.iter())
            .map(|(value, ty)| self.cast(builder, value.value, ty.clone(), span))
            .collect()
    }

    fn cast(
        &mut self,
        builder: &mut FunctionBuilder<'_, OpBuilder>,
        value: ValueRef,
        ty: Type,
        span: SourceSpan,
    ) -> Result<ValueRef> {
        if value.borrow().ty() == &ty {
            return Ok(value);
        }
        builder.unrealized_conversion_cast(value, ty, span).map_err(Into::into)
    }

    fn local(&self, id: u16, span: SourceSpan) -> Result<LocalVariable> {
        self.locals
            .get(&id)
            .copied()
            .ok_or_else(|| error::error(format!("invalid local index {id} at {span:?}")))
    }

    fn resolve_local_target(&self, target: &InvocationTarget) -> Result<FunctionRef> {
        match target {
            InvocationTarget::Symbol(name) => self
                .functions
                .get(name.as_str())
                .copied()
                .ok_or_else(|| error::error(format!("unresolved local callee '{name}'"))),
            InvocationTarget::Path(path) => {
                let key = invocation_path_key(path.inner());
                self.external_functions.get(&key).copied().ok_or_else(|| {
                    error::error(format!(
                        "unresolved external callee '{}'; no external signature was provided",
                        path.inner()
                    ))
                })
            }
            InvocationTarget::MastRoot(_) => {
                Err(error::error("MAST root invocation targets are not supported"))
            }
        }
    }

    fn push_value(&mut self, value: ValueRef, span: SourceSpan) {
        self.stack.push(StackValue { value, span });
    }

    fn pop(&mut self, span: SourceSpan) -> Result<StackValue> {
        self.stack
            .pop()
            .ok_or_else(|| error::error(format!("stack underflow at {span:?}")))
    }

    fn pop_binary(&mut self, span: SourceSpan) -> Result<(StackValue, StackValue)> {
        let rhs = self.pop(span)?;
        let lhs = self.pop(span)?;
        Ok((lhs, rhs))
    }

    fn drop_n(&mut self, n: usize, span: SourceSpan) -> Result<()> {
        for _ in 0..n {
            self.pop(span)?;
        }
        Ok(())
    }

    fn dup(&mut self, depth: usize, span: SourceSpan) -> Result<()> {
        let index = self.index_from_top(depth, span)?;
        self.stack.push(self.stack[index]);
        Ok(())
    }

    fn dup_word(&mut self, depth: usize, span: SourceSpan) -> Result<()> {
        for _ in 0..4 {
            self.dup(depth * 4 + 3, span)?;
        }
        Ok(())
    }

    fn swap(&mut self, depth: usize, span: SourceSpan) -> Result<()> {
        let index = self.index_from_top(depth, span)?;
        let top = self.index_from_top(0, span)?;
        self.stack.swap(index, top);
        Ok(())
    }

    fn swap_word(&mut self, depth: usize, span: SourceSpan) -> Result<()> {
        self.swap_chunks(4, depth, span)
    }

    fn swap_double_word(&mut self, span: SourceSpan) -> Result<()> {
        self.swap_chunks(8, 1, span)
    }

    fn swap_chunks(&mut self, chunk_len: usize, depth: usize, span: SourceSpan) -> Result<()> {
        let total = chunk_len * (depth + 1);
        self.require_depth(total - 1, span)?;
        let len = self.stack.len();
        let top_start = len - chunk_len;
        let other_start = len - total;
        for offset in 0..chunk_len {
            self.stack.swap(other_start + offset, top_start + offset);
        }
        Ok(())
    }

    fn movup(&mut self, depth: usize, span: SourceSpan) -> Result<()> {
        let index = self.index_from_top(depth, span)?;
        let value = self.stack.remove(index);
        self.stack.push(value);
        Ok(())
    }

    fn movup_word(&mut self, depth: usize, span: SourceSpan) -> Result<()> {
        self.move_chunk_to_top(4, depth, span)
    }

    fn move_chunk_to_top(
        &mut self,
        chunk_len: usize,
        depth: usize,
        span: SourceSpan,
    ) -> Result<()> {
        let total = chunk_len * (depth + 1);
        self.require_depth(total - 1, span)?;
        let start = self.stack.len() - total;
        let chunk: Vec<_> = self.stack.drain(start..start + chunk_len).collect();
        self.stack.extend(chunk);
        Ok(())
    }

    fn movdn(&mut self, depth: usize, span: SourceSpan) -> Result<()> {
        self.require_depth(depth, span)?;
        let value = self.stack.pop().unwrap();
        let index = self.stack.len().saturating_sub(depth);
        self.stack.insert(index, value);
        Ok(())
    }

    fn movdn_word(&mut self, depth: usize, span: SourceSpan) -> Result<()> {
        self.move_top_chunk_down(4, depth, span)
    }

    fn move_top_chunk_down(
        &mut self,
        chunk_len: usize,
        depth: usize,
        span: SourceSpan,
    ) -> Result<()> {
        self.require_depth(chunk_len * (depth + 1) - 1, span)?;
        let len = self.stack.len();
        let chunk: Vec<_> = self.stack.drain(len - chunk_len..).collect();
        let index = self.stack.len() - (chunk_len * depth);
        self.stack.splice(index..index, chunk);
        Ok(())
    }

    fn reverse_word(&mut self, span: SourceSpan) -> Result<()> {
        self.require_depth(3, span)?;
        let len = self.stack.len();
        self.stack[len - 4..].reverse();
        Ok(())
    }

    fn reverse_double_word(&mut self, span: SourceSpan) -> Result<()> {
        self.require_depth(7, span)?;
        let len = self.stack.len();
        self.stack[len - 8..].reverse();
        Ok(())
    }

    fn pop_word(&mut self, span: SourceSpan) -> Result<Vec<StackValue>> {
        self.pop_chunk(4, span)
    }

    fn pop_chunk(&mut self, chunk_len: usize, span: SourceSpan) -> Result<Vec<StackValue>> {
        self.require_depth(chunk_len - 1, span)?;
        Ok(self.stack.split_off(self.stack.len() - chunk_len))
    }

    fn index_from_top(&self, depth: usize, span: SourceSpan) -> Result<usize> {
        self.require_depth(depth, span)?;
        Ok(self.stack.len() - 1 - depth)
    }

    fn require_depth(&self, depth: usize, span: SourceSpan) -> Result<()> {
        if self.stack.len() <= depth {
            Err(error::error(format!("stack underflow at {span:?}")))
        } else {
            Ok(())
        }
    }
}

enum InvokeKind {
    Exec,
    Call,
    Syscall,
}

fn unsupported_instruction(inst: &Instruction, span: SourceSpan) -> Result<()> {
    Err(error::error(format!(
        "MASM instruction {inst:?} is not supported during disassembly at {span:?}"
    )))
}

fn immediate_u32(immediate: &Immediate<u32>) -> Result<u32> {
    match immediate {
        Immediate::Value(value) => Ok(value.into_inner()),
        Immediate::Constant(name) => Err(error::error(format!(
            "unresolved repeat count constant '{name}' is not supported during disassembly"
        ))),
    }
}

fn immediate_value<T: Copy>(immediate: &Immediate<T>) -> Result<T> {
    match immediate {
        Immediate::Value(value) => Ok(value.into_inner()),
        Immediate::Constant(name) => Err(error::error(format!(
            "unresolved immediate constant '{name}' is not supported during disassembly"
        ))),
    }
}

fn stack_types(stack: &[StackValue]) -> Vec<Type> {
    stack.iter().map(|value| value.value.borrow().ty().clone()).collect()
}

fn stack_from_block_args(block: BlockRef) -> Vec<StackValue> {
    block
        .borrow()
        .arguments()
        .iter()
        .map(|arg| StackValue {
            value: *arg as ValueRef,
            span: arg.borrow().span(),
        })
        .collect()
}

fn append_results(
    builder: &mut FunctionBuilder<'_, OpBuilder>,
    mut owner: OperationRef,
    result_types: &[Type],
    span: SourceSpan,
) {
    let context = builder.builder().context();
    let mut owner_mut = owner.borrow_mut();
    for result_ty in result_types {
        let result = context.make_result(span, result_ty.clone(), owner, 0);
        owner_mut.results_mut().push(result);
    }
}

fn op_results_as_stack(owner: OperationRef, span: SourceSpan) -> Vec<StackValue> {
    owner
        .borrow()
        .results()
        .all()
        .iter()
        .map(|result| StackValue {
            value: result.borrow().as_value_ref(),
            span,
        })
        .collect()
}
