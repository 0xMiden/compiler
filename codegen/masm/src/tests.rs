#![allow(unused_imports)]
use std::sync::Arc;

use midenc_hir::{
    self as hir,
    pass::{AnalysisManager, ConversionPass},
    testing::{self, TestContext},
    AbiParam, CallConv, Felt, FieldElement, FunctionIdent, Immediate, InstBuilder, Linkage,
    OperandStack, ProgramBuilder, Signature, SourceSpan, Stack, Type,
};
use prop::test_runner::{Config, TestRunner};
use proptest::prelude::*;
use smallvec::{smallvec, SmallVec};

use super::*;
use crate::intrinsics::I32_INTRINSICS_MODULE_NAME;

const MEMORY_SIZE_BYTES: u32 = 1048576 * 2; // Twice the size of the default Rust shadow stack size
const MEMORY_SIZE_VM_WORDS: u32 = MEMORY_SIZE_BYTES / 16;
/// In miden-sdk-alloc we require all allocations to be minimally word-aligned, i.e. 32 byte
/// alignment
const MIN_ALIGN: u32 = 32;

#[cfg(test)]
#[allow(unused_macros)]
macro_rules! assert_masm_output {
    ($lhs:expr, $rhs:expr) => {{
        let lhs = $lhs;
        let rhs = $rhs;
        if lhs != rhs {
            panic!(
                r#"
assertion failed: `(left matches right)`
left: `{}`,
right: `{}`"#,
                lhs, rhs
            );
        }
    }};
}

struct TestByEmulationHarness {
    context: TestContext,
    emulator: Emulator,
}
impl Default for TestByEmulationHarness {
    fn default() -> Self {
        let mut harness = Self {
            context: Default::default(),
            emulator: Default::default(),
        };
        // Ensure we request all codegen outputs
        harness
            .context
            .session
            .options
            .output_types
            .insert(midenc_session::OutputType::Masm, None);
        harness
            .context
            .session
            .options
            .output_types
            .insert(midenc_session::OutputType::Mast, None);
        harness
    }
}
#[allow(unused)]
impl TestByEmulationHarness {
    pub fn with_emulator_config(
        memory_size: usize,
        hp: usize,
        lp: usize,
        print_stack: bool,
    ) -> Self {
        let mut harness = Self {
            context: TestContext::default(),
            emulator: Emulator::new(
                memory_size.try_into().expect("invalid memory size"),
                hp.try_into().expect("invalid address"),
                lp.try_into().expect("invalid address"),
                print_stack,
            ),
        };

        // Ensure we request all codegen outputs
        harness
            .context
            .session
            .options
            .output_types
            .insert(midenc_session::OutputType::Masm, None);
        harness
            .context
            .session
            .options
            .output_types
            .insert(midenc_session::OutputType::Mast, None);

        // Set a default, reasonable, execution limit
        harness.set_cycle_budget(2000);
        harness
    }

    pub fn codegen(
        &self,
        program: &hir::Program,
        function: &mut hir::Function,
    ) -> CompilerResult<Box<Function>> {
        use midenc_hir::{
            pass::{Analysis, RewritePass},
            ProgramAnalysisKey,
        };
        use midenc_hir_analysis as analysis;
        use midenc_hir_transform as transform;

        // Analyze function
        let mut analyses = AnalysisManager::new();

        // Register program-wide analyses
        let global_analysis = analysis::GlobalVariableAnalysis::<hir::Program>::analyze(
            program,
            &mut analyses,
            &self.context.session,
        )?;
        analyses.insert(ProgramAnalysisKey, global_analysis);

        // Apply pre-codegen transformations
        let mut rewrites = crate::default_function_rewrites(&self.context.session);
        rewrites.apply(function, &mut analyses, &self.context.session)?;

        println!("{}", function);

        // Run stackification
        let mut convert_to_masm = ConvertHirToMasm::<&hir::Function>::default();
        convert_to_masm
            .convert(function, &mut analyses, &self.context.session)
            .map(Box::new)
    }

    pub fn set_cycle_budget(&mut self, budget: usize) {
        self.emulator.set_max_cycles(budget);
    }

    #[allow(unused)]
    pub fn set_breakpoint(&mut self, bp: Breakpoint) {
        self.emulator.set_breakpoint(bp);
    }

    #[allow(unused)]
    pub fn clear_breakpoint(&mut self, bp: Breakpoint) {
        self.emulator.clear_breakpoint(bp);
    }

    #[allow(unused)]
    pub fn resume_until_break(&mut self) {
        match self.emulator.resume() {
            Ok(_) | Err(EmulationError::BreakpointHit(_)) => (),
            Err(other) => panic!("unexpected emulation error: {other}"),
        }
    }

    #[allow(unused)]
    pub fn resume_until_break_with_info(&mut self) {
        match self.emulator.resume() {
            Ok(_) | Err(EmulationError::BreakpointHit(_)) => {
                dbg!(self.emulator.info());
            }
            Err(other) => panic!("unexpected emulation error: {other}"),
        }
    }

    pub fn malloc(&mut self, size: usize) -> u32 {
        self.emulator.malloc(size)
    }

    #[inline(always)]
    pub fn store(&mut self, addr: usize, value: Felt) {
        self.emulator.store(addr, value);
    }

    pub fn execute_module(
        &mut self,
        module: Arc<Module>,
        args: &[Felt],
    ) -> Result<OperandStack<Felt>, EmulationError> {
        let entrypoint = module.entrypoint().expect("cannot execute a library");
        self.emulator.load_module(module).expect("failed to load module");
        self.emulator.invoke(entrypoint, args)
    }

