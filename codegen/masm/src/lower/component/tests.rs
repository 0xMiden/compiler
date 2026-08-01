use alloc::{format, rc::Rc, string::String};

use midenc_hir::{Context, OperationRef, PointerType, diagnostics::Uri};
use midenc_session::{
    InputFile, Options, Session,
    diagnostics::{CaptureEmitter, DefaultSourceManager},
};

use super::*;

// -------------------------------------------------------------------------------------
// Fixtures.
//
// Task 7's, copied from `midenc-compile/src/pipeline/frontends/hir.rs` rather than shared:
// `midenc-compile` depends on this crate, so nothing here can import from it. Its report
// records which shapes parse — in particular that a component id is *one quoted*
// symbol-path component, because `ComponentId::try_from` splits the `:` and the `@` back
// out of it itself.
// -------------------------------------------------------------------------------------

/// A component, written on its own — the other half of the equivalence [`WORLD`] pins.
const COMPONENT: &str = r#"
builtin.component private @"hir_ns:test@1.0.0" {
builtin.module private @test {
    builtin.function public extern("C") @main() {
        builtin.ret;
    };
};
};
"#;

/// [`COMPONENT`] inside the world that declares it — the *shape* `--emit=hir` writes, and so
/// the shape a whole-world `.hir` file has.
///
/// Not its literal text, though: `OpPrinter for builtin::Component` prints the id **bare**
/// (`@hir_ns:test@1.0.0`) while the parser requires it **quoted**, so `--emit=hir` output
/// holding a component does not re-parse. The quoting here works around that; the defect
/// is recorded as a `TODO(hir)` on that printer.
const WORLD: &str = r#"
builtin.world {
builtin.component private @"hir_ns:test@1.0.0" {
    builtin.module private @test {
        builtin.function public extern("C") @main() {
            builtin.ret;
        };
    };
};
};
"#;

/// [`WORLD`] with an *external dependency represented in the IR* beside its component.
///
/// The sibling's one function is declared and not defined — an empty body is what
/// `Symbol::is_declaration` keys on, and therefore what `is_declaration_only` asks about.
///
/// The braces are empty rather than absent on purpose. `builtin.function` carries the
/// `SingleRegion` trait, so a function written with no region at all fails verification
/// ("requires exactly one region, but got 0") even though that is precisely how the printer
/// writes a declaration. An empty region is a body with no blocks, which is what
/// `is_declaration` means.
const WORLD_WITH_DECLARATION_ONLY_SIBLING: &str = r#"
builtin.world {
builtin.component private @"hir_ns:test@1.0.0" {
    builtin.module private @test {
        builtin.function public extern("C") @main() {
            builtin.ret;
        };
    };
};
builtin.module public @external_dep {
    builtin.function public extern("C") @sibling() {
    };
};
};
"#;

/// [`WORLD`] with a *supporting module* beside its component.
///
/// Identical to [`WORLD_WITH_DECLARATION_ONLY_SIBLING`] but for the sibling's body, which is
/// the single bit `is_declaration_only` decides on — and, since this one defines a function
/// and owns no memory, the fixture every "translated beside the component" assertion rests on.
const WORLD_WITH_SUPPORTING_SIBLING: &str = r#"
builtin.world {
builtin.component private @"hir_ns:test@1.0.0" {
    builtin.module private @test {
        builtin.function public extern("C") @main() {
            builtin.ret;
        };
    };
};
builtin.module public @supporting {
    builtin.function public extern("C") @sibling() {
        builtin.ret;
    };
};
};
"#;

/// [`WORLD_WITH_SUPPORTING_SIBLING`] whose component *calls* the sibling beside it.
///
/// Written out rather than derived, because the call is what it exists for: it is the only
/// fixture here with a cross-item invocation for the assembler's linker to resolve.
///
/// The `@test` module is `public` because its `main` is this component's whole interface: the
/// assembled package surface is derived from the root's public submodules, so a private module
/// here would leave the library with no exports at all.
const WORLD_CALLING_ITS_SUPPORTING_SIBLING: &str = r#"
builtin.world {
builtin.component private @"hir_ns:test@1.0.0" {
    builtin.module public @test {
        builtin.function public extern("C") @main() {
            hir.exec ::@supporting::@sibling() : extern("C") () -> ();
            builtin.ret;
        };
    };
};
builtin.module public @supporting {
    builtin.function public extern("C") @sibling() {
        builtin.ret;
    };
};
};
"#;

/// [`WORLD_WITH_SUPPORTING_SIBLING`] whose sibling also *defines a global variable*.
///
/// Derived from the shared fixture rather than written out, so the only thing that can differ
/// between "translated beside the component" and "diagnosed" is the item this inserts —
/// exactly the bit `module_owns_memory` decides on.
fn world_with_a_sibling_defining_a_global() -> String {
    WORLD_WITH_SUPPORTING_SIBLING.replace(
        "builtin.module public @supporting {",
        "builtin.module public @supporting {\n        builtin.global_variable public @g : i32 \
         {\n            builtin.ret_imm 1 : i32;\n        };",
    )
}

/// [`WORLD_WITH_SUPPORTING_SIBLING`] whose sibling *declares* a global rather than defining
/// one.
///
/// The discriminating half of [`world_with_a_sibling_defining_a_global`]: a declaration
/// contributes nothing to the data layout — `Linker::link` skips it — but it is still an item
/// a component owns, so the rule treats it the same way.
fn world_with_a_sibling_declaring_a_global() -> String {
    WORLD_WITH_SUPPORTING_SIBLING.replace(
        "builtin.module public @supporting {",
        "builtin.module public @supporting {\n        builtin.global_variable public @g : i32;",
    )
}

/// [`WORLD_WITH_SUPPORTING_SIBLING`] with the identity `frontend/wasm` gives the component it
/// wraps around a core Wasm module.
///
/// The id alone does not make it the wrapper — see [`mark_as_synthetic_wrapper`], which is what
/// a fixture standing in for the wrapper has to be put through as well.
fn wrapper_world_with_a_supporting_sibling() -> String {
    WORLD_WITH_SUPPORTING_SIBLING.replace("hir_ns:test@1.0.0", "root_ns:root@1.0.0")
}

/// Mark `world`'s component as one the compiler invented, the way `frontend/wasm` marks the
/// wrapper it builds around a bare core Wasm module.
///
/// This cannot be written into a fixture: `builtin.component`'s textual form carries no
/// attributes, so the marker has to be set on the parsed IR — which is where the frontend sets
/// it too, rather than on any text.
fn mark_as_synthetic_wrapper(world: builtin::WorldRef) {
    let mut component = {
        let world = world.borrow();
        let body = world.body();
        let component = body
            .entry()
            .body()
            .iter()
            .find_map(|op| op.as_operation_ref().try_downcast_op::<builtin::Component>().ok());
        component.expect("the fixture must declare a component to mark")
    };
    component.borrow_mut().mark_synthetic_wrapper();
}

/// [`WORLD_WITH_SUPPORTING_SIBLING`] whose supporting module also holds a function with **no
/// body**.
///
/// Derived from the shared fixture, so the sibling still owns no memory and still defines
/// `@sibling` — it is therefore *not* declaration-only, and `classify_siblings` hands it over to
/// be translated. The inserted function is the only difference, and the only thing that can make
/// the lowering fail.
///
/// A module whose functions are *all* body-less is a different case entirely: that is
/// [`WORLD_WITH_DECLARATION_ONLY_SIBLING`], an external dependency represented in the IR, which
/// is dropped before it ever reaches codegen. Only a module that mixes the two gets here.
fn world_with_a_sibling_declaring_one_of_its_functions() -> String {
    WORLD_WITH_SUPPORTING_SIBLING.replace(
        "builtin.module public @supporting {",
        "builtin.module public @supporting {\n        builtin.function public extern(\"C\") \
         @undefined() {\n        };",
    )
}

