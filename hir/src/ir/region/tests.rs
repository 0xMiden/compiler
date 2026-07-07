use alloc::rc::Rc;

use crate::{
    Context, Region, SymbolTable,
    diagnostics::{Report, Uri},
    dialects::builtin::{Function, Module},
    parse::{self, ParserConfig},
};

type TestResult = Result<(), Report>;

/// `Region::find_common_ancestor` returns the innermost region containing all operations.
///
/// Regression coverage: the previous implementation returned `None` for every multi-operation
/// query, and did not terminate when an operation lay outside the first operation's innermost
/// region.
#[test]
fn find_common_ancestor_returns_innermost_common_region() -> TestResult {
    let context = Rc::new(Context::default());
    // The parser anchors parsed operations in a fresh `builtin.world`, so this source produces
    // the region hierarchy: world body > module body > function bodies.
    let source = r#"builtin.module public @test {
    builtin.function public extern("C") @a(%x: i32) -> u32 {
        %v = builtin.unrealized_conversion_cast %x <{ ty = #builtin.type<u32> }>;
        builtin.ret %v : (u32);
    };
    builtin.function public extern("C") @b() -> u32 {
        builtin.ret_imm 42 : u32;
    };
};"#;
    let config = ParserConfig::new(context.clone());
    let module_op = parse::parse_any(config, Uri::new("find_common_ancestor.hir"), source)?;

    let (func_a_op, func_b_op) = {
        let module_op = module_op.borrow();
        let module = module_op.downcast_ref::<Module>().expect("expected a module");
        let symbols = module.symbol_manager();
        (
            symbols.lookup_op("a").expect("'a' was not registered in the symbol table"),
            symbols.lookup_op("b").expect("'b' was not registered in the symbol table"),
        )
    };
    let (cast_a, ret_a) = {
        let function =
            func_a_op.try_downcast_op::<Function>().expect("expected '@a' to be a function");
        let function = function.borrow();
        let cast = function.body().entry().front().unwrap();
        let ret = function.body().entry().terminator().unwrap();
        (cast, ret)
    };
    let ret_b = {
        let function =
            func_b_op.try_downcast_op::<Function>().expect("expected '@b' to be a function");
        let function = function.borrow();
        function.body().entry().terminator().unwrap()
    };

    let body_a = cast_a.borrow().parent_region().expect("op must be in a function body");
    let module_region = func_a_op.borrow().parent_region().expect("function must be in a module");
    let world_region = module_op.borrow().parent_region().expect("module must be in a world");
    let world_op = world_region.parent().expect("world region must have an owner");

    assert_eq!(Region::find_common_ancestor(&[]), None);
    assert_eq!(Region::find_common_ancestor(&[cast_a]), Some(body_a));
    assert_eq!(
        Region::find_common_ancestor(&[cast_a, ret_a]),
        Some(body_a),
        "operations in one region share that region"
    );
    assert_eq!(
        Region::find_common_ancestor(&[cast_a, ret_b]),
        Some(module_region),
        "operations in sibling function bodies share the module body region"
    );
    assert_eq!(
        Region::find_common_ancestor(&[cast_a, func_b_op]),
        Some(module_region),
        "nested and outer operations share the outer region"
    );
    assert_eq!(
        Region::find_common_ancestor(&[func_b_op, cast_a]),
        Some(module_region),
        "the result is independent of operation order"
    );
    assert_eq!(
        Region::find_common_ancestor(&[cast_a, ret_a, ret_b]),
        Some(module_region),
        "a region containing only some of the operations is not a common ancestor"
    );
    assert_eq!(
        Region::find_common_ancestor(&[cast_a, module_op]),
        Some(world_region),
        "the candidate walk crosses multiple nesting levels"
    );
    assert_eq!(
        Region::find_common_ancestor(&[world_op, cast_a]),
        None,
        "a top-level operation has no enclosing region, so there is no common ancestor"
    );
    assert_eq!(
        Region::find_common_ancestor(&[cast_a, world_op]),
        None,
        "a top-level operation has no enclosing region, so there is no common ancestor"
    );
    Ok(())
}
