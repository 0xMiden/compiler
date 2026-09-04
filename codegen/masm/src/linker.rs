use midenc_hir::{
    FxHashMap, Op, Symbol, WalkResult,
    dialects::builtin::{
        self, DataSegmentError, SegmentRef,
        attributes::{U64Attr, UnitAttr},
    },
};

/// The page size used for the linker's own memory layout, in bytes.
const DEFAULT_PAGE_SIZE: u32 = 2u32.pow(16);
/// The default number of pages reserved before any compiler-managed memory region.
///
/// This is a fallback floor for modules that carry no
/// [builtin::Module::RESERVED_MEMORY_ATTR] attribute, conservatively sized to cover the stack
/// and static-data conventions of common module producers (e.g. rustc's default 16-page shadow
/// stack plus a page of `static` data). Modules with the attribute are laid out past their
/// declared reservation instead, which dominates this default whenever it is larger.
const DEFAULT_RESERVATION: u32 = 17;

/// Fixed memory cells the compiler reserves in the address band no program can reach.
///
/// Guest pointers are 32-bit byte addresses, so guest-reachable element addresses end below
/// [`Self::GUEST_ADDRESS_LIMIT`]; procedure locals are framed upwards from the VM's initial frame
/// pointer, [`Self::LOCALS_FRAME_START`]. The band in between belongs to no program, and every
/// fixed cell the compiler needs is allocated from it here, so cells cannot collide. The cells
/// need no initialization: each use is a store immediately followed by the instruction consuming
/// it, and a fresh context starts zero-filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReservedCell {
    /// The word to which a `dyncall` lowering spills the callee's MAST root, which the VM reads
    /// from memory.
    DyncallRoot,
}

impl ReservedCell {
    /// All reserved cells, in address order.
    pub const ALL: [Self; 1] = [Self::DyncallRoot];
    /// Number of elements every cell spans: one word.
    pub const ELEMENTS: u32 = 4;
    /// First element address past guest-reachable memory: `u32::MAX` bytes, in elements.
    pub const GUEST_ADDRESS_LIMIT: u64 = (u32::MAX as u64 + 1) / Self::ELEMENTS as u64;
    /// The VM's initial frame pointer, from which procedure locals are allocated upwards.
    ///
    /// Mirrors `miden_core::FMP_INIT_VALUE`, which is not usable in constant expressions; the
    /// unit tests check the two agree.
    pub const LOCALS_FRAME_START: u64 = 1 << 31;

    /// Returns the word-aligned element address of this cell.
    pub const fn element_addr(self) -> u32 {
        match self {
            Self::DyncallRoot => Self::GUEST_ADDRESS_LIMIT as u32,
        }
    }
}

// The band invariants are checked when the crate compiles: a cell that a guest pointer or a
// locals frame could reach would be silent memory corruption, never a diagnostic.
const _: () = {
    let cells = ReservedCell::ALL;
    let mut i = 0;
    while i < cells.len() {
        let addr = cells[i].element_addr() as u64;
        assert!(
            addr.is_multiple_of(ReservedCell::ELEMENTS as u64),
            "reserved cell is not word-aligned"
        );
        assert!(addr >= ReservedCell::GUEST_ADDRESS_LIMIT, "reserved cell is guest-reachable");
        assert!(
            addr + ReservedCell::ELEMENTS as u64 <= ReservedCell::LOCALS_FRAME_START,
            "reserved cell overlaps procedure locals"
        );
        let mut j = i + 1;
        while j < cells.len() {
            let other = cells[j].element_addr() as u64;
            assert!(
                addr + ReservedCell::ELEMENTS as u64 <= other
                    || other + ReservedCell::ELEMENTS as u64 <= addr,
                "reserved cells overlap"
            );
            j += 1;
        }
        i += 1;
    }
};

pub struct LinkInfo {
    component: Option<builtin::ComponentId>,
    globals_layout: GlobalVariableLayout,
    segment_layout: builtin::DataSegmentLayout,
    function_tables: FunctionTableLayout,
    component_start: Option<builtin::FunctionRef>,
    heap_base: u32,
}

impl LinkInfo {
    #[cfg(test)]
    pub fn new(id: Option<builtin::ComponentId>) -> Self {
        Self {
            component: id,
            globals_layout: Default::default(),
            segment_layout: Default::default(),
            function_tables: Default::default(),
            component_start: None,
            heap_base: 0,
        }
    }

    #[inline]
    pub fn component(&self) -> Option<&builtin::ComponentId> {
        self.component.as_ref()
    }

    pub fn has_globals(&self) -> bool {
        !self.globals_layout.offsets.is_empty()
    }

    pub fn has_data_segments(&self) -> bool {
        !self.segment_layout.is_empty()
    }

    pub fn has_function_tables(&self) -> bool {
        !self.function_tables.is_empty()
    }

    pub fn globals_layout(&self) -> &GlobalVariableLayout {
        &self.globals_layout
    }

    #[allow(unused)]
    pub fn segment_layout(&self) -> &builtin::DataSegmentLayout {
        &self.segment_layout
    }

    pub fn function_tables(&self) -> &FunctionTableLayout {
        &self.function_tables
    }

    /// Returns the core Wasm function which must run after ordinary component initialization.
    #[inline]
    pub fn component_start(&self) -> Option<builtin::FunctionRef> {
        self.component_start
    }