/// [`COMPONENT`] whose module also holds a function with **no body**.
///
/// The component-path counterpart of
/// [`world_with_a_sibling_declaring_one_of_its_functions`], derived from the very fixture that
/// `a_component_lowers_rooted_at_its_own_id` lowers successfully, so the inserted function is
/// again the only difference. See [`WORLD_WITH_DECLARATION_ONLY_SIBLING`] on why the braces are
/// empty rather than absent.
fn component_with_a_declared_function_in_its_module() -> String {
    COMPONENT.replace(
        "builtin.module private @test {",
        "builtin.module private @test {\n        builtin.function public extern(\"C\") \
         @undefined() {\n        };",
    )
}

/// [`COMPONENT`] with a body-less function of its own, beside its module rather than inside it.
///
/// A component may declare functions directly — `Component`'s own docs call them
/// "component-level functions, e.g. a program entrypoint" — and those reach
/// `MasmComponentBuilder::define_function` rather than `MasmModuleBuilder::define_function`,
/// which is the *other* caller of `MasmFunctionBuilder::new`.
fn component_with_a_declared_component_level_function() -> String {
    COMPONENT.replace(
        "builtin.module private @test {",
        "builtin.function public extern(\"C\") @undefined() {\n    };\n    builtin.module private \
         @test {",
    )
}

/// Define a data segment in `module`.
///
/// Built rather than written, because a `builtin.segment` **cannot be written in `.hir` text
/// at all**: its `offset` is a `U32Attr`, and an attribute dictionary parses its values with
/// `Type::Unknown` (`Parser::parse_attribute_dict`), for which an integer literal falls
/// through to "invalid attribute value" (`hir/src/ir/parse/parser.rs:412`). Recorded here
/// rather than only in a report, since the next person to want a segment fixture will land on
/// the same wall.
fn define_a_data_segment_in(module: builtin::ModuleRef) {
    use midenc_hir::dialects::builtin::ModuleBuilder;

    ModuleBuilder::new(module)
        .define_data_segment(1024, alloc::vec![1u8, 2, 3, 4], true, SourceSpan::default())
        .expect("should define a data segment");
}

/// The top-level module of `world` named `name`.
fn top_level_module(world: &builtin::World, name: &str) -> builtin::ModuleRef {
    world
        .body()
        .entry()
        .body()
        .iter()
        .find_map(|op| {
            op.as_operation_ref()
                .try_downcast_op::<builtin::Module>()
                .ok()
                .filter(|module| module.borrow().name().as_str() == name)
        })
        .unwrap_or_else(|| panic!("the fixture declares a top-level module named '{name}'"))
}

/// The paths of `component`'s modules, in the order lowering defined them.
fn module_paths(component: &MasmComponent) -> Vec<String> {
    component.modules.iter().map(|module| module.path().to_string()).collect()
}

/// A world declaring two components, which is what this crate does not implement.
const TWO_COMPONENT_WORLD: &str = r#"
builtin.world {
builtin.component private @"hir_ns:first@1.0.0" {
    builtin.module private @first {
        builtin.function public extern("C") @main() {
            builtin.ret;
        };
    };
};
builtin.component private @"hir_ns:second@1.0.0" {
    builtin.module private @second {
        builtin.function public extern("C") @other() {
            builtin.ret;
        };
    };
};
};
"#;

/// A bare `builtin.module`, which the parser likewise anchors at a world of its own.
///
/// That world holds no component at all, which is the shape `frontend/masm`'s disassembler
/// produces — `declare_modules` defines modules directly on the world — and therefore the
/// live path this change must leave alone.
const MODULE: &str = r#"
builtin.module public @lib {
builtin.function public extern("C") @main() {
    builtin.ret;
};
};
"#;

/// [`MODULE`] twice over: a component-less world declaring **several** top-level modules.
///
/// Derived from the shared fixture rather than written out, so it cannot drift from the
/// single-module shape it is the counterpart of — the same construction
/// `a_hir_root_declaring_several_top_level_modules_declares_nothing` uses in
/// `midenc-compile/src/pipeline/prepare.rs`, which is the preparation half of the same
/// question.
fn two_module_world() -> String {
    format!("builtin.world {{{}{}}};\n", MODULE, MODULE.replace("@lib", "@second"))
}

/// A library target whose namespace is `namespace`, as [`MasmComponent::source_inputs`]
/// receives one.
fn library_target(namespace: &str) -> midenc_session::miden_project::Target {
    midenc_session::miden_project::Target::library(
        Arc::<masm::Path>::from(
            masm::LibraryPath::new(namespace)
                .unwrap()
                .to_absolute()
                .unwrap()
                .into_owned()
                .into_boxed_path(),
        ),
        Uri::new("lib.hir"),
    )
}

/// Parse `text`, returning the top-level operation it holds.
///
/// `verify: true` matches what the `.hir` frontend does, since HIR that arrives as text has
/// not been through any of the builders that maintain the IR's invariants.
fn parse(context: &Rc<Context>, text: &str) -> OperationRef {
    let config = midenc_hir::parse::ParserConfig {
        context: context.clone(),
        verify: true,
    };
    midenc_hir::parse::parse_any(config, Uri::new("test.hir"), text)
        .expect("the fixture should parse")
}

/// Lower `world`, as `pipeline::backend::codegen` does when extraction named no component:
/// the analysis manager is rooted at the world, not at anything inside it.
fn lower_world(world: builtin::WorldRef) -> Result<MasmComponent, Report> {
    let analysis_manager = AnalysisManager::new(world.as_operation_ref(), None);
    let world = world.borrow();
    world.to_masm_component(analysis_manager)
}

/// Lower `text`, whose top-level operation must be a `builtin.component`, as
/// `pipeline::backend::codegen` does when extraction *did* name a component: through the
/// component impl directly, with the world the parser anchored it at never consulted. This is
/// the path every WebAssembly and Rust build takes.
fn lower_component(context: &Rc<Context>, text: &str) -> Result<MasmComponent, Report> {
    let op = parse(context, text);
    let component = op
        .try_downcast_op::<builtin::Component>()
        .unwrap_or_else(|_| panic!("the fixture should parse as a component"));
    let analysis_manager = AnalysisManager::new(op, None);
    component.borrow().to_masm_component(analysis_manager)
}

/// Parse `text`, whose top-level operation must be a `builtin.world`.
fn parse_world(context: &Rc<Context>, text: &str) -> builtin::WorldRef {
    parse(context, text)
        .try_downcast_op::<builtin::World>()
        .unwrap_or_else(|_| panic!("the fixture should parse as a world"))
}

/// The world the parser anchored `op` at.
///
/// Only for fixtures whose top-level operation is *not* a world; one that is comes back as
/// the root, with nothing above it. See [`parse_world`].
fn anchoring_world(op: OperationRef) -> builtin::WorldRef {
    op.parent_op()
        .expect("the parser anchors every non-world top-level operation at a world it creates")
        .try_downcast_op::<builtin::World>()
        .unwrap_or_else(|_| panic!("and that anchor is a world"))
}

/// A context whose session captures its diagnostics instead of printing them.
///
/// Needed because the sibling stub's whole observable behaviour is a *warning*: it must not
/// fail the build and must not be silent, and neither half is checkable against a session
/// that writes to stderr.
fn capturing_context() -> (Rc<Context>, alloc::sync::Arc<CaptureEmitter>) {
    let emitter = alloc::sync::Arc::new(CaptureEmitter::new());
    let options = alloc::boxed::Box::new(Options::default());
    let source_manager = alloc::sync::Arc::new(DefaultSourceManager::default());
    let session = Session::new(InputFile::empty(), options, Some(emitter.clone()), source_manager)
        .expect("should build a session");
    (Rc::new(Context::new(Rc::new(session))), emitter)
}

/// Everything a caller can observe about a lowered component, as one comparable value.
fn summarize(component: &MasmComponent) -> String {
    format!(
        "id: {:?}\nroot: {}\ninit: {:?}\nentrypoint: {:?}\nheap_base: {}\nstack_pointer: \
         {:?}\nrodata: {:?}\n{component}",
        component.id.as_ref().map(|id| id.to_string()),
        component.root,
        component.init,
        component.entrypoint,
        component.heap_base,
        component.stack_pointer,
        component.rodata,
    )
}

