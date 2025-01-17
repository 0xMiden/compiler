mod breakpoints;
mod debug;
mod events;
mod functions;
mod memory;

use std::{cell::RefCell, cmp, rc::Rc, sync::Arc};

use memory::Memory;
use miden_assembly::{ast::ProcedureName, LibraryNamespace};
use midenc_hir::{
    assert_matches, Felt, FieldElement, FunctionIdent, FxHashMap, FxHashSet, Ident, OperandStack,
    Stack,
};

use self::functions::{Activation, Stub};
pub use self::{
    breakpoints::*,
    debug::{CallFrame, DebugInfo, DebugInfoWithStack},
    events::{BreakpointEvent, ControlEffect, EmulatorEvent},
    functions::{Instruction, InstructionWithOp, NativeFn},
};
use crate::{BlockId, Function, Module, Op, Program};

/// This type represents the various sorts of errors which can occur when
/// running the emulator on a MASM program. Some errors may result in panics,
/// but those which we can handle are represented here.
#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum EmulationError {
    /// The given module is already loaded
    #[error("unable to load module: '{0}' is already loaded")]
    AlreadyLoaded(Ident),
    /// The given function is already loaded
    #[error("unable to load function: '{0}' is already loaded")]
    DuplicateFunction(FunctionIdent),
    /// The given function cannot be found
    #[error("unable to invoke function: '{0}' is not defined")]
    UndefinedFunction(FunctionIdent),
    /// The emulator ran out of available memory
    #[error("system limit: out of memory")]
    OutOfMemory,
    /// The emulator was terminated due to a program failing to terminate in its budgeted time
    #[error("execution terminated prematurely: maximum cycle count reached")]
    CycleBudgetExceeded,
    /// A breakpoint was reached, so execution was suspended and can be resumed
    #[error("execution suspended by breakpoint")]
    BreakpointHit(BreakpointEvent),
    /// An attempt was made to run the emulator without specifying an entrypoint
    #[error("unable to start the emulator without an entrypoint")]
    NoEntrypoint,
}

/// The size/type of pointers in the emulator
pub type Addr = u32;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct InstructionPointer {
    /// The block in which the instruction pointer is located
    pub block: BlockId,
    /// The index of the instruction pointed to
    pub index: usize,
}
impl InstructionPointer {
    pub const fn new(block: BlockId) -> Self {
        Self { block, index: 0 }
    }
}

/// This enum represents the state transitions for the emulator.
///
/// * The emulator starts in `Init`
/// * Once some code is loaded, it becomes `Loaded`
/// * Once the emulator has started executing some code, it becomes `Started`
/// * If the emulator suspends due to a breakpoint or stepping, it becomes `Suspended`
/// * Once the emulator finishes executing whatever entrypoint was invoked, it becomes `Stopped`
/// * If an error occurs between `Started` and `Stopped`, it becomes `Faulted`
///
/// Once `Started`, it is not possible to `start` the emulator again until it reaches the
/// `Stopped` state, or is explicitly reset to the `Init` or `Loaded` states using `reset`
/// or `stop` respectively.
#[derive(Debug, Default)]
enum Status {
    /// The emulator is in its initial state
    ///
    /// In this state, the emulator cannot execute any code because there
    /// is no code loaded yet.
    #[default]
    Init,
    /// A program has been loaded into the emulator, but not yet started
    ///
    /// This is the clean initial state from which a program or function can
    /// start executing. Once the emulator leaves this status, the state of
    /// the emulator is "dirty", i.e. it is no longer a clean slate.
    Loaded,
    /// The emulator has started running the current program, or a specified function.
    Started,
    /// The emulator is suspended, and awaiting resumption
    Suspended,
    /// The emulator finished running the current program, or a specified function,
    /// and the state of the emulator has not yet been reset.
    Stopped,
    /// The emulator has stopped due to an error, and cannot proceed further
    Faulted(EmulationError),
}

/// [Emulator] provides us with a means to execute our MASM IR directly
/// without having to emit "real" MASM and run it via the Miden VM.
/// In other words, it's a convenient way to run tests to verify the
/// expected behavior of a program without all of the baggage of the
/// Miden VM.
///
/// [Emulator] is necessarily a more limited execution environment:
///
/// * It only handles instructions which are defined in the [Op] enum
/// * Anything related to proving, calling contracts, etc. is not supported
/// * The default environment is empty, i.e. there are no Miden VM standard
///   library functions available. Users must emit Miden IR for all functions
///   they wish to call, or alternatively, provide native stubs.
pub struct Emulator {
    status: Status,
    functions: FxHashMap<FunctionIdent, Stub>,
    locals: FxHashMap<FunctionIdent, Addr>,
    modules_loaded: FxHashMap<Ident, Arc<Module>>,
    modules_pending: FxHashSet<Ident>,
    memory: Memory,
    stack: OperandStack<Felt>,
    advice_stack: OperandStack<Felt>,
    callstack: Vec<Activation>,
    hp_start: u32,
    hp: u32,
    lp_start: u32,
    lp: u32,
    breakpoints: BreakpointManager,
    step_over: Option<InstructionPointer>,
    clk: usize,
    clk_limit: usize,
    entrypoint: Option<FunctionIdent>,
    print_trace: bool,
}
impl Default for Emulator {
    fn default() -> Self {
        Self::new(
            Self::DEFAULT_HEAP_SIZE,
            Self::DEFAULT_HEAP_START,
            Self::DEFAULT_LOCALS_START,
            false,
        )
    }
}
impl Emulator {
    pub const DEFAULT_HEAP_SIZE: u32 = (4 * Self::PAGE_SIZE) / 16;
    pub const DEFAULT_HEAP_START: u32 = (2 * Self::PAGE_SIZE) / 16;
    pub const DEFAULT_LOCALS_START: u32 = (3 * Self::PAGE_SIZE) / 16;
    const PAGE_SIZE: u32 = 64 * 1024;

    /// Construct a new, empty emulator with:
    ///
    /// * A linear memory heap of `memory_size` words
    /// * The start of the usable heap set to `hp` (an address in words)
    /// * The start of the reserved heap used for locals set to `lp` (an address in words)
    pub fn new(memory_size: u32, hp: u32, lp: u32, print_stack: bool) -> Self {
        let memory = Memory::new(memory_size as usize);
        Self {
            status: Status::Init,
            functions: Default::default(),
            locals: Default::default(),
            modules_loaded: Default::default(),
            modules_pending: Default::default(),
            memory,
            stack: Default::default(),
            advice_stack: Default::default(),
            callstack: vec![],
            hp_start: hp,
            hp,
            lp_start: lp,
            lp,
            breakpoints: Default::default(),
            step_over: None,
            clk: 0,
            clk_limit: usize::MAX,
            entrypoint: None,
            print_trace: print_stack,
        }
    }

    /// Place a cap on the number of cycles the emulator will execute before failing with an error
    pub fn set_max_cycles(&mut self, max: usize) {
        self.clk_limit = max;
    }