    pub fn execute_program(
        &mut self,
        program: Arc<Program>,
        args: &[Felt],
    ) -> Result<OperandStack<Felt>, EmulationError> {
        self.emulator.load_program(program).expect("failed to load program");
        {
            let stack = self.emulator.stack_mut();
            for arg in args.iter().copied().rev() {
                stack.push(arg);
            }
        }
        self.emulator.start()
    }

    #[allow(unused)]
    pub fn execute_program_with_entry(
        &mut self,
        program: Arc<Program>,
        entrypoint: FunctionIdent,
        args: &[Felt],
    ) -> Result<OperandStack<Felt>, EmulationError> {
        self.emulator.load_program(program).expect("failed to load program");
        self.emulator.invoke(entrypoint, args)
    }

    #[inline]
    pub fn invoke(
        &mut self,
        entrypoint: FunctionIdent,
        args: &[Felt],
    ) -> Result<OperandStack<Felt>, EmulationError> {
        self.emulator.invoke(entrypoint, args)
    }

    #[inline]
    pub fn enter(&mut self, entrypoint: FunctionIdent, args: &[Felt]) {
        match self.emulator.enter(entrypoint, args) {
            Ok(_) | Err(EmulationError::BreakpointHit(_)) => {
                dbg!(self.emulator.info());
            }
            Err(other) => panic!("unexpected emulation error: {other}"),
        }
    }

    pub fn step_with_info(&mut self) {
        match self.emulator.step() {
            Ok(_) | Err(EmulationError::BreakpointHit(_)) => {
                dbg!(self.emulator.info());
            }
            Err(other) => panic!("unexpected emulation error: {other}"),
        }
    }

    #[inline]
    pub fn step(&mut self) -> Result<EmulatorEvent, EmulationError> {
        self.emulator.step()
    }

    #[inline]
    pub fn step_over(&mut self) -> Result<EmulatorEvent, EmulationError> {
        self.emulator.step_over()
    }

    fn reset(&mut self) {
        self.emulator.reset();
    }
}

#[test]
fn issue56() {
    let harness = TestByEmulationHarness::default();

    let mut builder = ProgramBuilder::new(&harness.context.session.diagnostics);
    let mut mb = builder.module("test");
    testing::issue56(mb.as_mut(), &harness.context);
    mb.build().unwrap();

    let program = builder.with_entrypoint("test::entrypoint".parse().unwrap()).link().unwrap();

    let mut compiler = MasmCompiler::new(&harness.context.session);
    compiler.compile(program).expect("compilation failed");
}

/// Test the emulator on the fibonacci function
#[test]
fn fib_emulator() {
    let mut harness = TestByEmulationHarness::default();

    // Build a simple program
    let mut builder = ProgramBuilder::new(&harness.context.session.diagnostics);

    // Build test module with fib function
    let mut mb = builder.module("test");
    testing::fib1(mb.as_mut(), &harness.context);
    mb.build().expect("unexpected error constructing test module");

    // Link the program
    let program = builder
        .with_entrypoint("test::fib".parse().unwrap())
        .link()
        .expect("failed to link program");

    let mut compiler = MasmCompiler::new(&harness.context.session);
    let program = compiler.compile(program).expect("compilation failed").unwrap_executable();

    println!("{}", program.get("test").unwrap());

    // Test it via the emulator
    let n = Felt::new(10);
    let mut stack = harness.execute_program(program.freeze(), &[n]).expect("execution failed");
    assert_eq!(stack.len(), 1);
    assert_eq!(stack.pop().map(|e| e.as_int()), Some(55));
}

/// Test the code generator on a very simple program with a conditional as a sanity check
#[test]
fn codegen_fundamental_if() {
    let mut harness = TestByEmulationHarness::default();

    // Build a simple program
    let mut builder = ProgramBuilder::new(&harness.context.session.diagnostics);

    // Build test module with function that adds two numbers if the
    // first number is odd, and multiplies them if the first number is even
    let mut mb = builder.module("test");
    let id = {
        let mut fb = mb
            .function(
                "add_odd_mul_even",
                Signature::new(
                    [AbiParam::new(Type::U32), AbiParam::new(Type::U32)],
                    [AbiParam::new(Type::U32)],
                ),
            )
            .expect("unexpected symbol conflict");
        let entry = fb.current_block();
        let (a, b) = {
            let args = fb.block_params(entry);
            (args[0], args[1])
        };
        let is_odd_blk = fb.create_block();
        let is_even_blk = fb.create_block();
        let is_odd = fb.ins().is_odd(a, SourceSpan::UNKNOWN);
        fb.ins().cond_br(is_odd, is_odd_blk, &[], is_even_blk, &[], SourceSpan::UNKNOWN);
        fb.switch_to_block(is_odd_blk);
        let c = fb.ins().add_checked(a, b, SourceSpan::UNKNOWN);
        fb.ins().ret(Some(c), SourceSpan::UNKNOWN);
        fb.switch_to_block(is_even_blk);
        let d = fb.ins().mul_checked(a, b, SourceSpan::UNKNOWN);
        fb.ins().ret(Some(d), SourceSpan::UNKNOWN);
        fb.build().expect("unexpected error building function")
    };

    mb.build().expect("unexpected error constructing test module");

    // Link the program
    let program = builder.with_entrypoint(id).link().expect("failed to link program");

    let mut compiler = MasmCompiler::new(&harness.context.session);
    let program = compiler.compile(program).expect("compilation failed").unwrap_executable();

    let a = Felt::new(3);
    let b = Felt::new(4);

    let mut stack = harness.execute_program(program.freeze(), &[a, b]).expect("execution failed");
    assert_eq!(stack.len(), 1);
    assert_eq!(stack.pop().map(|e| e.as_int()), Some(12));
}

