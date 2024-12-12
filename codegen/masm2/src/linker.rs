use midenc_hir2::{dialects::builtin, Alignable, FxHashMap, SymbolTable};

pub struct LinkInfo {
    component: builtin::ComponentInterface,
    globals_layout: FxHashMap<builtin::ComponentId, GlobalVariableLayout>,
    segment_layout: FxHashMap<builtin::ComponentId, Vec<DataSegment>>,
    reserved_memory_pages: u32,
    page_size: u32,
}

impl LinkInfo {
    #[inline]
    pub fn component(&self) -> &builtin::ComponentInterface {
        &self.component
    }

    pub fn globals_layout_for_component(
        &self,
        component: &builtin::ComponentId,
    ) -> &GlobalVariableLayout {
        &self.globals_layout[component]
    }

    pub fn segment_layout_for_component(&self, component: &builtin::ComponentId) -> &[DataSegment] {
        &self.segment_layout[component]
    }

    #[inline(always)]
    pub fn reserved_memory_pages(&self) -> u32 {
        self.reserved_memory_pages
    }

    #[inline(always)]
    pub fn page_size(&self) -> u32 {
        self.page_size
    }
}

pub struct Linker {
    globals_layout: FxHashMap<builtin::ComponentId, GlobalVariableLayout>,
    segment_layout: FxHashMap<builtin::ComponentId, Vec<DataSegment>>,
    reserved_memory_pages: u32,
    page_size: u32,
}

impl Default for Linker {
    fn default() -> Self {
        Self::new(0, 2u32.pow(16))
    }
}

impl Linker {
    pub fn new(reserved_memory_pages: u32, page_size: u32) -> Self {
        let page_size = if page_size > 0 {
            assert!(page_size.is_power_of_two());
            page_size
        } else {
            2u32.pow(16)
        };
        Self {
            globals_layout: Default::default(),
            segment_layout: Default::default(),
            reserved_memory_pages,
            page_size,
        }
    }

    pub fn link(mut self, component: &builtin::Component) -> Result<LinkInfo, LinkerError> {
        let interface = builtin::ComponentInterface::new(component);

        if interface.is_externally_defined() {
            return Err(LinkerError::Undefined);
        }

        if !interface.visibility().is_public() {
            return Err(LinkerError::Visibility);
        }

        self.compute_layout_for_component(interface.id(), component)?;

        Ok(LinkInfo {
            component: interface,
            globals_layout: core::mem::take(&mut self.globals_layout),
            segment_layout: core::mem::take(&mut self.segment_layout),
            reserved_memory_pages: self.reserved_memory_pages,
            page_size: self.page_size,
        })
    }

    fn compute_layout_for_component(
        &mut self,
        id: &builtin::ComponentId,
        component: &builtin::Component,
    ) -> Result<(), LinkerError> {
        // Locate all data segments and global variables within this component (not including
        // nested components, as those have their own layouts).

        // Data segments must be declared at the component level, so if there are any segments, we
        // will find them by walking the operations of the component body.
        self.compute_segment_layout_for_component(id, component)?;

        // Global variables
        let symbol_manager = component.symbol_manager();
        for symbol_ref in symbol_manager.symbols().symbols() {
            let symbol = symbol_ref.borrow();
            let symbol_op = symbol.as_symbol_operation();
            if let Some(module) = symbol_op.downcast_ref::<builtin::Module>() {
                // Place all global variables of this module
                self.extend_layout_from_module(id, module)?;
            } else if let Some(nested) = symbol_op.downcast_ref::<builtin::Component>() {
                // Compute the layout for nested components
                let interface = builtin::ComponentInterface::new(nested);
                if interface.is_externally_defined() {
                    continue;
                }
                self.compute_layout_for_component(interface.id(), nested)?;
            }
        }

        Ok(())
    }

    fn compute_segment_layout_for_component(
        &mut self,
        id: &builtin::ComponentId,
        component: &builtin::Component,
    ) -> Result<(), LinkerError> {
        let segments = self.segment_layout.entry(id.clone()).or_default();
        let body = component.body();
        let body = body.entry();
        for op in body.body() {
            if let Some(segment) = op.downcast_ref::<builtin::Segment>() {
                let offset = *segment.offset();
                match segments.binary_search_by_key(&offset, |s| s.offset) {
                    Ok(_index) => {
                        return Err(LinkerError::OverlappingSegments {
                            id: id.clone(),
                            offset,
                        });
                    }
                    Err(index) => {
                        // TODO: Need to compute the segment size here
                        segments.insert(index, DataSegment { offset, size: 0 });
                    }
                }
            }
        }

        Ok(())
    }

    fn extend_layout_from_module(
        &mut self,
        id: &builtin::ComponentId,
        module: &builtin::Module,
    ) -> Result<(), LinkerError> {
        let segment_layout = &self.segment_layout[id];
        let next_available_offset =
            segment_layout.last().map(|segment| segment.offset + segment.size).unwrap_or(0);
        let global_table_offset = core::cmp::max(
            (self.reserved_memory_pages * self.page_size).next_multiple_of(32),
            next_available_offset,
        );
        let globals_layout = self
            .globals_layout
            .entry(id.clone())
            .or_insert_with(|| GlobalVariableLayout::new(global_table_offset));

        let symbol_manager = module.symbol_manager();
        for symbol_ref in symbol_manager.symbols().symbols() {
            let symbol = symbol_ref.borrow();
            let symbol_op = symbol.as_symbol_operation();
            if let Some(global) = symbol_op.downcast_ref::<builtin::GlobalVariable>() {
                globals_layout.insert(global);
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
    /// The provided component must have public visibility, but has private or internal visibility
    #[error("invalid root component: must have public visibility")]
    Visibility,
    /// Multiple segments were defined in the same component with the same offset
    #[error("invalid component: '{id}' has overlapping data segments at offset {offset}")]
    OverlappingSegments {
        id: builtin::ComponentId,
        offset: u32,
    },
}

pub struct DataSegment {
    pub offset: u32,
    pub size: u32,
}

/// This struct contains data about the layout of global variables in linear memory
#[derive(Default, Clone)]
pub struct GlobalVariableLayout {
    global_table_offset: u32,
    next_offset: u32,
    offsets: FxHashMap<builtin::GlobalVariableRef, u32>,
}
impl GlobalVariableLayout {
    fn new(global_table_offset: u32) -> Self {
        Self {
            global_table_offset,
            next_offset: global_table_offset,
            offsets: Default::default(),
        }
    }

    /// Get the address/offset at which global variables will start being allocated
    pub fn global_table_offset(&self) -> u32 {
        self.global_table_offset
    }

    /// Get the statically-allocated address at which the global variable `gv` is to be placed.
    ///
    /// This function returns `None` if the given global variable is unresolvable.
    pub fn get_computed_addr(&self, gv: builtin::GlobalVariableRef) -> Option<u32> {
        self.offsets.get(&gv).copied()
    }

    pub fn insert(&mut self, gv: &builtin::GlobalVariable) {
        let ty = gv.ty();
        let offset = self.next_offset.align_up(ty.min_alignment() as u32);
        let key = unsafe { builtin::GlobalVariableRef::from_raw(gv) };
        if self.offsets.try_insert(key, offset).is_ok() {
            self.next_offset = offset + ty.size_in_bytes() as u32;
        }
    }
}
