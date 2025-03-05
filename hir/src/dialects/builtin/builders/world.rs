use crate::{
    dialects::builtin::{
        Component, ComponentId, ComponentRef, Module, ModuleRef, PrimComponentBuilder,
        PrimModuleBuilder, WorldRef,
    },
    version::Version,
    Builder, Ident, Op, OpBuilder, Report, Spanned, SymbolName, SymbolTable,
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
        let component_ref = builder(ns, name, ver.clone())?;
        let is_new = self
            .world
            .borrow_mut()
            .symbol_manager_mut()
            .insert_new(component_ref, crate::ProgramPoint::Invalid);
        assert!(
            is_new,
            "component {} already exists in world",
            ComponentId {
                namespace: ns.name,
                name: name.name,
                version: ver
            }
        );
        Ok(component_ref)
    }

    pub fn find_component(&self, id: &ComponentId) -> Option<ComponentRef> {
        self.world
            .borrow()
            .get(SymbolName::intern(id.to_string()))
            .and_then(|symbol_ref| {
                let op = symbol_ref.borrow();
                op.as_symbol_operation()
                    .downcast_ref::<Component>()
                    .map(|c| c.as_component_ref())
            })
    }

    /// Declare a new world-level module `name`
    pub fn declare_module(&mut self, name: Ident) -> Result<ModuleRef, Report> {
        let builder = PrimModuleBuilder::new(&mut self.builder, name.span());
        let module_ref = builder(name)?;
        let is_new = self
            .world
            .borrow_mut()
            .symbol_manager_mut()
            .insert_new(module_ref, crate::ProgramPoint::Invalid);
        assert!(is_new, "module with the name {name} already exists in world",);
        Ok(module_ref)
    }

    /// Resolve a world-level module with `name`, if declared/defined
    pub fn find_module(&self, name: SymbolName) -> Option<ModuleRef> {
        self.world.borrow().get(name).and_then(|symbol_ref| {
            let op = symbol_ref.borrow();
            op.as_symbol_operation().downcast_ref::<Module>().map(|m| m.as_module_ref())
        })
    }
}
