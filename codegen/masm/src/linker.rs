use midenc_hir::{
    Alignable, FxHashMap, Op, Symbol,
    dialects::builtin::{self, DataSegmentError, SegmentRef, attributes::U64Attr},
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

pub struct LinkInfo {
    component: Option<builtin::ComponentId>,
    globals_layout: GlobalVariableLayout,
    segment_layout: builtin::DataSegmentLayout,
    function_tables: FunctionTableLayout,
    reserved_memory_pages: u32,
    page_size: u32,
}

impl LinkInfo {
    #[cfg(test)]
    pub fn new(id: Option<builtin::ComponentId>) -> Self {
        Self {
            component: id,
            globals_layout: Default::default(),
            segment_layout: Default::default(),
            function_tables: Default::default(),
            reserved_memory_pages: 0,
            page_size: DEFAULT_PAGE_SIZE,
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

    /// Returns true if the component requires an `init` procedure to set up linear memory
    /// (data segments, global variables, or function tables) before execution.
    pub fn requires_init(&self) -> bool {
        self.has_globals() || self.has_data_segments() || self.has_function_tables()
    }

    /// Get the address of the first page boundary past all statically-allocated memory (global
    /// variables and function tables), or the end of reserved memory if larger; this is where
    /// the dynamic heap starts when the program is executed.
    pub fn heap_base(&self) -> u32 {
        let after_static = core::cmp::max(
            self.globals_layout.next_page_boundary(),
            self.function_tables.end_offset().next_multiple_of(self.page_size),
        );
        let heap_base = core::cmp::max(self.reserved_memory_bytes(), after_static as usize);
        u32::try_from(heap_base)
            .expect("unable to allocate dynamic heap: static memory layout too large")
    }

    #[inline(always)]
    pub fn reserved_memory_pages(&self) -> u32 {
        self.reserved_memory_pages
    }

    #[inline]
    pub fn reserved_memory_bytes(&self) -> usize {
        self.reserved_memory_pages() as usize * self.page_size() as usize
    }

    #[inline(always)]
    pub fn page_size(&self) -> u32 {
        self.page_size
    }
}

pub struct Linker {
    globals_layout: GlobalVariableLayout,
    segment_layout: builtin::DataSegmentLayout,
    function_tables: Vec<builtin::FunctionTableRef>,
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

        // 2. Visit each Module in the component and discover Segment, GlobalVariable, and
        // FunctionTable items, along with the memory claimed by the modules themselves
        let mut declared_reserved_memory = 0u64;
        let body = body.entry();
        for item in body.body() {
            if let Some(module) = item.downcast_ref::<builtin::Module>() {
                if let Some(reserved) = module
                    .as_operation()
                    .get_typed_attribute::<U64Attr>(builtin::Module::RESERVED_MEMORY_ATTR)
                {
                    declared_reserved_memory = declared_reserved_memory.max(**reserved.borrow());
                }

                let module_body = module.body();
                if module_body.is_empty() {
                    continue;
                }

                let module_body = module_body.entry();
                for item in module_body.body() {
                    if let Some(segment) = item.downcast_ref::<builtin::Segment>() {
                        log::debug!(target: "linker",
                            "inserting segment at offset {:#x}, size: {} bytes",
                            *segment.get_offset(),
                            segment.size_in_bytes()
                        );
                        self.segment_layout
                            .insert(unsafe { SegmentRef::from_raw(segment) })
                            .map_err(|err| {
                                if let Some(id) = id.as_ref() {
                                    LinkerError::InvalidComponentDataSegment {
                                        id: id.clone(),
                                        err,
                                    }
                                } else {
                                    LinkerError::InvalidDataSegment { err }
                                }
                            })?;
                        continue;
                    }

                    if let Some(global) = item.downcast_ref::<builtin::GlobalVariable>() {
                        if global.is_declaration() {
                            continue;
                        }
                        self.globals_layout.insert(global);
                        continue;
                    }

                    if let Some(table) = item.downcast_ref::<builtin::FunctionTable>() {
                        log::debug!(target: "linker",
                            "discovered function table '{}' with {} slots",
                            table.get_name().as_str(),
                            *table.get_num_slots()
                        );
                        self.function_tables
                            .push(unsafe { builtin::FunctionTableRef::from_raw(table) });
                    }
                }
            }
        }

        // 3. Layout global variables past all memory claimed by the modules themselves
        let next_available_offset = self.segment_layout.next_available_offset();
        let reserved_offset = (self.reserved_memory_pages * self.page_size).next_multiple_of(4);
        // We add a page after the data segments as headroom for producer-placed data that
        // occupies address space without being visible as data segments (e.g. zero-initialized
        // statics).
        let next_available_offset_with_headroom = next_available_offset + DEFAULT_PAGE_SIZE;
        // A module's declared memory reservation is a sound upper bound on everything its
        // producer placed in linear memory, whereas the one-page allowance above the data
        // segments is only a heuristic.
        let declared_reserved_offset = u32::try_from(declared_reserved_memory).expect(
            "invalid module memory reservation: it leaves no room for compiler-managed memory",
        );
        log::debug!(target: "linker",
            "next_available_offset (with headroom) from segments: {:#x}, reserved_offset: {:#x}, \
             declared_reserved_offset: {:#x}, segment_count: {}",
            next_available_offset_with_headroom,
            reserved_offset,
            declared_reserved_offset,
            self.segment_layout.len()
        );
        self.globals_layout.update_global_table_offset(
            core::cmp::max(reserved_offset, next_available_offset_with_headroom)
                .max(declared_reserved_offset),
        );
        log::debug!(target: "linker",
            "global_table_offset set to: {:#x}",
            self.globals_layout.global_table_offset()
        );

        // 4. Lay out function tables in the page following the global table, one word (16 bytes)
        // per slot; page alignment implies the word alignment required by `dynexec`.
        let mut function_tables = FunctionTableLayout::default();
        let mut next_table_offset = self.globals_layout.next_page_boundary();
        for table_ref in self.function_tables.drain(..) {
            let slots = *table_ref.borrow().get_num_slots();
            let size_in_bytes =
                slots.checked_mul(16).expect("invalid function table: too many slots");
            log::debug!(target: "linker",
                "function table with {slots} slots allocated at offset {next_table_offset:#x}"
            );
            function_tables.tables.push((table_ref, next_table_offset));
            next_table_offset = next_table_offset
                .checked_add(size_in_bytes)
                .expect("invalid function table: table does not fit in linear memory");
            function_tables.end_offset = next_table_offset;
        }

        Ok(LinkInfo {
            component: id,
            globals_layout: core::mem::take(&mut self.globals_layout),
            segment_layout: core::mem::take(&mut self.segment_layout),
            function_tables,
            reserved_memory_pages: self.reserved_memory_pages,
            page_size: self.page_size,
        })
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

    /// Get the address/offset of the next page boundary following the last inserted global variable
    pub fn next_page_boundary(&self) -> u32 {
        self.next_offset.next_multiple_of(self.page_size)
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
    pub fn update_global_table_offset(&mut self, new_offset: u32) {
        let old_offset = self.global_table_offset;

        // Update the base offset
        self.global_table_offset = new_offset;

        // If there are existing globals, we need to adjust their offsets
        if !self.offsets.is_empty() {
            // Calculate the difference between old and new offset
            let offset_diff = new_offset as i32 - old_offset as i32;

            // Update all existing global offsets
            for offset in self.offsets.values_mut() {
                *offset = (*offset as i32 + offset_diff) as u32;
            }

            // Update the stack pointer offset if it exists
            if let Some(sp_offset) = self.stack_pointer.as_mut() {
                *sp_offset = (*sp_offset as i32 + offset_diff) as u32;
            }

            // Update the next offset to maintain the same relative position
            self.next_offset = (self.next_offset as i32 + offset_diff) as u32;
        } else {
            // If no globals have been inserted yet, just update next_offset to match
            self.next_offset = new_offset;
        }

        log::debug!(target: "linker",
            "GlobalVariableLayout: updated global_table_offset from {old_offset:#x} to {new_offset:#x}"
        );
    }

    pub fn insert(&mut self, gv: &builtin::GlobalVariable) {
        let key = unsafe { builtin::GlobalVariableRef::from_raw(gv) };

        // Ensure the stack pointer is tracked and uses the same offset globally
        let is_stack_pointer = gv.get_name().as_symbol() == "__stack_pointer";
        if is_stack_pointer && let Some(offset) = self.stack_pointer {
            let _ = self.offsets.try_insert(key, offset);
            return;
        }

        let ty = gv.ty();
        let offset = self.next_offset.align_up(ty.min_alignment() as u32);
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
            self.next_offset = offset + ty.size_in_bytes() as u32;
        }
    }
}

/// This struct contains data about the layout of function tables in linear memory.
///
/// Each table occupies one word (16 bytes) of memory per slot, holding the MAST root of the
/// referenced function, and its base address is word-aligned as required by `dynexec`.
#[derive(Default, Clone)]
pub struct FunctionTableLayout {
    /// Tables and their base addresses (byte offsets), in discovery order
    tables: Vec<(builtin::FunctionTableRef, u32)>,
    /// The first byte offset past the end of the last table, or 0 if there are none
    end_offset: u32,
}

impl FunctionTableLayout {
    /// Returns true if the layout has no function tables
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
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

    /// The first byte offset past the end of the last table, or 0 if there are none
    pub fn end_offset(&self) -> u32 {
        self.end_offset
    }
}
