use super::BuiltinOpBuilder;
use crate::{
    Builder, Ident, Op, OpBuilder, Report, Signature, SymbolName, SymbolPath, SymbolTable,
    dialects::builtin::{ComponentRef, FunctionRef, InterfaceRef, Module, ModuleRef},
};

pub struct ComponentBuilder {
    pub component: ComponentRef,
    builder: OpBuilder,
}
impl ComponentBuilder {
    pub fn new(component: ComponentRef) -> Self {
        let component_ref = component.borrow();
        let context = component_ref.as_operation().context_rc();
        let mut builder = OpBuilder::new(context);

        let body = component_ref.body();
        if let Some(current_block) = body.entry_block_ref() {
            builder.set_insertion_point_to_end(current_block);
        } else {
            let body_ref = body.as_region_ref();
            drop(body);
            builder.create_block(body_ref, None, &[]);
        }

        Self { component, builder }
    }

    pub fn define_interface(&mut self, name: Ident) -> Result<InterfaceRef, Report> {
        self.builder.create_interface(name)
    }

    pub fn define_module(&mut self, name: Ident) -> Result<ModuleRef, Report> {
        let module_ref = self.builder.create_module(name)?;
        Ok(module_ref)
    }

    pub fn find_module(&self, name: SymbolName) -> Option<ModuleRef> {
        self.component.borrow().get(name).and_then(|symbol_ref| {
            let op = symbol_ref.borrow();
            op.as_symbol_operation().downcast_ref::<Module>().map(|m| m.as_module_ref())
        })
    }

    pub fn resolve_module(&self, path: &SymbolPath) -> Option<ModuleRef> {
        self.component.borrow().resolve(path).and_then(|symbol_ref| {
            let op = symbol_ref.borrow();
            op.as_symbol_operation().downcast_ref::<Module>().map(|m| m.as_module_ref())
        })
    }

    /// Declare a new [crate::dialects::hir::Function] in this component with the given name and
    /// signature.
    pub fn define_function(
        &mut self,
        name: Ident,
        signature: Signature,
    ) -> Result<FunctionRef, Report> {
        let function_ref = self.builder.create_function(name, signature)?;
        Ok(function_ref)
    }
}
