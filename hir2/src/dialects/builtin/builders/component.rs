use super::ModuleBuilder;
use crate::{
    dialects::builtin::{
        ComponentRef, InterfaceRef, ModuleRef, PrimInterfaceBuilder, PrimModuleBuilder,
    },
    Builder, FunctionIdent, Ident, Op, OpBuilder, Report, Signature, Spanned, SymbolTable,
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
        let builder = PrimInterfaceBuilder::new(&mut self.builder, name.span());
        builder(name)
    }

    pub fn define_module(&mut self, name: Ident) -> Result<ModuleRef, Report> {
        let builder = PrimModuleBuilder::new(&mut self.builder, name.span());
        let module_ref = builder(name)?;
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

    pub fn define_import(&mut self, func_id: FunctionIdent, sig: Signature) {
        let module_ref = self.define_module(func_id.module).expect("failed to define module");
        let mut module_builder = ModuleBuilder::new(module_ref);
        module_builder
            .define_function(func_id.function, sig)
            .expect("failed to define function");
    }
}