    /// Returns all watchpoints that are currently managed by this [BreakpointManager]
    pub fn watchpoints(&self) -> impl Iterator<Item = Watchpoint> + '_ {
        self.breakpoints.watchpoints()
    }

    /// Returns all breakpoints that are currently managed by this [BreakpointManager]
    pub fn breakpoints(&self) -> impl Iterator<Item = Breakpoint> {
        self.breakpoints.breakpoints()
    }

    /// Sets a breakpoint for the emulator
    pub fn set_breakpoint(&mut self, bp: Breakpoint) {
        self.breakpoints.set(bp);
    }

    /// Removes the given breakpoint from the emulator
    pub fn clear_breakpoint(&mut self, bp: Breakpoint) {
        self.breakpoints.unset(bp);
    }

    /// Removes the all breakpoints from the emulator
    pub fn clear_breakpoints(&mut self) {
        self.breakpoints.unset_all();
    }

    /// Sets a watchpoint in the emulator
    pub fn set_watchpoint(&mut self, addr: Addr, size: u32, mode: WatchMode) -> WatchpointId {
        self.breakpoints.watch(addr, size, mode)
    }

    /// Sets a watchpoint in the emulator
    pub fn clear_watchpoint(&mut self, id: WatchpointId) {
        self.breakpoints.unwatch(id);
    }

    /// Set the watch mode for a [Watchpoint] using the identifier returned by [watch]
    pub fn watchpoint_mode(&mut self, id: WatchpointId, mode: WatchMode) {
        self.breakpoints.watch_mode(id, mode);
    }

    /// Clears all watchpoints
    pub fn clear_watchpoints(&mut self) {
        self.breakpoints.unwatch_all();
    }

    /// Clear all breakpoints and watchpoints
    pub fn clear_break_and_watchpoints(&mut self) {
        self.breakpoints.clear();
    }

    /// Get's debug information about the current emulator state
    pub fn info(&self) -> Option<DebugInfo<'_>> {
        let current = self.callstack.last()?;
        // This returns the pending activation state for the current function,
        // i.e. the next instruction to be executed, what control flow effects
        // will occur to reach that instruction, and the actual instruction pointer
        let ip = current.peek_with_op();
        Some(DebugInfo {
            cycle: self.clk,
            function: current.function().name,
            fp: current.fp(),
            ip,
            stack: &self.stack,
        })
    }

    /// Get a stacktrace for the code running in the emulator
    pub fn stacktrace(&self) -> Vec<CallFrame> {
        let mut frames = Vec::with_capacity(self.callstack.len());
        for frame in self.callstack.iter() {
            frames.push(CallFrame {
                function: frame.function().name,
                fp: frame.fp(),
                ip: Some(frame.ip()),
            })
        }
        frames
    }

    /// Get the instruction pointer that will be next executed by the emulator
    pub fn current_ip(&self) -> Option<Instruction> {
        self.callstack.last().and_then(|activation| activation.peek())
    }

    /// Get the name of the function that is currently executing
    pub fn current_function(&self) -> Option<FunctionIdent> {
        self.callstack.last().map(|activation| activation.function().name)
    }

    /// Get access to the current state of the operand stack
    pub fn stack(&mut self) -> &OperandStack<Felt> {
        &self.stack
    }

    /// Get mutable access to the current state of the operand stack
    pub fn stack_mut(&mut self) -> &mut OperandStack<Felt> {
        &mut self.stack
    }

    /// Load `program` into this emulator
    ///
    /// This resets the emulator state, as only one program may be loaded at a time.
    pub fn load_program(&mut self, program: Arc<Program>) -> Result<(), EmulationError> {
        // Ensure the emulator state is reset
        if !matches!(self.status, Status::Init) {
            self.reset();
        }

        let modules = program.unwrap_frozen_modules();
        let mut cursor = modules.front();
        while let Some(module) = cursor.clone_pointer() {
            self.load_module(module)?;
            cursor.move_next();
        }
        self.entrypoint = Some(program.entrypoint());

        // TODO: Load data segments

        self.status = Status::Loaded;

        Ok(())
    }

    /// Load `module` into this emulator
    ///
    /// An error is returned if a module with the same name is already loaded.
    pub fn load_module(&mut self, module: Arc<Module>) -> Result<(), EmulationError> {
        use hashbrown::hash_map::Entry;

        assert_matches!(
            self.status,
            Status::Init | Status::Loaded,
            "cannot load modules once execution has started without calling stop() or reset() \
             first"
        );

        match self.modules_loaded.entry(module.id) {
            Entry::Occupied(_) => return Err(EmulationError::AlreadyLoaded(module.id)),
            Entry::Vacant(entry) => {
                entry.insert(module.clone());
            }
        }

        // Register module dependencies
        for import in module.imports.iter() {
            let name = Ident::with_empty_span(import.name);
            if self.modules_loaded.contains_key(&name) {
                continue;
            }
            self.modules_pending.insert(name);
        }
        self.modules_pending.remove(&module.id);

        // Load functions from this module
        let functions = module.unwrap_frozen_functions();
        let mut cursor = functions.front();
        while let Some(function) = cursor.clone_pointer() {
            self.load_function(function)?;
            cursor.move_next();
        }

        self.status = Status::Loaded;

        Ok(())
    }

    /// Reloads a loaded module, `name`.
    ///
    /// This function will panic if the named module is not currently loaded.
    pub fn reload_module(&mut self, module: Arc<Module>) -> Result<(), EmulationError> {
        self.unload_module(module.id);
        self.load_module(module)
    }

    /// Unloads a loaded module, `name`.
    ///
    /// This function will panic if the named module is not currently loaded.
    pub fn unload_module(&mut self, name: Ident) {
        assert_matches!(
            self.status,
            Status::Loaded,
            "cannot unload modules once execution has started without calling stop() or reset() \
             first"
        );

        let prev = self
            .modules_loaded
            .remove(&name)
            .expect("cannot reload a module that was not previously loaded");

        // Unload all functions associated with the previous load
        for f in prev.functions() {
            self.functions.remove(&f.name);
            self.locals.remove(&f.name);
        }

        // Determine if we need to add `name` to `modules_pending` if there are dependents still
        // loaded
        for module in self.modules_loaded.values() {
            if module.imports.is_import(&name) {
                self.modules_pending.insert(name);
                break;
            }
        }
    }

    /// Load `function` into this emulator
    fn load_function(&mut self, function: Arc<Function>) -> Result<(), EmulationError> {
        let id = function.name;
        if self.functions.contains_key(&id) {
            return Err(EmulationError::DuplicateFunction(id));
        }
        let fp = self.lp;
        self.lp += function.locals().len() as u32;
        self.functions.insert(id, Stub::Asm(function));
        self.locals.insert(id, fp);

        Ok(())
    }

    /// Load `function` into this emulator, with the given identifier
    ///
    /// Because we don't know the set of [FuncId] that have already been allocated,
    /// we leave the choice up to the caller. We assert that functions do
    /// not get defined twice to catch conflicts, just in case.
    pub fn load_nif(
        &mut self,
        id: FunctionIdent,
        function: Box<NativeFn>,
    ) -> Result<(), EmulationError> {
        assert_matches!(
            self.status,
            Status::Init | Status::Loaded,
            "cannot load nifs once execution has started without calling stop() or reset() first"
        );

        if self.functions.contains_key(&id) {
            return Err(EmulationError::DuplicateFunction(id));
        }
        self.functions.insert(id, Stub::Native(Rc::new(RefCell::new(function))));

        Ok(())
    }

    /// Allocate space for `value` on the emulator heap, and copy it's contents there.
    ///
    /// NOTE: The smallest unit of addressable memory is 4 bytes (32 bits). If you provide
    /// a value that is smaller than this, or is not a multiple of 4, the data will be padded
    /// with zeroes to ensure that it is.
    pub fn write_bytes_to_memory(&mut self, value: &[u8]) -> u32 {
        let addr = self.hp;
        if value.is_empty() {
            return addr;
        }

        let mut elem_idx = 0;
        for chunk in value.chunks(4) {
            let elem = match chunk.len() {
                4 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
                3 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]),
                2 => u32::from_le_bytes([chunk[0], chunk[1], 0, 0]),
                1 => u32::from_le_bytes([chunk[0], 0, 0, 0]),
                0 => 0,
                _ => unreachable!(),
            };
            if elem_idx == 4 {
                elem_idx = 0;
                assert!(self.hp + 1 < self.lp, "heap has overflowed into reserved region");
                self.hp += 1;
            }
            self.memory[self.hp as usize][elem_idx] = Felt::new(elem as u64);
            elem_idx += 1;
        }

        addr
    }

    /// Allocate enough words to hold `size` bytes of memory
    ///
    /// Returns the pointer as a byte-addressable address
    pub fn malloc(&mut self, size: usize) -> u32 {
        let addr = self.hp;

        if size == 0 {
            return addr;
        }

        let size = size as u32;
        let extra = size % 16;
        let words = (size / 16) + (extra > 0) as u32;
        assert!(self.hp + words < self.lp, "heap has overflowed into reserved region");
        self.hp += words;

        addr * 16
    }

    /// Write `value` to the word at `addr`, and element `index`
    pub fn store(&mut self, addr: usize, value: Felt) {
        use crate::NativePtr;

        let ptr = NativePtr::from_ptr(addr.try_into().expect("invalid address"));
        let addr = ptr.waddr as usize;
        assert_eq!(ptr.offset, 0, "invalid store: unaligned address {addr:#?}");
        assert!(addr < self.memory.len(), "invalid address");

        self.memory[addr][ptr.index as usize] = value;
    }

    /// Start executing the current program by `invoke`ing the top-level initialization block (the
    /// entrypoint).
    ///
    /// This function will run the program to completion, and return the state of the operand stack
    /// on exit.
    ///
    /// NOTE: If no entrypoint has been loaded, an error is returned.
    ///
    /// The emulator is automatically reset when it exits successfully.
    pub fn start(&mut self) -> Result<OperandStack<Felt>, EmulationError> {
        match self.status {
            Status::Init => return Err(EmulationError::NoEntrypoint),
            Status::Loaded => (),
            Status::Stopped => {
                self.stop();
            }
            Status::Started | Status::Suspended => panic!(
                "cannot start the emulator when it is already started without calling stop() or \
                 reset() first"
            ),
            Status::Faulted(ref err) => return Err(err.clone()),
        }

        let main_fn = self.entrypoint.unwrap_or_else(|| FunctionIdent {
            module: LibraryNamespace::EXEC_PATH.into(),
            function: ProcedureName::MAIN_PROC_NAME.into(),
        });

        // Run to completion
        let stack = self.invoke(main_fn, &[]).map_err(|err| match err {
            EmulationError::UndefinedFunction(f) if f == main_fn => EmulationError::NoEntrypoint,
            err => err,
        })?;

        // Reset the emulator on exit
        self.stop();

        // Return the output contained on the operand stack
        Ok(stack)
    }

    /// Start emulation by `enter`ing the top-level initialization block (the entrypoint).
    ///
    /// This should be called instead of `start` when stepping through a program rather than
    /// executing it to completion in one call.
    ///
    /// NOTE: If no entrypoint has been loaded, an error is returned.
    ///
    /// It is up to the caller to reset the emulator when the program exits, unlike `start`.
    pub fn init(&mut self) -> Result<EmulatorEvent, EmulationError> {
        match self.status {
            Status::Init => return Err(EmulationError::NoEntrypoint),
            Status::Loaded => (),
            Status::Stopped => {
                self.stop();
            }
            Status::Started | Status::Suspended => panic!(
                "cannot start the emulator when it is already started without calling stop() or \
                 reset() first"
            ),
            Status::Faulted(ref err) => return Err(err.clone()),
        }

        let main_fn = FunctionIdent {
            module: LibraryNamespace::EXEC_PATH.into(),
            function: ProcedureName::MAIN_PROC_NAME.into(),
        };

        // Step into the entrypoint
        self.enter(main_fn, &[]).map_err(|err| match err {
            EmulationError::UndefinedFunction(f) if f == main_fn => EmulationError::NoEntrypoint,
            err => err,
        })
    }

    /// Stop running the currently executing function, and reset the cycle counter, operand stack,
    /// and linear memory.
    ///
    /// This function preserves loaded code, breakpoints, and other configuration items.
    ///
    /// If an attempt is made to run the emulator in the stopped state, a panic will occur
    pub fn stop(&mut self) {
        self.callstack.clear();
        self.stack.clear();
        self.memory.reset();
        self.hp = self.hp_start;
        self.lp = self.lp_start;
        self.step_over = None;
        self.clk = 0;
        self.status = Status::Loaded;
    }

    /// Reset the emulator state to its initial state at creation.
    ///
    /// In addition to resetting the cycle counter, operand stack, and linear memory,
    /// this function also unloads all code, and clears all breakpoints. Only the
    /// configuration used to initialize the emulator is preserved.
    ///
    /// To use the emulator after calling this function, you must load a program or module again.
    pub fn reset(&mut self) {
        self.stop();
        self.functions.clear();
        self.locals.clear();
        self.modules_loaded.clear();
        self.modules_pending.clear();
        self.breakpoints.clear();
        self.status = Status::Init;
    }

    /// Run the emulator by invoking `callee` with `args` placed on the
    /// operand stack in FIFO order.
    ///
    /// If a fatal error occurs during emulation, `Err` is returned,
    /// e.g. if `callee` has not been loaded.
    ///
    /// When `callee` returns, it's result will be returned wrapped in `Ok`.
    /// For functions with no return value, this will be `Ok(None)`, or all
    /// others it will be `Ok(Some(value))`.
    pub fn invoke(
        &mut self,
        callee: FunctionIdent,
        args: &[Felt],
    ) -> Result<OperandStack<Felt>, EmulationError> {
        assert_matches!(
            self.status,
            Status::Loaded,
            "cannot start executing a function when the emulator is already started without \
             calling stop() or reset() first"
        );
        let fun = self
            .functions
            .get(&callee)
            .cloned()
            .ok_or(EmulationError::UndefinedFunction(callee))?;
        self.status = Status::Started;
        match fun {
            Stub::Asm(ref function) => match self.invoke_function(function.clone(), args) {
                done @ Ok(_) => {
                    self.status = Status::Stopped;
                    done
                }
                Err(err @ EmulationError::BreakpointHit(_)) => {
                    self.status = Status::Suspended;
                    Err(err)
                }
                Err(err) => {
                    self.status = Status::Faulted(err.clone());
                    Err(err)
                }
            },
            Stub::Native(function) => {
                let mut function = function.borrow_mut();
                function(self, args)?;
                Ok(self.stack.clone())
            }
        }
    }

    /// Invoke a function defined in MASM IR, placing the given arguments on the
    /// operand stack in FIFO order, and suspending immediately if any breakpoints
    /// would have been triggered by the invocation.
    #[inline]
    fn invoke_function(
        &mut self,
        function: Arc<Function>,
        args: &[Felt],
    ) -> Result<OperandStack<Felt>, EmulationError> {
        // Place the arguments on the operand stack
        //assert_eq!(args.len(), function.arity());
        for arg in args.iter().copied().rev() {
            self.stack.push(arg);
        }

        // Schedule `function`
        let name = function.name;
        let fp = self.locals[&name];
        let state = Activation::new(function, fp);
        self.callstack.push(state);

        match self
            .breakpoints
            .handle_event(EmulatorEvent::EnterFunction(name), self.current_ip())
        {
            Some(bp) => Err(EmulationError::BreakpointHit(bp)),
            None => {
                self.run()?;

                Ok(self.stack.clone())
            }
        }
    }

    /// Run the emulator by invoking `callee` with `args` placed on the
    /// operand stack in FIFO order.
    ///
    /// If a fatal error occurs during emulation, `Err` is returned,
    /// e.g. if `callee` has not been loaded.
    ///
    /// When `callee` returns, it's result will be returned wrapped in `Ok`.
    /// For functions with no return value, this will be `Ok(None)`, or all
    /// others it will be `Ok(Some(value))`.
    pub fn enter(
        &mut self,
        callee: FunctionIdent,
        args: &[Felt],
    ) -> Result<EmulatorEvent, EmulationError> {
        assert_matches!(
            self.status,
            Status::Loaded,
            "cannot start executing a function when the emulator is already started without \
             calling stop() or reset() first"
        );

        let fun = self
            .functions
            .get(&callee)
            .cloned()
            .ok_or(EmulationError::UndefinedFunction(callee))?;
        self.status = Status::Started;
        match fun {
            Stub::Asm(ref function) => self.enter_function(function.clone(), args),
            Stub::Native(function) => {
                let mut function = function.borrow_mut();
                function(self, args)?;
                Ok(EmulatorEvent::ExitFunction(callee))
            }
        }
    }

    /// Stage a MASM IR function for execution by the emulator, placing the given arguments on the
    /// operand stack in FIFO order, then immediately suspending execution until the next
    /// resumption.
    #[inline]
    fn enter_function(
        &mut self,
        function: Arc<Function>,
        args: &[Felt],
    ) -> Result<EmulatorEvent, EmulationError> {
        // Place the arguments on the operand stack
        //assert_eq!(args.len(), function.arity());
        for arg in args.iter().copied().rev() {
            self.stack.push(arg);
        }

        // Schedule `function`
        let name = function.name;
        let fp = self.locals[&name];
        let state = Activation::new(function, fp);
        self.callstack.push(state);

        self.status = Status::Suspended;

        Ok(EmulatorEvent::Suspended)
    }

    /// Resume execution when the emulator suspended due to a breakpoint
    #[inline]
    pub fn resume(&mut self) -> Result<EmulatorEvent, EmulationError> {
        assert_matches!(
            self.status,
            Status::Suspended,
            "cannot resume the emulator from any state other than suspended"
        );
        self.run()
    }
}

