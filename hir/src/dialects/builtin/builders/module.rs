use super::BuiltinOpBuilder;
use crate::{
    AsCallableSymbolRef, Builder, BuilderExt, Ident, Op, OpBuilder, Report, SourceSpan, Spanned,
    SymbolName, SymbolTable, Type, UnsafeIntrusiveEntityRef, Visibility,
    constants::ConstantData,
    dialects::builtin::{
        Function, FunctionRef, FunctionTableEntry, FunctionTableRef, GlobalVariable,
        GlobalVariableRef, Module, ModuleRef, PrimModuleBuilder, Segment, attributes::Signature,
    },
};

/// A specialized builder for constructing/modifying [crate::dialects::builtin::Module]
pub struct ModuleBuilder {
    pub module: ModuleRef,
    builder: OpBuilder,
}
impl ModuleBuilder {
    /// Create a builder over `module`
    pub fn new(module: ModuleRef) -> Self {
        let module_ref = module.borrow();
        let context = module_ref.as_operation().context_rc();
        let mut builder = OpBuilder::new(context);

        {
            let body = module_ref.body();

            if let Some(current_block) = body.entry_block_ref() {
                builder.set_insertion_point_to_end(current_block);
            } else {
                let body_ref = body.as_region_ref();
                drop(body);
                builder.create_block(body_ref, None, &[]);
            }
        }

        Self { module, builder }
    }

    /// Get the underlying [OpBuilder]
    pub fn builder(&mut self) -> &mut OpBuilder {
        &mut self.builder
    }

    /// Declare a new [crate::dialects::builtin::Function] in this module with the given name and
    /// signature.
    ///
    /// The returned [FunctionRef] can be used to construct a [super::FunctionBuilder] to define the
    /// body of the function.
    pub fn define_function(
        &mut self,
        name: Ident,
        visibility: Visibility,
        signature: Signature,
    ) -> Result<FunctionRef, Report> {
        self.builder.create_function(name, visibility, signature)
    }

    /// Declare a new [GlobalVariable] in this module with the given name, visibility, and type.
    ///
    /// The returned [UnsafeIntrusiveEntityRef] can be used to construct a builder over the body
    /// of the global variable initializer region.
    pub fn define_global_variable(
        &mut self,
        name: Ident,
        visibility: Visibility,
        ty: Type,
    ) -> Result<UnsafeIntrusiveEntityRef<GlobalVariable>, Report> {
        let global_var_ref = self.builder.create_global_variable(name, visibility, ty)?;
        Ok(global_var_ref)
    }

    pub fn define_data_segment(
        &mut self,
        offset: u32,
        data: impl Into<ConstantData>,
        readonly: bool,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<Segment>, Report> {
        self.builder.create_data_segment(offset, data, readonly, span)
    }

    /// Declare a new [FunctionTable] in this module with the given name, visibility, and slot
    /// count, with an empty set of entries.
    ///
    /// Initialized slots are added with [Self::append_function_table_entry].
    pub fn define_function_table(
        &mut self,
        name: Ident,
        visibility: Visibility,
        size: u32,
    ) -> Result<FunctionTableRef, Report> {
        let mut table_ref = self.builder.create_function_table(name, visibility, size)?;
        let context = table_ref.borrow().as_operation().context_rc();
        let entries_region_ref = {
            let mut table = table_ref.borrow_mut();
            table.entries_mut().as_region_ref()
        };
        let mut op_builder = OpBuilder::new(context);
        op_builder.create_block(entries_region_ref, None, &[]);
        Ok(table_ref)
    }

    /// Append a [FunctionTableEntry] to `table`, filling slot `index` with the MAST root of
    /// `callee` at program startup.
    pub fn append_function_table_entry<C: AsCallableSymbolRef>(
        &mut self,
        table: FunctionTableRef,
        index: u32,
        callee: C,
        span: SourceSpan,
    ) -> Result<(), Report> {
        let context = table.borrow().as_operation().context_rc();
        let entries_block = {
            let table = table.borrow();
            let entries = table.entries();
            entries.entry_block_ref().expect("expected function table entries block")
        };
        let mut op_builder = OpBuilder::new(context);
        op_builder.set_insertion_point_to_end(entries_block);
        let entry_builder = op_builder.create::<FunctionTableEntry, (_, C)>(span);
        entry_builder(index, callee)?;
        Ok(())
    }

    pub fn get_function(&self, name: &str) -> Option<FunctionRef> {
        let symbol = SymbolName::intern(name);
        match self.module.borrow().get(symbol) {
            Some(symbol_ref) => {
                let op = symbol_ref.borrow();
                match op.as_symbol_operation().downcast_ref::<Function>() {
                    Some(function) => Some(function.as_function_ref()),
                    None => panic!("expected {name} to be a function"),
                }
            }
            None => None,
        }
    }

    pub fn set_function_visibility(&mut self, name: &str, visibility: Visibility) {
        let symbol = SymbolName::intern(name);
        match self.module.borrow_mut().get(symbol) {
            Some(mut symbol_ref) => {
                let mut op = symbol_ref.borrow_mut();
                match op.as_symbol_operation_mut().downcast_mut::<Function>() {
                    Some(function) => {
                        *function.get_linkage_mut() = visibility;
                    }
                    None => panic!("expected {name} to be a function"),
                }
            }
            None => {
                panic!(
                    "failed to find function {name} in module {}",
                    self.module.borrow().get_name()
                )
            }
        }
    }

    pub fn get_global_var(&self, name: SymbolName) -> Option<GlobalVariableRef> {
        self.module.borrow().get(name).and_then(|gv_symbol| {
            let op_ref = gv_symbol.borrow().as_operation_ref();
            op_ref
                .borrow()
                .downcast_ref::<GlobalVariable>()
                .map(|gv| gv.as_global_var_ref())
        })
    }

    /// Declare a new nested module `name`
    pub fn declare_module(&mut self, name: Ident) -> Result<ModuleRef, Report> {
        let builder = PrimModuleBuilder::new(&mut self.builder, name.span());
        let module_ref = builder(name)?;
        Ok(module_ref)
    }

    /// Resolve a nested module with `name`, if declared/defined
    pub fn find_module(&self, name: SymbolName) -> Option<ModuleRef> {
        self.module.borrow().get(name).and_then(|symbol_ref| {
            let op = symbol_ref.borrow();
            op.as_symbol_operation().downcast_ref::<Module>().map(|m| m.as_module_ref())
        })
    }
}
