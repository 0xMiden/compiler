use crate::{
    lower::units::{fpi::Fpi, memory::ByteMemmove},
    plan::*,
};

#[test]
fn real_lowering_definitions_have_semantic_stack_and_storage_contracts() {
    let inputs = (0..3).map(|id| Atom::new(ValueId(id), 0)).collect::<alloc::vec::Vec<_>>();
    let (unit, proof) = ByteMemmove.select(SelectedStepId(0), SourceStepId(0), &inputs).unwrap();
    assert_eq!(unit.stack_inputs, inputs);
    assert_eq!(proof.loops.len(), 1);
    assert_eq!(unit.effects.memory, SourceMemoryEffects::READ_WRITE);
    let prefix = core::array::from_fn(|id| Atom::new(ValueId(id as u32), 0));
    let payload = (6..22).map(|id| Atom::new(ValueId(id), 0)).collect::<alloc::vec::Vec<_>>();
    let outputs = (22..38).map(|id| Atom::new(ValueId(id), 0)).collect::<alloc::vec::Vec<_>>();
    let (unit, proof) = Fpi
        .select(SelectedStepId(1), SourceStepId(1), prefix, &payload, &outputs)
        .unwrap();
    assert_eq!(unit.stored_inputs.len(), 6);
    assert_eq!(unit.stack_inputs.len(), 16);
    assert_eq!(proof.calls[0].abi.arguments(), 22);
    assert_eq!(proof.calls[0].abi.results(), 16);
}

use alloc::{boxed::Box, collections::BTreeMap, vec, vec::Vec};

use miden_core::Felt;
use midenc_hir::{Report, Type, formatter::PrettyPrint};

use crate::{
    allocate::{allocate, schedule::AllocationOptions, slots::assign_frame},
    lower::{
        expand::*,
        units::{LoweringDefinition, LoweringRegistry, calls::ContextCall},
    },
    masm::{self, Instruction},
    verify::{
        self, DefinitionEvidence, GeneratedDefinition, SourceContext, UnitEvidence,
        VerificationError,
    },
};

#[derive(Clone)]
pub(crate) enum Case {
    Add,
    Memmove,
    Fpi(usize),
    FpiShared,
    Context {
        invocation: Invocation,
        arguments: u32,
        results: u32,
        root_argument: bool,
    },
}
#[derive(Clone)]
pub(crate) struct Source {
    pub case: Case,
    pub live: Vec<u32>,
}
impl Source {
    fn context(&self) -> ContextCall {
        let Case::Context {
            invocation,
            arguments,
            results,
            ..
        } = self.case
        else {
            panic!("context case")
        };
        ContextCall {
            kind: LoweringKind(1002),
            abi: ProcedureAbi::new(
                if invocation == Invocation::SysCall {
                    CallingConvention::Kernel
                } else {
                    CallingConvention::Context
                },
                arguments,
                results,
            )
            .unwrap(),
            instruction: match invocation {
                Invocation::Call => Instruction::Call(symbol("callee")),
                Invocation::DynCall => Instruction::DynCall,
                Invocation::SysCall => Instruction::SysCall(symbol("callee")),
                _ => panic!(),
            },
            root: ScratchId(0),
        }
    }

    pub fn definition(&self) -> Box<dyn LoweringDefinition> {
        match self.case {
            Case::Add => Box::new(crate::lower::units::Scalar::Add),
            Case::Memmove => Box::new(ByteMemmove),
            Case::Fpi(_) | Case::FpiShared => Box::new(Fpi),
            Case::Context { .. } => Box::new(self.context()),
        }
    }

    pub fn inputs(&self) -> u32 {
        match self.case {
            Case::Add => 2,
            Case::Memmove => 3,
            Case::Fpi(n) => 6 + n as u32,
            Case::FpiShared => 22,
            Case::Context {
                invocation,
                arguments,
                ..
            } => {
                arguments
                    + if invocation == Invocation::DynCall {
                        4
                    } else {
                        0
                    }
            }
        }
    }

    pub fn results(&self) -> u32 {
        match self.case {
            Case::Add => 1,
            Case::Memmove => 0,
            Case::Fpi(_) | Case::FpiShared => 16,
            Case::Context { results, .. } => results,
        }
    }