/// Test the code generator on a very simple program with a loop as a sanity check
#[test]
fn codegen_fundamental_loops() {
    let mut harness = TestByEmulationHarness::default();

    // Build a simple program
    let mut builder = ProgramBuilder::new(&harness.context.session.diagnostics);

    // Build test module with function that increments a number until a
    // given iteration count is reached
    let mut mb = builder.module("test");
    let id = {
        let mut fb = mb
            .function(
                "incr_until",
                Signature::new(
                    [AbiParam::new(Type::U32), AbiParam::new(Type::U32)],
                    [AbiParam::new(Type::U32)],
                ),
            )
            .expect("unexpected symbol conflict");
        let entry = fb.current_block();
        let (a, n) = {
            let args = fb.block_params(entry);
            (args[0], args[1])
        };
        let loop_header_blk = fb.create_block();
        let a1 = fb.append_block_param(loop_header_blk, Type::U32, SourceSpan::UNKNOWN);
        let n1 = fb.append_block_param(loop_header_blk, Type::U32, SourceSpan::UNKNOWN);
        let loop_body_blk = fb.create_block();
        let loop_exit_blk = fb.create_block();
        let result0 = fb.append_block_param(loop_exit_blk, Type::U32, SourceSpan::UNKNOWN);
        fb.ins().br(loop_header_blk, &[a, n], SourceSpan::UNKNOWN);

        fb.switch_to_block(loop_header_blk);
        let is_zero = fb.ins().eq_imm(n1, Immediate::U32(0), SourceSpan::UNKNOWN);
        fb.ins()
            .cond_br(is_zero, loop_exit_blk, &[a1], loop_body_blk, &[], SourceSpan::UNKNOWN);

        fb.switch_to_block(loop_body_blk);
        let a2 = fb.ins().incr_checked(a1, SourceSpan::UNKNOWN);
        let n2 = fb.ins().sub_imm_checked(n1, Immediate::U32(1), SourceSpan::UNKNOWN);
        fb.ins().br(loop_header_blk, &[a2, n2], SourceSpan::UNKNOWN);

        fb.switch_to_block(loop_exit_blk);
        fb.ins().ret(Some(result0), SourceSpan::UNKNOWN);

        fb.build().expect("unexpected error building function")
    };

    mb.build().expect("unexpected error constructing test module");

    // Link the program
    let program = builder.with_entrypoint(id).link().expect("failed to link program");

    let mut compiler = MasmCompiler::new(&harness.context.session);
    let program = compiler.compile(program).expect("compilation failed").unwrap_executable();

    let a = Felt::new(3);
    let n = Felt::new(4);

    let mut stack = harness.execute_program(program.freeze(), &[a, n]).expect("execution failed");
    assert_eq!(stack.len(), 1);
    assert_eq!(stack.pop().map(|e| e.as_int()), Some(7));
}

/// Test the code generator on a simple program containing [testing::sum_matrix].
#[test]
fn codegen_sum_matrix() {
    let mut harness = TestByEmulationHarness::default();

    // Build a simple program
    let mut builder = ProgramBuilder::new(&harness.context.session.diagnostics);

    // Build test module with fib function
    let mut mb = builder.module("test");
    testing::sum_matrix(mb.as_mut(), &harness.context);
    mb.build().expect("unexpected error constructing test module");

    // Link the program
    let program = builder
        .with_entrypoint("test::sum_matrix".parse().unwrap())
        .link()
        .expect("failed to link program");

    // Compile
    let mut compiler = MasmCompiler::new(&harness.context.session);
    let program = compiler.compile(program).expect("compilation failed").unwrap_executable();

    println!("{}", program.get("test").unwrap());

    // Prep emulator
    let addr = harness.malloc(core::mem::size_of::<u32>() * 3 * 3);
    let ptr = Felt::new(addr as u64);
    let rows = Felt::new(3);
    let cols = Felt::new(3);

    // [1, 0, 1,
    //  0, 1, 0,
    //  1, 1, 1] == 6
    let addr = addr as usize;
    harness.store(addr, Felt::ONE);
    harness.store(addr + 4, Felt::ZERO);
    harness.store(addr + 8, Felt::ONE);
    harness.store(addr + 12, Felt::ZERO);
    harness.store(addr + 16, Felt::ONE);
    harness.store(addr + 20, Felt::ZERO);
    harness.store(addr + 24, Felt::ONE);
    harness.store(addr + 28, Felt::ONE);
    harness.store(addr + 32, Felt::ONE);

    // Execute test::sum_matrix
    let mut stack = harness
        .execute_program(program.freeze(), &[ptr, rows, cols])
        .expect("execution failed");
    assert_eq!(stack.len(), 1);
    assert_eq!(stack.pop().map(|e| e.as_int()), Some(6));
}

#[test]
#[should_panic(expected = "assertion failed: expected false, got true")]
fn i32_checked_neg() {
    let mut harness = TestByEmulationHarness::default();

    harness
        .emulator
        .load_module(
            Box::new(
                intrinsics::load(
                    I32_INTRINSICS_MODULE_NAME,
                    &harness.context.session.source_manager,
                )
                .expect("undefined intrinsic module"),
            )
            .freeze(),
        )
        .expect("failed to load intrinsics::i32");

    let min = Felt::new(i32::MIN as u32 as u64);

    let neg = "intrinsics::i32::checked_neg".parse().unwrap();
    // i32::MIN
    harness.invoke(neg, &[min]).expect("execution failed");
}

