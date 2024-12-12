use crate::{
    dialects::builtin::{Component, ModuleRef, PrimModuleBuilder, Segment, SegmentBuilder},
    Builder, Ident, Op, OpBuilder, Report, SourceSpan, Spanned, UnsafeIntrusiveEntityRef,
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

    pub fn define_module(&mut self, name: Ident) -> Result<ModuleRef, Report> {
        let builder = PrimModuleBuilder::new(&mut self.builder, name.span());
        builder(name)
    }

    pub fn define_data_segment(
        &mut self,
        offset: u32,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<Segment>, Report> {
        let builder = SegmentBuilder::new(&mut self.builder, span);
        builder(offset, /*readonly= */ false, /*zeroed= */ false)
    }
}