    pub fn unit(&self) -> (SelectedLowering, UnitEvidence) {
        let args = (0..self.inputs()).map(atom).collect::<Vec<_>>();
        let outputs = (0..self.results()).map(|i| atom(1000 + i)).collect::<Vec<_>>();
        match self.case {
            Case::Add => crate::lower::units::Scalar::Add
                .select(SelectedStepId(0), SourceStepId(0), &args, &outputs)
                .unwrap(),
            Case::FpiShared => {
                let mut payload = args[6..].to_vec();
                payload[0] = args[0];
                Fpi.select(
                    SelectedStepId(0),
                    SourceStepId(0),
                    core::array::from_fn(|i| args[i]),
                    &payload,
                    &outputs,
                )
                .unwrap()
            }
            Case::Memmove => ByteMemmove.select(SelectedStepId(0), SourceStepId(0), &args).unwrap(),
            Case::Fpi(n) => Fpi
                .select(
                    SelectedStepId(0),
                    SourceStepId(0),
                    core::array::from_fn(|i| args[i]),
                    &args[6..6 + n],
                    &outputs,
                )
                .unwrap(),
            Case::Context {
                invocation,
                root_argument,
                ..
            } => {
                let dynamic = invocation == Invocation::DynCall;
                let mut arguments = args[if dynamic { 4 } else { 0 }..].to_vec();
                if root_argument {
                    let reused = 4.min(arguments.len());
                    arguments[..reused].copy_from_slice(&args[..reused]);
                }
                self.context()
                    .select(
                        SelectedStepId(0),
                        SourceStepId(0),
                        &arguments,
                        dynamic.then(|| core::array::from_fn(|i| args[i])),
                        &outputs,
                    )
                    .unwrap()
            }
        }
    }
}
pub(crate) fn atom(id: u32) -> Atom {
    Atom::new(ValueId(id), 0)
}
pub(crate) fn symbol(name: &str) -> masm::InvocationTarget {
    masm::InvocationTarget::Symbol(masm::Ident::new(name).unwrap())
}
impl GeneratedDefinition for Source {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(8000)
    }

    fn instantiate(&self, parameters: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        if !parameters.is_empty() {
            return Err(PlanError::new(
                DiagnosticCategory::InvalidSelectedLowering,
                "unit fixture takes no parameters",
            ));
        }
        let (unit, proof) = self.unit();
        let mut values = BTreeMap::new();
        for i in 0..self.inputs() {
            values.insert(
                ValueId(i),
                SelectedValue {
                    id: ValueId(i),
                    origin: ValueOrigin::Parameter {
                        segment: SegmentId(0),
                        index: i,
                    },
                    layout: ValueLayout::new(Type::Felt)?,
                    reconstruction: None,
                },
            );
        }
        for i in 0..self.results() {
            values.insert(
                ValueId(1000 + i),
                SelectedValue {
                    id: ValueId(1000 + i),
                    origin: ValueOrigin::Temporary {
                        step: SourceStepId(0),
                        index: i,
                    },
                    layout: ValueLayout::new(Type::Felt)?,
                    reconstruction: None,
                },
            );
        }
        let returns = (0..self.results())
            .map(|i| 1000 + i)
            .chain(self.live.iter().copied())
            .map(|i| ParameterBinding {
                destination: ValueId(i),
                source: ValueId(i),
            })
            .collect();
        Ok(DefinitionEvidence {
            source_locals: Default::default(),
            program: SelectedProgram {
                origin: ProcedureOrigin::Generated {
                    definition: self.id(),
                    parameters: vec![],
                },
                values,
                segments: vec![SelectedSegment {
                    id: SegmentId(0),
                    parameters: (0..self.inputs()).map(ValueId).collect(),
                    content_parameters: vec![],
                    units: vec![unit],
                }],
                edges: vec![TransferEdge {
                    id: EdgeId(0),
                    source: SegmentId(0),
                    destination: None,
                    kind: EdgeKind::Return,
                    bindings: returns,
                    content_bindings: vec![],
                }],
                structure: ControlTree::Sequence(vec![
                    ControlTree::Segment(SegmentId(0)),
                    ControlTree::Edge(EdgeId(0)),
                ]),
                abi: ProcedureAbi::new(
                    CallingConvention::SameContext,
                    self.inputs(),
                    self.results() + self.live.len() as u32,
                )?,
                source_map: BTreeMap::from([(
                    SourceStepId(0),
                    SourceRecord {
                        origin: SourceOrigin::Generated {
                            definition: self.id(),
                            stage: 0,
                        },
                        location: Default::default(),
                    },
                )]),
            },
            units: BTreeMap::from([(SelectedStepId(0), proof)]),
        })
    }
}
pub(crate) fn ready(
    source: &Source,
    options: &AllocationOptions,
) -> (crate::plan::actions::ProcedurePlan, FinalFrameLayout, AnnotatedMasmBody) {
    let selected = verify::check_selection(
        source.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(source),
    )
    .unwrap();
    let mut plan = allocate(selected, options).unwrap();
    let frame = assign_frame(&plan, &SourceLocalLayout::default()).unwrap();
    resolve_frame_bindings(&mut plan, &frame).unwrap();
    let definition = source.definition();
    let mut registry = LoweringRegistry::default();
    registry.register(definition.as_ref()).unwrap();
    let mut body = expand_procedure(&plan, &frame, &registry, &ExpansionEnv::default()).unwrap();
    crate::opt::peephole::simplify_annotated(&mut body);
    (plan, frame, body)
}
pub(crate) fn procedure(source: &Source) -> masm::Procedure {
    let (plan, frame, body) = ready(source, &AllocationOptions::default());
    verify::verify(&plan, &frame, body)
        .unwrap_or_else(|error| panic!("{error}\n{plan}"))
        .into_masm(masm::ProcedureName::new("fixture").unwrap())
        .unwrap()
}