    /// Returns true if the component requires an `init` procedure to set up linear memory
    /// (data segments, global variables, or function tables) and run its core Wasm start function
    /// before execution.
    pub fn requires_init(&self) -> bool {
        self.has_globals()
            || self.has_data_segments()
            || self.has_function_tables()
            || self.component_start.is_some()
    }

    /// Get the address of the first page boundary past all statically-allocated memory (global
    /// variables and function tables), or the end of reserved memory if larger; this is where
    /// the dynamic heap starts when the program is executed.
    ///
    /// The address is computed and validated by [Linker::link], which fails with
    /// [LinkerError::LayoutOverflow] when static memory leaves no representable heap base.
    #[inline(always)]
    pub fn heap_base(&self) -> u32 {
        self.heap_base
    }
}

pub struct Linker {
    globals_layout: GlobalVariableLayout,
    segment_layout: builtin::DataSegmentLayout,
    function_tables: Vec<builtin::FunctionTableRef>,
    component_start: Option<builtin::FunctionRef>,
    reserved_memory_pages: u32,
    page_size: u32,
}

impl Default for Linker {
    fn default() -> Self {
        Self::new(DEFAULT_RESERVATION, DEFAULT_PAGE_SIZE)
    }
}

impl Linker {
    pub fn new(reserved_memory_pages: u32, page_size: u32) -> Self {
        let page_size = if page_size > 0 {
            assert!(page_size.is_power_of_two());
            page_size
        } else {
            DEFAULT_PAGE_SIZE
        };
        let globals_start = reserved_memory_pages * page_size;
        Self {
            globals_layout: GlobalVariableLayout::new(globals_start, page_size),
            segment_layout: Default::default(),
            function_tables: Default::default(),
            component_start: None,
            reserved_memory_pages,
            page_size,
        }
    }

    pub fn link(
        mut self,
        id: Option<builtin::ComponentId>,
        component: &midenc_hir::Operation,
    ) -> Result<LinkInfo, LinkerError> {
        // Gather information needed to compute component data layout

        // 1. Verify that the component is non-empty
        if !component.has_regions() {
            // This component has no definition
            return Err(LinkerError::Undefined);
        }
        let body = component.region(0);
        if body.is_empty() {
            // This component has no definition
            return Err(LinkerError::Undefined);
        }

        // 2. Discover and validate the optional core Wasm start marker before computing layout.
        // This preflight walks every operation nested in the selected component so a misplaced
        // marker is diagnosed rather than silently ignored. Supporting world siblings are not
        // nested here and therefore remain outside this component link.
        self.discover_component_start(component)?;

        // 3. Visit each Module in the component and discover Segment, GlobalVariable, and
        // FunctionTable items, along with the memory claimed by the modules themselves
        let mut declared_reserved_memory = 0u64;
        let body = body.entry();
        for item in body.body() {
            if let Some(module) = item.downcast_ref::<builtin::Module>() {
                self.visit_module(module, &mut declared_reserved_memory, id.as_ref())?;
            }
        }

        // 4. Layout global variables past all memory claimed by the modules themselves
        let next_available_offset =
            self.segment_layout.next_available_offset().ok_or_else(|| {
                LinkerError::LayoutOverflow {
                    reason: alloc::string::String::from(
                        "the data segments reach the end of the 32-bit address space, leaving no \
                         room for compiler-managed memory",
                    ),
                }
            })?;
        let reserved_offset = (self.reserved_memory_pages * self.page_size).next_multiple_of(4);
        // We add a page after the data segments as headroom for producer-placed data that
        // occupies address space without being visible as data segments (e.g. zero-initialized
        // statics).
        let next_available_offset_with_headroom = next_available_offset
            .checked_add(DEFAULT_PAGE_SIZE)
            .ok_or_else(|| LinkerError::LayoutOverflow {
                reason: alloc::format!(
                    "data segments ending at {next_available_offset:#x} leave no room for \
                     compiler-managed memory"
                ),
            })?;
        // A module's declared memory reservation is a sound upper bound on everything its
        // producer placed in linear memory, whereas the one-page allowance above the data
        // segments is only a heuristic.
        let declared_reserved_offset =
            u32::try_from(declared_reserved_memory).map_err(|_| LinkerError::LayoutOverflow {
                reason: alloc::format!(
                    "the declared module memory reservation ({declared_reserved_memory} bytes) \
                     leaves no room for compiler-managed memory"
                ),
            })?;
        log::debug!(target: "linker",
            "next_available_offset (with headroom) from segments: {:#x}, reserved_offset: {:#x}, \
             declared_reserved_offset: {:#x}, segment_count: {}",
            next_available_offset_with_headroom,
            reserved_offset,
            declared_reserved_offset,
            self.segment_layout.len()
        );
        self.globals_layout.update_global_table_offset(
            reserved_offset
                .max(next_available_offset_with_headroom)
                .max(declared_reserved_offset),
        )?;
        log::debug!(target: "linker",
            "global_table_offset set to: {:#x}",
            self.globals_layout.global_table_offset()
        );

        // 5. Lay out function tables in the page following the global table, two words per slot
        // (MAST root digest + signature tag). Page alignment makes the first table word-aligned
        // as `dynexec` requires, and each table's byte size is a word multiple, so subsequent
        // tables stay word-aligned too.
        let mut function_tables = FunctionTableLayout::default();
        let globals_boundary = self.globals_layout.next_page_boundary().ok_or_else(|| {
            LinkerError::LayoutOverflow {
                reason: alloc::format!(
                    "global variables ending at {:#x} overflow the 32-bit address space when \
                     rounded to the next page",
                    self.globals_layout.next_offset
                ),
            }
        })?;
        let mut next_table_offset = globals_boundary;
        for table_ref in self.function_tables.drain(..) {
            let slots = *table_ref.borrow().get_num_slots();
            let size_in_bytes = slots
                .checked_mul(FunctionTableLayout::SLOT_SIZE_BYTES)
                .and_then(|size| next_table_offset.checked_add(size).and(Some(size)))
                .ok_or_else(|| LinkerError::LayoutOverflow {
                    reason: alloc::format!(
                        "a function table with {slots} slots at offset {next_table_offset:#x} \
                         does not fit in linear memory"
                    ),
                })?;
            log::debug!(target: "linker",
                "function table '{}' with {slots} slots allocated at offset {next_table_offset:#x}",
                table_ref.borrow().get_name().as_str()
            );
            function_tables.tables.push((table_ref, next_table_offset));
            next_table_offset += size_in_bytes;
            function_tables.end_offset = next_table_offset;
        }

        // 6. Compute the dynamic heap base: the first page boundary past all
        // statically-allocated memory (global variables and function tables), or the end of
        // reserved memory if larger. Static memory that reaches the last page leaves no
        // representable heap base, so that is a link failure rather than a panic in an accessor.
        let after_tables = function_tables
            .end_offset
            .checked_next_multiple_of(self.page_size)
            .ok_or_else(|| LinkerError::LayoutOverflow {
                reason: alloc::format!(
                    "function tables ending at {:#x} leave no room for the dynamic heap",
                    function_tables.end_offset
                ),
            })?;
        let after_static = globals_boundary.max(after_tables);
        let reserved_bytes = self.reserved_memory_pages as u64 * self.page_size as u64;
        let heap_base = u32::try_from((after_static as u64).max(reserved_bytes)).map_err(|_| {
            LinkerError::LayoutOverflow {
                reason: alloc::string::String::from(
                    "static and reserved memory leave no room for the dynamic heap in the 32-bit \
                     address space",
                ),
            }
        })?;

        Ok(LinkInfo {
            component: id,
            globals_layout: core::mem::take(&mut self.globals_layout),
            segment_layout: core::mem::take(&mut self.segment_layout),
            function_tables,
            component_start: self.component_start,
            heap_base,
        })
    }

