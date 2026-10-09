use super::BuiltinOpBuilder;
use crate::{
    Builder, Ident, Op, OpBuilder, Report, Symbol, SymbolName, SymbolPath, SymbolTable, Visibility,
    dialects::builtin::{
        ComponentRef, FunctionRef, InterfaceRef, Module, ModuleRef, World, attributes::Signature,
    },
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

    /// Define a new interface named by the raw segment `name` in this component, see
    /// [BuiltinOpBuilder::create_interface].
    ///
    /// Returns an error when another world-level component nests in this one, see
    /// [ComponentBuilder::define_module].
    pub fn define_interface(&mut self, name: Ident) -> Result<InterfaceRef, Report> {
        self.reject_definition_in_nesting_component()?;
        self.builder.create_interface(name)
    }

    /// Define a new module named by the raw segment `name` in this component, see
    /// [BuiltinOpBuilder::create_module].
    ///
    /// Returns an error when another world-level component's name extends this component's: a
    /// component may only have another nested in it while it holds declarations only (see
    /// [World::reject_component_shadowing]).
    pub fn define_module(&mut self, name: Ident) -> Result<ModuleRef, Report> {
        self.reject_definition_in_nesting_component()?;
        self.builder.create_module(name)
    }

    /// Resolve the module named by the raw segment `name` in this component (see
    /// [Module::canonical_name]), if defined.
    pub fn find_module(&self, name: SymbolName) -> Option<ModuleRef> {
        let name = Module::canonical_name(name).ok()?;
        (&*self.component.borrow() as &dyn SymbolTable).find::<Module>(name)
    }

    /// Resolve the module at `path` from this component (see [SymbolTable::resolve]), if it is
    /// one.
    pub fn resolve_module(&self, path: &SymbolPath) -> Option<ModuleRef> {
        let op = self.component.borrow().symbol_manager().lookup_symbol_ref(path)?;
        op.try_downcast_op::<Module>().ok()
    }

    /// Returns an error when this component sits in a world holding another component whose name
    /// extends this component's.
    fn reject_definition_in_nesting_component(&self) -> Result<(), Report> {
        let component = self.component.borrow();
        let Some(parent) = component.as_operation().parent_op() else {
            return Ok(());
        };
        let Ok(world) = parent.try_downcast_op::<World>() else {
            return Ok(());
        };
        world.borrow().reject_definition_in_nesting_component(Symbol::name(&*component))
    }

    /// Declare a new [crate::dialects::builtin::Function] in this component with the given name and
    /// signature.
    pub fn define_function(
        &mut self,
        name: Ident,
        visibility: Visibility,
        signature: Signature,
    ) -> Result<FunctionRef, Report> {
        self.builder.create_function(name, visibility, signature)
    }
}