#[test]
fn fpi_payload_widths_0_to_16_execute_stub_contract() -> Result<(), Report> {
    for (width, shared) in (0..=16).map(|width| (width, false)).chain([(16, true)]) {
        let live = core::iter::once(0)
            .chain((width != 0).then_some(6))
            .chain((width > 1).then_some(6 + width as u32 - 1))
            .collect::<Vec<_>>();
        let source = Source {
            case: if shared {
                Case::FpiShared
            } else {
                Case::Fpi(width)
            },
            live: live.clone(),
        };
        let procedure = procedure(&source).to_pretty_string();
        let prefix = [101, 103, 107, 109, 113, 127];
        let mut stub = String::from("@locals(6) pub proc execute_foreign_procedure ");
        for (i, value) in prefix.iter().enumerate() {
            stub.push_str(&alloc::format!("loc_store.{i} loc_load.{i} push.{value} assert_eq "));
        }
        for i in 0..16 {
            stub.push_str(&alloc::format!(
                "dup.{i} push.{} assert_eq ",
                if shared && i == 0 {
                    101
                } else if i < width {
                    201 + i
                } else {
                    0
                }
            ));
        }
        stub.push_str("end");
        let manager = alloc::sync::Arc::new(miden_assembly::DefaultSourceManager::default());
        let module =
            miden_assembly::ast::Module::parser(Some(miden_assembly::ast::ModuleKind::Library))
                .parse_str(
                    Some(miden_assembly::Path::new("::miden::protocol::tx")),
                    &stub,
                    manager.clone(),
                )?;
        let library: alloc::sync::Arc<miden_assembly::package::Package> =
            miden_assembly::Assembler::new(manager.clone())
                .assemble_library("fpi_stub", module, core::iter::empty::<&str>())?
                .into();
        let mut assembler = miden_assembly::Assembler::new(manager);
        assembler.link_package(library.clone(), miden_assembly::Linkage::Dynamic)?;
        let inputs = prefix.into_iter().chain((0..width).map(|i| 201 + i)).collect::<Vec<_>>();
        let expected = (0..16)
            .map(|i| {
                if shared && i == 0 {
                    101
                } else if i < width {
                    201 + i
                } else {
                    0
                }
            })
            .chain(live.iter().map(|i| inputs[*i as usize]))
            .chain([901, 903, 907])
            .collect::<Vec<_>>();
        let program = assembler
            .assemble_program(
                "fpi_pipeline",
                format!(
                    "{procedure} begin push.907.903.901 {} exec.fixture {} end",
                    push_top(&inputs),
                    assert_top(&expected)
                ),
            )?
            .try_into_program()?;
        let mut host = host().with_library(library).map_err(|e| Report::msg(e.to_string()))?;
        miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
            .execute_sync(&program, &mut host)
            .map_err(|e| Report::msg(e.to_string()))?;
    }
    Ok(())
}
pub(crate) fn push_top(values: &[usize]) -> String {
    values.iter().rev().map(|v| format!("push.{v} ")).collect()
}
pub(crate) fn assert_top(values: &[usize]) -> String {
    values.iter().map(|v| format!("push.{v} assert_eq ")).collect()
}