#[test]
fn codegen_mem_store_sw_load_sw() {
    let context = TestContext::default();
    let mut builder = ProgramBuilder::new(&context.session.diagnostics);
    let mut mb = builder.module("test");
    let id = {
        let mut fb = mb
            .function(
                "store_load_sw",
                Signature::new(
                    [AbiParam::new(Type::U32), AbiParam::new(Type::U32)],
                    [AbiParam::new(Type::U32)],
                ),
            )
            .expect("unexpected symbol conflict");
        let entry = fb.current_block();
        let (ptr_u32, value) = {
            let args = fb.block_params(entry);
            (args[0], args[1])
        };
        let ptr = fb.ins().inttoptr(ptr_u32, Type::Ptr(Type::U32.into()), SourceSpan::UNKNOWN);
        fb.ins().store(ptr, value, SourceSpan::UNKNOWN);
        let loaded_value = fb.ins().load(ptr, SourceSpan::UNKNOWN);
        fb.ins().ret(Some(loaded_value), SourceSpan::UNKNOWN);
        fb.build().expect("unexpected error building function")
    };

    mb.build().expect("unexpected error constructing test module");

    let program = builder.with_entrypoint(id).link().expect("failed to link program");

    let mut compiler = MasmCompiler::new(&context.session);
    let program = compiler
        .compile(program)
        .expect("compilation failed")
        .unwrap_executable()
        .freeze();

    // eprintln!("{}", program);

    fn roundtrip(program: Arc<Program>, ptr: u32, value: u32) -> u32 {
        eprintln!("---------------------------------");
        eprintln!("testing store_sw/load_sw ptr: {ptr}, value: {value}");
        eprintln!("---------------------------------");
        let mut harness = TestByEmulationHarness::with_emulator_config(
            MEMORY_SIZE_VM_WORDS as usize,
            Emulator::DEFAULT_HEAP_START as usize,
            Emulator::DEFAULT_LOCALS_START as usize,
            true,
        );
        let mut stack = harness
            .execute_program(program.clone(), &[Felt::new(ptr as u64), Felt::new(value as u64)])
            .expect("execution failed");
        stack.pop().unwrap().as_int() as u32
    }

    TestRunner::new(Config::with_cases(1024))
        .run(&(0u32..MEMORY_SIZE_BYTES - 4, any::<u32>()), move |(ptr, value)| {
            let out = roundtrip(program.clone(), ptr, value);
            prop_assert_eq!(out, value);
            Ok(())
        })
        .unwrap();
}

#[test]
fn codegen_mem_store_dw_load_dw() {
    let context = TestContext::default();
    let mut builder = ProgramBuilder::new(&context.session.diagnostics);
    let mut mb = builder.module("test");
    let id = {
        let mut fb = mb
            .function(
                "store_load_dw",
                Signature::new(
                    [AbiParam::new(Type::U32), AbiParam::new(Type::U64)],
                    [AbiParam::new(Type::U64)],
                ),
            )
            .expect("unexpected symbol conflict");
        let entry = fb.current_block();
        let (ptr_u32, value) = {
            let args = fb.block_params(entry);
            (args[0], args[1])
        };
        let ptr = fb.ins().inttoptr(ptr_u32, Type::Ptr(Type::U64.into()), SourceSpan::UNKNOWN);
        fb.ins().store(ptr, value, SourceSpan::UNKNOWN);
        let loaded_value = fb.ins().load(ptr, SourceSpan::UNKNOWN);
        fb.ins().ret(Some(loaded_value), SourceSpan::UNKNOWN);
        fb.build().expect("unexpected error building function")
    };

    mb.build().expect("unexpected error constructing test module");

    let program = builder.with_entrypoint(id).link().expect("failed to link program");

    let ir_module = program
        .modules()
        .iter()
        .take(1)
        .collect::<Vec<&midenc_hir::Module>>()
        .first()
        .expect("no module in IR program")
        .to_string();

    eprintln!("{}", ir_module.as_str());

    let mut compiler = MasmCompiler::new(&context.session);
    let program = compiler
        .compile(program)
        .expect("compilation failed")
        .unwrap_executable()
        .freeze();

    eprintln!("{}", program);

    fn roundtrip(program: Arc<Program>, ptr: u32, value: u64) -> u64 {
        eprintln!("---------------------------------");
        eprintln!("testing store_dw/load_dw ptr: {ptr}, value: {value}");
        eprintln!("---------------------------------");
        let mut harness = TestByEmulationHarness::with_emulator_config(
            MEMORY_SIZE_VM_WORDS as usize,
            Emulator::DEFAULT_HEAP_START as usize,
            Emulator::DEFAULT_LOCALS_START as usize,
            true,
        );
        let mut args: SmallVec<[Felt; 4]> = smallvec!(Felt::new(ptr as u64));
        args.extend(value.canonicalize());
        let mut stack = harness.execute_program(program.clone(), &args).expect("execution failed");
        u64::from_stack(&mut stack)
    }

    TestRunner::new(Config::with_cases(1024))
        .run(&(0u32..MEMORY_SIZE_BYTES - 4, any::<u64>()), move |(ptr, value)| {
            let out = roundtrip(program.clone(), ptr, value);
            prop_assert_eq!(out, value);
            Ok(())
        })
        .unwrap();
}