    /// Discover the single function carrying the frontend/backend component-start contract.
    ///
    /// The marker is deliberately legal only on a public, defined `extern("C") () -> ()`
    /// function nested in this component's core-module tree. In particular, component/interface
    /// functions and arbitrary operations cannot use it to acquire initialization semantics.
    fn discover_component_start(
        &mut self,
        component: &midenc_hir::Operation,
    ) -> Result<(), LinkerError> {
        let root = component.as_operation_ref();
        component
            .prewalk(|op| {
                if !op.has_attribute(midenc_dialect_hir::WASM_COMPONENT_START_ATTR) {
                    return WalkResult::Continue(());
                }

                if op
                    .get_typed_attribute::<UnitAttr>(midenc_dialect_hir::WASM_COMPONENT_START_ATTR)
                    .is_none()
                {
                    return WalkResult::Break(LinkerError::InvalidComponentStartMarker {
                        reason: alloc::format!(
                            "attribute '{}' on '{}' must have value `unit`",
                            midenc_dialect_hir::WASM_COMPONENT_START_ATTR,
                            op.name()
                        ),
                    });
                }

                let Some(function) = op.downcast_ref::<builtin::Function>() else {
                    return WalkResult::Break(LinkerError::InvalidComponentStartMarker {
                        reason: alloc::format!(
                            "attribute '{}' is only valid on a core-module function, not '{}'",
                            midenc_dialect_hir::WASM_COMPONENT_START_ATTR,
                            op.name()
                        ),
                    });
                };

                // Walk from the function's containing operation to the selected component root.
                // Every operation on that path must be a module; otherwise this is a component-
                // level or interface function rather than translated core Wasm. The outermost
                // module is a direct child of `init`'s root and may be private. A nested module,
                // however, must be public for a direct qualified `exec` from the root to traverse
                // it under MASM visibility rules.
                let mut containing_modules = alloc::vec::Vec::<builtin::ModuleRef>::new();
                let mut parent = function.as_operation().parent_op();
                let mut reached_root = false;
                while let Some(parent_ref) = parent {
                    if parent_ref == root {
                        reached_root = true;
                        break;
                    }
                    let parent_op = parent_ref.borrow();
                    let next = parent_op.parent_op();
                    let Some(module) = parent_op.downcast_ref::<builtin::Module>() else {
                        return WalkResult::Break(LinkerError::InvalidComponentStartMarker {
                            reason: alloc::format!(
                                "function '{}' is not in the selected component's core-module tree",
                                function.path()
                            ),
                        });
                    };
                    containing_modules.push(module.as_module_ref());
                    drop(parent_op);
                    parent = next;
                }
                if !reached_root || containing_modules.is_empty() {
                    return WalkResult::Break(LinkerError::InvalidComponentStartMarker {
                        reason: alloc::format!(
                            "function '{}' is not in the selected component's core-module tree",
                            function.path()
                        ),
                    });
                }

                if function.is_declaration() {
                    return WalkResult::Break(LinkerError::InvalidComponentStartFunction {
                        function: function.path().to_string(),
                        reason: alloc::string::String::from(
                            "it is a declaration, not a definition",
                        ),
                    });
                }
                if !function.is_public() {
                    return WalkResult::Break(LinkerError::InvalidComponentStartFunction {
                        function: function.path().to_string(),
                        reason: alloc::string::String::from(
                            "it is not public and cannot be executed from component `init`",
                        ),
                    });
                }

                let signature = function.signature();
                if signature.cc != midenc_hir::CallConv::C
                    || !signature.params.is_empty()
                    || !signature.results.is_empty()
                {
                    return WalkResult::Break(LinkerError::InvalidComponentStartFunction {
                        function: function.path().to_string(),
                        reason: alloc::format!(
                            "expected `extern(\"C\") () -> ()`, got calling convention '{}' with \
                             {} parameters and {} results",
                            signature.cc,
                            signature.params.len(),
                            signature.results.len()
                        ),
                    });
                }

                if let Some(inaccessible) = containing_modules
                    .iter()
                    .take(containing_modules.len().saturating_sub(1))
                    .find(|module| !module.borrow().is_public())
                {
                    return WalkResult::Break(LinkerError::InvalidComponentStartFunction {
                        function: function.path().to_string(),
                        reason: alloc::format!(
                            "nested module '{}' is private, so component `init` cannot reach it",
                            inaccessible.borrow().path()
                        ),
                    });
                }

                let function_ref = function.as_function_ref();
                if let Some(previous) = self.component_start {
                    return WalkResult::Break(LinkerError::MultipleComponentStartFunctions {
                        first: previous.borrow().path().to_string(),
                        second: function.path().to_string(),
                    });
                }
                self.component_start = Some(function_ref);
                WalkResult::Continue(())
            })
            .into_result()
    }