/// A world holding a single component lowers to exactly what that component lowers to.
///
/// The defect: the world's *own* operation used to be handed to
/// [`MasmComponentBuilder::build`], which walks a component *body* and accepts only
/// modules, interfaces and functions — so it panicked with "invalid component-level
/// operation: 'builtin.component' is not supported in a component body" on the first
/// component it met.
///
/// The equality is the point, and it is why the fix delegates rather than reimplements: a
/// component is what a Miden package is rooted at, so the world around it must not change
/// the answer.
///
/// The world here is parsed from `.hir` text rather than taken from the parser's anchor, so
/// the world under test is the one the file declares — the shape `--emit=hir` writes. See
/// [`WORLD`] for why the id is quoted here but is not in what `--emit=hir` actually prints.
#[test]
fn a_world_holding_one_component_lowers_as_that_component() {
    let context = Rc::new(Context::default());
    let from_world =
        lower_world(parse_world(&context, WORLD)).expect("a single-component world lowers");

    // A second context, so that neither lowering can be reading anything the other cached.
    let context = Rc::new(Context::default());
    let op = parse(&context, COMPONENT);
    let component = op
        .try_downcast_op::<builtin::Component>()
        .unwrap_or_else(|_| panic!("the fixture parses as a component"));
    let analysis_manager = AnalysisManager::new(op, None);
    let from_component = component
        .borrow()
        .to_masm_component(analysis_manager)
        .expect("and so does the component on its own");

    assert_eq!(
        summarize(&from_world),
        summarize(&from_component),
        "a world holding one component must lower to what that component lowers to"
    );
}

/// And a component on its own still lowers rooted at its own id.
///
/// The discriminating half. Without it the equality above could be satisfied by breaking
/// the *component* path to match the world's — no id, a root taken from the enclosing
/// namespace — which is the path every Wasm, Rust and manifest build takes.
#[test]
fn a_component_lowers_rooted_at_its_own_id() {
    let context = Rc::new(Context::default());
    let op = parse(&context, COMPONENT);
    let component = op
        .try_downcast_op::<builtin::Component>()
        .unwrap_or_else(|_| panic!("the fixture parses as a component"));
    let analysis_manager = AnalysisManager::new(op, None);
    let lowered = component
        .borrow()
        .to_masm_component(analysis_manager)
        .expect("the component lowers");

    let id = lowered.id.as_ref().expect("a component knows its own id");
    assert_eq!(id.to_string(), "hir_ns:test@1.0.0");
    assert_eq!(
        lowered.root.to_string(),
        "::\"hir_ns:test@1.0.0\"",
        "a component's Miden Assembly is rooted at its id, as one quoted path component"
    );
    assert!(
        format!("{lowered}").contains("main"),
        "and its function must have been lowered: {lowered}"
    );
}

/// A world declaring more than one component is reported, not merged and not panicked on.
#[test]
fn a_world_declaring_two_components_is_reported_as_unimplemented() {
    let context = Rc::new(Context::default());
    let op = parse(&context, TWO_COMPONENT_WORLD);
    let world = op
        .try_downcast_op::<builtin::World>()
        .unwrap_or_else(|_| panic!("the fixture parses as a world"));
    let err = lower_world(world)
        .err()
        .expect("lowering two components into one package is not implemented");

    let msg = format!("{err}");
    assert!(
        msg.contains("lowering a world containing 2 components"),
        "the report must say what it found, and how many of them: {msg}"
    );
    assert!(
        msg.contains("not yet implemented"),
        "and must read as a limitation of the compiler rather than a malformed input: {msg}"
    );
}

/// A declaration-only sibling is ignored entirely, and changes nothing about the result.
///
/// This is an *external dependency represented in the IR* — normal, expected, and worth
/// nothing to code generation. The assertion is the strong one: the world lowers to exactly
/// what the same component lowers to with no sibling at all, so the sibling cannot have
/// leaked into the output. And nothing is warned about, because warning here would make the
/// ordinary case noisy.
#[test]
fn a_declaration_only_sibling_is_ignored() {
    let (context, emitter) = capturing_context();
    let world = parse_world(&context, WORLD_WITH_DECLARATION_ONLY_SIBLING);
    let with_sibling = lower_world(world).expect("a declaration-only sibling must not fail");

    let context = Rc::new(Context::default());
    let alone = lower_world(parse_world(&context, WORLD)).expect("and neither must its absence");

    assert_eq!(
        summarize(&with_sibling),
        summarize(&alone),
        "a sibling that only declares symbols contributes no Miden Assembly"
    );
    assert!(
        emitter.captured().is_empty(),
        "and it is ignored by design, so it must not be reported: {}",
        emitter.captured()
    );
}

/// A sibling module that owns no memory is translated 1:1, beside the component, silently.
///
/// This is the *supporting module* half of what a world's siblings can be: it is emitted as an
/// ordinary Miden Assembly module and handed to the assembler as an ad-hoc support module,
/// which is what [`MasmComponent::source_inputs`] does with every module that is not the root.
///
/// Silence is asserted as strongly as translation. The previous behaviour warned and dropped
/// it, and a build that now translates the module but still warns about it would be telling
/// the reader something false.
#[test]
fn a_sibling_module_owning_no_memory_is_translated_beside_the_component() {
    let (context, emitter) = capturing_context();
    let world = parse_world(&context, WORLD_WITH_SUPPORTING_SIBLING);

    let lowered =
        lower_world(world).expect("a supporting module beside a component must not fail the build");
    assert!(
        !context.session().diagnostics.has_errors(),
        "and must not be reported as an error either"
    );
    assert!(
        emitter.captured().is_empty(),
        "a supporting module is translated now, so there is nothing to report: {}",
        emitter.captured()
    );

    // The component itself is untouched by the sibling beside it.
    assert_eq!(
        lowered.id.as_ref().map(|id| id.to_string()).as_deref(),
        Some("hir_ns:test@1.0.0")
    );
    assert_eq!(lowered.root.to_string(), "::\"hir_ns:test@1.0.0\"");

    assert_eq!(
        module_paths(&lowered),
        vec!["::\"hir_ns:test@1.0.0\"", "::\"hir_ns:test@1.0.0\"::test", "::supporting"],
        "the sibling is a top-level module of its own, not a child of the component"
    );
    assert!(
        format!("{lowered}").contains("sibling"),
        "and its procedure must have been lowered: {lowered}"
    );

    // And it reaches the assembler as a support module, which is what makes it linkable.
    let target = library_target("hir_ns:test@1.0.0");
    let sources = lowered
        .source_inputs(&target, context.session())
        .expect("its source inputs are what the assembler is handed");
    assert_eq!(sources.root.path().to_string(), "::\"hir_ns:test@1.0.0\"");
    assert_eq!(
        sources
            .support
            .iter()
            .map(|module| module.path().to_string())
            .collect::<Vec<_>>(),
        vec!["::\"hir_ns:test@1.0.0\"::test", "::supporting"],
    );
}

/// A sibling module that *defines* a global variable is diagnosed, not translated.
///
/// A global variable is owned by a component: the component is what lays out memory for it
/// and emits the code that initializes it. A module at the top level of a world has no parent
/// component, so there is nothing to own its memory — and `Linker::link` cannot see it from
/// the component either, so translating it would lay its globals over the component's.
#[test]
fn a_sibling_module_defining_a_global_variable_is_diagnosed() {
    let (context, emitter) = capturing_context();
    let world = parse_world(&context, &world_with_a_sibling_defining_a_global());

    let lowered = lower_world(world).expect("and it must still not fail the build");
    assert!(
        !context.session().diagnostics.has_errors(),
        "nor be reported as an error, which would reject a legitimate world"
    );

    let captured = emitter.captured();
    assert!(
        captured.contains("global variable"),
        "the report must name the rule, not merely refuse: {captured}"
    );
    assert!(
        captured.contains("component"),
        "and must say who owns those items, which is what teaches the rule: {captured}"
    );
    assert!(
        captured.contains("supporting"),
        "and must name the item it left out: {captured}"
    );

    assert_eq!(
        module_paths(&lowered),
        vec!["::\"hir_ns:test@1.0.0\"", "::\"hir_ns:test@1.0.0\"::test"],
        "a module that owns memory is left out of the generated package: {lowered}"
    );
}

