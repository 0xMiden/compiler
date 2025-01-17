use midenc_hir::{
    pass::{Analysis, AnalysisManager, AnalysisResult},
    Function, FunctionIdent, FxHashMap, GlobalValue, GlobalValueData, GlobalVariableTable, Module,
    Program,
};
use midenc_session::Session;

/// This analysis calculates the addresses/offsets of all global variables in a [Program] or
/// [Module]
pub struct GlobalVariableAnalysis<T> {
    layout: GlobalVariableLayout,
    _marker: core::marker::PhantomData<T>,
}
impl<T> Default for GlobalVariableAnalysis<T> {
    fn default() -> Self {
        Self {
            layout: Default::default(),
            _marker: core::marker::PhantomData,
        }
    }
}
impl<T> GlobalVariableAnalysis<T> {
    pub fn layout(&self) -> &GlobalVariableLayout {
        &self.layout
    }
}

impl Analysis for GlobalVariableAnalysis<Program> {
    type Entity = Program;

    fn analyze(
        program: &Self::Entity,
        _analyses: &mut AnalysisManager,
        _session: &Session,
    ) -> AnalysisResult<Self> {
        let mut layout = GlobalVariableLayout {
            global_table_offset: core::cmp::max(
                program.reserved_memory_bytes().next_multiple_of(32),
                program.segments().next_available_offset(),
            ),
            ..GlobalVariableLayout::default()
        };

        let globals = program.globals();
        for module in program.modules().iter() {
            for function in module.functions() {
                let mut function_offsets = FxHashMap::default();
                for gv in function.dfg.globals.keys() {
                    if let Some(addr) =
                        compute_global_value_addr(gv, layout.global_table_offset, function, globals)
                    {
                        function_offsets.insert(gv, addr);
                    }
                }
                layout.offsets.insert(function.id, function_offsets);
            }
        }

        Ok(Self {
            layout,
            _marker: core::marker::PhantomData,
        })
    }
}

impl Analysis for GlobalVariableAnalysis<Module> {
    type Entity = Module;

    fn analyze(
        module: &Self::Entity,
        _analyses: &mut AnalysisManager,
        _session: &Session,
    ) -> AnalysisResult<Self> {
        let mut layout = GlobalVariableLayout {
            global_table_offset: core::cmp::max(
                module.reserved_memory_bytes().next_multiple_of(32),
                module.segments().next_available_offset(),
            ),
            ..GlobalVariableLayout::default()
        };

        let globals = module.globals();
        for function in module.functions() {
            let mut function_offsets = FxHashMap::default();
            for gv in function.dfg.globals.keys() {
                if let Some(addr) =
                    compute_global_value_addr(gv, layout.global_table_offset, function, globals)
                {
                    function_offsets.insert(gv, addr);
                }
            }
            layout.offsets.insert(function.id, function_offsets);
        }

        Ok(Self {
            layout,
            _marker: core::marker::PhantomData,
        })
    }
}

/// This struct contains data about the layout of global variables in linear memory
#[derive(Default, Clone)]
pub struct GlobalVariableLayout {
    global_table_offset: u32,
    offsets: FxHashMap<FunctionIdent, FxHashMap<GlobalValue, u32>>,
}
impl GlobalVariableLayout {
    /// Get the address/offset at which global variables will start being allocated
    pub fn global_table_offset(&self) -> u32 {
        self.global_table_offset
    }

    /// Get the statically-allocated address at which the global value `gv` for `function` is
    /// stored.
    ///
    /// This function returns `None` if the analysis does not know about `function`, `gv`, or if
    /// the symbol which `gv` resolves to was undefined.
    pub fn get_computed_addr(&self, function: &FunctionIdent, gv: GlobalValue) -> Option<u32> {
        self.offsets.get(function).and_then(|offsets| offsets.get(&gv).copied())
    }
}

/// Computes the absolute offset (address) represented by the given global value
fn compute_global_value_addr(
    mut gv: GlobalValue,
    global_table_offset: u32,
    function: &Function,
    globals: &GlobalVariableTable,
) -> Option<u32> {
    let mut relative_offset = 0;
    loop {
        let gv_data = function.dfg.global_value(gv);
        relative_offset += gv_data.offset();
        match gv_data {
            GlobalValueData::Symbol { name, .. } => {
                let var = globals.find(*name)?;
                let base_offset = unsafe { globals.offset_of(var) };
                if relative_offset >= 0 {
                    return Some((global_table_offset + base_offset) + relative_offset as u32);
                } else {
                    return Some(
                        (global_table_offset + base_offset) - relative_offset.unsigned_abs(),
                    );
                }
            }
            GlobalValueData::IAddImm { base, .. } => {
                gv = *base;
            }
            GlobalValueData::Load { base, .. } => {
                gv = *base;
            }
        }
    }
}
