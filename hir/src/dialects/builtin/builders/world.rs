use alloc::format;

use crate::{
    dialects::builtin::{
        Component, ComponentId, ComponentRef, Module, ModuleBuilder, ModuleRef,
        PrimComponentBuilder, PrimModuleBuilder, World, WorldRef,
    },
    version::Version,
    Builder, Ident, Op, OpBuilder, Report, Spanned, SymbolName, SymbolNameComponent, SymbolPath,
    SymbolTable, UnsafeIntrusiveEntityRef,
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
        Ok(component_ref)
    }

    pub fn find_component(&self, id: &ComponentId) -> Option<ComponentRef> {
        self.world.borrow().get(SymbolName::intern(id)).and_then(|symbol_ref| {
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
        Ok(module_ref)
    }

    /// Resolve a world-level module with `name`, if declared/defined
    pub fn find_module(&self, name: SymbolName) -> Option<ModuleRef> {
        self.world.borrow().get(name).and_then(|symbol_ref| {
            let op = symbol_ref.borrow();
            op.as_symbol_operation().downcast_ref::<Module>().map(|m| m.as_module_ref())
        })
    }

    /// Recursively declare a hierarchy of modules, given a [SymbolPath] which contains the modules
    /// that must either exist, or will be created.
    ///
    /// Think of this as `mkdir -p <path>` for modules.
    ///
    /// NOTE: The entire [SymbolPath], ignoring root and leaf components, must resolve to a Module,
    /// or to nothing. A path component which resolves to some other operation will be treated as
    /// a conflict, and an error will be returned.
    pub fn declare_module_tree(&mut self, path: &SymbolPath) -> Result<ModuleRef, Report> {
        let mut parts = path.components().peekable();
        parts.next_if_eq(&SymbolNameComponent::Root);

        let mut current_symbol_table = self.world.as_operation_ref();
        let mut leaf_module = None;
        while let Some(SymbolNameComponent::Component(module_name)) = parts.next() {
            let symbol = current_symbol_table.borrow().as_symbol_table().unwrap().get(module_name);
            if symbol.is_some_and(|sym| !sym.borrow().as_symbol_operation().is::<Module>()) {
                return Err(Report::msg(format!(
                    "could not declare module path component '{module_name}': a non-module symbol \
                     with that name already exists"
                )));
            }

            let module = symbol.and_then(|symbol_ref| {
                symbol_ref
                    .borrow()
                    .as_symbol_operation()
                    .downcast_ref::<Module>()
                    .map(|m| m.as_module_ref())
            });
            let is_parent_module = current_symbol_table.borrow().is::<Module>();
            let module = match module {
                Some(module) => module,
                None if is_parent_module => {
                    let parent_module = {
                        current_symbol_table
                            .borrow()
                            .downcast_ref::<Module>()
                            .unwrap()
                            .as_module_ref()
                    };
                    let mut module_builder = ModuleBuilder::new(parent_module);
                    module_builder.declare_module(module_name.into())?
                }
                None => {
                    let world = unsafe {
                        UnsafeIntrusiveEntityRef::from_raw(
                            current_symbol_table.borrow().downcast_ref::<World>().unwrap(),
                        )
                    };
                    let mut world_builder = WorldBuilder::new(world);
                    world_builder.declare_module(module_name.into())?
                }
            };
            current_symbol_table = module.as_operation_ref();
            leaf_module = Some(module);
        }

        Ok(leaf_module.expect("invalid empty module path"))
    }
}
