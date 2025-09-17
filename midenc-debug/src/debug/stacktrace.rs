use std::{
    borrow::Cow,
    cell::{OnceCell, RefCell},
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt,
    path::Path,
    rc::Rc,
    sync::Arc,
};

use miden_core::AssemblyOp;
use miden_debug_types::Location;
use miden_processor::{Operation, RowIndex, VmState};
use midenc_hir::demangle::demangle;
use midenc_session::{
    diagnostics::{SourceFile, SourceSpan},
    Session,
};

use crate::TraceEvent;

#[derive(Debug, Clone)]
struct SpanContext {
    frame_index: usize,
    location: Option<Location>,
}

pub struct CallStack {
    trace_events: Rc<RefCell<BTreeMap<RowIndex, TraceEvent>>>,
    contexts: BTreeSet<Rc<str>>,
    frames: Vec<CallFrame>,
    block_stack: Vec<Option<SpanContext>>,
}
impl CallStack {
    pub fn new(trace_events: Rc<RefCell<BTreeMap<RowIndex, TraceEvent>>>) -> Self {
        Self {
            trace_events,
            contexts: BTreeSet::default(),
            frames: vec![],
            block_stack: vec![],
        }
    }

    pub fn stacktrace<'a>(
        &'a self,
        recent: &'a VecDeque<Operation>,
        session: &'a Session,
    ) -> StackTrace<'a> {
        StackTrace::new(self, recent, session)
    }

    pub fn current_frame(&self) -> Option<&CallFrame> {
        self.frames.last()
    }

    pub fn current_frame_mut(&mut self) -> Option<&mut CallFrame> {
        self.frames.last_mut()
    }

    pub fn nth_frame(&self, n: usize) -> Option<&CallFrame> {
        self.frames.iter().nth_back(n)
    }

    pub fn frames(&self) -> &[CallFrame] {
        self.frames.as_slice()
    }

    /// Updates the call stack from `state`
    ///
    /// Returns the call frame exited this cycle, if any
    pub fn next(&mut self, state: &VmState) -> Option<CallFrame> {
        if let Some(op) = state.op {
            // Do not do anything if this cycle is a continuation of the last instruction
            //let skip = state.asmop.as_ref().map(|op| op.cycle_idx() > 1).unwrap_or(false);
            //if skip {
            //return;
            //}

            // Get the current procedure name context, if available
            let procedure =
                state.asmop.as_ref().map(|op| self.cache_procedure_name(op.context_name()));
            /*
                       if procedure.is_none() {
                           dbg!(self.frames.last().map(|frame| frame.procedure.as_deref()));
                           dbg!(self.block_stack.last().map(|ctx| ctx.as_ref()));
                       }
            */
            // Handle trace events for this cycle
            let event = self.trace_events.borrow().get(&state.clk).copied();
            log::trace!("handling {op} at cycle {}: {:?}", state.clk, &event);
            let popped_frame = self.handle_trace_event(event, procedure.as_ref());
            let is_frame_end = popped_frame.is_some();

            // These ops we do not record in call frame details
            let ignore = matches!(
                op,
                Operation::Join
                    | Operation::Split
                    | Operation::Span
                    | Operation::Respan
                    | Operation::End
            );

            // Manage block stack
            match op {
                Operation::Span => {
                    if let Some(asmop) = state.asmop.as_ref() {
                        log::debug!("{asmop:#?}");
                        self.block_stack.push(Some(SpanContext {
                            frame_index: self.frames.len().saturating_sub(1),
                            location: asmop.as_ref().location().cloned(),
                        }));
                    } else {
                        self.block_stack.push(None);
                    }
                }
                Operation::End => {
                    self.block_stack.pop();
                }
                Operation::Join | Operation::Split => {
                    self.block_stack.push(None);
                }
                _ => (),
            }

            if ignore || is_frame_end {
                return popped_frame;
            }

            // Attempt to supply procedure context from the current span context, if needed +
            // available
            let (procedure, asmop) = match procedure {
                proc @ Some(_) => {
                    (proc, state.asmop.as_ref().map(|info| info.as_ref()).map(Cow::Borrowed))
                }
                None => match self.block_stack.last() {
                    Some(Some(span_ctx)) => {
                        let proc =
                            self.frames.get(span_ctx.frame_index).and_then(|f| f.procedure.clone());
                        let info = state
                            .asmop
                            .as_ref()
                            .map(|info| info.as_ref())
                            .map(Cow::Borrowed)
                            .or_else(|| {
                                let context_name =
                                    proc.as_deref().unwrap_or("<unknown>").to_string();
                                let raw_asmop = miden_core::AssemblyOp::new(
                                    span_ctx.location.clone(),
                                    context_name,
                                    1,
                                    op.to_string(),
                                    false,
                                );
                                Some(Cow::Owned(raw_asmop))
                            });
                        (proc, info)
                    }
                    _ => (None, state.asmop.as_ref().map(|info| info.as_ref()).map(Cow::Borrowed)),
                },
            };

            // Use the current frame's procedure context, if no other more precise context is
            // available
            let procedure =
                procedure.or_else(|| self.frames.last().and_then(|f| f.procedure.clone()));

            // Do we have a frame? If not, create one
            if self.frames.is_empty() {
                self.frames.push(CallFrame::new(procedure.clone()));
            }

            let current_frame = self.frames.last_mut().unwrap();

            // Does the current frame have a procedure context/location? Use the one from this op if
            // so
            let procedure_context_updated =
                current_frame.procedure.is_none() && procedure.is_some();
            if procedure_context_updated {
                current_frame.procedure.clone_from(&procedure);
            }

            // If this is the frame pointer prologue/epilogue drop the last op, which should be a
            // push
            if matches!(op, Operation::FmpUpdate) {
                current_frame.context.pop_back();
            }

            // Push op into call frame if this is any op other than `nop` or frame setup
            if !matches!(op, Operation::Noop | Operation::FmpUpdate) {
                let cycle_idx = state.asmop.as_ref().map(|info| info.cycle_idx()).unwrap_or(1);
                current_frame.push(op, cycle_idx, asmop.as_deref());
            }

            // Check if we should also update the caller frame's exec detail
            let num_frames = self.frames.len();
            if procedure_context_updated && num_frames > 1 {
                let caller_frame = &mut self.frames[num_frames - 2];
                if let Some(OpDetail::Exec { ref mut callee }) = caller_frame.context.back_mut() {
                    if callee.is_none() {
                        *callee = procedure;
                    }
                }
            }
        }

        None
    }

    // Get or cache procedure name/context as `Rc<str>`
    fn cache_procedure_name(&mut self, context_name: &str) -> Rc<str> {
        match self.contexts.get(context_name) {
            Some(name) => Rc::clone(name),
            None => {
                let name = Rc::from(context_name.to_string().into_boxed_str());
                self.contexts.insert(Rc::clone(&name));
                name
            }
        }
    }

    fn handle_trace_event(
        &mut self,
        event: Option<TraceEvent>,
        procedure: Option<&Rc<str>>,
    ) -> Option<CallFrame> {
        // Do we need to handle any frame events?
        if let Some(event) = event {
            match event {
                TraceEvent::FrameStart => {
                    // Record the fact that we exec'd a new procedure in the op context
                    if let Some(current_frame) = self.frames.last_mut() {
                        current_frame.push_exec(procedure.cloned());
                    }
                    // Push a new frame
                    self.frames.push(CallFrame::new(procedure.cloned()));
                }
                TraceEvent::Unknown(code) => log::debug!("unknown trace event: {code}"),
                TraceEvent::FrameEnd => {
                    return self.frames.pop();
                }
                _ => (),
            }
        }
        None
    }
}

