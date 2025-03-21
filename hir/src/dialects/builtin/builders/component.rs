use super::BuiltinOpBuilder;
use crate::{
    dialects::builtin::{ComponentRef, FunctionRef, InterfaceRef, Module, ModuleRef},
    Builder, Ident, Op, OpBuilder, Report, Signature, SymbolName, SymbolTable,
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
        let is_new = self
            .component
            .borrow_mut()
            .symbol_manager_mut()
            .insert_new(module_ref, crate::ProgramPoint::Invalid);
        assert!(
            is_new,
            "module with the name {name} already exists in component {}",
            self.component.borrow().name()
        );
        Ok(module_ref)
    }

    pub fn find_module(&self, name: SymbolName) -> Option<ModuleRef> {
        self.component.borrow().get(name).and_then(|symbol_ref| {
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
        let is_new = self
            .component
            .borrow_mut()
            .symbol_manager_mut()
            .insert_new(function_ref, crate::ProgramPoint::Invalid);
        assert!(is_new, "function with the name {name} already exists");
        Ok(function_ref)
    }
}
