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
        builder(name)
    }
}
