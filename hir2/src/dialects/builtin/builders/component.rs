use crate::{
    dialects::builtin::{
        ComponentRef, InterfaceRef, ModuleRef, PrimInterfaceBuilder, PrimModuleBuilder,
    },
    Builder, Ident, Op, OpBuilder, Report, Spanned,
};

pub struct ComponentBuilder {
    pub component: ComponentRef,
    builder: OpBuilder,
}
impl ComponentBuilder {
    pub fn new(mut component: ComponentRef) -> Self {
        let component_ref = component.borrow_mut();
        let context = component_ref.as_operation().context_rc();
        let mut builder = OpBuilder::new(context);

        if component_ref.body().is_empty() {
            builder.create_block(component_ref.body().as_region_ref(), None, &[]);
        } else {
            let current_block = component_ref.body().entry_block_ref().unwrap();
            builder.set_insertion_point_to_end(current_block);
        }

        Self { component, builder }
    }

    pub fn define_interface(&mut self, name: Ident) -> Result<InterfaceRef, Report> {
        let builder = PrimInterfaceBuilder::new(&mut self.builder, name.span());
        builder(name)
    }

    pub fn define_module(&mut self, name: Ident) -> Result<ModuleRef, Report> {
        let builder = PrimModuleBuilder::new(&mut self.builder, name.span());
        builder(name)
    }
}