#[test]
fn codegen_mem_store_felt_load_felt() {
    let context = TestContext::default();
    let mut builder = ProgramBuilder::new(&context.session.diagnostics);
    let mut mb = builder.module("test");
    let id = {
        let mut fb = mb
            .function(
                "store_load_felt",
                Signature::new(
                    [AbiParam::new(Type::U32), AbiParam::new(Type::Felt)],
                    [AbiParam::new(Type::Felt)],
                ),
            )
            .expect("unexpected symbol conflict");
        let entry = fb.current_block();
        let (ptr_u32, value) = {
            let args = fb.block_params(entry);
            (args[0], args[1])
        };
        let ptr = fb.ins().inttoptr(ptr_u32, Type::Ptr(Type::Felt.into()), SourceSpan::UNKNOWN);
        fb.ins().store(ptr, value, SourceSpan::UNKNOWN);
        let loaded_value = fb.ins().load(ptr, SourceSpan::UNKNOWN);
        fb.ins().ret(Some(loaded_value), SourceSpan::UNKNOWN);
        fb.build().expect("unexpected error building function")
    };

    mb.build().expect("unexpected error constructing test module");

    let program = builder.with_entrypoint(id).link().expect("failed to link program");

    let ir_module = program
        .modules()
        .iter()
        .take(1)
        .collect::<Vec<&midenc_hir::Module>>()
        .first()
        .expect("no module in IR program")
        .to_string();

    eprintln!("{}", ir_module.as_str());

    let mut compiler = MasmCompiler::new(&context.session);
    let program = compiler
        .compile(program)
        .expect("compilation failed")
        .unwrap_executable()
        .freeze();

    eprintln!("{}", program);

    fn roundtrip(program: Arc<Program>, ptr: u32, value: Felt) -> Felt {
        eprintln!("---------------------------------");
        eprintln!("testing store_felt/load_felt ptr: {ptr}, value: {value}");
        eprintln!("---------------------------------");
        let mut harness = TestByEmulationHarness::with_emulator_config(
            MEMORY_SIZE_VM_WORDS as usize,
            Emulator::DEFAULT_HEAP_START as usize,
            Emulator::DEFAULT_LOCALS_START as usize,
            true,
        );
        let mut stack = harness
            .execute_program(program.clone(), &[Felt::new(ptr as u64), value])
            .expect("execution failed");
        stack.pop().unwrap()
    }

    TestRunner::new(Config::with_cases(1024))
        .run(
            &(0u32..(MEMORY_SIZE_BYTES / MIN_ALIGN - 1), (0u64..u64::MAX).prop_map(Felt::new)),
            move |(word_ptr, value)| {
                // a felt memory pointer must be naturally aligned, i.e. a multiple of MIN_ALIGN
                let ptr = word_ptr * MIN_ALIGN;
                let out = roundtrip(program.clone(), ptr, value);
                prop_assert_eq!(out, value);
                Ok(())
            },
        )
        .unwrap();
}

#[allow(unused)]
macro_rules! proptest_unary_numeric_op {
    ($ty_name:ident :: $op:ident, $ty:ty => $ret:ty, $rust_op:ident) => {
        proptest_unary_numeric_op_impl!($ty_name :: $op, $ty => $ret, $rust_op, 0..$ty_name::MAX);
    };

    ($ty_name:ident :: $op:ident, $ty:ty => $ret:ty, $rust_op:ident, $strategy:expr) => {
        proptest_unary_numeric_op_impl!($ty_name :: $op, $ty => $ret, $rust_op, $strategy);
    };
}

#[allow(unused)]
macro_rules! proptest_unary_numeric_op_impl {
    ($ty_name:ident :: $op:ident, $ty:ty => $ret:ty, $rust_op:ident, $strategy:expr) => {
        paste::paste! {
            #[test]
            fn [<$ty_name _ $op>]() {
                let mut harness = TestByEmulationHarness::default();

                // Build a simple program that invokes the clz instruction
                let mut builder = ProgramBuilder::new(&harness.context.session.diagnostics);

                let mut mb = builder.module("test");
                let sig = Signature {
                    params: vec![AbiParam::new(<$ty as ToCanonicalRepr>::ir_type())],
                    results: vec![AbiParam::new(<$ret as ToCanonicalRepr>::ir_type())],
                    cc: CallConv::SystemV,
                    linkage: Linkage::External,
                };
                let mut fb = mb.function(stringify!([<$ty_name _ $op>]), sig).expect("unexpected symbol conflict");

                let entry = fb.current_block();
                // Get the value for `v0`
                let v0 = {
                    let args = fb.block_params(entry);
                    args[0]
                };

                let v1 = fb.ins().$op(v0, harness.context.current_span());
                fb.ins().ret(Some(v1), harness.context.current_span());

                let entrypoint = fb.build().expect("unexpected validation error, see diagnostics output");
                mb.build().expect("unexpected error constructing test module");

                // Link the program
                let program = builder.with_entrypoint(entrypoint).link().expect("failed to link program");

                // Compile
                let mut compiler = MasmCompiler::new(&harness.context.session);
                let program = compiler.compile(program).expect("compilation failed").unwrap_executable();

                harness
                    .emulator
                    .load_program(program.freeze())
                    .expect("failed to load test program");

                let harness = RefCell::new(harness);

                proptest!(ProptestConfig { cases: 1000, failure_persistence: None, ..Default::default() }, |(n in ($strategy))| {
                    let mut harness = harness.borrow_mut();
                    harness.emulator.stop();

                    // Convert to canonical Miden representation, N field elements, each containing a
                    // 32-bit chunk, highest bits closest to top of stack.
                    let elems = n.canonicalize();

                    let mut stack = harness.invoke(entrypoint, &elems).expect("execution failed");
                    harness.emulator.stop();
                    prop_assert_eq!(stack.len(), <$ret as ToCanonicalRepr>::ir_type().size_in_felts());

                    // Obtain the count of leading zeroes from stack and check that it matches the expected
                    // count
                    let result = <$ret as ToCanonicalRepr>::from_stack(&mut stack);
                    prop_assert_eq!(result, n.$rust_op());
                });
            }
        }
    };
}