/// Pops the top element off the advice stack
macro_rules! adv_pop {
    ($emu:ident) => {
        $emu.advice_stack.pop().expect("advice stack is empty")
    };

    ($emu:ident, $msg:literal) => {
        $emu.advice_stack.pop().expect($msg)
    };

    ($emu:ident, $msg:literal, $($arg:expr),+) => {
        match $emu.advice_stack.pop() {
            Some(value) => value,
            None => panic!($msg, $($arg),*),
        }
    }
}

/// Pops the top word off the advice stack
macro_rules! adv_popw {
    ($emu:ident) => {
        $emu.advice_stack.popw().expect("advice stack does not contain a full word")
    };

    ($emu:ident, $msg:literal) => {
        $emu.advice_stack.popw().expect($msg)
    };

    ($emu:ident, $msg:literal, $($arg:expr),+) => {{
        match $emu.advice_stack.popw() {
            Some(value) => value,
            None => panic!($msg, $($arg),*),
        }
    }}
}

/// Pops the top element off the stack
macro_rules! pop {
    ($emu:ident) => {
        $emu.stack.pop().expect("operand stack is empty")
    };

    ($emu:ident, $msg:literal) => {
        $emu.stack.pop().expect($msg)
    };

    ($emu:ident, $msg:literal, $($arg:expr),+) => {
        match $emu.stack.pop() {
            Some(value) => value,
            None => panic!($msg, $($arg),*),
        }
    }
}

