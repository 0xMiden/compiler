use crate::{
    dialects::builtin::{
        FunctionRef, GlobalVariable, GlobalVariableBuilder, Module, PrimFunctionBuilder,
    },
    Builder, FunctionIdent, Ident, Op, OpBuilder, Report, Signature, Spanned, Type,
    UnsafeIntrusiveEntityRef, Visibility,
};

/// A specialized builder for constructing/modifying [crate::dialects::hir::Module]
pub struct ModuleBuilder<'f> {
    pub module: &'f mut Module,
    builder: OpBuilder,
}
impl<'b> ModuleBuilder<'b> {
    /// Create a builder over `module`
    pub fn new(module: &'b mut Module) -> Self {
        let context = module.as_operation().context_rc();
        let mut builder = OpBuilder::new(context);

        if module.body().is_empty() {
            builder.create_block(module.body().as_region_ref(), None, &[]);
        } else {
            let current_block = module.body().entry_block_ref().unwrap();
            builder.set_insertion_point_to_end(current_block);
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
        let name = FunctionIdent {
            module: *self.module.name(),
            function: name,
        };
        builder(name, signature)
    }

    /// Declare the import of an externally-defined [crate::dialects::hir::Function] into this
    /// module - imported from `module` with the given name and signature.
    ///
    /// NOTE: The given `name` must match an exported symbol from `module`, and the provided
    /// `signature` must be consistent between the definition and the import.
    ///
    /// It is not valid to define a body for the returned function - in order for symbol resolution
    /// and linking to proceed as expected, imported functions must remain declarations.
    pub fn import_function(
        &mut self,
        module: Ident,
        name: Ident,
        signature: Signature,
    ) -> Result<FunctionRef, Report> {
        let builder = PrimFunctionBuilder::new(&mut self.builder, name.span());
        let name = FunctionIdent {
            module,
            function: name,
        };
        builder(name, signature)
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
        builder(name, visibility, ty)
    }
}
