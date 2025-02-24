use crate::{
    dialects::builtin::{ComponentRef, PrimComponentBuilder, WorldRef},
    version::Version,
    Builder, Ident, Op, OpBuilder, Report, Spanned,
};

pub struct WorldBuilder {
    pub world: WorldRef,
    builder: OpBuilder,
}
impl WorldBuilder {
    pub fn new(world_ref: WorldRef) -> Self {
        let world = world_ref.borrow();
        let context = world.as_operation().context_rc();
        let mut builder = OpBuilder::new(context);

        let body = world.body();
        if let Some(current_block) = body.entry_block_ref() {
            builder.set_insertion_point_to_end(current_block);
        } else {
            let body_ref = body.as_region_ref();
            drop(body);
            builder.create_block(body_ref, None, &[]);
        }

        Self {
            world: world_ref,
            builder,
        }
    }

    pub fn define_component(
        &mut self,
        ns: Ident,
        name: Ident,
        ver: Version,
    ) -> Result<ComponentRef, Report> {
        let builder = PrimComponentBuilder::new(&mut self.builder, name.span());
        builder(ns, name, ver)
    }
}