//proptest_unary_numeric_op!(u64::clz, u64 => u32, leading_zeros);
//proptest_unary_numeric_op!(i128::clz, i128 => u32, leading_zeros);
//proptest_unary_numeric_op!(u64::ctz, u64 => u32, trailing_zeros);
//proptest_unary_numeric_op!(i128::ctz, i128 => u32, trailing_zeros);
//proptest_unary_numeric_op!(u64::clo, u64 => u32, leading_ones);
//proptest_unary_numeric_op!(i128::clo, i128 => u32, leading_ones);
//proptest_unary_numeric_op!(u64::cto, u64 => u32, trailing_ones);
//proptest_unary_numeric_op!(i128::cto, i128 => u32, trailing_ones);
//proptest_unary_numeric_op!(u64::ilog2, u64 => u32, ilog2, 1..u64::MAX);
//proptest_unary_numeric_op!(i128::ilog2, i128 => u32, ilog2, 1..i128::MAX);

#[allow(unused)]
trait ToCanonicalRepr {
    fn ir_type() -> Type;
    fn canonicalize(self) -> SmallVec<[Felt; 4]>;
    fn from_stack(stack: &mut OperandStack<Felt>) -> Self;
}

impl ToCanonicalRepr for u8 {
    fn ir_type() -> Type {
        Type::U8
    }

    fn canonicalize(self) -> SmallVec<[Felt; 4]> {
        smallvec![Felt::new(self as u64)]
    }

    fn from_stack(stack: &mut OperandStack<Felt>) -> Self {
        let raw = stack.pop().unwrap().as_int();
        u8::try_from(raw).unwrap_or_else(|_| {
            panic!("invalid result: expected valid u8, but value is out of range: {raw}")
        })
    }
}

impl ToCanonicalRepr for i8 {
    fn ir_type() -> Type {
        Type::I8
    }

    fn canonicalize(self) -> SmallVec<[Felt; 4]> {
        smallvec![Felt::new(self as u8 as u64)]
    }

    fn from_stack(stack: &mut OperandStack<Felt>) -> Self {
        <u8 as ToCanonicalRepr>::from_stack(stack) as i8
    }
}

impl ToCanonicalRepr for u16 {
    fn ir_type() -> Type {
        Type::U16
    }

    fn canonicalize(self) -> SmallVec<[Felt; 4]> {
        smallvec![Felt::new(self as u64)]
    }

    fn from_stack(stack: &mut OperandStack<Felt>) -> Self {
        let raw = stack.pop().unwrap().as_int();
        u16::try_from(raw).unwrap_or_else(|_| {
            panic!("invalid result: expected valid u16, but value is out of range: {raw}")
        })
    }
}

impl ToCanonicalRepr for i16 {
    fn ir_type() -> Type {
        Type::I16
    }

    fn canonicalize(self) -> SmallVec<[Felt; 4]> {
        smallvec![Felt::new(self as u16 as u64)]
    }

    fn from_stack(stack: &mut OperandStack<Felt>) -> Self {
        <u16 as ToCanonicalRepr>::from_stack(stack) as i16
    }
}

impl ToCanonicalRepr for u32 {
    fn ir_type() -> Type {
        Type::U32
    }

    fn canonicalize(self) -> SmallVec<[Felt; 4]> {
        smallvec![Felt::new(self as u64)]
    }

    fn from_stack(stack: &mut OperandStack<Felt>) -> Self {
        let raw = stack.pop().unwrap().as_int();
        u32::try_from(raw).unwrap_or_else(|_| {
            panic!("invalid result: expected valid u32, but value is out of range: {raw}")
        })
    }
}

impl ToCanonicalRepr for i32 {
    fn ir_type() -> Type {
        Type::I32
    }

    fn canonicalize(self) -> SmallVec<[Felt; 4]> {
        smallvec![Felt::new(self as u32 as u64)]
    }

    fn from_stack(stack: &mut OperandStack<Felt>) -> Self {
        <u32 as ToCanonicalRepr>::from_stack(stack) as i32
    }
}

impl ToCanonicalRepr for u64 {
    fn ir_type() -> Type {
        Type::U64
    }

    fn canonicalize(self) -> SmallVec<[Felt; 4]> {
        let lo = self.rem_euclid(2u64.pow(32));
        let hi = self.div_euclid(2u64.pow(32));
        smallvec![Felt::new(hi), Felt::new(lo)]
    }

    fn from_stack(stack: &mut OperandStack<Felt>) -> Self {
        let hi = stack.pop().unwrap().as_int() * 2u64.pow(32);
        let lo = stack.pop().unwrap().as_int();
        hi + lo
    }
}

impl ToCanonicalRepr for i64 {
    fn ir_type() -> Type {
        Type::I64
    }

    fn canonicalize(self) -> SmallVec<[Felt; 4]> {
        (self as u64).canonicalize()
    }

    fn from_stack(stack: &mut OperandStack<Felt>) -> Self {
        <u64 as ToCanonicalRepr>::from_stack(stack) as i64
    }
}

impl ToCanonicalRepr for i128 {
    fn ir_type() -> Type {
        Type::I128
    }

    fn canonicalize(self) -> SmallVec<[Felt; 4]> {
        let lo = self.rem_euclid(2i128.pow(64));
        let hi = self.div_euclid(2i128.pow(64));
        let mut out = (hi as u64).canonicalize();
        out.extend_from_slice(&(lo as u64).canonicalize());
        out
    }