    /// Discover the memory-owning items of `module` and of any module nested within it.
    ///
    /// Nesting is explicitly permitted by `builtin.Module`, and a nested module's items are as
    /// much a part of the component's memory as a top-level module's: a table the layout never
    /// visits has no address, and the dispatch reaching it would read a MAST root from nowhere.
    fn visit_module(
        &mut self,
        module: &builtin::Module,
        declared_reserved_memory: &mut u64,
        id: Option<&builtin::ComponentId>,
    ) -> Result<(), LinkerError> {
        if let Some(reserved) = module
            .as_operation()
            .get_typed_attribute::<U64Attr>(builtin::Module::RESERVED_MEMORY_ATTR)
        {
            *declared_reserved_memory = (*declared_reserved_memory).max(**reserved.borrow());
        }

        let module_body = module.body();
        if module_body.is_empty() {
            return Ok(());
        }

        for item in module_body.entry().body() {
            if let Some(nested) = item.downcast_ref::<builtin::Module>() {
                self.visit_module(nested, declared_reserved_memory, id)?;
                continue;
            }

            if let Some(segment) = item.downcast_ref::<builtin::Segment>() {
                log::debug!(target: "linker",
                    "inserting segment at offset {:#x}, size: {} bytes",
                    *segment.get_offset(),
                    segment.size_in_bytes()
                );
                self.segment_layout.insert(unsafe { SegmentRef::from_raw(segment) }).map_err(
                    |err| {
                        if let Some(id) = id {
                            LinkerError::InvalidComponentDataSegment {
                                id: id.clone(),
                                err,
                            }
                        } else {
                            LinkerError::InvalidDataSegment { err }
                        }
                    },
                )?;
                continue;
            }

            if let Some(global) = item.downcast_ref::<builtin::GlobalVariable>() {
                if global.is_declaration() {
                    continue;
                }
                self.globals_layout.insert(global)?;
                continue;
            }

            if let Some(table) = item.downcast_ref::<builtin::FunctionTable>() {
                log::debug!(target: "linker",
                    "discovered function table '{}' with {} slots",
                    table.get_name().as_str(),
                    *table.get_num_slots()
                );
                self.function_tables.push(table.as_function_table_ref());
            }
        }

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LinkerError {
    /// The provided component is undefined (i.e. we only know its interface, but have none of
    /// the actual definitions).
    #[error("invalid root component: expected definition, got declaration")]
    Undefined,
    /// Multiple segments were defined in the same component with the same offset
    #[error("invalid component: '{id}' has invalid data segment: {err}")]
    InvalidComponentDataSegment {
        id: builtin::ComponentId,
        #[source]
        err: DataSegmentError,
    },
    /// Multiple segments were defined in the same component with the same offset
    #[error("invalid data segment: {err}")]
    InvalidDataSegment {
        #[source]
        err: DataSegmentError,
    },
    /// A computed memory layout does not fit in the 32-bit linear address space
    #[error("invalid memory layout: {reason}")]
    LayoutOverflow { reason: alloc::string::String },
    /// The component-start marker was attached to an invalid operation or has the wrong type.
    #[error("invalid Wasm component start marker: {reason}")]
    InvalidComponentStartMarker { reason: alloc::string::String },
    /// The marked function does not satisfy the backend component-start contract.
    #[error("invalid Wasm component start function '{function}': {reason}")]
    InvalidComponentStartFunction {
        function: alloc::string::String,
        reason: alloc::string::String,
    },
    /// The deliberately scoped representation supports one start function per component.
    #[error(
        "unsupported Wasm component: multiple start functions are marked ('{first}' and \
         '{second}')"
    )]
    MultipleComponentStartFunctions {
        first: alloc::string::String,
        second: alloc::string::String,
    },
}

