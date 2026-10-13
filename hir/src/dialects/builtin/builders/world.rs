use alloc::{format, rc::Rc};

use super::BuiltinOpBuilder;
use crate::{
    Builder, BuilderExt, Context, Ident, Op, OpBuilder, Report, SmallVec, SymbolName,
    SymbolNameComponent, SymbolPath, SymbolTable,
    dialects::builtin::{
        Component, ComponentRef, Module, ModuleBuilder, ModuleRef, World, WorldRef,
        ops::shadowing_error,
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
    /// The name is the Miden Assembly spelling of the component's namespace path (see
    /// [SymbolPath::to_symbol_name]), e.g. `miden::counter_contract::counter_contract`. Callers
    /// holding a [SymbolPath] use [SymbolPath::to_symbol_name]. Any other name is one raw segment,
    /// which is spelled (see [Component::canonical_name]).
    ///
    /// Returns an error when Miden Assembly cannot spell a segment of the name, when the world
    /// declares a module tree reaching the path the name spells, which the component would
    /// shadow, or another component whose name nests with this one (one is a segment-prefix of
    /// the other): resolution prefers the longest registered name.
    pub fn define_component(&mut self, name: Ident) -> Result<ComponentRef, Report> {
        let name = Ident::new(Component::canonical_name(name.name)?, name.span);
        self.world.borrow().reject_component_shadowing(name.name)?;
        // `name` is spelled already, so it is not spelled again by `create_component`
        let op_builder = self.builder.create::<Component, (_,)>(name.span);
        op_builder(name)
    }

    /// Resolve a world-level component with `name`, if defined.
    ///
    /// The name is the component's namespace path, see [WorldBuilder::define_component].
    pub fn find_component(&self, name: SymbolName) -> Option<ComponentRef> {
        let name = Component::canonical_name(name).ok()?;
        (&*self.world.borrow() as &dyn SymbolTable).find::<Component>(name)
    }

    /// Declare a new world-level module named by the raw segment `name`, see
    /// [BuiltinOpBuilder::create_module].
    pub fn declare_module(&mut self, name: Ident) -> Result<ModuleRef, Report> {
        self.builder.create_module(name)
    }

    /// Resolve the world-level module named by the raw segment `name` (see
    /// [Module::canonical_name]), if declared/defined
    pub fn find_module(&self, name: SymbolName) -> Option<ModuleRef> {
        let name = Module::canonical_name(name).ok()?;
        (&*self.world.borrow() as &dyn SymbolTable).find::<Module>(name)
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
        // Each raw segment is validated here, once, and looked up by its spelling below
        let spelled = modules
            .iter()
            .map(|module| Module::canonical_name(module.as_symbol_name()))
            .collect::<Result<SmallVec<[_; 4]>, _>>()?;
        let module_path = SymbolPath::join_components(&modules);
        let shadowing = self.world.borrow().component_shadowing(module_path);
        if let Some(component) = shadowing {
            return Err(shadowing_error(component, module_path, component));
        }

        let mut parts = path.components().peekable();
        parts.next_if_eq(&SymbolNameComponent::Root);

        let mut current_symbol_table = self.world.as_operation_ref();
        let mut leaf_module = None;
        let mut spelled = spelled.into_iter();
        while let Some(SymbolNameComponent::Component(module_name)) = parts.next() {
            let spelled = spelled.next().expect("one spelling per component");
            let symbol = current_symbol_table.borrow().as_symbol_table().unwrap().get(spelled);
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
                    "component `{second}` and component `{first}` nest (`{shorter}` is a \
                     segment-prefix of `{longer}`); component namespaces must not nest"
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
    fn a_module_name_containing_a_path_separator_is_rejected() {
        use alloc::string::{String, ToString};

        use crate::dialects::builtin::ComponentBuilder;

        let context = Rc::new(Context::default());
        let mut builder = OpBuilder::new(context);
        let world =
            builder.create::<World, ()>(SourceSpan::default())().expect("failed to create world");
        let mut world_builder = WorldBuilder::new(world);
        let component = world_builder
            .define_component(Ident::from("acme::app::app"))
            .expect("failed to define component");
        let top = world_builder.declare_module(Ident::from("top")).expect("failed to declare top");

        let expected = "module `a::b`: a module name cannot contain `::` (only components are \
                        named by `::`-joined paths)";
        let errors: [(&str, Result<ModuleRef, Report>); 3] = [
            ("world", world_builder.declare_module(Ident::from("a::b"))),
            ("component", ComponentBuilder::new(component).define_module(Ident::from("a::b"))),
            ("module", ModuleBuilder::new(top).declare_module(Ident::from("a::b"))),
        ];
        for (parent, result) in errors {
            let err = result.map(|_| ()).map_err(|err| err.to_string());
            assert_eq!(err, Err(String::from(expected)), "in a {parent}");
        }
    }

    /// A stub module tree declared from a Miden Assembly path with a quoted root (`"masm-dep"`)
    /// and a component named from the raw namespace `masm-dep` meet under one name, in either
    /// order.
    #[test]
    fn a_quoted_module_tree_root_and_a_raw_component_namespace_shadow() {
        use alloc::string::ToString;

        let stub_tree = SymbolPath::from_masm_module_id("\"masm-dep\"::math");
        let namespace = SymbolPath::from_iter([
            SymbolNameComponent::Root,
            SymbolNameComponent::Component(SymbolName::intern("masm-dep")),
        ]);
        let expected = "component `\"masm-dep\"` and module path `\"masm-dep\"::math` share the \
                        prefix `\"masm-dep\"`; a component name must not shadow a module tree";

        let (_world, mut world_builder) = empty_world();
        world_builder
            .declare_module_tree(&stub_tree)
            .expect("failed to declare module tree");
        let name = namespace.to_symbol_name().expect("the segments are spellable");
        let Err(err) = world_builder.define_component(name.into()) else {
            panic!("the component shadows the stub module tree");
        };
        assert_eq!(
            err.to_string(),
            "component `\"masm-dep\"` and module path `\"masm-dep\"` share the prefix \
             `\"masm-dep\"`; a component name must not shadow a module tree"
        );

        let (_world, mut world_builder) = empty_world();
        world_builder.define_component(name.into()).expect("failed to define component");
        let Err(err) = world_builder.declare_module_tree(&stub_tree) else {
            panic!("the component shadows the stub module tree");
        };
        assert_eq!(err.to_string(), expected);
    }

    /// The component builder rejects a raw segment Miden Assembly cannot spell.
    #[test]
    fn the_component_builder_rejects_an_unspellable_segment() {
        use alloc::string::{String, ToString};

        let (_world, mut world_builder) = empty_world();
        let err = world_builder
            .define_component(Ident::from(r"a\b"))
            .map(|_| ())
            .map_err(|err| err.to_string());
        assert_eq!(
            err,
            Err(String::from(
                r"invalid component name: the segment `a\b` cannot be spelled in Miden Assembly"
            ))
        );
    }

    /// The component and interface verifiers reject a name set after construction that is not
    /// the Miden Assembly spelling of its segments, or has a segment Miden Assembly cannot spell.
    #[test]
    fn the_symbol_table_verifiers_reject_a_misspelled_name() {
        use alloc::string::{String, ToString};

        use crate::{Verify, dialects::builtin::ComponentBuilder};

        let (world, mut world_builder) = empty_world();
        let context = world.borrow().as_operation().context_rc();
        let mut component = world_builder
            .define_component(Ident::from("ok"))
            .expect("failed to define component");
        let mut interface = ComponentBuilder::new(component)
            .define_interface(Ident::from("api"))
            .expect("failed to define interface");
        let verify =
            |op: &dyn Verify<dyn SymbolTable>| op.verify(&context).map_err(|err| err.to_string());
        assert_eq!(verify(&*component.borrow()), Ok(()));
        assert_eq!(verify(&*interface.borrow()), Ok(()));

        for (name, expected) in [
            (
                "masm-dep",
                "component `masm-dep` is not the Miden Assembly spelling of its segments; name \
                 components through `SymbolPath::to_symbol_name`",
            ),
            (
                r#"app::"a#b""#,
                "invalid component name: the segment `a#b` cannot be spelled in Miden Assembly",
            ),
        ] {
            component.borrow_mut().set_name(SymbolName::intern(name));
            assert_eq!(verify(&*component.borrow()), Err(String::from(expected)), "`{name}`");
        }
        for (name, expected) in [
            (
                "my-api",
                "interface `my-api` is not the Miden Assembly spelling of its segments",
            ),
            (
                r#""a#b""#,
                "invalid interface name: the segment `a#b` cannot be spelled in Miden Assembly",
            ),
        ] {
            interface.borrow_mut().set_name(SymbolName::intern(name));
            assert_eq!(verify(&*interface.borrow()), Err(String::from(expected)), "`{name}`");
        }
    }

    /// A module or interface builder reads its name as one raw segment, and rejects a segment
    /// Miden Assembly cannot spell, a quoted one included.
    #[test]
    fn a_module_name_miden_assembly_cannot_spell_is_rejected() {
        use alloc::string::ToString;

        use crate::dialects::builtin::ComponentBuilder;

        let (_world, mut world_builder) = empty_world();
        let component = world_builder
            .define_component(Ident::from("app"))
            .expect("failed to define component");
        for raw in [r#""app""#, r"a\b", "ü", ""] {
            let module = world_builder
                .declare_module(Ident::from(raw))
                .map(|_| ())
                .map_err(|err| err.to_string());
            let interface = ComponentBuilder::new(component)
                .define_interface(Ident::from(raw))
                .map(|_| ())
                .map_err(|err| err.to_string());
            let expected = |kind: &str| {
                Err(alloc::format!(
                    "invalid {kind} name: the segment `{raw}` cannot be spelled in Miden Assembly"
                ))
            };
            assert_eq!(module, expected("module"), "`{raw}`");
            assert_eq!(interface, expected("interface"), "`{raw}`");
        }
        assert!(world_builder.find_module(r#""app""#.into()).is_none());
    }

    /// A symbol table uniqued on insertion gets the suffix on its raw last segment, spelled again.
    #[test]
    fn a_uniqued_module_name_is_spelled() {
        use crate::{AsSymbolRef, Symbol, SymbolMap, dialects::builtin::BuiltinOpBuilder};

        let mut builder = OpBuilder::new(Rc::new(Context::default()));
        let mut module =
            || builder.create_module(Ident::from("masm-dep")).expect("failed to create module");
        let (first, second) = (module(), module());
        let mut symbols = SymbolMap::default();
        let name = Symbol::name(&*first.borrow());
        assert_eq!(symbols.insert(name, first.as_symbol_ref()).as_str(), "\"masm-dep\"");
        assert_eq!(symbols.insert(name, second.as_symbol_ref()).as_str(), "\"masm-dep_1\"");
        assert_eq!(Symbol::name(&*second.borrow()).as_str(), "\"masm-dep_1\"");
    }

    /// Renaming a component rewrites the raw segments its uses end in, so a use still resolves.
    #[test]
    fn a_renamed_component_is_still_resolved_by_its_uses() {
        use alloc::string::ToString;

        use crate::{
            AsSymbolRef, Symbol, SymbolUse,
            dialects::builtin::attributes::{SymbolRef, SymbolRefAttr},
        };

        let (mut world, mut world_builder) = empty_world();
        let context = world.borrow().as_operation().context_rc();
        let component = world_builder
            .define_component(Ident::from("miden::a::b"))
            .expect("failed to define component");
        let owner = world_builder
            .define_component(Ident::from("other"))
            .expect("failed to define component");
        let path = component.borrow().path();
        let mut attr = context.create_attribute::<SymbolRefAttr, _>(SymbolRef::new(path, None));
        let user = context.alloc_tracked(SymbolUse::new(owner.as_operation_ref(), attr));
        attr.borrow_mut().set_user(user);
        attr.borrow_mut().link(component.as_symbol_ref());

        world
            .borrow_mut()
            .rename("miden::a::b".into(), "miden::a::b_1".into())
            .expect("failed to rename component");
        let path = attr.borrow().path().clone();
        assert_eq!(path.to_string(), "::miden::a::b_1");
        let resolved = world.borrow().resolve(&path).expect("the use should resolve");
        assert_eq!(resolved.borrow().as_operation_ref(), component.as_operation_ref());
    }

    /// The module verifier reads a quoted name for its raw segment, which must not hold `::`.
    #[test]
    fn the_module_verifier_rejects_a_quoted_path() {
        use alloc::string::{String, ToString};

        use crate::Verify;

        let (world, mut world_builder) = empty_world();
        let context = world.borrow().as_operation().context_rc();
        let mut module = world_builder
            .declare_module(Ident::from("m"))
            .expect("failed to declare module");
        module.borrow_mut().set_name(SymbolName::intern("\"a::b\""));
        let err = Verify::<dyn SymbolTable>::verify(&*module.borrow(), &context)
            .map_err(|err| err.to_string());
        assert_eq!(
            err,
            Err(String::from(
                "module `a::b`: a module name cannot contain `::` (only components are named by \
                 `::`-joined paths)"
            ))
        );
    }

    /// An empty world and its builder.
    fn empty_world() -> (WorldRef, WorldBuilder) {
        let context = Rc::new(Context::default());
        let mut builder = OpBuilder::new(context);
        let world =
            builder.create::<World, ()>(SourceSpan::default())().expect("failed to create world");
        (world, WorldBuilder::new(world))
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