/// A sibling module that only *declares* a global variable is diagnosed too.
///
/// The discriminating half of the test above, and the one place this predicate is
/// deliberately stricter than `Linker::link`, which skips declarations when computing the
/// layout. A declaration whose definition lives elsewhere is not in the component's layout, so
/// lowering a use of it would panic in `GlobalVariableLayout::get_computed_addr`; and it is a
/// global variable declared by a module with no parent component either way, which is the rule.
#[test]
fn a_sibling_module_declaring_a_global_variable_is_diagnosed() {
    let (context, emitter) = capturing_context();
    let world = parse_world(&context, &world_with_a_sibling_declaring_a_global());

    let lowered = lower_world(world).expect("and it must still not fail the build");

    assert!(
        emitter.captured().contains("global variable"),
        "a declared global is still a global a component owns: {}",
        emitter.captured()
    );
    assert_eq!(
        module_paths(&lowered),
        vec!["::\"hir_ns:test@1.0.0\"", "::\"hir_ns:test@1.0.0\"::test"],
    );
}

/// A sibling module declaring a data segment is diagnosed, for the same reason.
///
/// Data segments are the other half of what a component owns, and the other half of what
/// `Linker::link` scans a component's modules for.
#[test]
fn a_sibling_module_declaring_a_data_segment_is_diagnosed() {
    let (context, emitter) = capturing_context();
    let world = parse_world(&context, WORLD_WITH_SUPPORTING_SIBLING);
    define_a_data_segment_in(top_level_module(&world.borrow(), "supporting"));

    let lowered = lower_world(world).expect("and it must still not fail the build");
    assert!(!context.session().diagnostics.has_errors());

    let captured = emitter.captured();
    assert!(
        captured.contains("data segment"),
        "the report must name the rule, not merely refuse: {captured}"
    );
    assert!(captured.contains("component"), "and must say who owns those items: {captured}");

    assert_eq!(
        module_paths(&lowered),
        vec!["::\"hir_ns:test@1.0.0\"", "::\"hir_ns:test@1.0.0\"::test"],
        "a module that owns memory is left out of the generated package: {lowered}"
    );
}

/// A supporting sibling does **not** move when the component beside it is re-rooted.
///
/// A component-less world is re-rooted at its target's namespace, and so is a world holding
/// the synthetic wrapper `frontend/wasm` builds — see
/// [`MasmComponent::has_no_authored_identity`]. That rewrite replaces a root *the compiler
/// invented*, and it applies to the modules nested under it. A top-level sibling is not one of
/// them: its path is a name the source declares, and lowering defines it top-level rather than
/// under the component's root, so `Rebase` leaves it exactly where it is.
///
/// What must move with the root is anything the sibling *calls* inside the component, which is
/// the same walk and needs nothing extra here; this fixture has no such call, so the assertion
/// is about the sibling's own path.
#[test]
fn a_supporting_sibling_does_not_move_when_the_component_is_re_rooted() {
    let context = Rc::new(Context::default());
    let world = parse_world(&context, &wrapper_world_with_a_supporting_sibling());
    mark_as_synthetic_wrapper(world);
    let lowered = lower_world(world).expect("a wrapper world with a sibling lowers");
    assert_eq!(
        lowered.root.to_string(),
        "::\"root_ns:root@1.0.0\"",
        "the fixture must really be the wrapper, or this test is about some other case"
    );

    let target = library_target("::example");
    let sources = lowered
        .source_inputs(&target, context.session())
        .expect("its source inputs are what the assembler is handed");

    assert_eq!(sources.root.path(), target.namespace.inner().as_ref());
    assert_eq!(
        sources
            .support
            .iter()
            .map(|module| module.path().to_string())
            .collect::<Vec<_>>(),
        vec!["::example::test", "::supporting"],
        "the component's own modules move with its root; the sibling, whose name the source \
         declares and which was never under that root, does not"
    );
}

/// And the component and its supporting sibling assemble, together.
///
/// A module that reaches `support` but cannot be linked would be a worse outcome than the
/// warning this replaces, so this goes as far as building a real library: the assembler parses
/// every module, resolves every recorded callee, and compiles the result to MAST.
///
/// The fixture's component *calls* the sibling, which is what makes this about linking rather
/// than about parsing. A supporting module the assembler accepted but could not resolve a call
/// into would satisfy every assertion above this one and fail every real build.
///
/// The second half is what makes the first half mean something: with the very same root and
/// the sibling withheld, the same assembly must *fail*. Without it, a call the assembler had
/// quietly dropped would look exactly like a call it had resolved.
#[test]
fn a_component_and_its_supporting_sibling_assemble() {
    /// The assembler `MasmComponent::source_inputs` feeds, with the compiler intrinsics
    /// linked as every real build links them.
    fn assembler(session: &Session) -> miden_assembly::Assembler {
        let mut assembler = miden_assembly::Assembler::new(session.source_manager.clone());
        assembler
            .link_package(crate::intrinsics::load(), miden_assembly::Linkage::Static)
            .expect("the compiler intrinsics should link");
        assembler
    }

    let context = Rc::new(Context::default());
    let world = parse_world(&context, WORLD_CALLING_ITS_SUPPORTING_SIBLING);
    let lowered = lower_world(world).expect("a supporting module beside a component lowers");
    let target = library_target("hir_ns:test@1.0.0");

    let sources = lowered
        .source_inputs(&target, context.session())
        .expect("its source inputs are what the assembler is handed");
    assert!(
        sources.support.iter().any(|module| module.path() == "::supporting"),
        "the supporting module must be among the sources, or neither half proves anything"
    );
    assembler(context.session())
        .assemble_library("hir_ns:test@1.0.0", sources.root, sources.support)
        .unwrap_or_else(|err| {
            panic!("a component and its supporting sibling should assemble: {err}")
        });

    // The discriminating half: the same root, without the module it calls into.
    let sources = lowered
        .source_inputs(&target, context.session())
        .expect("its source inputs are what the assembler is handed");
    let withheld = sources
        .support
        .into_iter()
        .filter(|module| module.path() != "::supporting")
        .collect::<Vec<_>>();
    let err = assembler(context.session())
        .assemble_library("hir_ns:test@1.0.0", sources.root, withheld)
        .expect_err("without the supporting module, the component's call cannot resolve");
    assert!(
        format!("{err}").contains("undefined"),
        "and it must fail for that reason, not some other: {err}"
    );
}

/// A function that reaches codegen with no body is **invalid input**, on the world path.
///
/// Not an unsupported case: nothing could ever supply the missing definition. A body-less
/// function names a procedure whose implementation is expected from elsewhere, and Miden
/// Assembly has no later step that could provide one — so this is the input being wrong, which
/// is deliberately *unlike*
/// `a_world_declaring_two_components_is_reported_as_unimplemented`, where the compiler is the
/// thing that is incomplete. The message is asserted for that difference, not merely for
/// erroring.
///
/// Why an error rather than something to skip: a declaration nothing refers to would be removed
/// by dead symbol elimination, so one that survives this far is assumed *referenced*. Skipping
/// it would emit a module whose callers name a procedure it does not define, and the failure
/// would surface out of the assembler with nothing to point at.
///
/// The sibling here is a **mixed** module — one function defined, one not — which is what makes
/// the case reachable at all: a module whose functions are all body-less is declaration-only and
/// is dropped before codegen. Before this check, such a module was translated and then panicked
/// in `Function::entry_block`'s "cannot get entry block for declaration".
#[test]
fn a_body_less_function_in_a_supporting_sibling_is_invalid_input() {
    let (context, emitter) = capturing_context();
    let world = parse_world(&context, &world_with_a_sibling_declaring_one_of_its_functions());

    let err = lower_world(world)
        .err()
        .expect("a function with no body cannot be emitted as Miden Assembly");

    let msg = format!("{err}");
    assert!(
        msg.contains("cannot emit masm for a function with no body"),
        "the report must say what it cannot emit, and why there is nothing to emit: {msg}"
    );
    assert!(
        msg.contains("nothing can provide its definition"),
        "and must name the reason the input is invalid rather than unsupported: {msg}"
    );
    assert!(
        !msg.contains("not yet implemented"),
        "it is not a limitation of the compiler, which is what `too_many_components` is: {msg}"
    );
    assert!(
        emitter.captured().is_empty(),
        "and the module itself owns no memory, so the sibling rule has no quarrel with it and \
         must not add one: {}",
        emitter.captured()
    );

    // The discriminating half: the same sibling with that one function removed still lowers,
    // silently and beside the component, exactly as
    // `a_sibling_module_owning_no_memory_is_translated_beside_the_component` pins in full.
    let context = Rc::new(Context::default());
    let lowered = lower_world(parse_world(&context, WORLD_WITH_SUPPORTING_SIBLING))
        .expect("a sibling whose functions all have bodies is unaffected by this check");
    assert!(
        module_paths(&lowered).contains(&String::from("::supporting")),
        "and is still translated beside the component: {lowered}"
    );
}