pub struct CallFrame {
    procedure: Option<Rc<str>>,
    context: VecDeque<OpDetail>,
    display_name: std::cell::OnceCell<Rc<str>>,
    finishing: bool,
}
impl CallFrame {
    pub fn new(procedure: Option<Rc<str>>) -> Self {
        Self {
            procedure,
            context: Default::default(),
            display_name: Default::default(),
            finishing: false,
        }
    }

    pub fn procedure(&self, strip_prefix: &str) -> Option<Rc<str>> {
        self.procedure.as_ref()?;
        let name = self.display_name.get_or_init(|| {
            let name = self.procedure.as_deref().unwrap();
            let name = match name.split_once("::") {
                Some((module, rest)) if module == strip_prefix => demangle(rest),
                _ => demangle(name),
            };
            Rc::from(name.into_boxed_str())
        });
        Some(Rc::clone(name))
    }

    pub fn push_exec(&mut self, callee: Option<Rc<str>>) {
        if self.context.len() == 5 {
            self.context.pop_front();
        }

        self.context.push_back(OpDetail::Exec { callee });
    }

    pub fn push(&mut self, opcode: Operation, cycle_idx: u8, op: Option<&AssemblyOp>) {
        if cycle_idx > 1 {
            // Should we ignore this op?
            let skip = self.context.back().map(|detail| matches!(detail, OpDetail::Full { op, .. } | OpDetail::Basic { op } if op == &opcode)).unwrap_or(false);
            if skip {
                return;
            }
        }

        if self.context.len() == 5 {
            self.context.pop_front();
        }

        match op {
            Some(op) => {
                let location = op.location().cloned();
                self.context.push_back(OpDetail::Full {
                    op: opcode,
                    location,
                    resolved: Default::default(),
                });
            }
            None => {
                // If this instruction does not have a location, inherit the location
                // of the previous op in the frame, if one is present
                if let Some(loc) = self.context.back().map(|op| op.location().cloned()) {
                    self.context.push_back(OpDetail::Full {
                        op: opcode,
                        location: loc,
                        resolved: Default::default(),
                    });
                } else {
                    self.context.push_back(OpDetail::Basic { op: opcode });
                }
            }
        }
    }