fn clobbering_callee(arguments: u32, results: u32, kernel: bool) -> String {
    let mut body = String::from(if kernel {
        "pub proc callee "
    } else {
        "proc callee "
    });
    for i in 0..16 {
        body.push_str(&format!(
            "dup.{i} push.{} assert_eq ",
            if i < arguments { 11 + i } else { 0 }
        ));
    }
    body.push_str(if kernel {
        "mem_load.99 push.123 assert_eq push.124 mem_store.99 "
    } else {
        "mem_load.99 assertz "
    });
    for i in 0..16 {
        if i == 1 {
            body.push_str("swap.1 ");
        } else if i > 1 {
            body.push_str(&format!("movup.{i} "));
        }
        body.push_str(&format!(
            "push.{} swap.1 drop ",
            if i < results { 700 + i } else { 5000 + i }
        ));
        if i == 1 {
            body.push_str("swap.1 ");
        } else if i > 1 {
            body.push_str(&format!("movdn.{i} "));
        }
    }
    body.push_str("end");
    body
}
#[test]
fn context_call_window_args_and_results_0_to_16() -> Result<(), Report> {
    for invocation in [Invocation::Call, Invocation::DynCall, Invocation::SysCall] {
        for arguments in 0..=16 {
            for results in 0..=16 {
                let source = Source {
                    case: Case::Context {
                        invocation,
                        arguments,
                        results,
                        root_argument: false,
                    },
                    live: if arguments == 0 {
                        vec![]
                    } else {
                        vec![if invocation == Invocation::DynCall {
                            4
                        } else {
                            0
                        }]
                    },
                };
                let procedure = procedure(&source).to_pretty_string();
                let callee =
                    clobbering_callee(arguments, results, invocation == Invocation::SysCall);
                let mut assembler = miden_assembly::Assembler::default();
                let mut host = host();
                let direct = if invocation == Invocation::SysCall {
                    let manager =
                        alloc::sync::Arc::new(miden_assembly::DefaultSourceManager::default());
                    let module = miden_assembly::ast::Module::parser(Some(
                        miden_assembly::ast::ModuleKind::Kernel,
                    ))
                    .parse_str(
                        Some(miden_assembly::Path::KERNEL),
                        &callee,
                        manager.clone(),
                    )?;
                    let kernel: alloc::sync::Arc<miden_assembly::package::Package> =
                        miden_assembly::Assembler::new(manager.clone())
                            .assemble_kernel("window_kernel", module, None)?
                            .into();
                    assembler = miden_assembly::Assembler::with_kernel(manager, kernel.clone())?;
                    host = host.with_library(kernel).map_err(|e| Report::msg(e.to_string()))?;
                    "".to_string()
                } else {
                    callee
                };
                let expected = (0..results)
                    .map(|i| 700 + i as usize)
                    .chain((arguments != 0).then_some(11))
                    .chain([901, 903, 907])
                    .collect::<Vec<_>>();
                let args = (0..arguments).map(|i| 11 + i as usize).collect::<Vec<_>>();
                let code = format!(
                    "{direct} {procedure} begin push.123 mem_store.99 push.907.903.901 {} {} \
                     exec.fixture {} mem_load.99 push.{} assert_eq end",
                    push_top(&args),
                    if invocation == Invocation::DynCall {
                        "procref.callee"
                    } else {
                        ""
                    },
                    assert_top(&expected),
                    if invocation == Invocation::SysCall {
                        124
                    } else {
                        123
                    }
                );
                let program =
                    assembler.assemble_program("context_pipeline", code)?.try_into_program()?;
                miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
                    .execute_sync(&program, &mut host)
                    .map_err(|e| {
                        Report::msg(format!("{invocation:?} {arguments}->{results}: {e}"))
                    })?;
            }
        }
    }
    Ok(())
}