/// Peeks the top element of the stack
macro_rules! peek {
    ($emu:ident) => {
        $emu.stack.peek().expect("operand stack is empty")
    };

    ($emu:ident, $msg:literal) => {
        $emu.stack.peek().expect($msg)
    };

    ($emu:ident, $msg:literal, $($arg:expr),+) => {
        match $emu.stack.peek() {
            Some(value) => value,
            None => panic!($msg, $($arg),*),
        }
    }
}

/// Pops the top word off the stack
macro_rules! popw {
    ($emu:ident) => {
        $emu.stack.popw().expect("operand stack does not contain a full word")
    };

    ($emu:ident, $msg:literal) => {
        $emu.stack.popw().expect($msg)
    };

    ($emu:ident, $msg:literal, $($arg:expr),+) => {{
        match $emu.stack.popw() {
            Some(value) => value,
            None => panic!($msg, $($arg),*),
        }
    }}
}

/// Pops the top two elements off the stack, returning them in order of appearance
macro_rules! pop2 {
    ($emu:ident) => {{
        let b = pop!($emu);
        let a = pop!($emu);
        (b, a)
    }};
}

/// Pops a u32 value from the top of the stack, and asserts if it is out of range
macro_rules! pop_u32 {
    ($emu:ident) => {{
        let value = pop!($emu).as_int();
        assert!(value < 2u64.pow(32), "assertion failed: {value} is not a valid u32, value is out of range");
        value as u32
    }};

    ($emu:ident, $format:literal $(, $args:expr)*) => {{
        let value = pop!($emu).as_int();
        assert!(value < 2u64.pow(32), $format, value, $($args),*);
        value as u32
    }}
}

/// Peeks a u32 value from the top of the stack, and asserts if it is out of range
#[allow(unused)]
macro_rules! peek_u32 {
    ($emu:ident) => {{
        let value = peek!($emu).as_int();
        assert!(value < 2u64.pow(32), "assertion failed: {value} is not a valid u32, value is out of range");
        value as u32
    }};

    ($emu:ident, $format:literal $(, $args:expr)*) => {{
        let value = peek!($emu).as_int();
        assert!(value < 2u64.pow(32), $format, value, $($args),*);
        value as u32
    }}
}

/// Pops a pointer value from the top of the stack, and asserts if it is not a valid boolean
macro_rules! pop_addr {
    ($emu:ident) => {{
        let addr = pop_u32!($emu, "expected valid 32-bit address, got {}") as usize;
        assert!(
            addr < $emu.memory.len(),
            "out of bounds memory access, addr: {}, available memory: {}",
            addr,
            $emu.memory.len()
        );
        addr
    }};
}

/// Pops a boolean value from the top of the stack, and asserts if it is not a valid boolean
macro_rules! pop_bool {
    ($emu:ident) => {{
        let value = pop!($emu).as_int();
        assert!(
            value < 2,
            "assertion failed: {value} is not a valid boolean, value must be either 1 or 0"
        );
        value == 1
    }};
}

/// Applies a binary operator that produces a result of the same input type:
///
/// 1. The top two elements of the stack
/// 2. The top element of the stack and an immediate.
macro_rules! binop {
    ($emu:ident, $op:ident) => {{
        use core::ops::*;
        let b = pop!($emu);
        let a = pop!($emu);
        $emu.stack.push(a.$op(b));
    }};

    ($emu:ident, $op:ident, $imm:expr) => {{
        use core::ops::*;
        let a = pop!($emu);
        $emu.stack.push(a.$op($imm));
    }};
}

/// Applies a binary operator to two u32 values, either:
///
/// 1. The top two elements of the stack
/// 2. The top element of the stack and an immediate.
macro_rules! binop32 {
    ($emu:ident, $op:ident) => {{
        #[allow(unused)]
        use core::ops::*;
        let b = pop_u32!($emu);
        let a = pop_u32!($emu);
        $emu.stack.push_u32(a.$op(b));
    }};

    ($emu:ident, $op:ident, $imm:expr) => {{
        #[allow(unused)]
        use core::ops::*;
        let a = pop_u32!($emu);
        $emu.stack.push_u32(a.$op($imm));
    }};
}

/// Applies a checked binary operator to two u32 values, either:
///
/// 1. The top two elements of the stack
/// 2. The top element of the stack and an immediate.
macro_rules! binop_unchecked_u32 {
    ($emu:ident, $op:ident) => {{
        #[allow(unused)]
        use core::ops::*;
        let b = pop!($emu);
        let a = pop!($emu);
        $emu.stack.push(Felt::new(a.as_int().$op(b.as_int())));
    }};

    ($emu:ident, $op:ident, $imm:expr) => {{
        #[allow(unused)]
        use core::ops::*;
        let a = pop!($emu);
        $emu.stack.push(Felt::new(a.as_int().$op($imm)));
    }};
}

/// Applies an overflowing binary operator to two u32 values, either:
///
/// 1. The top two elements of the stack
/// 2. The top element of the stack and an immediate.
macro_rules! binop_overflowing_u32 {
    ($emu:ident, $op:ident) => {{
        paste::paste! {
            binop_overflowing_u32_impl!($emu, [<overflowing_ $op>]);
        }
    }};

    ($emu:ident, $op:ident, $imm:expr) => {{
        paste::paste! {
            binop_overflowing_u32_impl!($emu, [<overflowing_ $op>], $imm);
        }
    }};
}

#[doc(hidden)]
macro_rules! binop_overflowing_u32_impl {
    ($emu:ident, $op:ident) => {{
        #[allow(unused)]
        use core::ops::*;
        let b = pop_u32!($emu);
        let a = pop_u32!($emu);
        let (result, overflowed) = a.$op(b);
        $emu.stack.push_u32(result);
        $emu.stack.push_u8(overflowed as u8);
    }};

    ($emu:ident, $op:ident, $imm:expr) => {{
        #[allow(unused)]
        use core::ops::*;
        let a = pop_u32!($emu);
        let (result, overflowed) = a.$op($imm);
        $emu.stack.push_u32(result);
        $emu.stack.push_u8(overflowed as u8);
    }};
}