/// This struct contains data about the layout of global variables in linear memory
#[derive(Default, Clone)]
pub struct GlobalVariableLayout {
    global_table_offset: u32,
    stack_pointer: Option<u32>,
    next_offset: u32,
    page_size: u32,
    offsets: FxHashMap<builtin::GlobalVariableRef, u32>,
}
impl GlobalVariableLayout {
    fn new(global_table_offset: u32, page_size: u32) -> Self {
        Self {
            global_table_offset,
            stack_pointer: None,
            next_offset: global_table_offset,
            page_size,
            offsets: Default::default(),
        }
    }

    /// Get the address/offset at which global variables will start being allocated
    #[allow(unused)]
    pub fn global_table_offset(&self) -> u32 {
        self.global_table_offset
    }

    /// Get the address/offset at which the global stack pointer variable will be allocated
    pub fn stack_pointer_offset(&self) -> Option<u32> {
        self.stack_pointer
    }

    /// Get the address/offset of the next page boundary following the last inserted global
    /// variable, or `None` if that boundary does not fit in the 32-bit address space.
    pub fn next_page_boundary(&self) -> Option<u32> {
        self.next_offset.checked_next_multiple_of(self.page_size)
    }

    /// Get the statically-allocated address at which the global variable `gv` is to be placed.
    ///
    /// This function returns `None` if the given global variable is unresolvable.
    pub fn get_computed_addr(&self, gv: builtin::GlobalVariableRef) -> Option<u32> {
        self.offsets.get(&gv).copied()
    }

    /// Update the global table offset and adjust existing global variable offsets if necessary.
    ///
    /// This method should be used instead of directly modifying the `global_table_offset` field.
    /// If globals have already been inserted, their offsets will be adjusted to maintain
    /// their relative positions from the new base offset.
    ///
    /// Fails with [LinkerError::LayoutOverflow] when the move pushes a global variable outside
    /// the 32-bit address space, which a large module memory reservation can cause with valid
    /// input; the partially-rebased layout must then be discarded.
    pub fn update_global_table_offset(&mut self, new_offset: u32) -> Result<(), LinkerError> {
        let old_offset = self.global_table_offset;

        // Update the base offset
        self.global_table_offset = new_offset;

        // If there are existing globals, we need to adjust their offsets
        if !self.offsets.is_empty() {
            // Calculate the difference between old and new offset; the arithmetic is done in
            // 64 bits with checked conversions back to the 32-bit address space
            let offset_diff = new_offset as i64 - old_offset as i64;
            let rebase = |offset: u32| {
                u32::try_from(offset as i64 + offset_diff).map_err(|_| {
                    LinkerError::LayoutOverflow {
                        reason: alloc::format!(
                            "moving the global variables to base {new_offset:#x} pushes offset \
                             {offset:#x} outside the 32-bit address space"
                        ),
                    }
                })
            };

            // Update all existing global offsets
            for offset in self.offsets.values_mut() {
                *offset = rebase(*offset)?;
            }

            // Update the stack pointer offset if it exists
            if let Some(sp_offset) = self.stack_pointer.as_mut() {
                *sp_offset = rebase(*sp_offset)?;
            }

            // Update the next offset to maintain the same relative position
            self.next_offset = rebase(self.next_offset)?;
        } else {
            // If no globals have been inserted yet, just update next_offset to match
            self.next_offset = new_offset;
        }

        log::debug!(target: "linker",
            "GlobalVariableLayout: updated global_table_offset from {old_offset:#x} to {new_offset:#x}"
        );
        Ok(())
    }

    /// Allocate `gv` at the next suitably-aligned offset.
    ///
    /// Fails with [LinkerError::LayoutOverflow] when the placement does not fit in the 32-bit
    /// address space.
    pub fn insert(&mut self, gv: &builtin::GlobalVariable) -> Result<(), LinkerError> {
        let key = unsafe { builtin::GlobalVariableRef::from_raw(gv) };

        // Ensure the stack pointer is tracked and uses the same offset globally
        let is_stack_pointer = gv.get_name().as_symbol() == "__stack_pointer";
        if is_stack_pointer && let Some(offset) = self.stack_pointer {
            let _ = self.offsets.try_insert(key, offset);
            return Ok(());
        }

        let layout_overflow = || LinkerError::LayoutOverflow {
            reason: alloc::format!(
                "global variable '{}' does not fit in the 32-bit address space",
                gv.get_name().as_str()
            ),
        };
        let ty = gv.ty();
        let offset = self
            .next_offset
            .checked_next_multiple_of(ty.min_alignment() as u32)
            .ok_or_else(layout_overflow)?;
        if self.offsets.try_insert(key, offset).is_ok() {
            log::debug!(target: "linker",
                "GlobalVariableLayout: allocated global '{}' at offset {:#x} (size: {} bytes)",
                gv.get_name().as_str(),
                offset,
                ty.size_in_bytes()
            );
            if is_stack_pointer {
                self.stack_pointer = Some(offset);
            }
            self.next_offset =
                offset.checked_add(ty.size_in_bytes() as u32).ok_or_else(layout_overflow)?;
        }
        Ok(())
    }
}