    pub fn last_location(&self) -> Option<&Location> {
        match self.context.back() {
            Some(OpDetail::Full { location, .. }) => {
                let loc = location.as_ref();
                if loc.is_none() {
                    dbg!(&self.context);
                }
                loc
            }
            Some(OpDetail::Basic { .. }) => None,
            Some(OpDetail::Exec { .. }) => {
                let op = self.context.iter().rev().nth(1)?;
                op.location()
            }
            None => None,
        }
    }

    pub fn last_resolved(&self, session: &Session) -> Option<&ResolvedLocation> {
        self.context.back().and_then(|op| op.resolve(session))
    }

    pub fn recent(&self) -> &VecDeque<OpDetail> {
        &self.context
    }

    #[inline(always)]
    pub fn should_break_on_exit(&self) -> bool {
        self.finishing
    }

    #[inline(always)]
    pub fn break_on_exit(&mut self) {
        self.finishing = true;
    }
}

#[derive(Debug, Clone)]
pub enum OpDetail {
    Full {
        op: Operation,
        location: Option<Location>,
        resolved: OnceCell<Option<ResolvedLocation>>,
    },
    Exec {
        callee: Option<Rc<str>>,
    },
    Basic {
        op: Operation,
    },
}
impl OpDetail {
    pub fn callee(&self, strip_prefix: &str) -> Option<Box<str>> {
        match self {
            Self::Exec { callee: None } => Some(Box::from("<unknown>")),
            Self::Exec {
                callee: Some(ref callee),
            } => {
                let name = match callee.split_once("::") {
                    Some((module, rest)) if module == strip_prefix => demangle(rest),
                    _ => demangle(callee),
                };
                Some(name.into_boxed_str())
            }
            _ => None,
        }
    }

    pub fn display(&self) -> String {
        match self {
            Self::Full { op, .. } | Self::Basic { op } => format!("{op}"),
            Self::Exec {
                callee: Some(callee),
            } => format!("exec.{callee}"),
            Self::Exec { callee: None } => "exec.<unavailable>".to_string(),
        }
    }

    pub fn opcode(&self) -> Operation {
        match self {
            Self::Full { op, .. } | Self::Basic { op } => *op,
            Self::Exec { .. } => panic!("no opcode associated with execs"),
        }
    }