/// Applies a wrapping binary operator to two u32 values, either:
///
/// 1. The top two elements of the stack
/// 2. The top element of the stack and an immediate.
macro_rules! binop_wrapping_u32 {
    ($emu:ident, $op:ident) => {{
        paste::paste! {
            binop_wrapping_u32_impl!($emu, [<wrapping_ $op>]);
        }
    }};

    ($emu:ident, $op:ident, $imm:expr) => {{
        paste::paste! {
            binop_wrapping_u32_impl!($emu, [<wrapping_ $op>], $imm);
        }
    }};
}

#[doc(hidden)]
macro_rules! binop_wrapping_u32_impl {
    ($emu:ident, $op:ident) => {{
        #[allow(unused)]
        use core::ops::*;
        let b = pop_u32!($emu);
        let a = pop_u32!($emu);
        $emu.stack.push_u32(a.$op(b));
    }};

    ($emu:ident, $op:ident, $imm:expr) => {{
        #[allow(unused)]
        use core::ops::*;
        let a = pop_u32!($emu);
        $emu.stack.push_u32(a.$op($imm));
    }};
}

/// Applies a binary comparison operator, to either:
///
/// 1. The top two elements of the stack
/// 2. The top element of the stack and an immediate.
macro_rules! comparison {
    ($emu:ident, $op:ident) => {{
        let b = pop!($emu).as_int();
        let a = pop!($emu).as_int();
        let result: bool = a.$op(&b);
        $emu.stack.push_u8(result as u8);
    }};

    ($emu:ident, $op:ident, $imm:expr) => {{
        let a = pop!($emu).as_int();
        let result: bool = a.$op(&$imm);
        $emu.stack.push_u8(result as u8);
    }};
}

impl Emulator {
    /// Step the emulator forward one cycle, returning the type of event produced
    /// during that cycle, or an error.
    pub fn step(&mut self) -> Result<EmulatorEvent, EmulationError> {
        match self
            .breakpoints
            .handle_event(EmulatorEvent::CycleStart(self.clk), self.current_ip())
        {
            Some(bp) => {
                self.status = Status::Suspended;
                Ok(EmulatorEvent::Breakpoint(bp))
            }
            None => match self.run_once() {
                Ok(EmulatorEvent::Stopped) => {
                    self.status = Status::Stopped;
                    Ok(EmulatorEvent::Stopped)
                }
                suspended @ Ok(_) => {
                    self.status = Status::Suspended;
                    suspended
                }
                Err(err) => {
                    self.status = Status::Faulted(err.clone());
                    Err(err)
                }
            },
        }
    }

    /// Step the emulator forward one step, but stepping past any nested blocks or function calls,
    /// returning the type of event produced during that cycle, or an error.
    pub fn step_over(&mut self) -> Result<EmulatorEvent, EmulationError> {
        match self.step_over.take() {
            None => self.step(),
            Some(ip) => {
                self.breakpoints.set(Breakpoint::At(ip));
                match self.run() {
                    Ok(EmulatorEvent::Stopped) => {
                        self.status = Status::Stopped;
                        Ok(EmulatorEvent::Stopped)
                    }
                    Ok(EmulatorEvent::Breakpoint(bp)) | Err(EmulationError::BreakpointHit(bp)) => {
                        self.status = Status::Suspended;
                        if self.current_ip().map(|ix| ix.ip) == Some(ip) {
                            return Ok(EmulatorEvent::Suspended);
                        }
                        Ok(EmulatorEvent::Breakpoint(bp))
                    }
                    Ok(event) => panic!(
                        "unexpected event produced by emulator loop when stepping over: {event:?}"
                    ),
                    Err(err) => {
                        self.status = Status::Faulted(err.clone());
                        Err(err)
                    }
                }
            }
        }
    }

    /// Step the emulator forward until control returns from the current function.
    pub fn step_out(&mut self) -> Result<EmulatorEvent, EmulationError> {
        let current_function = self.current_function();
        self.breakpoints.break_on_return(true);
        match self.run() {
            Ok(EmulatorEvent::Stopped) => {
                self.status = Status::Stopped;
                Ok(EmulatorEvent::Stopped)
            }
            Ok(EmulatorEvent::Breakpoint(bp)) | Err(EmulationError::BreakpointHit(bp)) => {
                self.status = Status::Suspended;
                if self.current_function() == current_function {
                    return Ok(EmulatorEvent::Suspended);
                }
                Ok(EmulatorEvent::Breakpoint(bp))
            }
            Ok(event) => {
                panic!("unexpected event produced by emulator loop when stepping over: {event:?}")
            }
            Err(err) => {
                self.status = Status::Faulted(err.clone());
                Err(err)
            }
        }
    }

    /// Run the emulator until all calls are completed, the cycle budget is exhausted,
    /// or a breakpoint is hit.
    ///
    /// It is expected that the caller has set up the operand stack with the correct
    /// number of arguments. If not, undefined behavior (from the perspective of the
    /// MASM program) will result.
    #[inline(never)]
    fn run(&mut self) -> Result<EmulatorEvent, EmulationError> {
        // This is the core interpreter loop for MASM IR, it runs until one of the
        // following occurs:
        //
        // * We run out of code to execute, i.e. the function is returning normally
        // * Execution was explicitly aborted from within the function
        // * Execution traps due to a MASM invariant being violated, indicating the
        // code is malformed.
        // * Execution traps due to a runtime system error, e.g. out of memory
        // * Execution traps due to exceeding the predefined execution budget
        // * Execution breaks due to a breakpoint
        let mut event = self.step()?;
        loop {
            match event {
                // We should suspend when encountering these events
                event @ EmulatorEvent::Breakpoint(_) => break Ok(event),
                event @ EmulatorEvent::Stopped => break Ok(event),
                ev => {
                    // We must handle catching certain breakpoints when using this event loop
                    match self.breakpoints.handle_event(ev, self.current_ip()) {
                        Some(bp) => break Ok(EmulatorEvent::Breakpoint(bp)),
                        None => match ev {
                            // There was no code remaining in the current function, effectively
                            // returning from it. Since no instructions were dispatched, we don't
                            // count the cycle, and resume immediately at the continuation point
                            // in the caller
                            EmulatorEvent::ExitFunction(_) => {
                                if self.callstack.is_empty() {
                                    break Ok(EmulatorEvent::Stopped);
                                }
                                event = self.run_once()?;
                                continue;
                            }
                            _ => {
                                event = self.step()?;
                            }
                        },
                    }
                }
            }
        }
    }