/// The same on the component path: a function declared inside a component's own module.
///
/// The component impl is reached directly by `pipeline::backend::codegen` whenever extraction
/// named a component — every WebAssembly and Rust build — so it never passes through the world
/// impl above. That is why the check sits in `MasmFunctionBuilder::new`, the one point every
/// function reaches, rather than at either impl: the two entry points share nothing else.
#[test]
fn a_body_less_function_in_a_components_module_is_invalid_input() {
    let context = Rc::new(Context::default());
    let err = lower_component(&context, &component_with_a_declared_function_in_its_module())
        .err()
        .expect("the component path reaches the same check, by a different route");

    let msg = format!("{err}");
    assert!(
        msg.contains("cannot emit masm for a function with no body"),
        "and reports it identically, since it is the same defect in the input: {msg}"
    );

    // The discriminating half: the fixture this is derived from, unmodified, lowers — so the
    // difference is the declared function and not the route taken to it.
    let context = Rc::new(Context::default());
    let lowered = lower_component(&context, COMPONENT)
        .expect("a component whose functions all have bodies is unaffected by this check");
    assert!(
        format!("{lowered}").contains("main"),
        "and its module's procedure is emitted as before: {lowered}"
    );
}

/// And a *component-level* function with no body, which reaches the check by the third route.
///
/// `MasmComponentBuilder::define_function` handles a function declared directly by a component;
/// `MasmModuleBuilder::define_function` handles one inside a module or an interface. Both call
/// `MasmFunctionBuilder::new`, and this is the half of that claim the two tests above do not
/// cover — without it, moving the check down into the module builder would still pass them.
#[test]
fn a_body_less_component_level_function_is_invalid_input() {
    let context = Rc::new(Context::default());
    let err = lower_component(&context, &component_with_a_declared_component_level_function())
        .err()
        .expect("a component-level function with no body cannot be emitted either");

    assert!(
        format!("{err}").contains("cannot emit masm for a function with no body"),
        "whichever kind of item declares it, the answer is the same: {err}"
    );
}

/// A world holding no component at all still lowers as one logical component.
///
/// The other live path this change must not disturb: `frontend/masm`'s disassembler builds
/// exactly this shape, and `frontend/masm/tests/e2e.rs` lowers it back through this impl.
#[test]
fn a_world_of_modules_still_lowers_as_a_component_body() {
    let context = Rc::new(Context::default());
    let module = parse(&context, MODULE);
    let lowered =
        lower_world(anchoring_world(module)).expect("a world of modules lowers as it always did");

    assert!(lowered.id.is_none(), "a world declares no component id of its own");
    assert_eq!(
        lowered.root.to_string(),
        "::lib",
        "its root is the single top-level namespace it holds"
    );
    assert!(
        format!("{lowered}").contains("main"),
        "and the module's function must have been lowered: {lowered}"
    );
}

/// A component-less world's Miden Assembly is rooted at the *target's* namespace.
///
/// Lowering has no target and so cannot answer this: with several top-level modules
/// `world_body_to_masm_component` falls through to the placeholder `::init`, which is not a
/// name any source declares and which therefore no synthesized namespace can equal. Since
/// `load_target_sources` rejects a root module that does not sit exactly at its target's
/// namespace, such a build could not assemble at all. The first assertion pins that lowering
/// still produces the placeholder, which is what makes the second one about
/// [`MasmComponent::source_inputs`] rather than about lowering.
///
/// # What this does *not* claim
///
/// The second assertion pins the limitation that comes with it, so the next person reads it
/// here rather than rediscovering it. With several top-level modules the placeholder root is
/// an **empty** module and the real ones are its *siblings*, not its children —
/// `define_module` finds their absolute paths do not begin with the root and defines them
/// top-level — so moving the root moves nothing else, and they stay outside the namespace.
/// Such a build therefore still does not assemble; what it no longer does is fail on a
/// namespace no source could have produced.
///
/// TODO(codegen): decide what a world of several top-level modules should *be*. Nesting them
/// under the target's namespace would rename every procedure in them, which is not this
/// change's to do; rejecting the shape outright may well be the better answer.
#[test]
fn a_world_of_several_modules_is_rooted_at_the_target_namespace() {
    let context = Rc::new(Context::default());
    let lowered = lower_world(parse_world(&context, &two_module_world()))
        .expect("a world of several modules lowers");
    assert_eq!(
        lowered.root.to_string(),
        "::init",
        "lowering has no target to root at, so it still picks its placeholder"
    );

    let target = library_target("::example");
    let sources = lowered
        .source_inputs(&target, context.session())
        .expect("and its source inputs are what the assembler is handed");

    assert_eq!(
        sources.root.path(),
        target.namespace.inner().as_ref(),
        "a world declaring no component has no identity of its own, so its root is the namespace \
         its target names"
    );
    assert_eq!(
        sources
            .support
            .iter()
            .map(|module| module.path().to_string())
            .collect::<Vec<_>>(),
        vec!["::lib", "::second"],
        "and the modules the world declares are siblings of the placeholder rather than children \
         of it, so they do not move with it"
    );
}

/// A component-less world whose root already agrees with its target comes back unchanged.
///
/// The single-module shape, end to end from `.hir`: lowering roots at `::{module}`, and
/// preparation's `.hir` scan reads that same module's name, so the two normally agree and
/// subsuming this case into the same rule costs the common case nothing.
///
/// What this pins is the *outcome* — that nothing observable moved — which is what a caller
/// sees. It does not pin the equality guard in `MasmComponent::source_inputs`, and cannot:
/// this fixture's one procedure calls nothing, so there is no call target whose rewriting
/// would be detectable, and re-rooting to the same path is lossless anyway. The guard itself
/// is pinned by `a_component_less_world_already_at_the_target_namespace_is_left_alone` in
/// `artifact.rs`, against a fixture that does have callees.
#[test]
fn a_world_of_one_module_already_at_its_targets_namespace_is_left_alone() {
    let context = Rc::new(Context::default());
    let module = parse(&context, MODULE);
    let lowered = lower_world(anchoring_world(module)).expect("a world of one module lowers");
    let emitted = format!("{}", lowered.modules[0]);

    let target = library_target("::lib");
    assert_eq!(
        lowered.root.as_ref(),
        target.namespace.inner().as_ref(),
        "the module's own name and the target's namespace must really be the same path, or this \
         test is about some other case"
    );

    let sources = lowered
        .source_inputs(&target, context.session())
        .expect("its source inputs are what the assembler is handed");

    assert_eq!(sources.root.path(), target.namespace.inner().as_ref());
    assert_eq!(format!("{}", sources.root), emitted, "and nothing in it moved");
}

/// A world holding a component keeps that component's id, whatever its target is called.
///
/// The discriminating half of the two above, at the seam that decides it: re-rooting is
/// justified only for a component-less world, whose modules have no identity beyond the
/// namespace they sit in. An authored component id *is* the code's identity — every dependent
/// addresses its procedures through it — so a target named something else must not silently
/// rename them, and this is the shape every Wasm and Rust build produces.
#[test]
fn a_world_holding_one_component_keeps_that_components_id() {
    let context = Rc::new(Context::default());
    let lowered =
        lower_world(parse_world(&context, WORLD)).expect("a single-component world lowers");

    let target = library_target("::example");
    let sources = lowered
        .source_inputs(&target, context.session())
        .expect("its source inputs are what the assembler is handed");

    assert_eq!(
        sources.root.path().to_string(),
        "::\"hir_ns:test@1.0.0\"",
        "an authored component's root is its own library path, and a target named otherwise must \
         fail the assembler's root-module check rather than be quietly accommodated"
    );
}