pub(crate) fn session() -> midenc_session::Session {
    let mut options = midenc_session::Options::default();
    if let Some(sysroot) = std::env::var_os("MIDEN_SYSROOT") {
        options.sysroot = Some(sysroot.into());
    }
    midenc_session::Session::new_project(
        "unit-lowering".into(),
        None,
        Box::new(options),
        None,
        alloc::sync::Arc::new(miden_assembly::DefaultSourceManager::default()),
    )
}
pub(crate) fn execute_memmove(
    procedure: &masm::Procedure,
    src: usize,
    dst: usize,
    count: usize,
    expected: Option<&[u8]>,
    live: bool,
) -> Result<(), Report> {
    let intrinsics = crate::intrinsics::load(&session())?;
    let mut assembler = miden_assembly::Assembler::default();
    assembler.link_package(intrinsics.clone(), miden_assembly::Linkage::Dynamic)?;
    let original = (0..48).map(|i| (i * 37 + 11) as u8).collect::<Vec<_>>();
    let initialize = original
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .map(|(i, c)| format!("push.{} mem_store.{} ", u32::from_le_bytes(*c), 256 + i))
        .collect::<String>();
    let checks = expected
        .map(|bytes| {
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    format!("mem_load.{} push.{} assert_eq ", 256 + i, u32::from_le_bytes(*c))
                })
                .collect::<String>()
        })
        .unwrap_or_default();
    let expected = if live {
        vec![src, dst, count, 901, 903, 907]
    } else {
        vec![901, 903, 907]
    };
    let code = format!(
        "{} begin {initialize} push.907.903.901 {} exec.fixture {} {checks} end",
        procedure.to_pretty_string(),
        push_top(&[src, dst, count]),
        assert_top(&expected)
    );
    let program = assembler.assemble_program("memmove_pipeline", code)?.try_into_program()?;
    let mut host = host().with_library(intrinsics).map_err(|e| Report::msg(e.to_string()))?;
    miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
        .execute_sync(&program, &mut host)
        .map_err(|e| Report::msg(e.to_string()))?;
    Ok(())
}
#[test]
fn byte_memmove_aligned_and_directional_paths_execute() -> Result<(), Report> {
    let source = Source {
        case: Case::Memmove,
        live: vec![0, 1, 2],
    };
    let procedure = procedure(&source);
    for (src, dst, count) in [
        (0, 4, 16),
        (8, 0, 16),
        (1, 3, 19),
        (5, 1, 19),
        (1, 29, 11),
        (3, 3, 17),
        (0, 8, 0),
        (1, 3, 0),
    ] {
        let mut expected = (0..48).map(|i| (i * 37 + 11) as u8).collect::<Vec<_>>();
        expected.copy_within(src..src + count, dst);
        execute_memmove(&procedure, 1024 + src, 1024 + dst, count, Some(&expected), true)?;
    }
    Ok(())
}
#[test]
fn byte_memmove_zero_and_range_traps_execute() -> Result<(), Report> {
    let procedure = procedure(&Source {
        case: Case::Memmove,
        live: vec![],
    });
    let expected = (0..48).map(|i| (i * 37 + 11) as u8).collect::<Vec<_>>();
    execute_memmove(&procedure, u32::MAX as usize, u32::MAX as usize, 0, Some(&expected), false)?;
    for (src, dst, message) in [
        (u32::MAX as usize, 1024, "memmove source range is out of bounds"),
        (1024, u32::MAX as usize, "memmove destination range is out of bounds"),
    ] {
        let error = execute_memmove(&procedure, src, dst, 1, None, false).unwrap_err();
        assert!(
            format!("{error:?}")
                .contains(&miden_core::mast::error_code_from_msg(message).to_string()),
            "{error:?}"
        );
    }
    Ok(())
}
pub(crate) fn rewrite(body: &mut AnnotatedMasmBody, change: &mut impl FnMut(&mut AnnotatedOp)) {
    for op in body.ops_mut() {
        change(op);
        match op {
            AnnotatedOp::If {
                then_body,
                else_body,
                ..
            } => {
                rewrite(then_body, change);
                rewrite(else_body, change);
            }
            AnnotatedOp::While { body, .. } | AnnotatedOp::Repeat { body, .. } => {
                rewrite(body, change)
            }
            _ => {}
        }
    }
}
#[test]
fn physically_valid_direction_mutation_needs_execution() -> Result<(), Report> {
    let (plan, frame, mut body) = ready(
        &Source {
            case: Case::Memmove,
            live: vec![],
        },
        &AllocationOptions::default(),
    );
    let mut count = 0;
    rewrite(&mut body, &mut |op| {
        if let AnnotatedOp::Instruction { instruction, .. } = op
            && *instruction == Instruction::U32Gt
        {
            *instruction = Instruction::Eq;
            count += 1;
        }
    });
    assert_eq!(count, 1);
    let verified = verify::verify(&plan, &frame, body).unwrap();
    let procedure = verified.into_masm(masm::ProcedureName::new("fixture").unwrap()).unwrap();
    let mut expected = (0..48).map(|i| (i * 37 + 11) as u8).collect::<Vec<_>>();
    expected.copy_within(1..20, 3);
    let error = execute_memmove(&procedure, 1025, 1027, 19, Some(&expected), false).unwrap_err();
    assert!(format!("{error:?}").contains("assert"), "{error:?}");
    Ok(())
}

