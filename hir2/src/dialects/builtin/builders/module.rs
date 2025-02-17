use crate::{
    constants::ConstantData,
    dialects::builtin::{
        Function, FunctionRef, GlobalVariable, GlobalVariableBuilder, GlobalVariableRef, ModuleRef,
        PrimFunctionBuilder, Segment, SegmentBuilder,
    },
    Builder, Ident, Op, OpBuilder, Report, Signature, SourceSpan, Spanned, SymbolName, SymbolTable,
    Type, UnsafeIntrusiveEntityRef, Visibility,
};

/// A specialized builder for constructing/modifying [crate::dialects::hir::Module]
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

    /// Declare a new [crate::dialects::hir::Function] in this module with the given name and
    /// signature.
    ///
    /// The returned [FunctionRef] can be used to construct a [FunctionBuilder] to define the body
    /// of the function.
    pub fn define_function(
        &mut self,
        name: Ident,
        signature: Signature,
    ) -> Result<FunctionRef, Report> {
        let builder = PrimFunctionBuilder::new(&mut self.builder, name.span());
        let function_ref = builder(name, signature)?;
        let is_new = self
            .module
            .borrow_mut()
            .symbol_manager_mut()
            .insert_new(function_ref, crate::ProgramPoint::Invalid);
        assert!(is_new, "function with the name {name} already exists");
        Ok(function_ref)
    }

    /// Declare a new [GlobalVariable] in this module with the given name, visibility, and type.
    ///
    /// The returned [UnsafeIntrusiveEntityRef] can be used to construct a [InitializerBuilder]
    /// over the body of the global variable initializer region.
    pub fn define_global_variable(
        &mut self,
        name: Ident,
        visibility: Visibility,
        ty: Type,
    ) -> Result<UnsafeIntrusiveEntityRef<GlobalVariable>, Report> {
        let builder = GlobalVariableBuilder::new(&mut self.builder, name.span());
        let global_var_ref = builder(name, visibility, ty)?;
        let is_new = self
            .module
            .borrow_mut()
            .symbol_manager_mut()
            .insert_new(global_var_ref, crate::ProgramPoint::Invalid);
        assert!(is_new, "global variable with the name {name} already exists");
        Ok(global_var_ref)
    }

    pub fn define_data_segment(
        &mut self,
        offset: u32,
        data: impl Into<ConstantData>,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<Segment>, Report> {
        let data = self.builder.context().create_constant(data);
        let builder = SegmentBuilder::new(&mut self.builder, span);
        builder(offset, data, /*readonly= */ false)
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

    pub fn get_global_var(&self, name: SymbolName) -> Option<GlobalVariableRef> {
        self.module.borrow().get(name).and_then(|gv_symbol| {
            let op_ref = gv_symbol.borrow().as_operation_ref();
            op_ref
                .borrow()
                .downcast_ref::<GlobalVariable>()
                .map(|gv| gv.as_global_var_ref())
        })
    }
}