#[test]
fn type_expr_from_hir_pointer_conversion_preserves_address_space() {
    for addrspace in [masm::types::AddressSpace::Byte, masm::types::AddressSpace::Element] {
        let ty = Type::from(PointerType::new_with_address_space(Type::U32, addrspace));

        let masm::TypeExpr::Ptr(ptr) = masm::TypeExpr::from(ty) else {
            panic!("expected pointer type expression");
        };
        assert_eq!(ptr.address_space(), addrspace);
    }
}

/// A component whose public interface is a component-level function, with its implementation in
/// a private module beside it.
///
/// The module's `helper` must be a *public procedure* for the cross-module `exec` to resolve,
/// which is exactly the combination the package surface must not leak: a public procedure of a
/// private submodule.
const WORLD_WITH_A_PRIVATE_MODULE_BEHIND_ITS_INTERFACE: &str = r#"
builtin.world {
builtin.component private @"hir_ns:test@1.0.0" {
    builtin.function public extern("C") @entry() {
        hir.exec ::@"hir_ns:test@1.0.0"::@test::@helper() : extern("C") () -> ();
        builtin.ret;
    };
    builtin.module private @test {
        builtin.function public extern("C") @helper() {
            builtin.ret;
        };
    };
};
};
"#;

/// A private module's public procedures are callable within the package, but are not part of
/// the assembled package's export surface: the surface is derived from the modules reachable
/// through *public* submodule declarations, and a private HIR module is declared private.
#[test]
fn a_private_module_is_not_part_of_the_package_surface() {
    let context = Rc::new(Context::default());
    let world = parse_world(&context, WORLD_WITH_A_PRIVATE_MODULE_BEHIND_ITS_INTERFACE);
    let lowered = lower_world(world).expect("a component with a private module lowers");
    let target = library_target("hir_ns:test@1.0.0");

    let sources = lowered
        .source_inputs(&target, context.session())
        .expect("its source inputs are what the assembler is handed");
    let package = miden_assembly::Assembler::new(context.session().source_manager.clone())
        .assemble_library("hir_ns:test@1.0.0", sources.root, sources.support)
        .expect("a public interface calling into a private module assembles");

    let exports = package
        .manifest
        .exports()
        .map(|export| export.path().as_ref().as_str().to_string())
        .collect::<Vec<_>>();
    assert!(
        exports.iter().any(|export| export.ends_with("entry")),
        "the component-level function is the public surface, got exports: {exports:?}"
    );
    assert!(
        !exports.iter().any(|export| export.contains("helper")),
        "a public procedure of a private module must not be exported, got exports: {exports:?}"
    );
}

/// [`WORLD_WITH_A_PRIVATE_MODULE_BEHIND_ITS_INTERFACE`] carrying the id `frontend/wasm` gives
/// the wrapper it invents around a bare core module — written by an author here, rather than
/// invented, which is the whole point: nothing marks this component synthetic.
///
/// Derived from the shared fixture rather than written out, so the component id is the only
/// thing that differs from the case beside it.
fn world_with_an_authored_root_id() -> String {
    WORLD_WITH_A_PRIVATE_MODULE_BEHIND_ITS_INTERFACE
        .replace("hir_ns:test@1.0.0", "root_ns:root@1.0.0")
}

/// The compiler's wrapper around a bare core module is recognized by a marker the frontend
/// sets, not by its id: an author may legitimately name a component `root_ns:root@1.0.0`, and
/// theirs keeps the module visibility they declared.
///
/// The discriminating half of [`a_private_module_is_not_part_of_the_package_surface`]: the same
/// fixture and the same assertion, with only the id changed. Recognizing the wrapper by
/// comparing the id forces this component's modules public, which puts `helper` — a procedure
/// its author put behind a private module — on the assembled package's export surface.
#[test]
fn an_authored_component_named_like_the_wrapper_keeps_private_modules_private() {
    let context = Rc::new(Context::default());
    let world = parse_world(&context, &world_with_an_authored_root_id());
    let lowered = lower_world(world).expect("an authored root-named component lowers");
    let target = library_target("root_ns:root@1.0.0");

    let sources = lowered
        .source_inputs(&target, context.session())
        .expect("its source inputs are what the assembler is handed");
    let package = miden_assembly::Assembler::new(context.session().source_manager.clone())
        .assemble_library("root_ns:root@1.0.0", sources.root, sources.support)
        .expect("it assembles");

    let exports = package
        .manifest
        .exports()
        .map(|export| export.path().as_ref().as_str().to_string())
        .collect::<Vec<_>>();
    assert!(
        exports.iter().any(|export| export.ends_with("entry")),
        "the component-level function is still the public surface, got exports: {exports:?}"
    );
    assert!(
        !exports.iter().any(|export| export.contains("helper")),
        "a private module of an authored component must not be exported, got: {exports:?}"
    );
}

/// `builtin.Module` permits nesting, and a nested module's procedures belong to the component
/// as much as a top-level module's. Lowering must place them at their own path rather than
/// panicking on a module it did not expect to find in a module body.
const WORLD_WITH_A_NESTED_MODULE: &str = r#"
builtin.world {
builtin.component private @"hir_ns:test@1.0.0" {
    builtin.module public @outer {
        builtin.function public extern("C") @entry() {
            builtin.ret;
        };

        builtin.module public @inner {
            builtin.function public extern("C") @nested() {
                builtin.ret;
            };
        };
    };
};
};
"#;

#[test]
fn a_nested_module_is_lowered_at_its_own_path() {
    let context = Rc::new(Context::default());
    let world = parse_world(&context, WORLD_WITH_A_NESTED_MODULE);
    let lowered = lower_world(world).expect("a component with a nested module lowers");

    let paths = lowered
        .modules
        .iter()
        .map(|module| module.path().to_string())
        .collect::<Vec<_>>();
    assert!(
        paths.iter().any(|path| path.ends_with("outer::inner")),
        "the nested module must be lowered at its own path, got: {paths:?}"
    );
}

/// [`WORLD_WITH_A_NESTED_MODULE`] with the *inner* module private.
///
/// Derived from the shared fixture rather than written out, because the inner module's
/// visibility is the only thing that differs, and it is the only thing this asks about.
fn world_with_a_private_nested_module() -> String {
    WORLD_WITH_A_NESTED_MODULE
        .replace("builtin.module public @inner", "builtin.module private @inner")
}

/// Nesting does not widen the surface. A private module's public procedures stay off the
/// assembled package's exports whether the module sits at the top level or inside another —
/// the surface is derived from the modules reachable from the root through *public* submodule
/// declarations, and a private declaration ends that walk at any depth.
///
/// The depth is what makes this worth pinning separately from
/// [`a_private_module_is_not_part_of_the_package_surface`]: nested modules are lowered by
/// recursion through `define_module`, and it is `define_module_tree` — which forces every
/// *intermediate* module public so references through it resolve — that decides which
/// declaration carries the author's visibility. A recursion that applied the visibility to the
/// wrong link of the chain would leave `nested` exported here while the top-level case still
/// passed.
#[test]
fn a_private_nested_module_is_not_part_of_the_package_surface() {
    let context = Rc::new(Context::default());
    let world = parse_world(&context, &world_with_a_private_nested_module());
    let lowered = lower_world(world).expect("a component with a private nested module lowers");
    let target = library_target("hir_ns:test@1.0.0");

    let sources = lowered
        .source_inputs(&target, context.session())
        .expect("its source inputs are what the assembler is handed");
    let package = miden_assembly::Assembler::new(context.session().source_manager.clone())
        .assemble_library("hir_ns:test@1.0.0", sources.root, sources.support)
        .expect("a public module holding a private one assembles");

    let exports = package
        .manifest
        .exports()
        .map(|export| export.path().as_ref().as_str().to_string())
        .collect::<Vec<_>>();
    assert!(
        exports.iter().any(|export| export.ends_with("entry")),
        "the public outer module's procedure is the surface, got exports: {exports:?}"
    );
    assert!(
        !exports.iter().any(|export| export.contains("nested")),
        "a public procedure of a private *nested* module must not be exported, got: {exports:?}"
    );
}