    #[inline(never)]
    fn run_once(&mut self) -> Result<EmulatorEvent, EmulationError> {
        const U32_P: u64 = 2u64.pow(32);

        // If there are no more activation records, we're done
        if self.callstack.is_empty() {
            return Ok(EmulatorEvent::Stopped);
        }

        // Terminate execution early if we reach a predetermined number of cycles
        self.clk += 1;
        if self.clk > self.clk_limit {
            return Err(EmulationError::CycleBudgetExceeded);
        }

        let mut state = self.callstack.pop().unwrap();
        let current_function = state.function().name;

        // Reset the next instruction to break at when stepping over instructions
        self.step_over = None;

        // If we have breakpoints set that require it, we may need to
        // break execution before executing the instruction that is pending
        if self.breakpoints.break_on_return || self.breakpoints.has_break_on_reached() {
            match state.peek() {
                Some(Instruction { ip, .. })
                    if self.breakpoints.should_break_at(ip.block, ip.index) =>
                {
                    self.callstack.push(state);
                    return Ok(EmulatorEvent::Breakpoint(BreakpointEvent::Reached(ip)));
                }
                None if self.breakpoints.break_on_return => {
                    self.callstack.push(state);
                    self.breakpoints.break_on_return(false);
                    return Ok(EmulatorEvent::Breakpoint(BreakpointEvent::StepOut));
                }
                _ => (),
            }
        }

        // Advance the instruction pointer, returning the instruction
        // that it previously pointed to, along with what, if any,
        // control flow effect occurred to reach it
        let ix_with_op = state.next();
        if let Some(ix_with_op) = ix_with_op {
            if self.print_trace {
                eprintln!("mem: {:?}", self.memory);
                eprintln!("stk: {}", self.stack.debug());
                eprintln!("op>: {:?}", ix_with_op.op);
            }
            match ix_with_op.op {
                Op::Padw => {
                    self.stack.padw();
                }
                Op::Push(v) => {
                    self.stack.push(v);
                }
                Op::Push2([a, b]) => {
                    self.stack.push(a);
                    self.stack.push(b);
                }
                Op::Pushw(word) => {
                    self.stack.pushw(word);
                }
                Op::PushU8(i) => {
                    self.stack.push_u8(i);
                }
                Op::PushU16(i) => {
                    self.stack.push_u16(i);
                }
                Op::PushU32(i) => {
                    self.stack.push_u32(i);
                }
                Op::Drop => {
                    self.stack.drop();
                }
                Op::Dropw => {
                    self.stack.dropw();
                }
                Op::Dup(pos) => {
                    self.stack.dup(pos as usize);
                }
                Op::Dupw(pos) => {
                    self.stack.dupw(pos as usize);
                }
                Op::Swap(pos) => {
                    self.stack.swap(pos as usize);
                }
                Op::Swapw(pos) => {
                    self.stack.swapw(pos as usize);
                }
                Op::Swapdw => {
                    self.stack.swapdw();
                }
                Op::Movup(pos) => {
                    self.stack.movup(pos as usize);
                }
                Op::Movupw(pos) => {
                    self.stack.movupw(pos as usize);
                }
                Op::Movdn(pos) => {
                    self.stack.movdn(pos as usize);
                }
                Op::Movdnw(pos) => {
                    self.stack.movdnw(pos as usize);
                }
                Op::Cswap => {
                    let cond = pop_bool!(self);
                    if cond {
                        self.stack.swap(1);
                    }
                }
                Op::Cswapw => {
                    let cond = pop_bool!(self);
                    if cond {
                        self.stack.swapw(1);
                    }
                }
                Op::Cdrop => {
                    let cond = pop_bool!(self);
                    let (b, a) = pop2!(self);
                    if cond {
                        self.stack.push(b);
                    } else {
                        self.stack.push(a);
                    }
                }
                Op::Cdropw => {
                    let cond = pop_bool!(self);
                    let b = popw!(self);
                    let a = popw!(self);
                    if cond {
                        self.stack.pushw(b);
                    } else {
                        self.stack.pushw(a);
                    }
                }
                Op::AdvPush(n) => {
                    assert!(
                        n > 0 && n <= 16,
                        "invalid adv_push operand: must be a value in the range 1..=16, got {n}"
                    );
                    for _ in 0..n {
                        let value = adv_pop!(self);
                        self.stack.push(value);
                    }
                }
                Op::AdvLoadw => {
                    let word = adv_popw!(self);
                    self.stack.dropw();
                    self.stack.pushw(word);
                }
                Op::AdvPipe => {
                    // We're overwriting the first two words, C and B, so drop them
                    self.stack.dropw();
                    self.stack.dropw();
                    // The third word, A, is saved, but unused
                    let a = popw!(self);
                    // The memory address to write to is the first element of the fourth word
                    let addr = pop_addr!(self);
                    // We update the original address += 2, and restore A
                    self.stack.push_u32(addr as u32 + 2);
                    self.stack.pushw(a);
                    // We then move words D and E from the advice stack to the operand stack,
                    // while also writing those words to memory starting at `addr`
                    let d = adv_popw!(self);
                    self.stack.pushw(d);
                    self.memory[addr] = d;
                    let e = adv_popw!(self);
                    self.stack.pushw(e);
                    self.memory[addr + 1] = e;
                    // Lastly, since we performed a memory write here, suspend like we do for other
                    // memory-modifying ops
                    self.callstack.push(state);
                    return Ok(EmulatorEvent::MemoryWrite {
                        addr: addr as u32,
                        size: 16,
                    });
                }
                Op::AdvInjectPushU64Div => {
                    const HI_MASK: u64 = u64::MAX << 32;
                    const LO_MASK: u64 = u32::MAX as u64;
                    let b_hi = pop_u32!(self) as u64;
                    let b_lo = pop_u32!(self) as u64;
                    let b = (b_hi << 32) | b_lo;
                    assert!(b > 0, "assertion failed: division by zero");
                    let a_hi = pop_u32!(self) as u64;
                    let a_lo = pop_u32!(self) as u64;
                    let a = (a_hi << 32) | a_lo;
                    let q = a / b;
                    let q_hi = (q & HI_MASK) >> 32;
                    let q_lo = q & LO_MASK;
                    let r = a % b;
                    let r_hi = (r & HI_MASK) >> 32;
                    let r_lo = r & LO_MASK;
                    self.advice_stack.push_u32(r_hi as u32);
                    self.advice_stack.push_u32(r_lo as u32);
                    self.advice_stack.push_u32(q_hi as u32);
                    self.advice_stack.push_u32(q_lo as u32);
                    self.stack.push_u32(a_lo as u32);
                    self.stack.push_u32(a_hi as u32);
                    self.stack.push_u32(b_lo as u32);
                    self.stack.push_u32(b_hi as u32);
                }
                Op::AdvInjectInsertMem
                | Op::AdvInjectInsertHperm
                | Op::AdvInjectInsertHdword
                | Op::AdvInjectInsertHdwordImm(_)
                | Op::AdvInjectPushMTreeNode
                | Op::AdvInjectPushMapVal
                | Op::AdvInjectPushMapValImm(_)
                | Op::AdvInjectPushMapValN
                | Op::AdvInjectPushMapValNImm(_) => unimplemented!(),
                Op::Assert => {
                    let cond = pop_bool!(self);
                    assert!(cond, "assertion failed: expected true, got false");
                }
                Op::Assertz => {
                    let cond = pop_bool!(self);
                    assert!(!cond, "assertion failed: expected false, got true");
                }
                Op::AssertEq => {
                    let (b, a) = pop2!(self);
                    assert_eq!(a, b, "equality assertion failed");
                }
                Op::AssertEqw => {
                    let b = popw!(self);
                    let a = popw!(self);
                    assert_eq!(a, b, "equality assertion failed");
                }
                Op::LocAddr(id) => {
                    let addr = state.fp() + id.as_usize() as u32;
                    debug_assert!(addr < self.memory.len() as u32);
                    self.stack.push_u32(addr * 16);
                }
                Op::LocStore(id) => {
                    let addr = (state.fp() + id.as_usize() as u32) as usize;
                    debug_assert!(addr < self.memory.len());
                    let value = pop!(self);
                    self.memory[addr][0] = value;
                    return Ok(EmulatorEvent::MemoryWrite {
                        addr: addr as u32,
                        size: 4,
                    });
                }
                Op::LocStorew(id) => {
                    let addr = (state.fp() + id.as_usize() as u32) as usize;
                    assert!(addr < self.memory.len() - 4, "out of bounds memory access");
                    let word =
                        self.stack.peekw().expect("operand stack does not contain a full word");
                    self.memory[addr] = word;
                    return Ok(EmulatorEvent::MemoryWrite {
                        addr: addr as u32,
                        size: 16,
                    });
                }
                Op::LocLoad(id) => {
                    let addr = (state.fp() + id.as_usize() as u32) as usize;
                    debug_assert!(addr < self.memory.len());
                    self.stack.push(self.memory[addr][0]);
                }
                Op::LocLoadw(id) => {
                    let addr = (state.fp() + id.as_usize() as u32) as usize;
                    debug_assert!(addr < self.memory.len());
                    self.stack.dropw();
                    self.stack.pushw(self.memory[addr]);
                }
                Op::MemLoad => {
                    let addr = pop_addr!(self);
                    self.stack.push(self.memory[addr][0]);
                }
                Op::MemLoadImm(addr) => {
                    let addr = addr as usize;
                    assert!(addr < self.memory.len(), "out of bounds memory access");
                    self.stack.push(self.memory[addr][0]);
                }
                Op::MemLoadw => {
                    let addr = pop_addr!(self);
                    self.stack.dropw();
                    let mut word = self.memory[addr];
                    word.reverse();
                    self.stack.pushw(word);
                }
                Op::MemLoadwImm(addr) => {
                    let addr = addr as usize;
                    assert!(addr < self.memory.len() - 4, "out of bounds memory access");
                    self.stack.dropw();
                    let mut word = self.memory[addr];
                    word.reverse();
                    self.stack.pushw(word);
                }
                Op::MemStore => {
                    let addr = pop_addr!(self);
                    let value = pop!(self);
                    self.memory[addr][0] = value;
                    self.callstack.push(state);
                    return Ok(EmulatorEvent::MemoryWrite {
                        addr: addr as u32,
                        size: 4,
                    });
                }
                Op::MemStoreImm(addr) => {
                    let addr = addr as usize;
                    assert!(addr < self.memory.len(), "out of bounds memory access");
                    let value = self.stack.pop().expect("operand stack is empty");
                    self.memory[addr][0] = value;
                    self.callstack.push(state);
                    return Ok(EmulatorEvent::MemoryWrite {
                        addr: addr as u32,
                        size: 4,
                    });
                }
                Op::MemStorew => {
                    let addr = pop_addr!(self);
                    let mut word =
                        self.stack.peekw().expect("operand stack does not contain a full word");
                    word.reverse();
                    self.memory[addr] = word;
                    self.callstack.push(state);
                    return Ok(EmulatorEvent::MemoryWrite {
                        addr: addr as u32,
                        size: 16,
                    });
                }
                Op::MemStorewImm(addr) => {
                    let addr = addr as usize;
                    assert!(addr < self.memory.len() - 4, "out of bounds memory access");
                    let mut word =
                        self.stack.peekw().expect("operand stack does not contain a full word");
                    word.reverse();
                    self.memory[addr] = word;
                    self.callstack.push(state);
                    return Ok(EmulatorEvent::MemoryWrite {
                        addr: addr as u32,
                        size: 16,
                    });
                }
                Op::If(then_blk, else_blk) => {
                    self.step_over = Some(state.ip());
                    let cond = pop_bool!(self);
                    let dest = if cond {
                        state.enter_block(then_blk);
                        then_blk
                    } else {
                        state.enter_block(else_blk);
                        else_blk
                    };
                    self.callstack.push(state);
                    return Ok(EmulatorEvent::Jump(dest));
                }
                Op::While(body_blk) => {
                    self.step_over = Some(state.ip());
                    let cond = pop_bool!(self);
                    if cond {
                        state.enter_while_loop(body_blk);
                        self.callstack.push(state);
                        return Ok(EmulatorEvent::EnterLoop(body_blk));
                    }
                }
                Op::Repeat(n, body_blk) => {
                    self.step_over = Some(state.ip());
                    state.repeat_block(body_blk, n);
                    self.callstack.push(state);
                    return Ok(EmulatorEvent::EnterLoop(body_blk));
                }
                Op::Exec(callee) => {
                    let fun = self
                        .functions
                        .get(&callee)
                        .cloned()
                        .ok_or(EmulationError::UndefinedFunction(callee))?;
                    self.step_over = Some(state.ip());
                    match fun {
                        Stub::Asm(ref function) => {
                            let fp = self.locals[&function.name];
                            let callee_state = Activation::new(function.clone(), fp);
                            // Suspend caller and scheduled callee next
                            self.callstack.push(state);
                            self.callstack.push(callee_state);
                            return Ok(EmulatorEvent::EnterFunction(function.name));
                        }
                        Stub::Native(_function) => unimplemented!(),
                    }
                }
                Op::Call(_callee) | Op::Syscall(_callee) => unimplemented!(),
                Op::Add => binop!(self, add),
                Op::AddImm(imm) => binop!(self, add, imm),
                Op::Sub => binop!(self, sub),
                Op::SubImm(imm) => binop!(self, sub, imm),
                Op::Mul => binop!(self, mul),
                Op::MulImm(imm) => binop!(self, mul, imm),
                Op::Div => binop!(self, div),
                Op::DivImm(imm) => binop!(self, div, imm),
                Op::Neg => {
                    let a = self.stack.pop().expect("operand stack is empty");
                    self.stack.push(-a);
                }
                Op::Inv => {
                    let a = self.stack.pop().expect("operand stack is empty");
                    self.stack.push(a.inv());
                }
                Op::Incr => binop!(self, add, Felt::ONE),
                Op::Ilog2 => {
                    let a = peek!(self).as_int();
                    assert!(a > 0, "invalid ilog2 argument: expected {a} to be > 0");
                    self.advice_stack.push_u32(a.ilog2());
                }
                Op::Pow2 => {
                    let a = pop!(self).as_int();
                    assert!(
                        a < 64,
                        "invalid power of two: expected {a} to be a value less than 64"
                    );
                    let two = Felt::new(2);
                    self.stack.push(two.exp(a));
                }
                Op::Exp => {
                    let (b, a) = pop2!(self);
                    let b = b.as_int();
                    assert!(
                        b < 64,
                        "invalid power of two: expected {b} to be a value less than 64"
                    );
                    self.stack.push(a.exp(b));
                }
                Op::ExpImm(pow) | Op::ExpBitLength(pow) => {
                    let pow = pow as u64;
                    let a = pop!(self);
                    assert!(
                        pow < 64,
                        "invalid power of two: expected {pow} to be a value less than 64"
                    );
                    self.stack.push(a.exp(pow));
                }
                Op::Not => {
                    let a = pop_bool!(self);
                    self.stack.push_u8(!a as u8);
                }
                Op::And => {
                    let b = pop_bool!(self);
                    let a = pop_bool!(self);
                    self.stack.push_u8((b & a) as u8);
                }
                Op::AndImm(b) => {
                    let a = pop_bool!(self);
                    self.stack.push_u8((a & b) as u8);
                }
                Op::Or => {
                    let b = pop_bool!(self);
                    let a = pop_bool!(self);
                    self.stack.push_u8((b | a) as u8);
                }
                Op::OrImm(b) => {
                    let a = pop_bool!(self);
                    self.stack.push_u8((a | b) as u8);
                }
                Op::Xor => {
                    let b = pop_bool!(self);
                    let a = pop_bool!(self);
                    self.stack.push_u8((b ^ a) as u8);
                }
                Op::XorImm(b) => {
                    let a = pop_bool!(self);
                    self.stack.push_u8((a ^ b) as u8);
                }
                Op::Eq => comparison!(self, eq),
                Op::EqImm(imm) => comparison!(self, eq, imm.as_int()),
                Op::Neq => comparison!(self, ne),
                Op::NeqImm(imm) => comparison!(self, ne, imm.as_int()),
                Op::Gt => comparison!(self, gt),
                Op::GtImm(imm) => comparison!(self, gt, imm.as_int()),
                Op::Gte => comparison!(self, ge),
                Op::GteImm(imm) => comparison!(self, ge, imm.as_int()),
                Op::Lt => comparison!(self, lt),
                Op::LtImm(imm) => comparison!(self, lt, imm.as_int()),
                Op::Lte => comparison!(self, le),
                Op::LteImm(imm) => comparison!(self, le, imm.as_int()),
                Op::IsOdd => {
                    let a = pop!(self).as_int();
                    self.stack.push_u8((a % 2 == 0) as u8);
                }
                Op::Eqw => {
                    let b = popw!(self);
                    let a = popw!(self);
                    self.stack.push_u8((a == b) as u8);
                }
                Op::Clk => {
                    self.stack.push(Felt::new(self.clk as u64));
                }
                Op::Sdepth => {
                    self.stack.push(Felt::new(self.stack.len() as u64));
                }
                Op::U32Test => {
                    let top = self.stack.peek().expect("operand stack is empty").as_int();
                    self.stack.push_u8((top < U32_P) as u8);
                }
                Op::U32Testw => {
                    let word = self.stack.peekw().expect("operand stack is empty");
                    let is_true = word.iter().all(|elem| elem.as_int() < U32_P);
                    self.stack.push_u8(is_true as u8);
                }
                Op::U32Assert => {
                    let top = self.stack.peek().expect("operand stack is empty").as_int();
                    assert!(top < U32_P, "assertion failed: {top} is larger than 2^32");
                }
                Op::U32Assert2 => {
                    let a = self.stack.peek().expect("operand stack is empty").as_int();
                    let b = self.stack.peek().expect("operand stack is empty").as_int();
                    assert!(a < U32_P, "assertion failed: {a} is larger than 2^32");
                    assert!(b < U32_P, "assertion failed: {b} is larger than 2^32");
                }
                Op::U32Assertw => {
                    let word = self.stack.peekw().expect("operand stack is empty");
                    for elem in word.into_iter() {
                        assert!(
                            elem.as_int() < U32_P,
                            "assertion failed: {elem} is larger than 2^32"
                        );
                    }
                }
                Op::U32Cast => {
                    let a = pop!(self).as_int();
                    self.stack.push(Felt::new(a % U32_P));
                }
                Op::U32Split => {
                    let a = pop!(self).as_int();
                    let hi = a / U32_P;
                    let lo = a % U32_P;
                    self.stack.push(Felt::new(lo));
                    self.stack.push(Felt::new(hi));
                }
                Op::U32OverflowingAdd => binop_overflowing_u32!(self, add),
                Op::U32OverflowingAddImm(imm) => binop_overflowing_u32!(self, add, imm),
                Op::U32WrappingAdd => binop_wrapping_u32!(self, add),
                Op::U32WrappingAddImm(imm) => binop_wrapping_u32!(self, add, imm),
                Op::U32OverflowingAdd3 => todo!(),
                Op::U32WrappingAdd3 => todo!(),
                Op::U32OverflowingSub => binop_overflowing_u32!(self, sub),
                Op::U32OverflowingSubImm(imm) => binop_overflowing_u32!(self, sub, imm),
                Op::U32WrappingSub => binop_wrapping_u32!(self, sub),
                Op::U32WrappingSubImm(imm) => binop_wrapping_u32!(self, sub, imm),
                Op::U32OverflowingMul => binop_overflowing_u32!(self, mul),
                Op::U32OverflowingMulImm(imm) => binop_overflowing_u32!(self, mul, imm),
                Op::U32WrappingMul => binop_wrapping_u32!(self, mul),
                Op::U32WrappingMulImm(imm) => binop_wrapping_u32!(self, mul, imm),
                Op::U32OverflowingMadd => {
                    let b = pop_u32!(self) as u64;
                    let a = pop_u32!(self) as u64;
                    let c = pop_u32!(self) as u64;
                    let result = a * b + c;
                    let d = result % 2u64.pow(32);
                    let e = result / 2u64.pow(32);
                    self.stack.push(Felt::new(d));
                    self.stack.push(Felt::new(e));
                }
                Op::U32WrappingMadd => {
                    let b = pop_u32!(self) as u64;
                    let a = pop_u32!(self) as u64;
                    let c = pop_u32!(self) as u64;
                    let d = (a * b + c) % 2u64.pow(32);
                    self.stack.push(Felt::new(d));
                }
                Op::U32Div => binop_unchecked_u32!(self, div),
                Op::U32DivImm(imm) => binop_unchecked_u32!(self, div, imm as u64),
                Op::U32Mod => {
                    let b = pop!(self).as_int();
                    let a = pop!(self).as_int();
                    self.stack.push(Felt::new(a % b));
                }
                Op::U32ModImm(imm) => {
                    let a = pop!(self).as_int();
                    self.stack.push(Felt::new(a % imm as u64));
                }
                Op::U32DivMod => {
                    let b = pop!(self).as_int();
                    let a = pop!(self).as_int();
                    self.stack.push(Felt::new(a / b));
                    self.stack.push(Felt::new(a % b));
                }
                Op::U32DivModImm(b) => {
                    let b = b as u64;
                    let a = pop!(self).as_int();
                    self.stack.push(Felt::new(a / b));
                    self.stack.push(Felt::new(a % b));
                }
                Op::U32And => binop32!(self, bitand),
                Op::U32Or => binop32!(self, bitor),
                Op::U32Xor => binop32!(self, bitxor),
                Op::U32Not => {
                    let a = pop_u32!(self);
                    self.stack.push_u32(!a);
                }
                Op::U32Shl => binop_wrapping_u32!(self, shl),
                Op::U32ShlImm(imm) => binop_wrapping_u32!(self, shl, imm),
                Op::U32Shr => binop_wrapping_u32!(self, shr),
                Op::U32ShrImm(imm) => binop_wrapping_u32!(self, shr, imm),
                Op::U32Rotl => {
                    let b = pop_u32!(self);
                    let a = pop_u32!(self);
                    self.stack.push_u32(a.rotate_left(b));
                }
                Op::U32RotlImm(imm) => {
                    let a = pop_u32!(self);
                    self.stack.push_u32(a.rotate_left(imm));
                }
                Op::U32Rotr => {
                    let b = pop_u32!(self);
                    let a = pop_u32!(self);
                    self.stack.push_u32(a.rotate_right(b));
                }
                Op::U32RotrImm(imm) => {
                    let a = pop_u32!(self);
                    self.stack.push_u32(a.rotate_right(imm));
                }
                Op::U32Popcnt => {
                    let a = pop_u32!(self);
                    self.stack.push_u32(a.count_ones());
                }
                Op::U32Clz => {
                    let a = pop_u32!(self);
                    self.stack.push_u32(a.leading_zeros());
                }
                Op::U32Clo => {
                    let a = pop_u32!(self);
                    self.stack.push_u32(a.leading_ones());
                }
                Op::U32Ctz => {
                    let a = pop_u32!(self);
                    self.stack.push_u32(a.trailing_zeros());
                }
                Op::U32Cto => {
                    let a = pop_u32!(self);
                    self.stack.push_u32(a.trailing_ones());
                }
                Op::U32Gt => comparison!(self, gt),
                Op::U32Gte => comparison!(self, ge),
                Op::U32Lt => comparison!(self, lt),
                Op::U32Lte => comparison!(self, le),
                Op::U32Min => {
                    let b = pop!(self).as_int();
                    let a = pop!(self).as_int();
                    self.stack.push(Felt::new(cmp::min(a, b)));
                }
                Op::U32Max => {
                    let b = pop!(self).as_int();
                    let a = pop!(self).as_int();
                    self.stack.push(Felt::new(cmp::max(a, b)));
                }
                Op::Breakpoint => {
                    self.callstack.push(state);
                    return Ok(EmulatorEvent::Breakpoint(BreakpointEvent::Step));
                }
                Op::DebugStack => {
                    dbg!(self.stack.debug());
                }
                Op::DebugStackN(n) => {
                    dbg!(self.stack.debug().take(n as usize));
                }
                Op::DebugMemory
                | Op::DebugMemoryAt(_)
                | Op::DebugMemoryRange(..)
                | Op::DebugFrame
                | Op::DebugFrameAt(_)
                | Op::DebugFrameRange(..) => (),
                Op::Emit(_) | Op::Trace(_) => (),
                op => unimplemented!("missing opcode implementation for {op:?}"),
            }

            match ix_with_op.effect {
                ControlEffect::Repeat(_) | ControlEffect::Loopback => {
                    self.callstack.push(state);
                    return Ok(EmulatorEvent::EnterLoop(ix_with_op.ip.block));
                }
                ControlEffect::Enter => {
                    self.callstack.push(state);
                    return Ok(EmulatorEvent::Jump(ix_with_op.ip.block));
                }
                ControlEffect::Exit => {
                    self.callstack.push(state);
                    return Ok(EmulatorEvent::Jump(ix_with_op.ip.block));
                }
                ControlEffect::None => (),
            }

            // Suspend the current activation record
            self.callstack.push(state);

            Ok(EmulatorEvent::Suspended)
        } else {
            // No more code left in the current function
            Ok(EmulatorEvent::ExitFunction(current_function))
        }
    }
}