/// This struct contains data about the layout of function tables in linear memory.
///
/// Each table occupies two words (32 bytes) of memory per slot: the first word holds the MAST
/// root of the referenced function, and the first element of the second word holds the callee's
/// signature tag (0 marks a null slot). The base address is word-aligned as required by
/// `dynexec`.
#[derive(Default, Clone)]
pub struct FunctionTableLayout {
    /// Tables and their base addresses (byte offsets), in discovery order
    tables: Vec<(builtin::FunctionTableRef, u32)>,
    /// The first byte offset past the end of the last table, or 0 if there are none
    end_offset: u32,
}

impl FunctionTableLayout {
    /// The size in bytes of one function table slot (two words: a MAST root digest, and a
    /// signature tag in the first element of the second word)
    pub const SLOT_SIZE_BYTES: u32 = 32;
    /// The size in field elements of one function table slot
    pub const SLOT_SIZE_ELEMENTS: u32 = 8;
    /// The element offset of the signature tag within a slot
    pub const TYPE_TAG_OFFSET_ELEMENTS: u32 = 4;

    /// Returns true if the layout has no function tables
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }

    /// Get the statically-allocated base of `table` as a word-aligned element address, the form
    /// expected by `dynexec` and the memory instructions that fill the table.
    pub fn element_addr_of(&self, table: builtin::FunctionTableRef) -> Option<u32> {
        let base = crate::lower::NativePtr::from_ptr(self.get_computed_addr(table)?);
        assert!(base.is_word_aligned(), "function tables must be word-aligned");
        Some(base.addr)
    }

    /// Traverse the function tables and their base addresses (byte offsets)
    pub fn iter(&self) -> impl Iterator<Item = (builtin::FunctionTableRef, u32)> + '_ {
        self.tables.iter().copied()
    }

    /// Get the statically-allocated base address (byte offset) of `table`.
    ///
    /// This function returns `None` if the given function table is unresolvable.
    pub fn get_computed_addr(&self, table: builtin::FunctionTableRef) -> Option<u32> {
        self.tables.iter().find_map(|(t, offset)| (*t == table).then_some(*offset))
    }
}

#[cfg(test)]
mod tests {
    use alloc::rc::Rc;

    /// The reserved band's upper bound mirrors the VM constant it cannot reference in constant
    /// expressions; a VM release moving the frame pointer must fail here, not corrupt memory.
    #[test]
    fn reserved_cells_stay_below_the_vm_locals_frame() {
        assert_eq!(
            miden_core::FMP_INIT_VALUE.as_canonical_u64(),
            super::ReservedCell::LOCALS_FRAME_START
        );
        for cell in super::ReservedCell::ALL {
            let addr = cell.element_addr() as u64;
            assert!(addr >= super::ReservedCell::GUEST_ADDRESS_LIMIT);
            assert!(
                addr + super::ReservedCell::ELEMENTS as u64
                    <= super::ReservedCell::LOCALS_FRAME_START
            );
        }
    }

    use midenc_hir::{
        BuilderExt, CallConv, Context, Ident, Op, SourceSpan, Type, Visibility,
        dialects::builtin::{
            self, BuiltinOpBuilder, ComponentBuilder, FunctionBuilder, FunctionRef, ModuleBuilder,
            World, WorldBuilder,
            attributes::{BoolAttr, Signature, U64Attr, UnitAttr},
        },
        version::Version,
    };

    use super::*;

    struct StartFixture {
        context: Rc<Context>,
        component: builtin::ComponentRef,
        module: builtin::ModuleRef,
        function: FunctionRef,
    }

    fn start_fixture(
        visibility: Visibility,
        call_conv: CallConv,
        params: impl IntoIterator<Item = Type>,
        results: impl IntoIterator<Item = Type>,
        define: bool,
    ) -> StartFixture {
        let context = Rc::new(Context::default());
        let world_ref =
            context.clone().builder().create::<World, ()>(Default::default())().unwrap();
        let mut world_builder = WorldBuilder::new(world_ref);
        let component = world_builder
            .define_component(
                Ident::from("test_ns"),
                Ident::from("test"),
                Version::parse("1.0.0").unwrap(),
            )
            .unwrap();
        let mut component_builder = ComponentBuilder::new(component);
        let module = component_builder.define_module(Ident::from("core")).unwrap();
        let signature = Signature::with_convention(&context, call_conv, params, results);
        let mut module_builder = ModuleBuilder::new(module);
        let function = module_builder
            .define_function(Ident::from("initialize"), visibility, signature)
            .unwrap();
        if define {
            FunctionBuilder::new(function, module_builder.builder())
                .ret(None, SourceSpan::default())
                .unwrap();
        }

        StartFixture {
            context,
            component,
            module,
            function,
        }
    }

    fn mark_start(context: &Rc<Context>, mut function: FunctionRef) {
        let marker = context.create_attribute::<UnitAttr, _>(());
        function
            .borrow_mut()
            .as_operation_mut()
            .set_attribute(midenc_dialect_hir::WASM_COMPONENT_START_ATTR, marker);
    }

    fn link_start_fixture(fixture: &StartFixture) -> Result<LinkInfo, LinkerError> {
        let component = fixture.component.borrow();
        Linker::default().link(None, component.as_operation())
    }

    fn expect_link_error(result: Result<LinkInfo, LinkerError>, message: &str) -> LinkerError {
        match result {
            Ok(_) => panic!("{message}"),
            Err(err) => err,
        }
    }