/// The shape pure core-Wasm translation produces: a public module (the artifact's interface)
/// holding a private, address-taken function in a function table.
const WORLD_WITH_A_PRIVATE_TABLE_CALLEE: &str = r#"
builtin.world {
builtin.component private @"root_ns:root@1.0.0" {
    builtin.module public @wasm {
        builtin.function private extern("C") @private_callee() {
            builtin.ret;
        };

        builtin.function_table private @tbl : 1 {
            builtin.function_table_entry 0 @private_callee tag 1;
        };

        builtin.function public extern("C") @dispatch(%index: u32) {
            hir.exec_indirect @tbl[%index] : extern("C") () -> () tag 1;
            builtin.ret;
        };
    };
};
};
"#;

/// A function whose address is taken is still private: the package's export manifest is its
/// author's interface, and an address-taken private function is not part of it. Initialization
/// must reach the callee without promoting it, which it does by emitting the `procref` in the
/// callee's own module.
#[test]
fn a_private_table_callee_is_not_part_of_the_package_surface() {
    let context = Rc::new(Context::default());
    let world = parse_world(&context, WORLD_WITH_A_PRIVATE_TABLE_CALLEE);
    let lowered = lower_world(world).expect("a component with a private table callee lowers");
    let target = library_target("root_ns:root@1.0.0");

    let sources = lowered
        .source_inputs(&target, context.session())
        .expect("its source inputs are what the assembler is handed");
    // A component owning a function table needs an `init`, and `init` opens by calling the
    // intrinsic heap initializer, so this one must link the intrinsics as a real build does.
    let mut assembler = miden_assembly::Assembler::new(context.session().source_manager.clone());
    assembler
        .link_package(crate::intrinsics::load(), miden_assembly::Linkage::Static)
        .expect("the compiler intrinsics should link");
    let package = assembler
        .assemble_library("root_ns:root@1.0.0", sources.root, sources.support)
        .expect("a table initialized from its callee's own module assembles");

    let mut exports = package
        .manifest
        .exports()
        .map(|export| export.path().as_ref().as_str().to_string())
        .collect::<Vec<_>>();
    exports.sort();

    // The whole set, not just the two memberships this is named for. Initializing a table trades
    // one symbol off the manifest for another: `@private_callee` stays the author's, while
    // `__init_function_table` — the compiler's own, and public only so `init` can reach it —
    // joins the surface. That trade is the design, so it is asserted rather than tolerated, and
    // any *third* symbol arriving on the public surface should be a test failure and not a
    // discovery made downstream.
    assert_eq!(
        exports,
        vec![
            "::\"root_ns:root@1.0.0\"::init",
            "::\"root_ns:root@1.0.0\"::wasm::__init_function_table",
            "::\"root_ns:root@1.0.0\"::wasm::dispatch",
        ],
        "the public surface is the author's `dispatch` plus the compiler's own initializers"
    );
}

/// A table entry naming a function in a declaration-only sibling.
///
/// The sibling is an external dependency represented in the IR — `@sibling` has no body — so
/// `classify_siblings` drops it and no module is ever lowered at `::external_dep`. The entry
/// still resolves and `@sibling` still has a signature, so the `hir.exec_indirect` verifier and
/// legalization both accept this: it reaches code generation intact.
const WORLD_WITH_A_TABLE_CALLEE_IN_A_DECLARATION_ONLY_SIBLING: &str = r#"
builtin.world {
builtin.component private @"hir_ns:test@1.0.0" {
    builtin.module public @wasm {
        builtin.function_table private @tbl : 1 {
            builtin.function_table_entry 0 ::@external_dep::@sibling tag 1;
        };

        builtin.function public extern("C") @dispatch(%index: u32) {
            hir.exec_indirect @tbl[%index] : extern("C") () -> () tag 1;
            builtin.ret;
        };
    };
};
builtin.module public @external_dep {
    builtin.function public extern("C") @sibling() {
    };
};
};
"#;

/// A table entry whose callee has no definition to take the address of is invalid input, and is
/// diagnosed as such.
///
/// This is the shape that used to panic. Everything upstream accepts it — the entry resolves and
/// the callee's signature matches the call site's, which is all the verifier asks — so the first
/// thing a producer of this IR saw was a `.expect` firing inside the slot-filling code, with a
/// message written for a compiler bug rather than for them.
#[test]
fn a_table_callee_in_an_unlowered_module_is_invalid_input() {
    let context = Rc::new(Context::default());
    let world = parse_world(&context, WORLD_WITH_A_TABLE_CALLEE_IN_A_DECLARATION_ONLY_SIBLING);
    let err = lower_world(world)
        .err()
        .expect("a table entry naming a callee with no definition must not lower");
    let err = err.to_string();

    assert!(
        err.contains("sibling") && err.contains("external_dep"),
        "the diagnostic must name the callee and the module it was expected in, got: {err}"
    );
    assert!(
        err.contains("only declarations"),
        "the diagnostic must say why there was no module to attach the code to, got: {err}"
    );
}

/// A component whose table callees are spread across three modules, one of them nested.
///
/// `@a` declares the only table, and its three slots name callees in three different modules —
/// so the slot-filling code for a *single* table is split three ways, which is the case grouping
/// by callee rather than by table exists for. `@outer` defines a callee of its own, which makes
/// it the nearest fragment-bearing ancestor of `@inner` and so the module whose initializer has
/// to reach it.
const WORLD_WITH_TABLE_CALLEES_ACROSS_MODULES: &str = r#"
builtin.world {
builtin.component private @"hir_ns:test@1.0.0" {
    builtin.module public @a {
        builtin.function private extern("C") @callee_a() {
            builtin.ret;
        };

        builtin.function_table private @tbl : 3 {
            builtin.function_table_entry 0 @callee_a tag 1;
            builtin.function_table_entry 1 ::@"hir_ns:test@1.0.0"::@outer::@callee_b tag 1;
            builtin.function_table_entry 2 ::@"hir_ns:test@1.0.0"::@outer::@inner::@callee_c tag 1;
        };

        builtin.function public extern("C") @dispatch(%index: u32) {
            hir.exec_indirect @tbl[%index] : extern("C") () -> () tag 1;
            builtin.ret;
        };
    };

    builtin.module public @outer {
        builtin.function private extern("C") @callee_b() {
            builtin.ret;
        };

        builtin.module public @inner {
            builtin.function private extern("C") @callee_c() {
                builtin.ret;
            };
        };
    };
};
};
"#;

/// A table whose only slot is written twice: a dead entry naming a callee in `@z`, then the live
/// entry naming `@live` in `@a`.
///
/// The table's documented rule is last-entry-per-slot-wins, so slot 0 holds `@live` and nothing
/// else. The module names are what make this discriminating: slot-filling code is grouped by the
/// callee's defining module and those groups run in path order, so `@a` runs before `@z` and a
/// codegen that emitted *both* stores would leave `@dead`'s MAST root in the slot — the entry the
/// `hir.exec_indirect` verifier never looked at, with a signature it never checked.
const WORLD_WITH_AN_OVERWRITTEN_TABLE_SLOT: &str = r#"
builtin.world {
builtin.component private @"hir_ns:test@1.0.0" {
    builtin.module public @a {
        builtin.function private extern("C") @live() {
            builtin.ret;
        };

        builtin.function_table private @tbl : 1 {
            builtin.function_table_entry 0 ::@"hir_ns:test@1.0.0"::@z::@dead tag 1;
            builtin.function_table_entry 0 @live tag 1;
        };

        builtin.function public extern("C") @dispatch(%index: u32) {
            hir.exec_indirect @tbl[%index] : extern("C") () -> () tag 1;
            builtin.ret;
        };
    };

    builtin.module public @z {
        builtin.function private extern("C") @dead(%v: u32) {
            builtin.ret;
        };
    };
};
};
"#;