    fn from_stack(stack: &mut OperandStack<Felt>) -> Self {
        let hi = (<u64 as ToCanonicalRepr>::from_stack(stack) as i128) * 2i128.pow(64);
        let lo = <u64 as ToCanonicalRepr>::from_stack(stack) as i128;
        hi + lo
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 1000, failure_persistence: None, ..Default::default() })]

    #[test]
    fn i32_icmp(a: i32, b: i32) {
        use core::cmp::Ordering;

        let mut harness = TestByEmulationHarness::default();

        harness
            .emulator
            .load_module(
                Box::new(
                    intrinsics::load(I32_INTRINSICS_MODULE_NAME, &harness.context.session.source_manager)
                        .expect("undefined intrinsics module"),
                )
                .freeze(),
            )
            .expect("failed to load intrinsics::i32");
        // NOTE: arguments are passed in reverse, i.e. [b, a] not [a, b]
        let icmp = "intrinsics::i32::icmp".parse().unwrap();
        let is_gt = "intrinsics::i32::is_gt".parse().unwrap();
        let is_lt = "intrinsics::i32::is_lt".parse().unwrap();

        let a_felt = Felt::new(a as u32 as u64);
        let b_felt = Felt::new(b as u32 as u64);
        let mut stack = harness.invoke(icmp, &[b_felt, a_felt]).expect("execution failed");
        harness.emulator.stop();
        prop_assert_eq!(stack.len(), 1);

        const NEG_ONE_U64: u64 = -1i32 as u32 as u64;
        let result = match stack.pop().unwrap().as_int() {
            NEG_ONE_U64 => Ordering::Less,
            0 => Ordering::Equal,
            1 => Ordering::Greater,
            other => panic!("invalid comparison result, expected -1, 0, or 1 but got {other}"),
        };

        prop_assert_eq!(result, a.cmp(&b));

        let mut stack = harness.invoke(is_gt, &[b_felt, a_felt]).expect("execution failed");
        harness.emulator.stop();
        prop_assert_eq!(stack.len(), 1);

        let result = match stack.pop().unwrap().as_int() {
            0 => false,
            1 => true,
            other => panic!("invalid boolean result, expected 0 or 1, got {other}"),
        };

        prop_assert_eq!(result, a.cmp(&b).is_gt());

        let mut stack = harness.invoke(is_lt, &[b_felt, a_felt]).expect("execution failed");
        harness.emulator.stop();
        prop_assert_eq!(stack.len(), 1);

        let result = match stack.pop().unwrap().as_int() {
            0 => false,
            1 => true,
            other => panic!("invalid boolean result, expected 0 or 1, got {other}"),
        };

        prop_assert_eq!(result, a.cmp(&b).is_lt());
    }

    #[test]
    fn i32_is_signed(a: i32) {
        let mut harness = TestByEmulationHarness::default();

        harness
            .emulator
            .load_module(
                Box::new(
                    intrinsics::load(I32_INTRINSICS_MODULE_NAME, &harness.context.session.source_manager)
                        .expect("undefined intrinsic module"),
                )
                .freeze(),
            )
            .expect("failed to load intrinsics::i32");

        let a_felt = Felt::new(a as u32 as u64);
        let is_signed = "intrinsics::i32::is_signed".parse().unwrap();
        let mut stack = harness.invoke(is_signed, &[a_felt]).expect("execution failed");
        prop_assert_eq!(stack.len(), 1);
        let result = stack.pop().unwrap().as_int();

        prop_assert_eq!(result, a.is_negative() as u64);
    }

    #[test]
    fn i32_overflowing_add(a: i32, b: i32) {
        let mut harness = TestByEmulationHarness::default();

        harness
            .emulator
            .load_module(
                Box::new(
                    intrinsics::load(I32_INTRINSICS_MODULE_NAME, &harness.context.session.source_manager)
                        .expect("undefined intrinsic module"),
                )
                .freeze(),
            )
            .expect("failed to load intrinsics::i32");

        let a_felt = Felt::new(a as u32 as u64);
        let b_felt = Felt::new(b as u32 as u64);
        // NOTE: arguments are passed in reverse, i.e. [b, a] not [a, b]
        let add = "intrinsics::i32::overflowing_add".parse().unwrap();
        let mut stack = harness
            .invoke(add, &[b_felt, a_felt])
            .expect("execution failed");
        prop_assert_eq!(stack.len(), 2);

        let overflowed = stack.pop().unwrap() == Felt::ONE;
        let raw_result = stack.pop().unwrap().as_int();
        let result = u32::try_from(raw_result).map_err(|_| raw_result).map(|res| res as i32);
        let (expected_result, expected_overflow) = a.overflowing_add(b);

        prop_assert_eq!((result, overflowed), (Ok(expected_result), expected_overflow));
    }

    #[test]
    fn i32_overflowing_sub(a: i32, b: i32) {
        let mut harness = TestByEmulationHarness::default();

        harness
            .emulator
            .load_module(
                Box::new(
                    intrinsics::load(I32_INTRINSICS_MODULE_NAME, &harness.context.session.source_manager)
                        .expect("undefined intrinsic module"),
                )
                .freeze(),
            )
            .expect("failed to load intrinsics::i32");

        let a_felt = Felt::new(a as u32 as u64);
        let b_felt = Felt::new(b as u32 as u64);
        // NOTE: arguments are passed in reverse, i.e. [b, a] not [a, b]
        let sub = "intrinsics::i32::overflowing_sub".parse().unwrap();
        let mut stack = harness
            .invoke(sub, &[b_felt, a_felt])
            .expect("execution failed");
        prop_assert_eq!(stack.len(), 2);

        let overflowed = stack.pop().unwrap() == Felt::ONE;
        let raw_result = stack.pop().unwrap().as_int();
        let result = u32::try_from(raw_result).map_err(|_| raw_result).map(|res| res as i32);
        let (expected_result, expected_overflow) = a.overflowing_sub(b);

        prop_assert_eq!((result, overflowed), (Ok(expected_result), expected_overflow));
    }

    #[test]
    fn i32_overflowing_mul(a: i32, b: i32) {
        let mut harness = TestByEmulationHarness::default();

        harness
            .emulator
            .load_module(
                Box::new(
                    intrinsics::load(I32_INTRINSICS_MODULE_NAME, &harness.context.session.source_manager)
                        .expect("undefined intrinsic module"),
                )
                .freeze(),
            )
            .expect("failed to load intrinsics::i32");

        let a_felt = Felt::new(a as u32 as u64);
        let b_felt = Felt::new(b as u32 as u64);
        // NOTE: arguments are passed in reverse, i.e. [b, a] not [a, b]
        let mul = "intrinsics::i32::overflowing_mul".parse().unwrap();
        let mut stack = harness
            .invoke(mul, &[b_felt, a_felt])
            .expect("execution failed");
        prop_assert_eq!(stack.len(), 2);

        let overflowed = stack.pop().unwrap() == Felt::ONE;
        let raw_result = stack.pop().unwrap().as_int();
        let result = u32::try_from(raw_result).map_err(|_| raw_result).map(|res| res as i32);
        let (expected_result, expected_overflow) = a.overflowing_mul(b);

        prop_assert_eq!((result, overflowed), (Ok(expected_result), expected_overflow));
    }

    #[test]
    fn i32_unchecked_neg(a: i32) {
        prop_assume!(a != i32::MIN, "unchecked_neg is meaningless for i32::MIN");

        let mut harness = TestByEmulationHarness::default();

        harness
            .emulator
            .load_module(
                Box::new(
                    intrinsics::load(I32_INTRINSICS_MODULE_NAME, &harness.context.session.source_manager)
                        .expect("undefined intrinsic module"),
                )
                .freeze(),
            )
            .expect("failed to load intrinsics::i32");

        let a_felt = Felt::new(a as u32 as u64);
        let neg = "intrinsics::i32::unchecked_neg".parse().unwrap();
        let mut stack = harness.invoke(neg, &[a_felt]).expect("execution failed");

        prop_assert_eq!(stack.len(), 1);
        let raw_result = stack.pop().unwrap().as_int();
        let result = u32::try_from(raw_result).map_err(|_| raw_result).map(|res| res as i32);
        prop_assert_eq!(result, Ok(-a));
    }

    #[test]
    fn i32_checked_div(a: i32, b: core::num::NonZeroI32) {
        let mut harness = TestByEmulationHarness::default();

        harness
            .emulator
            .load_module(
                Box::new(
                    intrinsics::load(I32_INTRINSICS_MODULE_NAME, &harness.context.session.source_manager)
                        .expect("undefined intrinsic module"),
                )
                .freeze(),
            )
            .expect("failed to load intrinsics::i32");

        let a_felt = Felt::new(a as u32 as u64);
        let b_felt = Felt::new(b.get() as u32 as u64);
        let div = "intrinsics::i32::checked_div".parse().unwrap();
        let mut stack = harness.invoke(div, &[b_felt, a_felt]).expect("execution failed");

        prop_assert_eq!(stack.len(), 1);
        let raw_result = stack.pop().unwrap().as_int();
        let result = u32::try_from(raw_result).map_err(|_| raw_result).map(|res| res as i32);
        prop_assert_eq!(result, Ok(a / b.get()));
    }

    #[test]
    fn i32_pow2(a in 0u32..30u32) {
        let mut harness = TestByEmulationHarness::default();

        harness
            .emulator
            .load_module(
                Box::new(
                    intrinsics::load(I32_INTRINSICS_MODULE_NAME, &harness.context.session.source_manager)
                        .expect("undefined intrinsic module"),
                )
                .freeze(),
            )
            .expect("failed to load intrinsics::i32");

        let a_felt = Felt::new(a as u64);
        let pow2 = "intrinsics::i32::pow2".parse().unwrap();
        let mut stack = harness.invoke(pow2, &[a_felt]).expect("execution failed");

        prop_assert_eq!(stack.len(), 1);
        let raw_result = stack.pop().unwrap().as_int();
        let result = u32::try_from(raw_result).map_err(|_| raw_result).map(|res| res as i32);
        prop_assert_eq!(result, Ok(2i32.pow(a)));
    }

    #[test]
    fn i32_ipow(a: i32, b in 0u32..30u32) {
        let mut harness = TestByEmulationHarness::default();

        harness
            .emulator
            .load_module(
                Box::new(
                    intrinsics::load(I32_INTRINSICS_MODULE_NAME, &harness.context.session.source_manager)
                        .expect("undefined intrinsic module"),
                )
                .freeze(),
            )
            .expect("failed to load intrinsics::i32");

        let a_felt = Felt::new(a as u32 as u64);
        let b_felt = Felt::new(b as u64);
        let ipow = "intrinsics::i32::ipow".parse().unwrap();
        let mut stack = harness.invoke(ipow, &[b_felt, a_felt]).expect("execution failed");

        prop_assert_eq!(stack.len(), 1);
        let raw_result = stack.pop().unwrap().as_int();
        let result = u32::try_from(raw_result).map_err(|_| raw_result).map(|res| res as i32);
        prop_assert_eq!(result, Ok(a.wrapping_pow(b)));
    }

    #[test]
    fn i32_checked_shr(a: i32, b in 0i32..32i32) {
        let mut harness = TestByEmulationHarness::default();

        harness
            .emulator
            .load_module(
                Box::new(
                    intrinsics::load(I32_INTRINSICS_MODULE_NAME, &harness.context.session.source_manager)
                        .expect("undefined intrinsic module"),
                )
                .freeze(),
            )
            .expect("failed to load intrinsics::i32");

        let a_felt = Felt::new(a as u32 as u64);
        let b_felt = Felt::new(b as u32 as u64);
        let shr = "intrinsics::i32::checked_shr".parse().unwrap();
        let mut stack = harness.invoke(shr, &[b_felt, a_felt]).expect("execution failed");

        prop_assert_eq!(stack.len(), 1);
        let raw_result = stack.pop().unwrap().as_int();
        let result = u32::try_from(raw_result).map_err(|_| raw_result).map(|res| res as i32);
        prop_assert_eq!(result, Ok(a >> b));
    }
}