    /// Build a component holding one module that reserves `reserved_bytes` of memory and
    /// declares one single-slot function table, then link it.
    fn link_reserved_module_with_table(reserved_bytes: u64) -> Result<LinkInfo, LinkerError> {
        let context = Rc::new(Context::default());
        let world_ref =
            context.clone().builder().create::<World, ()>(Default::default())().unwrap();
        let mut world_builder = WorldBuilder::new(world_ref);
        let component_ref = world_builder
            .define_component(
                Ident::from("test_ns"),
                Ident::from("test"),
                Version::parse("1.0.0").unwrap(),
            )
            .unwrap();
        let mut component_builder = ComponentBuilder::new(component_ref);
        let mut module_ref = component_builder.define_module(Ident::from("m")).unwrap();
        if reserved_bytes > 0 {
            let attr = context.create_attribute::<U64Attr, _>(reserved_bytes);
            module_ref
                .borrow_mut()
                .as_operation_mut()
                .set_attribute(builtin::Module::RESERVED_MEMORY_ATTR, attr);
        }
        let mut module_builder = ModuleBuilder::new(module_ref);
        module_builder
            .define_function_table(Ident::from("tbl"), Visibility::Private, 1)
            .unwrap();

        let component = component_ref.borrow();
        Linker::default().link(None, component.as_operation())
    }

    /// A valid 65,535-page module memory plus one table must link without panicking, and report
    /// that the rounded heap base does not fit the 32-bit address space.
    #[test]
    fn link_fails_when_static_memory_leaves_no_heap_base() {
        let err = match link_reserved_module_with_table(0xffff_0000) {
            Ok(_) => panic!("a heap base past the last page must be a link error"),
            Err(err) => err,
        };
        assert!(
            matches!(&err, LinkerError::LayoutOverflow { reason } if reason.contains("dynamic heap")),
            "unexpected link failure: {err}"
        );
    }

    /// A data segment may occupy the last bytes of the address space — `(memory 65536)` with a
    /// segment at `-16` is valid Wasm, and `wasm-tools validate` accepts it. Nothing can be laid
    /// out after it, so linking must report `LayoutOverflow` rather than overflowing a `u32`
    /// (a debug panic, and a wrapped address in release).
    #[test]
    fn link_fails_when_data_segments_fill_the_address_space() {
        let context = Rc::new(Context::default());
        let world_ref =
            context.clone().builder().create::<World, ()>(Default::default())().unwrap();
        let mut world_builder = WorldBuilder::new(world_ref);
        let component_ref = world_builder
            .define_component(
                Ident::from("test_ns"),
                Ident::from("test"),
                Version::parse("1.0.0").unwrap(),
            )
            .unwrap();
        let mut component_builder = ComponentBuilder::new(component_ref);
        let module_ref = component_builder.define_module(Ident::from("m")).unwrap();
        let mut module_builder = ModuleBuilder::new(module_ref);
        module_builder
            .define_data_segment(
                0xffff_fff0,
                [0u8; 16],
                /*readonly=*/ true,
                SourceSpan::default(),
            )
            .expect("a segment ending at the last byte of memory is valid");

        let component = component_ref.borrow();
        let err = match Linker::default().link(None, component.as_operation()) {
            Ok(_) => panic!("a full address space must be a link error"),
            Err(err) => err,
        };
        assert!(
            matches!(&err, LinkerError::LayoutOverflow { .. }),
            "unexpected link failure: {err}"
        );
    }

    /// `builtin.Module` permits nesting, so a table in a nested module is legal IR. The layout
    /// must find it: an undiscovered table has no address, and the dispatch that reaches it
    /// would have nowhere to read a MAST root from.
    #[test]
    fn link_discovers_tables_in_nested_modules() {
        let context = Rc::new(Context::default());
        let world_ref =
            context.clone().builder().create::<World, ()>(Default::default())().unwrap();
        let mut world_builder = WorldBuilder::new(world_ref);
        let component_ref = world_builder
            .define_component(
                Ident::from("test_ns"),
                Ident::from("test"),
                Version::parse("1.0.0").unwrap(),
            )
            .unwrap();
        let mut component_builder = ComponentBuilder::new(component_ref);
        let outer = component_builder.define_module(Ident::from("outer")).unwrap();
        let inner = ModuleBuilder::new(outer).declare_module(Ident::from("inner")).unwrap();
        let table = ModuleBuilder::new(inner)
            .define_function_table(Ident::from("tbl"), Visibility::Private, 1)
            .unwrap();

        let component = component_ref.borrow();
        let link_info = Linker::default()
            .link(None, component.as_operation())
            .expect("a nested table must lay out");
        assert!(
            link_info.function_tables().get_computed_addr(table).is_some(),
            "the nested table must have an address in the computed layout"
        );
    }

    /// The heap base is the first page boundary past the linked function tables.
    #[test]
    fn link_computes_heap_base_past_static_memory() {
        let link_info = link_reserved_module_with_table(0).expect("layout should fit");
        // The default reservation floor (17 pages) puts the table at 0x110000; its one 32-byte
        // slot rounds up to the next page boundary
        assert_eq!(link_info.heap_base(), 0x120000);
    }

    #[test]
    fn marked_start_alone_requires_component_init() {
        let fixture = start_fixture(Visibility::Public, CallConv::C, [], [], true);
        mark_start(&fixture.context, fixture.function);

        let link_info = link_start_fixture(&fixture).expect("a valid component start must link");
        assert!(link_info.requires_init(), "the start marker alone must require `init`");
        assert!(link_info.component_start() == Some(fixture.function));
    }