pub(crate) fn host() -> miden_processor::DefaultHost {
    let mut host = miden_processor::DefaultHost::default();
    for event in [crate::FRAME_START_EVENT, crate::FRAME_END_EVENT] {
        host.register_handler(
            event,
            alloc::sync::Arc::new(miden_processor::event::NoopEventHandler),
        )
        .unwrap();
    }
    host
}

#[test]
fn registered_scalar_alias_and_scratch_definitions_use_real_effects() {
    use crate::lower::units::{Scalar, memory::SpanningCell};
    let (unit, proof) = Scalar::Identity
        .select(SelectedStepId(0), SourceStepId(0), &[atom(0)], &[atom(1)])
        .unwrap();
    assert_eq!(proof.aliases.source(atom(1)), Some(atom(0)));
    assert_eq!(unit.stack_inputs, [atom(0)]);
    let requirement = ScratchRequirement {
        id: ScratchId(0),
        size: 1,
        alignment: 1,
        contiguous: true,
        address_space: AddressSpace::ProcedureLocal,
        lifetime: ScratchLifetime::Spanning,
        uses: vec![SelectedStepId(1), SelectedStepId(2)],
    };
    let (write, _) = SpanningCell::Write
        .select(SelectedStepId(1), SourceStepId(1), &requirement, ContentId(0), &[atom(0)], &[])
        .unwrap();
    let (read, proof) = SpanningCell::Read
        .select(SelectedStepId(2), SourceStepId(2), &requirement, ContentId(0), &[], &[atom(2)])
        .unwrap();
    assert_eq!(write.scratch_effects.len(), 1);
    assert_eq!(read.scratch_effects.len(), 1);
    assert_eq!(proof.scratch_outputs.len(), 1);
}

