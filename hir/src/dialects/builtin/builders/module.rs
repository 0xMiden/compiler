use super::BuiltinOpBuilder;
use crate::{
    constants::ConstantData,
    dialects::builtin::{
        Function, FunctionRef, GlobalVariable, GlobalVariableRef, Module, ModuleRef,
        PrimModuleBuilder, Segment,
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
        let function_ref = self.builder.create_function(name, signature)?;
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
                        function.signature_mut().visibility = visibility;
                    }
                    None => panic!("expected {name} to be a function"),
                }
            }
            None => {
                panic!("failed to find function {name} in module {}", self.module.borrow().name())
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