    pub fn location(&self) -> Option<&Location> {
        match self {
            Self::Full { ref location, .. } => location.as_ref(),
            Self::Basic { .. } | Self::Exec { .. } => None,
        }
    }

    pub fn resolve(&self, session: &Session) -> Option<&ResolvedLocation> {
        use midenc_session::diagnostics::SourceManagerExt;

        match self {
            Self::Full {
                location: Some(ref loc),
                ref resolved,
                ..
            } => resolved
                .get_or_init(|| {
                    let path = Path::new(loc.uri().as_str());
                    let source_file = if path.exists() {
                        session.source_manager.load_file(path).ok()?
                    } else {
                        session.source_manager.get_by_uri(loc.uri())?
                    };
                    let span = SourceSpan::new(source_file.id(), loc.start..loc.end);
                    let file_line_col = source_file.location(span);
                    Some(ResolvedLocation {
                        source_file,
                        line: file_line_col.line.to_u32(),
                        col: file_line_col.column.to_u32(),
                        span,
                    })
                })
                .as_ref(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedLocation {
    pub source_file: Arc<SourceFile>,
    // TODO(fabrio): Use LineNumber and ColumnNumber instead of raw `u32`.
    pub line: u32,
    pub col: u32,
    pub span: SourceSpan,
}
impl fmt::Display for ResolvedLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.source_file.uri().as_str(), self.line, self.col)
    }
}

pub struct CurrentFrame {
    pub procedure: Option<Rc<str>>,
    pub location: Option<ResolvedLocation>,
}

pub struct StackTrace<'a> {
    callstack: &'a CallStack,
    recent: &'a VecDeque<Operation>,
    session: &'a Session,
    current_frame: Option<CurrentFrame>,
}

impl<'a> StackTrace<'a> {
    pub fn new(
        callstack: &'a CallStack,
        recent: &'a VecDeque<Operation>,
        session: &'a Session,
    ) -> Self {
        let current_frame = callstack.current_frame().map(|frame| {
            let location = frame.last_resolved(session).cloned();
            let procedure = frame.procedure(session.name());
            CurrentFrame {
                procedure,
                location,
            }
        });
        Self {
            callstack,
            recent,
            session,
            current_frame,
        }
    }

    pub fn current_frame(&self) -> Option<&CurrentFrame> {
        self.current_frame.as_ref()
    }
}

impl fmt::Display for StackTrace<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use std::fmt::Write;

        let session_name = self.session.name();
        let num_frames = self.callstack.frames.len();

        writeln!(f, "\nStack Trace:")?;

        for (i, frame) in self.callstack.frames.iter().enumerate() {
            let is_top = i + 1 == num_frames;
            let name = frame.procedure(session_name);
            let name = name.as_deref().unwrap_or("<unknown>");
            if is_top {
                write!(f, " `-> {name}")?;
            } else {
                write!(f, " |-> {name}")?;
            }
            if let Some(resolved) = frame.last_resolved(self.session) {
                write!(f, " in {resolved}")?;
            } else {
                write!(f, " in <unavailable>")?;
            }
            if is_top {
                // Print op context
                let context_size = frame.context.len();
                writeln!(f, ":\n\nLast {context_size} Instructions (of current frame):")?;
                for (i, op) in frame.context.iter().enumerate() {
                    let is_last = i + 1 == context_size;
                    if let Some(callee) = op.callee(session_name) {
                        write!(f, " |   exec.{callee}")?;
                    } else {
                        write!(f, " |   {}", &op.opcode())?;
                    }
                    if is_last {
                        writeln!(f, "\n `-> <error occured here>")?;
                    } else {
                        f.write_char('\n')?;
                    }
                }

                let context_size = self.recent.len();
                writeln!(f, "\n\nLast {context_size} Instructions (any frame):")?;
                for (i, op) in self.recent.iter().enumerate() {
                    let is_last = i + 1 == context_size;
                    if is_last {
                        writeln!(f, " |   {}", &op)?;
                        writeln!(f, " `-> <error occured here>")?;
                    } else {
                        writeln!(f, " |   {}", &op)?;
                    }
                }
            } else {
                f.write_char('\n')?;
            }
        }

        Ok(())
    }
}