#[test]
fn fpi_full_width_reserves_six_stored_atoms_through_single_unit() {
    use alloc::collections::BTreeSet;
    let source = Source {
        case: Case::Fpi(16),
        live: vec![0, 6, 21],
    };
    let (plan, frame, body) = ready(&source, &AllocationOptions::default());
    let unit = &plan.selected.segments[0].units[0];
    assert_eq!(unit.semantic_inputs.len(), 22);
    assert_eq!(unit.stack_inputs.len(), 16);
    assert_eq!(unit.outputs.len(), 16);
    let bindings = &plan.lowering_bindings[&SelectedStepId(0)];
    let offsets = bindings
        .stored_inputs
        .values()
        .map(|s| frame.resolve(FrameRef::Spill(*s)).unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(offsets.len(), 6);
    assert!(frame.spills().values().collect::<BTreeSet<_>>().len() >= 8);
    assert_eq!(
        plan.segment_actions[&SegmentId(0)]
            .iter()
            .filter(|a| matches!(a, crate::plan::actions::PlanAction::RunLowering { .. }))
            .count(),
        1
    );
    let start = body
        .ops()
        .iter()
        .position(|op| matches!(op, AnnotatedOp::Boundary(Boundary::UnitStart(_))))
        .unwrap();
    let end = body
        .ops()
        .iter()
        .position(|op| matches!(op, AnnotatedOp::Boundary(Boundary::UnitEnd(_))))
        .unwrap();
    assert!(
        !body.ops()[start + 1..end]
            .iter()
            .any(|op| matches!(op, AnnotatedOp::Boundary(_)))
    );
    assert_eq!(
        body.ops()[start + 1..end]
            .iter()
            .filter(|op| matches!(
                op,
                AnnotatedOp::Instruction {
                    instruction: Instruction::MemLoad,
                    ..
                }
            ))
            .count(),
        6
    );
    verify::verify(&plan, &frame, body).unwrap();
}

#[test]
fn dynamic_roots_preserve_live_arguments_and_reused_root_lanes() -> Result<(), Report> {
    let source = Source {
        case: Case::Context {
            invocation: Invocation::DynCall,
            arguments: 16,
            results: 1,
            root_argument: true,
        },
        live: vec![0, 1, 2, 3, 4, 19],
    };
    let (plan, frame, body) = ready(&source, &AllocationOptions::default());
    let allocation = frame.scratch()[&ScratchId(0)];
    assert_eq!(allocation.offset % 4, 0);
    assert_eq!(allocation.size, 4);
    let procedure = verify::verify(&plan, &frame, body)?
        .into_masm(masm::ProcedureName::new("fixture").unwrap())?
        .to_pretty_string();
    let mut callee = String::from("proc callee ");
    for i in 1..16 {
        if i == 1 {
            callee.push_str("swap.1 ");
        } else {
            callee.push_str(&format!("movup.{i} "));
        }
        callee.push_str("push.999 swap.1 drop ");
        if i == 1 {
            callee.push_str("swap.1 ");
        } else {
            callee.push_str(&format!("movdn.{i} "));
        }
    }
    callee.push_str("end");
    let code = format!(
        "{callee} {procedure} begin push.907.903.901 {} procref.callee exec.fixture dup.0 dup.2 \
         assert_eq drop procref.callee assert_eqw {} end",
        push_top(&(11..27).collect::<Vec<_>>()),
        assert_top(&[11, 26, 901, 903, 907])
    );
    let program = miden_assembly::Assembler::default()
        .assemble_program("root_reuse", code)?
        .try_into_program()?;
    miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
        .execute_sync(&program, &mut host())
        .map_err(|e| Report::msg(e.to_string()))?;
    Ok(())
}

#[test]
fn physically_valid_arithmetic_mutation_needs_execution() -> Result<(), Report> {
    let (plan, frame, mut body) = ready(
        &Source {
            case: Case::Add,
            live: vec![],
        },
        &AllocationOptions::default(),
    );
    let valid = verify::verify(&plan, &frame, body.clone())?
        .into_masm(masm::ProcedureName::new("fixture").unwrap())?
        .to_pretty_string();
    let program = miden_assembly::Assembler::default()
        .assemble_program(
            "arithmetic_correct",
            format!("{valid} begin push.11.7 exec.fixture push.18 assert_eq end"),
        )?
        .try_into_program()?;
    miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
        .execute_sync(&program, &mut host())
        .map_err(|e| Report::msg(e.to_string()))?;
    rewrite(&mut body, &mut |op| {
        if let AnnotatedOp::Instruction { instruction, .. } = op
            && *instruction == Instruction::Add
        {
            *instruction = Instruction::Sub;
        }
    });
    let wrong = verify::verify(&plan, &frame, body)?
        .into_masm(masm::ProcedureName::new("fixture").unwrap())?
        .to_pretty_string();
    let program = miden_assembly::Assembler::default()
        .assemble_program(
            "arithmetic_wrong",
            format!("{wrong} begin push.11.7 exec.fixture push.18 assert_eq end"),
        )?
        .try_into_program()?;
    let error = miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
        .execute_sync(&program, &mut host())
        .unwrap_err();
    assert!(error.to_string().contains("assertion"), "{error}");
    Ok(())
}
