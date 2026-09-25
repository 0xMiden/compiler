use alloc::{format, rc::Rc};

use crate::{
    Builder, Context, Ident, Op, OpBuilder, Report, SmallVec, Spanned, SymbolName,
    SymbolNameComponent, SymbolPath, SymbolTable,
    dialects::builtin::{
        Component, ComponentRef, Module, ModuleBuilder, ModuleRef, PrimComponentBuilder,
        PrimModuleBuilder, World, WorldRef, ops::shadowing_error,
    },
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

    pub fn context_rc(&self) -> Rc<Context> {
        self.builder.context_rc()
    }

    /// Define a new world-level component `name`.
    ///
    /// The name is the component's namespace path, with its segments joined by `::`, e.g.
    /// `miden::counter_contract::counter_contract`. Callers holding a [SymbolPath] use
    /// [SymbolPath::to_symbol_name].
    ///
    /// Returns an error when the world declares a module tree reaching the path the name spells,
    /// which the component would shadow: resolution prefers the longest registered name.
    pub fn define_component(&mut self, name: Ident) -> Result<ComponentRef, Report> {
        self.world.borrow().reject_component_shadowing(name.name)?;
        let builder = PrimComponentBuilder::new(&mut self.builder, name.span());
        let component_ref = builder(name)?;
        Ok(component_ref)
    }

    /// Resolve a world-level component with `name`, if defined.
    ///
    /// The name is the component's namespace path, see [WorldBuilder::define_component].
    pub fn find_component(&self, name: SymbolName) -> Option<ComponentRef> {
        self.world.borrow().get(name).and_then(|symbol_ref| {
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
    /// a conflict, and an error will be returned. So is a world-level component named by a prefix
    /// of the path, which would shadow the module tree.
    pub fn declare_module_tree(&mut self, path: &SymbolPath) -> Result<ModuleRef, Report> {
        let modules = path
            .components()
            .filter(|component| matches!(component, SymbolNameComponent::Component(_)))
            .collect::<SmallVec<[_; 4]>>();
        for len in 1..=modules.len() {
            let prefix = SymbolPath::join_components(&modules[..len]);
            if self.find_component(prefix).is_some() {
                let module_path = SymbolPath::join_components(&modules);
                return Err(shadowing_error(prefix, module_path, prefix));
            }
        }

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
                    let world = current_symbol_table.try_downcast_op::<World>().unwrap();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BuilderExt, SourceSpan};

    #[test]
    fn declare_module_tree_creates_nested_modules_resolvable_from_world() {
        let context = Rc::new(Context::default());
        let mut builder = OpBuilder::new(context);
        let world =
            builder.create::<World, ()>(SourceSpan::default())().expect("failed to create world");
        let mut world_builder = WorldBuilder::new(world);

        let path = SymbolPath::from_masm_module_id("pkg::util::math");
        let leaf = world_builder
            .declare_module_tree(&path)
            .expect("failed to declare nested module tree");

        assert_eq!(leaf.borrow().get_name().as_str(), "math");
        let resolved = world
            .borrow()
            .resolve(&path)
            .expect("nested module should resolve from the world");
        assert!(resolved.borrow().as_symbol_operation().is::<Module>());
    }

    /// A component named by a multi-segment path coexists with a module tree sharing its first
    /// segment, and symbols in both resolve by their `::`-separated paths.
    #[test]
    fn multi_segment_component_name_resolves_beside_module_tree() {
        use alloc::string::ToString;

        use crate::{
            FunctionIdent, Symbol, Visibility,
            dialects::builtin::{ComponentBuilder, Function, attributes::Signature},
        };

        let context = Rc::new(Context::default());
        let mut builder = OpBuilder::new(context.clone());
        let world =
            builder.create::<World, ()>(SourceSpan::default())().expect("failed to create world");
        let mut world_builder = WorldBuilder::new(world);

        let x = world_builder
            .declare_module_tree(&SymbolPath::from_masm_module_id("miden::protocol::x"))
            .expect("failed to declare module tree");
        ModuleBuilder::new(x)
            .define_function(Ident::from("g"), Visibility::Public, Signature::new(&context, [], []))
            .expect("failed to declare g");

        let component = world_builder
            .define_component(Ident::from("miden::a::b"))
            .expect("failed to define component");
        let m = ComponentBuilder::new(component)
            .define_module(Ident::from("m"))
            .expect("failed to define module");
        let f = ModuleBuilder::new(m)
            .define_function(Ident::from("f"), Visibility::Public, Signature::new(&context, [], []))
            .expect("failed to define f");

        let resolve = |id: &str| {
            let id = id.parse::<FunctionIdent>().expect("valid function id");
            let path = SymbolPath::from_masm_function_id(id);
            world
                .borrow()
                .resolve(&path)
                .unwrap_or_else(|| panic!("'{path}' should resolve"))
        };
        let g = resolve("miden::protocol::x::g");
        assert_eq!(g.borrow().name().as_str(), "g");
        let resolved_f = resolve("miden::a::b::m::f");
        assert!(resolved_f.borrow().as_symbol_operation().is::<Function>());
        assert_eq!(
            resolved_f.borrow().as_symbol_operation().as_operation_ref(),
            f.as_operation_ref()
        );

        let f_path = f.borrow().path();
        assert_eq!(f_path.to_string(), "::miden::a::b::m::f");
        assert_eq!(f_path.to_library_path().to_string(), "::miden::a::b::m::f");
        assert_eq!(component.borrow().namespace_path().to_string(), "::miden::a::b");
        assert!(world_builder.find_component("miden::a::b".into()) == Some(component));
    }

    /// A function whose opaque name spells a joined path does not capture the resolution of a
    /// longer path through the module tree of the same name.
    #[test]
    fn joined_function_name_does_not_capture_module_tree_path() {
        use crate::{
            FunctionIdent, Visibility,
            dialects::builtin::{Function, attributes::Signature},
        };

        let context = Rc::new(Context::default());
        let mut builder = OpBuilder::new(context.clone());
        let world =
            builder.create::<World, ()>(SourceSpan::default())().expect("failed to create world");
        let mut world_builder = WorldBuilder::new(world);

        // `top` holds both the module tree `a` > `b` (function `g`) and a function named `a::b`
        let top = world_builder.declare_module(Ident::from("top")).expect("failed to declare top");
        let a = ModuleBuilder::new(top)
            .declare_module(Ident::from("a"))
            .expect("failed to declare a");
        let b = ModuleBuilder::new(a)
            .declare_module(Ident::from("b"))
            .expect("failed to declare b");
        let g = ModuleBuilder::new(b)
            .define_function(Ident::from("g"), Visibility::Public, Signature::new(&context, [], []))
            .expect("failed to define g");
        ModuleBuilder::new(top)
            .define_function(
                Ident::from("a::b"),
                Visibility::Public,
                Signature::new(&context, [], []),
            )
            .expect("failed to define a::b");

        let id = "top::a::b::g".parse::<FunctionIdent>().expect("valid function id");
        let resolved = world
            .borrow()
            .resolve(&SymbolPath::from_masm_function_id(id))
            .expect("'top::a::b::g' should resolve");
        assert!(resolved.borrow().as_symbol_operation().is::<Function>());
        assert_eq!(
            resolved.borrow().as_symbol_operation().as_operation_ref(),
            g.as_operation_ref()
        );
    }

    /// A world with the kernel-like module tree `miden::protocol::note` declared.
    fn world_with_module_tree() -> (WorldRef, WorldBuilder) {
        let context = Rc::new(Context::default());
        let mut builder = OpBuilder::new(context);
        let world =
            builder.create::<World, ()>(SourceSpan::default())().expect("failed to create world");
        let mut world_builder = WorldBuilder::new(world);
        world_builder
            .declare_module_tree(&SymbolPath::from_masm_module_id("miden::protocol::note"))
            .expect("failed to declare module tree");
        (world, world_builder)
    }

    #[test]
    fn a_component_named_like_a_declared_module_path_is_rejected() {
        use alloc::string::ToString;

        let (_world, mut world_builder) = world_with_module_tree();
        for name in ["miden::protocol::note", "miden::protocol", "miden"] {
            let Err(err) = world_builder.define_component(Ident::from(name)) else {
                panic!("the component `{name}` would shadow the module tree");
            };
            assert_eq!(
                err.to_string(),
                alloc::format!(
                    "component `{name}` and module path `{name}` share the prefix `{name}`; a \
                     component name must not shadow a module tree"
                )
            );
        }
        world_builder
            .define_component(Ident::from("miden::protocol::other"))
            .expect("a component beside the module tree is accepted");
    }

    /// Two world-level components whose names nest are rejected in either definition order;
    /// components sharing only a proper prefix of segments are accepted.
    #[test]
    fn nested_component_names_are_rejected() {
        use alloc::string::ToString;

        let new_world = || {
            let context = Rc::new(Context::default());
            let mut builder = OpBuilder::new(context);
            let world = builder.create::<World, ()>(SourceSpan::default())()
                .expect("failed to create world");
            WorldBuilder::new(world)
        };
        for (first, second) in [("acme::app::app", "acme::app::app::main"), ("a::b::c", "a::b")] {
            let mut world_builder = new_world();
            world_builder
                .define_component(Ident::from(first))
                .expect("failed to define component");
            let Err(err) = world_builder.define_component(Ident::from(second)) else {
                panic!("the components `{first}` and `{second}` nest");
            };
            let (shorter, longer) = if first.len() < second.len() {
                (first, second)
            } else {
                (second, first)
            };
            assert_eq!(
                err.to_string(),
                alloc::format!(
                    "component `{second}` and component `{first}` nest (`{shorter}` is a prefix \
                     of `{longer}`); component namespaces must not nest"
                )
            );
        }

        let mut world_builder = new_world();
        world_builder
            .define_component(Ident::from("acme::app::app"))
            .expect("failed to define component");
        for name in ["acme::app::apps", "acme::app::other", "acme::ap"] {
            world_builder
                .define_component(Ident::from(name))
                .unwrap_or_else(|err| panic!("`{name}` does not nest: {err}"));
        }
    }

    #[test]
    fn a_module_tree_below_a_component_name_is_rejected() {
        use alloc::string::ToString;

        let context = Rc::new(Context::default());
        let mut builder = OpBuilder::new(context);
        let world =
            builder.create::<World, ()>(SourceSpan::default())().expect("failed to create world");
        let mut world_builder = WorldBuilder::new(world);
        world_builder
            .define_component(Ident::from("miden::protocol::note"))
            .expect("failed to define component");
        world_builder
            .define_component(Ident::from("std"))
            .expect("failed to define component");

        for (path, prefix) in [
            ("miden::protocol::note::sub", "miden::protocol::note"),
            ("miden::protocol::note", "miden::protocol::note"),
            ("std::mem", "std"),
        ] {
            let Err(err) =
                world_builder.declare_module_tree(&SymbolPath::from_masm_module_id(path))
            else {
                panic!("the component `{prefix}` would shadow the module tree `{path}`");
            };
            assert_eq!(
                err.to_string(),
                alloc::format!(
                    "component `{prefix}` and module path `{path}` share the prefix `{prefix}`; a \
                     component name must not shadow a module tree"
                )
            );
        }
        world_builder
            .declare_module_tree(&SymbolPath::from_masm_module_id("miden::protocol::tx"))
            .expect("a module tree beside the component is accepted");
    }
}