    #[test]
    fn component_start_marker_must_be_unit_typed() {
        let mut fixture = start_fixture(Visibility::Public, CallConv::C, [], [], true);
        let marker = fixture.context.create_attribute::<BoolAttr, _>(true);
        fixture
            .function
            .borrow_mut()
            .as_operation_mut()
            .set_attribute(midenc_dialect_hir::WASM_COMPONENT_START_ATTR, marker);

        let err =
            expect_link_error(link_start_fixture(&fixture), "a non-unit marker must not link");
        assert!(err.to_string().contains("must have value `unit`"), "{err}");
    }

    #[test]
    fn component_start_marker_is_only_valid_on_core_module_functions() {
        let mut fixture = start_fixture(Visibility::Public, CallConv::C, [], [], true);
        let marker = fixture.context.create_attribute::<UnitAttr, _>(());
        fixture
            .module
            .borrow_mut()
            .as_operation_mut()
            .set_attribute(midenc_dialect_hir::WASM_COMPONENT_START_ATTR, marker);

        let err =
            expect_link_error(link_start_fixture(&fixture), "a marker on a module must not link");
        let message = err.to_string();
        assert!(message.contains("only valid on a core-module function"), "{message}");
        assert!(message.contains("builtin.module"), "{message}");
    }

    #[test]
    fn component_level_function_cannot_be_a_core_module_start() {
        let fixture = start_fixture(Visibility::Public, CallConv::C, [], [], true);
        let signature = Signature::with_convention(&fixture.context, CallConv::C, [], []);
        let mut component_builder = ComponentBuilder::new(fixture.component);
        let component_function = component_builder
            .define_function(Ident::from("component_entry"), Visibility::Public, signature)
            .unwrap();
        mark_start(&fixture.context, component_function);

        let err = expect_link_error(
            link_start_fixture(&fixture),
            "a component-level function cannot be a core-module start",
        );
        assert!(err.to_string().contains("core-module tree"), "{err}");
    }

    #[test]
    fn component_start_must_be_a_public_definition() {
        for (visibility, define, expected) in [
            (Visibility::Private, true, "not public"),
            (Visibility::Public, false, "declaration"),
        ] {
            let fixture = start_fixture(visibility, CallConv::C, [], [], define);
            mark_start(&fixture.context, fixture.function);
            let err =
                expect_link_error(link_start_fixture(&fixture), "an invalid start must not link");
            assert!(err.to_string().contains(expected), "{err}");
        }
    }

    #[test]
    fn component_start_must_have_the_core_c_void_signature() {
        for (call_conv, params, results) in [
            (CallConv::Fast, alloc::vec::Vec::new(), alloc::vec::Vec::new()),
            (CallConv::C, alloc::vec![Type::I32], alloc::vec::Vec::new()),
            (CallConv::C, alloc::vec::Vec::new(), alloc::vec![Type::I32]),
        ] {
            let fixture = start_fixture(Visibility::Public, call_conv, params, results, true);
            mark_start(&fixture.context, fixture.function);
            let err =
                expect_link_error(link_start_fixture(&fixture), "an invalid start must not link");
            assert!(err.to_string().contains("extern(\"C\") () -> ()"), "{err}");
        }
    }

    #[test]
    fn component_supports_only_one_marked_start() {
        let fixture = start_fixture(Visibility::Public, CallConv::C, [], [], true);
        mark_start(&fixture.context, fixture.function);
        let signature = Signature::with_convention(&fixture.context, CallConv::C, [], []);
        let mut module_builder = ModuleBuilder::new(fixture.module);
        let second = module_builder
            .define_function(Ident::from("second_initialize"), Visibility::Public, signature)
            .unwrap();
        FunctionBuilder::new(second, module_builder.builder())
            .ret(None, SourceSpan::default())
            .unwrap();
        mark_start(&fixture.context, second);

        let err = expect_link_error(link_start_fixture(&fixture), "two starts must not link");
        let message = err.to_string();
        assert!(message.contains("multiple start functions"), "{message}");
        assert!(message.contains("initialize") && message.contains("second_initialize"));
    }

    #[test]
    fn start_in_a_private_nested_module_is_not_reachable_from_init() {
        let fixture = start_fixture(Visibility::Public, CallConv::C, [], [], true);
        // Move the marker to a valid function in a private nested module; the original function
        // remains unmarked and merely keeps the top-level module non-empty.
        let nested = ModuleBuilder::new(fixture.module)
            .declare_module(Ident::from("nested"))
            .unwrap();
        let mut nested_builder = ModuleBuilder::new(nested);
        let signature = Signature::with_convention(&fixture.context, CallConv::C, [], []);
        let nested_start = nested_builder
            .define_function(Ident::from("nested_start"), Visibility::Public, signature)
            .unwrap();
        FunctionBuilder::new(nested_start, nested_builder.builder())
            .ret(None, SourceSpan::default())
            .unwrap();
        mark_start(&fixture.context, nested_start);

        let err = expect_link_error(
            link_start_fixture(&fixture),
            "a start behind a private nested module must not link",
        );
        assert!(err.to_string().contains("nested module"), "{err}");
        assert!(err.to_string().contains("private"), "{err}");
    }
}
