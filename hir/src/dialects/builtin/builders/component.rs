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

    /// Define a new interface `name` in this component.
    ///
    /// Returns an error when another world-level component nests in this one, see
    /// [ComponentBuilder::define_module].
    pub fn define_interface(&mut self, name: Ident) -> Result<InterfaceRef, Report> {
        self.reject_definition_in_nesting_component()?;
        self.builder.create_interface(name)
    }

    /// Define a new module `name` in this component.
    ///
    /// Returns an error when `name` contains `::`, see [Module::validate_name], or when another
    /// world-level component's name extends this component's: a component may only have another
    /// nested in it while it holds declarations only (see [World::reject_component_shadowing]).
    pub fn define_module(&mut self, name: Ident) -> Result<ModuleRef, Report> {
        Module::validate_name(name.name)?;
        self.reject_definition_in_nesting_component()?;
        let module_ref = self.builder.create_module(name)?;
        Ok(module_ref)
    }

    pub fn find_module(&self, name: SymbolName) -> Option<ModuleRef> {
        self.component.borrow().get(name).and_then(|symbol_ref| {
            let op = symbol_ref.borrow();
            op.as_symbol_operation().downcast_ref::<Module>().map(|m| m.as_module_ref())
        })
    }

    pub fn resolve_module(&self, path: &SymbolPath) -> Option<ModuleRef> {
        self.component.borrow().resolve(path).and_then(|symbol_ref| {
            let op = symbol_ref.borrow();
            op.as_symbol_operation().downcast_ref::<Module>().map(|m| m.as_module_ref())
        })
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
