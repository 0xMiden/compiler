use midenc_hir::{
    Alignable, FxHashMap, Symbol,
    dialects::builtin::{self, DataSegmentError, SegmentRef},
};

const DEFAULT_PAGE_SIZE: u32 = 2u32.pow(16);
/// Currently, Wasm modules produced by rustc reserve 16 pages for the Rust stack
/// (see __stack_pointer global variable value in Wasm).
// We start our reserved memory from the next page after the rustc reserved for the
// Rust stack to avoid overlapping with Rust `static` vars.
const DEFAULT_RESERVATION: u32 = 17;

pub struct LinkInfo {
    component: builtin::ComponentId,
    globals_layout: GlobalVariableLayout,
    segment_layout: builtin::DataSegmentLayout,
    reserved_memory_pages: u32,
    page_size: u32,
}

impl LinkInfo {
    #[cfg(test)]
    pub fn new(id: builtin::ComponentId) -> Self {
        Self {
            component: id,
            globals_layout: Default::default(),
            segment_layout: Default::default(),
            reserved_memory_pages: 0,
            page_size: DEFAULT_PAGE_SIZE,
        }
    }

    #[inline]
    pub fn component(&self) -> &builtin::ComponentId {
        &self.component
    }

    pub fn has_globals(&self) -> bool {
        !self.globals_layout.offsets.is_empty()
    }

    pub fn has_data_segments(&self) -> bool {
        !self.segment_layout.is_empty()
    }

    pub fn globals_layout(&self) -> &GlobalVariableLayout {
        &self.globals_layout
    }

    #[allow(unused)]
    pub fn segment_layout(&self) -> &builtin::DataSegmentLayout {
        &self.segment_layout
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
            reserved_memory_pages,
            page_size,
        }
    }

    pub fn link(mut self, component: &builtin::Component) -> Result<LinkInfo, LinkerError> {
        // Gather information needed to compute component data layout

        // 1. Verify that the component is non-empty
        let body = component.body();
        if body.is_empty() {
            // This component has no definition
            return Err(LinkerError::Undefined);
        }

        // 2. Visit each Module in the component and discover Segment and GlobalVariable items
        let body = body.entry();
        for item in body.body() {
            if let Some(module) = item.downcast_ref::<builtin::Module>() {
                let module_body = module.body();
                if module_body.is_empty() {
                    continue;
                }

                let module_body = module_body.entry();
                for item in module_body.body() {
                    if let Some(segment) = item.downcast_ref::<builtin::Segment>() {
                        log::debug!(target: "linker",
                            "inserting segment at offset {:#x}, size: {} bytes",
                            segment.offset(),
                            segment.size_in_bytes()
                        );
                        self.segment_layout
                            .insert(unsafe { SegmentRef::from_raw(segment) })
                            .map_err(|err| LinkerError::InvalidSegment {
                                id: component.id(),
                                err,
                            })?;
                        continue;
                    }

                    if let Some(global) = item.downcast_ref::<builtin::GlobalVariable>() {
                        if global.is_declaration() {
                            continue;
                        }
                        self.globals_layout.insert(global);
                    }
                }
            }
        }

        // 3. Layout global variables in the next page following the last data segment
        let next_available_offset = self.segment_layout.next_available_offset();
        let reserved_offset = (self.reserved_memory_pages * self.page_size).next_multiple_of(4);
        // We add a page after the data segments to avoid overlapping with Rust `static` vars which
        // are placed after data segments (if present).
        let next_available_offset_after_rust_statics = next_available_offset + DEFAULT_PAGE_SIZE;
        log::debug!(target: "linker",
            "next_available_offset (after Rust statics) from segments: {:#x}, reserved_offset: {:#x}, \
             segment_count: {}",
            next_available_offset_after_rust_statics,
            reserved_offset,
            self.segment_layout.len()
        );
        self.globals_layout.update_global_table_offset(core::cmp::max(
            reserved_offset,
            next_available_offset_after_rust_statics,
        ));
        log::debug!(target: "linker",
            "global_table_offset set to: {:#x}",
            self.globals_layout.global_table_offset()
        );

        Ok(LinkInfo {
            component: component.id(),
            globals_layout: core::mem::take(&mut self.globals_layout),
            segment_layout: core::mem::take(&mut self.segment_layout),
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
    InvalidSegment {
        id: builtin::ComponentId,
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
        let is_stack_pointer = gv.name() == "__stack_pointer";
        if is_stack_pointer && let Some(offset) = self.stack_pointer {
            let _ = self.offsets.try_insert(key, offset);
            return;
        }

        let ty = gv.ty();
        let offset = self.next_offset.align_up(ty.min_alignment() as u32);
        if self.offsets.try_insert(key, offset).is_ok() {
            log::debug!(target: "linker",
                "GlobalVariableLayout: allocated global '{}' at offset {:#x} (size: {} bytes)",
                gv.name(),
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
