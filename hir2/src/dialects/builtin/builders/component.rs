use crate::{
    dialects::builtin::{
        Component, InterfaceRef, ModuleRef, PrimInterfaceBuilder, PrimModuleBuilder,
    },
    Builder, Ident, Op, OpBuilder, Report, Spanned,
};

pub struct ComponentBuilder<'b> {
    pub component: &'b mut Component,
    builder: OpBuilder,
}
impl<'b> ComponentBuilder<'b> {
    pub fn new(component: &'b mut Component) -> Self {
        let context = component.as_operation().context_rc();
        let mut builder = OpBuilder::new(context);

        if component.body().is_empty() {
            builder.create_block(component.body().as_region_ref(), None, &[]);
        } else {
            let current_block = component.body().entry_block_ref().unwrap();
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