/// The target of every `procref` in `body`, in order.
fn procrefs_in(body: &masm::Block) -> Vec<String> {
    body.iter()
        .filter_map(|op| match op {
            masm::Op::Inst(inst) => match inst.inner() {
                masm::Instruction::ProcRef(masm::InvocationTarget::Path(path)) => {
                    Some(path.inner().as_str().to_string())
                }
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// An entry a later entry overwrites is dead, and dead entries are not written to their slot.
///
/// This is a soundness property, not a size optimization. The `hir.exec_indirect` verifier checks
/// only the entry that wins a slot, so a dead entry's signature is never compared against the call
/// site's — writing its MAST root into the slot anyway would let a callee with a different stack
/// contract run behind a tag the runtime check accepts, which is exactly the type confusion the
/// tag exists to prevent.
#[test]
fn a_dead_table_entry_is_not_written_to_its_slot() {
    let context = Rc::new(Context::default());
    let world = parse_world(&context, WORLD_WITH_AN_OVERWRITTEN_TABLE_SLOT);
    let lowered = lower_world(world).expect("a component with an overwritten table slot lowers");

    let mut procrefs = Vec::new();
    for module in lowered.modules.iter() {
        for procedure in module.procedures() {
            if procedure.name().as_str() == super::FUNCTION_TABLE_INIT_PROC {
                procrefs.extend(procrefs_in(procedure.body()));
            }
        }
    }
    procrefs.sort();

    assert_eq!(
        procrefs,
        vec!["::\"hir_ns:test@1.0.0\"::a::live"],
        "only the entry that wins the slot may be written to it"
    );

    // The dead entry was the sole reason `@z` would have carried an initializer at all, so its
    // absence is the same fact stated where a reader will notice it first.
    let owners = lowered
        .modules
        .iter()
        .filter(|module| {
            module
                .procedures()
                .any(|proc| proc.name().as_str() == super::FUNCTION_TABLE_INIT_PROC)
        })
        .map(|module| module.path().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        owners,
        vec!["::\"hir_ns:test@1.0.0\"::a"],
        "a module whose only table entry is dead defines no initializer"
    );
}

/// The path of the module whose `__init_function_table` each `exec` in `body` names, in order.
///
/// Only the generated initializers are of interest, so anything else `body` invokes — the heap
/// intrinsic `init` opens with, most of all — is skipped.
fn function_table_initializers_invoked(body: &masm::Block) -> Vec<String> {
    let suffix = format!("::{}", super::FUNCTION_TABLE_INIT_PROC);
    body.iter()
        .filter_map(|op| match op {
            masm::Op::Inst(inst) => match inst.inner() {
                masm::Instruction::Exec(masm::InvocationTarget::Path(path)) => {
                    path.inner().as_str().strip_suffix(&suffix).map(String::from)
                }
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// Every module defining a table callee initializes the slots of the callees *it* defines, and
/// is invoked exactly once — by its nearest enclosing initializer if it has one, and by the
/// component's `init` otherwise.
///
/// Both halves matter and neither implies the other. A module reached twice writes its slots
/// twice, which is merely wasteful; a module reached by nobody leaves its slots as the zero word,
/// and `dynexec` on a zero slot traps at runtime — a whole table's worth of dispatch failing for
/// a callee that happened to live one module over.
#[test]
fn every_module_defining_a_table_callee_is_initialized_exactly_once() {
    let context = Rc::new(Context::default());
    let world = parse_world(&context, WORLD_WITH_TABLE_CALLEES_ACROSS_MODULES);
    let lowered = lower_world(world).expect("a component with table callees across modules lowers");

    // The three modules defining a callee each carry an initializer, and no other module does
    let mut owners = lowered
        .modules
        .iter()
        .filter(|module| {
            module
                .procedures()
                .any(|proc| proc.name().as_str() == super::FUNCTION_TABLE_INIT_PROC)
        })
        .map(|module| module.path().to_string())
        .collect::<Vec<_>>();
    owners.sort();
    assert_eq!(
        owners,
        vec![
            "::\"hir_ns:test@1.0.0\"::a",
            "::\"hir_ns:test@1.0.0\"::outer",
            "::\"hir_ns:test@1.0.0\"::outer::inner",
        ],
        "a module defines an initializer if and only if it defines a table callee"
    );

    // ...and each is invoked exactly once across `init` and every initializer
    let mut invoked = Vec::new();
    for module in lowered.modules.iter() {
        for procedure in module.procedures() {
            if matches!(procedure.name().as_str(), "init" | super::FUNCTION_TABLE_INIT_PROC) {
                invoked.extend(function_table_initializers_invoked(procedure.body()));
            }
        }
    }
    invoked.sort();
    assert_eq!(invoked, owners, "every initializer must be reached exactly once");

    // Which one reaches which is the recursive shape a single component-global table would be
    // initialized with, and is not implied by the count above: `init` reaching all three
    // directly satisfies "exactly once" just as well, and would be a different design.
    let invoked_by = |module_path: &str, procedure_name: &str| -> Vec<String> {
        let module = lowered
            .modules
            .iter()
            .find(|module| module.path() == module_path)
            .unwrap_or_else(|| panic!("no module lowered at '{module_path}'"));
        let procedure = module
            .procedures()
            .find(|procedure| procedure.name().as_str() == procedure_name)
            .unwrap_or_else(|| panic!("no '{procedure_name}' in '{module_path}'"));
        let mut invoked = function_table_initializers_invoked(procedure.body());
        invoked.sort();
        invoked
    };

    assert_eq!(
        invoked_by("::\"hir_ns:test@1.0.0\"", "init"),
        vec!["::\"hir_ns:test@1.0.0\"::a", "::\"hir_ns:test@1.0.0\"::outer"],
        "`init` reaches the outermost initializers, and only those"
    );
    assert_eq!(
        invoked_by("::\"hir_ns:test@1.0.0\"::outer", super::FUNCTION_TABLE_INIT_PROC),
        vec!["::\"hir_ns:test@1.0.0\"::outer::inner"],
        "a module's initializer reaches the initializers of the modules nested within it"
    );
    for leaf in ["::\"hir_ns:test@1.0.0\"::a", "::\"hir_ns:test@1.0.0\"::outer::inner"] {
        assert!(
            invoked_by(leaf, super::FUNCTION_TABLE_INIT_PROC).is_empty(),
            "an initializer with nothing nested within it reaches no other, but '{leaf}' did"
        );
    }
}
#[test]
fn frame_base_locals_are_resolved_without_address_tagging() {
    use miden_core::serde::{Deserializable, Serializable};

    let expression = Expression::with_ops(vec![ExpressionOp::FrameBase {
        base: FrameBase::Local(2),
        byte_offset: 28,
    }]);
    let location = DebugVarLocation::Expression(expression.to_bytes());

    let patched = patch_debug_var_location(&location, 8, None).expect("location should resolve");
    let DebugVarLocation::Expression(bytes) = patched else {
        panic!("local frame bases must remain explicitly tagged expressions");
    };
    let expression = Expression::read_from_bytes(&bytes).unwrap();
    assert_eq!(
        expression.operations,
        vec![ExpressionOp::ResolvedFrameBase {
            base: ResolvedFrameBase::Local(-6),
            byte_offset: 28,
        }]
    );
}

#[test]
fn high_bit_global_addresses_are_not_reinterpreted_as_locals() {
    use miden_core::serde::Deserializable;

    let location = DebugVarLocation::FrameBase {
        global_index: 0,
        byte_offset: -4,
    };
    let address = 1 << 31;
    let patched = patch_debug_var_location(&location, 0, Some(address))
        .expect("global frame base should resolve");
    let DebugVarLocation::Expression(bytes) = patched else {
        panic!("high-bit globals must use an explicitly tagged expression");
    };
    let expression = Expression::read_from_bytes(&bytes).unwrap();
    assert_eq!(
        expression.operations,
        vec![ExpressionOp::ResolvedFrameBase {
            base: ResolvedFrameBase::Global(address),
            byte_offset: -4,
        }]
    );
}
