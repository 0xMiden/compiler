use alloc::{format, rc::Rc, string::ToString};
use core::ops::{Deref, DerefMut};

use litcheck_filecheck::{filecheck, litcheck};
use pretty_assertions::assert_eq;

use crate::{
    BuilderExt, CallConv, Context, FunctionType, Immediate, OpParser, OpRegistration, OperationRef,
    Symbol, SymbolTable, Type, UnsafeIntrusiveEntityRef, ValueRef, Visibility,
    attributes::IntegerLikeAttr,
    diagnostics::{Report, SourceSpan, Uri},
    dialects::{
        builtin::{
            BuiltinOpBuilder, Function, Module, Ret, RetImm, UnrealizedConversionCast, World,
            WorldRef,
            attributes::{AbiParam, Signature},
        },
        test::TestDialect,
    },
    parse::{self, ParseResult, ParserConfig},
    print::AsmPrinter,
    testing::Test,
    traits::Terminator,
};

type TestResult<T = ()> = Result<T, Report>;

#[crate::derive::operation(
    dialect = TestDialect,
    traits(Terminator)
)]
pub struct GroupedSuccessors {
    #[successors()]
    destinations: crate::Successor,
    #[successor]
    fallback: crate::Successor,
}

#[crate::derive::operation(dialect = TestDialect, traits(Terminator))]
pub struct VariadicSuccessors {
    #[successors()]
    destinations: crate::Successor,
}

#[crate::derive::operation(dialect = TestDialect, traits(Terminator))]
pub struct FixedSuccessors {
    #[successor]
    first: crate::Successor,
    #[successor]
    second: crate::Successor,
}

#[crate::derive::operation(dialect = TestDialect)]
pub struct GroupedOperands {
    #[operand]
    first: crate::traits::AnyType,
    #[operands]
    middle: crate::traits::AnyType,
    #[operand]
    last: crate::traits::AnyType,
}

#[test]
fn declared_operand_cardinality_is_checked_before_building_and_verifying() -> TestResult {
    use crate::{GenericOperationBuilder, Op, OpVerifier};

    let test = Test::new("operand_groups", &[Type::I1], &[]);
    let context = test.context_rc();
    context.get_or_register_dialect::<TestDialect>();
    let value = test.function().borrow().entry_block().borrow().arguments()[0] as ValueRef;
    let name = context.get_registered_name::<GroupedOperands>();

    // Fixed groups must have exactly one operand; the intervening variadic group may be empty.
    for (sizes, valid) in [
        (&[1, 0, 1][..], true),
        (&[1, 3, 1][..], true),
        (&[0, 0, 1][..], false),
        (&[2, 0, 1][..], false),
        (&[1, 0][..], false),
        (&[1, 0, 1, 1][..], false),
    ] {
        let mut builder = context.clone().builder();
        let op = name.alloc_default(context.clone());
        let mut op_builder = GenericOperationBuilder::new(&mut builder, op);
        for (group, count) in sizes.iter().copied().enumerate() {
            op_builder.with_operands_in_group(group, core::iter::repeat_n(value, count));
        }
        assert_eq!(op_builder.build().is_ok(), valid, "operand groups: {sizes:?}");
    }

    // Verification must catch corruption after construction, even for ops without trait verifiers.
    let mut builder = context.clone().builder();
    let op = name.alloc_default(context.clone());
    let mut op_builder = GenericOperationBuilder::new(&mut builder, op);
    op_builder.with_operands_in_group(0, [value]);
    op_builder.with_operands_in_group(1, []);
    op_builder.with_operands_in_group(2, [value]);
    let mut op = op_builder.build()?;
    op.borrow_mut().operands_mut().group_mut(0).clear();
    assert!(op.borrow().downcast_ref::<GroupedOperands>().unwrap().verify(&context).is_err());
    Ok(())
}

#[test]
fn generic_operand_cardinality_is_checked_before_type_inference() {
    let test = ParserTest::default();
    test.context().get_or_register_dialect::<TestDialect>();
    // Shl infers its result type from lhs. Missing operands must be rejected before that
    // accessor runs, even when recursive verification is disabled.
    let source = r#"builtin.function public extern("C") @missing() -> u32 {
    %result = "test.shl"() : () -> u32;
    builtin.ret %result : (u32);
};"#;
    let error = test
        .parse_any_unverified("missing_inference_operand.hir", source)
        .err()
        .expect("missing operands must not reach type inference");
    assert!(error.to_string().contains("invalid operand count"), "{error}");
}

#[test]
fn generic_parser_rejects_incorrect_provided_successor_counts() -> TestResult {
    use super::{
        DefaultParser, OperationState, ParserState, Scanner, TokenStream,
        operation::OperationParser,
    };

    for count in [0, 1, 3] {
        let context = Rc::new(Context::default());
        context.get_or_register_dialect::<TestDialect>();
        let world = context.clone().builder().create::<World, ()>(SourceSpan::UNKNOWN)()?;
        let tokens = TokenStream::new(super::SourceId::UNKNOWN, Scanner::new(": () -> ()"));
        let parser =
            DefaultParser::new(ParserState::new(ParserConfig::new(context.clone()), tokens));
        let mut parser = OperationParser::new(parser, world);
        let mut state = OperationState::new(
            SourceSpan::UNKNOWN,
            context.get_registered_name::<FixedSuccessors>(),
        );
        let successors = alloc::vec![context.create_block(); count];
        let error = parser
            .parse_generic_operation_after_name(
                &mut state,
                Some(&[]),
                Some(&successors),
                None,
                None,
                None,
            )
            .expect_err("provided successors must satisfy the declared count");
        assert!(error.to_string().contains("invalid successor count"), "{error}");
    }
    Ok(())
}

#[test]
fn generic_parser_rejects_provided_successors_with_multiple_groups() -> TestResult {
    use super::{
        DefaultParser, OperationState, ParserState, Scanner, TokenStream,
        operation::OperationParser,
    };

    let context = Rc::new(Context::default());
    context.get_or_register_dialect::<crate::dialects::test::TestDialect>();
    let world = context.clone().builder().create::<World, ()>(SourceSpan::UNKNOWN)()?;
    let tokens = TokenStream::new(super::SourceId::UNKNOWN, Scanner::new(": () -> ()"));
    let parser = DefaultParser::new(ParserState::new(ParserConfig::new(context.clone()), tokens));
    let mut parser = OperationParser::new(parser, world);
    let mut state = OperationState::new(
        SourceSpan::UNKNOWN,
        context.get_registered_name::<GroupedSuccessors>(),
    );
    let error = parser
        .parse_generic_operation_after_name(
            &mut state,
            Some(&[]),
            Some(&[context.create_block()]),
            None,
            None,
            None,
        )
        .expect_err("generic parsing must not flatten distinct successor groups");
    assert!(
        error
            .to_string()
            .contains("generic assembly cannot represent the successor layout"),
        "{error}"
    );
    Ok(())
}

#[test]
fn generic_parser_reserves_operand_groups_for_provided_successors() -> TestResult {
    use super::{
        DefaultParser, OperationState, ParserState, Scanner, TokenStream,
        operation::OperationParser,
    };

    let context = Rc::new(Context::default());
    context.get_or_register_dialect::<TestDialect>();
    let world = context.clone().builder().create::<World, ()>(SourceSpan::UNKNOWN)()?;
    for (name, count) in [
        (context.get_registered_name::<VariadicSuccessors>(), 0),
        (context.get_registered_name::<VariadicSuccessors>(), 3),
        (context.get_registered_name::<FixedSuccessors>(), 2),
        (context.get_registered_name::<Ret>(), 0),
    ] {
        let tokens = TokenStream::new(super::SourceId::UNKNOWN, Scanner::new(": () -> ()"));
        let parser =
            DefaultParser::new(ParserState::new(ParserConfig::new(context.clone()), tokens));
        let mut parser = OperationParser::new(parser, world);
        let mut state = OperationState::new(SourceSpan::UNKNOWN, name);
        let successors = alloc::vec![context.create_block(); count];
        parser.parse_generic_operation_after_name(
            &mut state,
            Some(&[]),
            Some(&successors),
            None,
            None,
            None,
        )?;

        assert_eq!(state.operands.len(), count + 1);
        assert!(state.operands.iter().all(|group| group.is_empty()));
        assert_eq!(state.successors.len(), count);
        for (i, successor) in state.successors.iter().enumerate() {
            assert_eq!(successor.block, successors[i]);
            assert_eq!(successor.operand_group as usize, i + 1);
        }
    }
    Ok(())
}

#[test]
fn parse_simple_function() -> TestResult {
    let mut test = ParserTest::default();

    let source = "\
builtin.function public extern(\"C\") @entrypoint(%a: i32) -> i32 {
    ret %a : (i32);
};";

    let entrypoint = test.parse::<Function>("parse_simple_function.hir", source)?;
    let entrypoint = entrypoint.borrow();

    assert_eq!(entrypoint.name().as_str(), "entrypoint");
    assert_eq!(
        &*entrypoint.get_signature(),
        &Signature::new(&test.context_rc(), [Type::I32], [Type::I32])
    );
    assert_eq!(entrypoint.num_locals(), 0);
    assert_eq!(entrypoint.body().entry().body().len(), 1);

    Ok(())
}

#[test]
#[ignore]
fn parse_simple_function_generic() -> TestResult {
    let mut test = ParserTest::default();

    let source = r#""builtin.function"() <{
        name = @entrypoint,
        signature: #builtin.signature<"public extern(\"C\") (i32) -> i32">,
    }> ({
^entry(%a: i32):
    "builtin.ret" %a : (i32) -> ();
}) : () -> ();"#;

    let world = test.parse_generic("parse_simple_function_generic.hir", source)?;
    let entrypoint = world.borrow().body().entry().front().unwrap();
    let entrypoint = entrypoint.borrow();
    let entrypoint = entrypoint.downcast_ref::<Function>().expect("expected to parse a function");

    assert_eq!(entrypoint.name().as_str(), "entrypoint");
    assert_eq!(
        &*entrypoint.get_signature(),
        &Signature::new(&test.context_rc(), [Type::I32], [Type::I32])
    );
    assert_eq!(entrypoint.num_locals(), 0);
    assert_eq!(entrypoint.body().entry().body().len(), 1);

    Ok(())
}

#[test]
fn parse_module_with_intra_function_symbol_references() -> TestResult {
    let mut test = ParserTest::default();

    let source = "\
    builtin.module public @test {
        builtin.global_variable public @var : i32;

        builtin.function public extern(\"C\") @entrypoint(%a: i32) -> ptr<u8, byte> {
            %ptr = builtin.global_symbol ::@test::@var+8 : ptr<u8, byte>;
            builtin.ret %ptr : (ptr<u8, byte>);
        };
    };";

    let parsed = test.parse_any("parse_module_with_intra_function_symbol_refs.hir", source)?;
    let parsed = parsed.borrow();
    let module = parsed.downcast_ref::<Module>().unwrap();

    assert_eq!(module.get_name().as_str(), "test");
    let symbol_manager = module.symbol_manager();
    assert_eq!(symbol_manager.symbols().symbols().count(), 2);
    let var = symbol_manager
        .lookup_op("var")
        .expect("'var' was not registered in symbol table after parsing");
    let entrypoint = symbol_manager
        .lookup_op("entrypoint")
        .expect("'entrypoint' was not registered in symbol table after parsing");
    let var = var.borrow();
    let var_uses = var.as_symbol().unwrap().iter_uses().count();
    assert_eq!(var_uses, 1);

    Ok(())
}

#[test]
fn module_symbol_attribute_resolves_from_enclosing_table() -> TestResult {
    use crate::dialects::builtin::attributes::SymbolRefAttr;

    let test = ParserTest::default();
    let source = "\
builtin.world {
    builtin.module public @reader attributes { reference = #builtin.symbol<@target> } {
        builtin.module public @target {};
    };
    builtin.module public @target {};
};";

    let parsed = test.parse_any("module_symbol_attribute.hir", source)?;
    let (reader, target) = {
        let world = parsed.borrow();
        let symbols = world.as_symbol_table().unwrap().symbol_manager();
        (symbols.lookup_op("reader").unwrap(), symbols.lookup_op("target").unwrap())
    };
    let (reference, inner_target) = {
        let reader = reader.borrow();
        (
            reader.get_typed_attribute::<SymbolRefAttr>("reference").unwrap(),
            reader.as_symbol_table().unwrap().symbol_manager().lookup_op("target").unwrap(),
        )
    };

    // The attribute uses the forward-declared sibling, even though the reader contains a
    // symbol with the same name. Resolution and the tracked use list must agree.
    let target = target.borrow();
    let mut uses = target.as_symbol().unwrap().iter_uses();
    let symbol_use = uses.next().expect("the sibling target must have a tracked use");
    assert!(uses.next().is_none(), "the sibling target must have exactly one use");
    assert_eq!(symbol_use.owner, reader);
    assert_eq!(symbol_use.attr, reference);
    assert_eq!(reference.borrow().user().borrow().owner, reader);
    assert_eq!(
        reference.borrow().resolve().unwrap().borrow().as_operation_ref(),
        target.as_operation_ref()
    );
    assert_eq!(inner_target.borrow().as_symbol().unwrap().iter_uses().count(), 0);

    Ok(())
}

#[test]
fn absolute_forward_symbol_uses_preserve_each_owner() -> TestResult {
    use crate::dialects::builtin::attributes::SymbolRefAttr;

    let source = r#"
builtin.module public @outer {
    builtin.module public @first attributes { reference = #builtin.symbol<::@outer::@target> } {};
    builtin.module public @second attributes { reference = #builtin.symbol<::@outer::@target> } {};
    builtin.module public @target {};
};
"#;
    for mode in ["module", "world"] {
        let test = ParserTest::default();
        let parsed = match mode {
            "module" => test.parse_any("forward_uses.hir", source)?,
            "world" => {
                test.parse_any("forward_uses.hir", &format!("builtin.world {{ {source} }};"))?
            }
            _ => unreachable!(),
        };
        let module = if parsed.borrow().is::<Module>() {
            parsed
        } else {
            parsed
                .borrow()
                .as_symbol_table()
                .unwrap()
                .symbol_manager()
                .lookup_op("outer")
                .unwrap()
        };
        let module = module.borrow();
        let symbols = module.as_symbol_table().unwrap().symbol_manager();
        let target = symbols.lookup_op("target").unwrap();
        let target = target.borrow();
        let uses = target.as_symbol().unwrap().iter_uses().collect::<alloc::vec::Vec<_>>();
        assert_eq!(uses.len(), 2, "{mode}");
        for name in ["first", "second"] {
            let owner = symbols.lookup_op(name).unwrap();
            let reference =
                owner.borrow().get_typed_attribute::<SymbolRefAttr>("reference").unwrap();
            assert_eq!(
                uses.iter()
                    .filter(|usage| usage.owner == owner && usage.attr == reference)
                    .count(),
                1
            );
            let reference = reference.borrow();
            assert_eq!(reference.user().borrow().owner, owner);
            assert_eq!(
                reference.resolve().unwrap().borrow().as_operation_ref(),
                target.as_operation_ref()
            );
        }
    }
    Ok(())
}

/// The `reserved_memory` module attribute and the function-table ops are inputs to the linker's
/// memory layout, so they must survive a print/parse round-trip of the textual HIR.
#[test]
fn module_reserved_memory_and_function_table_roundtrip() -> TestResult {
    use crate::{Op, dialects::builtin::attributes::U64Attr};

    let mut test = ParserTest::default();

    let source = "\
    builtin.module public @test {
        builtin.function internal extern(\"C\") @callee(%a: i32, %b: i32) -> i32 {
            builtin.ret %a : (i32);
        };

        builtin.function_table private @tbl : 3 {
            builtin.function_table_entry 1 @callee tag 4;
        };
    };";

    let parsed = test.parse_any("function_table_roundtrip.hir", source)?;

    // Attach the linker-facing memory reservation, then round-trip through print/parse and
    // verify nothing is lost
    {
        let attr = test.context_rc().create_attribute::<U64Attr, _>(0x110000u64);
        let mut op = parsed;
        let mut op = op.borrow_mut();
        op.set_attribute(Module::RESERVED_MEMORY_ATTR, attr);
    }

    let flags = Default::default();
    let mut printer = AsmPrinter::new(test.context_rc(), &flags);
    printer.print_operation(parsed.borrow());
    let printed = printer.finish().to_string();
    std::println!("{printed}");

    let reparsed = test.parse_any("function_table_roundtrip2.hir", &printed)?;
    let reparsed = reparsed.borrow();
    let module = reparsed.downcast_ref::<Module>().unwrap();

    let reserved = module
        .as_operation()
        .get_typed_attribute::<U64Attr>(Module::RESERVED_MEMORY_ATTR)
        .expect("reserved_memory attribute lost in print/parse round-trip");
    assert_eq!(**reserved.borrow(), 0x110000);

    let table = module
        .symbol_manager()
        .lookup_op("tbl")
        .expect("'tbl' was not registered in symbol table after parsing");
    let table = table.borrow();
    let table = table
        .downcast_ref::<crate::dialects::builtin::FunctionTable>()
        .expect("expected 'tbl' to be a function table");
    assert_eq!(*table.get_num_slots(), 3);
    let entries = table.entries();
    let entries = entries.entry();
    let mut entries = entries.body().iter();
    let entry = entries.next().expect("expected one table entry");
    let entry = entry
        .downcast_ref::<crate::dialects::builtin::FunctionTableEntry>()
        .expect("expected a function table entry");
    assert_eq!(*entry.get_index(), 1);
    assert_eq!(*entry.get_type_tag(), 4, "signature tag lost in print/parse round-trip");
    assert!(entries.next().is_none());

    Ok(())
}

#[test]
fn derive_roundtrip_test() -> TestResult {
    let test = Test::new("derive_roundtrip_test", &[Type::I32], &[Type::U32]);
    let mut test = ParserTest { test };

    {
        let mut f = test.function_builder();
        let v0 = f.entry_block().borrow().arguments()[0] as ValueRef;
        let v1 = f.builder_mut().unrealized_conversion_cast(v0, Type::U32, SourceSpan::UNKNOWN)?;
        f.builder_mut().ret([v1], SourceSpan::UNKNOWN);
    }

    let flags = Default::default();
    let mut printer = AsmPrinter::new(test.context_rc(), &flags);
    printer.print_operation(test.function().borrow());
    let source = printer.render().to_string();

    let parsed = test.parse::<Function>("derive_roundtrip.hir", &source)?;
    let parsed = parsed.borrow();

    printer.print_operation(&parsed);
    let roundtripped = printer.finish().to_string();

    std::println!("{source}");
    std::println!("{roundtripped}");
    //assert_eq!(&source, &roundtripped);
    filecheck!(
        &roundtripped,
        r#"
    // CHECK: builtin.function public extern("C") @derive_roundtrip_test([[V0:%\d+]]: i32) -> u32 {
    // CHECK-NEXT: [[V1:%\d+]] = builtin.unrealized_conversion_cast [[V0]] <{ ty = #builtin.type<u32> }>;
    // CHECK-NEXT: builtin.ret [[V1]] : (u32);
    // CHECK-NEXT: };
    "#
    );

    Ok(())
}

#[test]
fn parse_ret_imm_coerces_literal_to_declared_type() -> TestResult {
    let test = ParserTest::default();

    let source = "\
builtin.function public extern(\"C\") @retconst() -> u32 {
    builtin.ret_imm 42 : u32;
};";

    let function = test.parse::<Function>("parse_ret_imm.hir", source)?;
    let printed = format!("{}", function.as_operation_ref().borrow());
    assert!(
        printed.contains("builtin.ret_imm 42 : u32"),
        "expected the declared type to survive the round trip, got:\n{printed}"
    );

    let function = function.borrow();
    let ret_imm = function
        .body()
        .entry()
        .terminator()
        .unwrap()
        .try_downcast_op::<RetImm>()
        .expect("expected the function terminator to be builtin.ret_imm");
    let imm = ret_imm.borrow().value().as_ref().as_immediate();
    assert!(
        matches!(imm, Immediate::U32(42)),
        "expected the literal to be coerced to the declared type, got {imm:?}"
    );

    // A literal that cannot be represented in the declared type must be rejected.
    let result = test.parse::<Function>(
        "parse_ret_imm_invalid.hir",
        "\
builtin.function public extern(\"C\") @retconst() -> u8 {
    builtin.ret_imm 300 : u8;
};",
    );
    assert!(result.is_err(), "expected an out-of-range immediate to be rejected");

    Ok(())
}

/// A whole `builtin.world` — the shape `midenc --emit=hir` writes, and the shape that must
/// survive being fed back in.
const WORLD_SOURCE: &str = "\
builtin.world {
    builtin.module public @lib {
        builtin.function public extern(\"C\") @main() {
            builtin.ret;
        };
    };
};";

/// The same content without the enclosing world, which the parser does have to wrap.
const MODULE_SOURCE: &str = "\
builtin.module public @lib {
    builtin.function public extern(\"C\") @main() {
        builtin.ret;
    };
};";

#[test]
fn parsing_a_world_yields_that_world_rather_than_nesting_it() -> TestResult {
    let test = ParserTest::default();

    let parsed = test.parse_any("parse_world.hir", WORLD_SOURCE)?;
    let world = parsed
        .try_downcast_op::<World>()
        .expect("expected the parsed operation to be a 'builtin.world'");

    // The world the file declares *is* the root. Nothing was created around it.
    assert!(
        parsed.parent_op().is_none(),
        "a parsed 'builtin.world' must not be nested inside another operation, but it hangs from \
         a '{}'",
        parsed.parent_op().unwrap().borrow().name()
    );

    // And because it is the root, symbols inside it have well-formed paths. A world is an
    // anonymous symbol table, and `Symbol::path` asserts that an anonymous symbol table has no
    // parent — so a world nested in a world does not merely produce a wrong path here, it
    // panics.
    let module_op = world.borrow().body().entry().front().expect("the world declares a module");
    {
        let module = module_op.borrow();
        let module = module.downcast_ref::<Module>().expect("expected 'lib' to be a module");
        let path = module.path();
        assert!(path.is_absolute(), "a symbol path is rooted at the world");
        assert_eq!(path.to_string(), "::lib");
    }

    // The function's path crosses two symbol tables, so it also pins that the world contributes
    // no component of its own — it is the root, not a named ancestor.
    let function = {
        let module = module_op.borrow();
        let module = module.downcast_ref::<Module>().unwrap();
        module.body().entry().front().expect("the module defines a function")
    };
    let function = function.borrow();
    let function = function.downcast_ref::<Function>().expect("expected 'main' to be a function");
    assert_eq!(function.path().to_string(), "::lib::main");

    Ok(())
}

#[test]
fn parsing_a_module_still_wraps_it_in_a_world() -> TestResult {
    let test = ParserTest::default();

    let parsed = test.parse_any("parse_module_wrapped.hir", MODULE_SOURCE)?;
    let module = parsed
        .try_downcast_op::<Module>()
        .expect("expected the parsed operation to be a 'builtin.module'");

    // The discriminating half: an operation that is not a world still gets the world it needs to
    // resolve symbols against.
    let parent = parsed
        .parent_op()
        .expect("a top-level operation that is not a world must be given a world to live in");
    assert!(
        parent.borrow().is::<World>(),
        "expected the wrapper to be a 'builtin.world', got a '{}'",
        parent.borrow().name()
    );
    assert!(parent.parent_op().is_none(), "the wrapper world must itself be the root");

    let path = module.borrow().path();
    assert!(path.is_absolute(), "a symbol path is rooted at the wrapper world");
    assert_eq!(path.to_string(), "::lib");

    Ok(())
}

/// Well-formed as text, ill-formed as IR: `builtin.function` carries the `SingleRegion` trait, so
/// a function with no region at all parses (it is how a declaration is written) and then fails
/// verification with "requires exactly one region, but got 0".
const UNVERIFIABLE_SOURCE: &str = "\
builtin.module public @lib {
    builtin.function public extern(\"C\") @main();
};";

#[test]
fn parsing_verifies_what_it_parsed() {
    let test = ParserTest::default();

    // Verification used to run inside `OperationParser::finalize`, and moving it out to
    // `parse_anchored_source` left nothing pinning that it still runs at all: the tests that
    // exercise invalid IR reach the verifier by calling `recursively_verify` themselves, and the
    // one fixture that is deliberately malformed is parsed with verification turned off. Deleting
    // the call would have kept every one of them green. This is the test that would not be.
    let result = test.parse_any("parse_unverifiable.hir", UNVERIFIABLE_SOURCE);

    let err = result.err().map(|err| err.to_string()).unwrap_or_else(|| {
        panic!(
            "the default parser configuration verifies, so ill-formed IR must not parse \
             successfully"
        )
    });

    // And the failure has to be the *verifier's*: the same source parses when verification is
    // turned off, so nothing about the text itself is what rejected it.
    test.parse_any_unverified("parse_unverifiable_unverified.hir", UNVERIFIABLE_SOURCE)
        .expect("the source is well-formed as text — only verification rejects it");

    assert!(
        err.contains("invalid operation builtin.function"),
        "expected the verifier's rejection of the region-less function, got: {err}"
    );
}

#[test]
fn parse_preserves_full_width_frame_base_local_indices() -> TestResult {
    use crate::dialects::debuginfo::DebugInfoDialect;

    let mut test = ParserTest::default();
    test.context().get_or_register_dialect::<DebugInfoDialect>();
    let source = r#"builtin.function public extern("C") @frame_base(%0: i32) -> i32 {
    di.debug_declare <{ variable = #di.variable<{ name = "x", file = "test.rs", line = 1 }>, expression = #di.expression<[DI_OP_frame_base(local_slot, 2147483648+0)]> }>;
    builtin.ret %0 : (i32);
};"#;

    let function = test.parse::<Function>("frame_base_full_width_index.hir", source)?;
    let printed = format!("{}", function.as_operation_ref().borrow());
    assert!(
        printed.contains("DI_OP_frame_base(local_slot, 2147483648+0)"),
        "frame-base index was not preserved: {printed}"
    );

    Ok(())
}

#[derive(Default)]
struct ParserTest {
    test: Test,
}

impl Deref for ParserTest {
    type Target = Test;

    fn deref(&self) -> &Self::Target {
        &self.test
    }
}

impl DerefMut for ParserTest {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.test
    }
}

impl ParserTest {
    #[allow(unused)]
    pub fn parse_generic(&self, name: &str, source: &str) -> TestResult<WorldRef> {
        let config = ParserConfig::new(self.test.context_rc());
        parse::parse_generic(config, Uri::new(name), source)
    }

    pub fn parse<T: OpParser + OpRegistration>(
        &self,
        name: &str,
        source: &str,
    ) -> TestResult<UnsafeIntrusiveEntityRef<T>> {
        let config = ParserConfig::new(self.test.context_rc());
        parse::parse::<T>(config, Uri::new(name), source)
    }

    pub fn parse_any(&self, name: &str, source: &str) -> TestResult<OperationRef> {
        let config = ParserConfig::new(self.test.context_rc());
        parse::parse_any(config, Uri::new(name), source)
    }

    pub fn parse_any_unverified(&self, name: &str, source: &str) -> TestResult<OperationRef> {
        let config = ParserConfig::new(self.test.context_rc()).verify_after_parse(false);
        parse::parse_any(config, Uri::new(name), source)
    }
}

/// An unregistered dialect is a different failure from an unregistered operation inside a dialect
/// that exists, and the diagnostic has to say which one it is.
#[test]
fn an_unregistered_dialect_is_named_in_the_diagnostic() {
    let test = ParserTest::default();
    let Err(err) =
        test.parse_any("unknown_dialect.hir", "builtin.module public @t { nosuchdialect.op; };")
    else {
        panic!("an unregistered dialect must not parse");
    };
    let rendered = alloc::string::ToString::to_string(&err);
    assert!(
        rendered.contains("unknown dialect") && rendered.contains("nosuchdialect"),
        "the diagnostic must name the unregistered dialect, got: {rendered}"
    );
}

/// The control: an unknown operation inside a dialect that is registered still reports as an
/// invalid operation, not as an unknown dialect.
#[test]
fn an_unknown_operation_in_a_known_dialect_still_reports_separately() {
    let test = ParserTest::default();
    let Err(err) = test.parse_any("unknown_op.hir", "builtin.module public @t { builtin.nope; };")
    else {
        panic!("an unregistered operation must not parse");
    };
    let rendered = alloc::string::ToString::to_string(&err);
    assert!(
        !rendered.contains("unknown dialect"),
        "a known dialect must not be reported as unknown, got: {rendered}"
    );
}

/// A module with an empty body defines no symbols; building its symbol table must not unwrap the
/// missing entry block.
#[test]
fn an_empty_module_body_parses() {
    let test = ParserTest::default();
    test.parse_any("empty.hir", "builtin.module public @t {};")
        .expect("an empty module body should parse");
}

/// Two symbols with one name is something the author wrote, so it must be reported rather than
/// tripping the uniqueness assertion inside the symbol table.
#[test]
fn a_duplicate_symbol_is_reported() {
    let test = ParserTest::default();
    let source = "\
builtin.module public @t {
    builtin.function internal extern(\"C\") @f(%a: i32) -> i32 {
        builtin.ret %a : (i32);
    };
    builtin.function internal extern(\"C\") @f(%b: i32) -> i32 {
        builtin.ret %b : (i32);
    };
};";
    assert!(test.parse_any("dup.hir", source).is_err(), "a duplicate symbol must not parse");
}

/// A parsed world is held to the rule `WorldBuilder` enforces: a component name must not shadow
/// a module tree.
#[test]
fn a_component_shadowing_a_module_tree_is_reported() {
    let test = ParserTest::default();
    let source = "\
builtin.world {
    builtin.module public @miden {
        builtin.module public @a {
        };
    };
    builtin.component private @miden::@a {
    };
};";
    let err = test
        .parse_any("shadowing.hir", source)
        .expect_err("a component shadowing a module tree must not parse")
        .to_string();
    assert!(
        err.contains(
            "component `miden::a` and `builtin.module` `::miden::a` have the same path; a \
             component must not share its path with another symbol"
        ),
        "unexpected diagnostic: {err}"
    );
}

/// A parsed module is held to the rule the builders enforce: a module name has no `::`.
#[test]
fn a_module_named_by_a_path_is_reported() {
    let test = ParserTest::default();
    let source = "\
builtin.world {
    builtin.module public @\"a::b\" {
    };
};";
    let err = test
        .parse_any("module_path.hir", source)
        .expect_err("a module named by a path must not parse")
        .to_string();
    assert!(
        err.contains(
            "module `a::b`: a module name cannot contain `::` (only components are named by \
             `::`-joined paths)"
        ),
        "unexpected diagnostic: {err}"
    );
}

#[test]
fn nested_component_names_are_reported() {
    let test = ParserTest::default();
    let source = "\
builtin.world {
    builtin.component private @acme::@app {
    };
    builtin.component private @acme::@app::@main {
    };
};";
    let err = test
        .parse_any("nesting.hir", source)
        .expect_err("components with nesting names must not parse")
        .to_string();
    assert!(
        err.contains(
            "component `acme::app` and component `acme::app::main` nest (`acme::app` is a \
             segment-prefix of `acme::app::main`); component namespaces must not nest"
        ),
        "unexpected diagnostic: {err}"
    );
}

/// A component prints its name as a symbol path, one raw segment per segment of the name (see
/// `SymbolPath::to_symbol_name`), quoting a segment that is not a bare HIR identifier, and that
/// text parses back to the same name: the Miden Assembly spelling of the segments.
#[test]
fn component_name_roundtrips_through_the_printer() -> TestResult {
    use crate::{
        Op,
        dialects::builtin::{Component, WorldBuilder},
    };

    let test = ParserTest::default();
    for (raw, name, header) in [
        ("miden::a::b", "miden::a::b", "builtin.component private @miden::@a::@b {"),
        (
            "hir_ns:test@1.0.0",
            "\"hir_ns:test@1.0.0\"",
            "builtin.component private @\"hir_ns:test@1.0.0\" {",
        ),
        ("$kernel", "$kernel", "builtin.component private @\"$kernel\" {"),
        ("1st", "\"1st\"", "builtin.component private @\"1st\" {"),
        ("a-b", "\"a-b\"", "builtin.component private @\"a-b\" {"),
    ] {
        let world = test.context_rc().builder().create::<World, ()>(SourceSpan::UNKNOWN)()?;
        let component = WorldBuilder::new(world).define_component(raw.into())?;
        assert_eq!(Symbol::name(&*component.borrow()).as_str(), name);

        let flags = Default::default();
        let mut printer = AsmPrinter::new(test.context_rc(), &flags);
        printer.print_operation(component.as_operation_ref().borrow());
        let printed = printer.finish().to_string();
        assert!(printed.starts_with(header), "unexpected printed form:\n{printed}");

        let reparsed = test.parse::<Component>("component_roundtrip.hir", &printed)?;
        assert_eq!(Symbol::name(&*reparsed.borrow()).as_str(), name);
    }

    Ok(())
}

/// A component named by a path whose segment contains `::` prints that segment quoted, and the
/// text parses back to a component with the same segments.
#[test]
fn component_name_with_a_quoted_segment_roundtrips_through_the_printer() -> TestResult {
    use midenc_session::miden_assembly_syntax::Path;

    use crate::{
        Op, SymbolPath,
        dialects::builtin::{Component, WorldBuilder},
    };

    let test = ParserTest::default();
    let path = SymbolPath::from_library_path(Path::new("::\"acme::pkg\"::app"));
    let world = test.context_rc().builder().create::<World, ()>(SourceSpan::UNKNOWN)()?;
    let component = WorldBuilder::new(world).define_component(path.to_symbol_name()?.into())?;

    let flags = Default::default();
    let mut printer = AsmPrinter::new(test.context_rc(), &flags);
    printer.print_operation(component.as_operation_ref().borrow());
    let printed = printer.finish().to_string();
    assert!(
        printed.starts_with(r#"builtin.component private @"acme::pkg"::@app {"#),
        "unexpected printed form:\n{printed}"
    );

    let reparsed = test.parse::<Component>("component_roundtrip.hir", &printed)?;
    assert_eq!(Symbol::name(&*reparsed.borrow()).as_str(), r#""acme::pkg"::app"#);
    let namespace = reparsed.borrow().namespace_path();
    assert_eq!(namespace, path);
    assert_eq!(namespace.components().len(), 3, "the root and two segments: {namespace:?}");

    Ok(())
}

/// A component with an interface whose name needs quoting.
const INTERFACE_SOURCE: &str = "\
builtin.world {
    builtin.component private @app {
        builtin.interface @\"my-api\" {
            builtin.function public extern(\"C\") @f() {
                builtin.ret;
            };
        };
    };
};";

/// An interface is named by the Miden Assembly spelling of its raw segment, prints that raw
/// segment, and the printed text parses back to the same name.
#[test]
fn interface_name_roundtrips_through_the_printer() -> TestResult {
    use crate::dialects::builtin::Interface;

    let test = ParserTest::default();
    let source = "\
builtin.interface @\"my-api\" {
    builtin.function public extern(\"C\") @f() {
        builtin.ret;
    };
};";
    let interface = test.parse::<Interface>("interface_roundtrip.hir", source)?;
    assert_eq!(Symbol::name(&*interface.borrow()).as_str(), "\"my-api\"");

    let flags = Default::default();
    let mut printer = AsmPrinter::new(test.context_rc(), &flags);
    printer.print_operation(interface.as_operation_ref().borrow());
    let printed = printer.finish().to_string();
    assert!(
        printed.starts_with("builtin.interface @\"my-api\" {"),
        "unexpected printed form:\n{printed}"
    );

    let reparsed = test.parse::<Interface>("interface_roundtrip2.hir", &printed)?;
    assert_eq!(Symbol::name(&*reparsed.borrow()).as_str(), "\"my-api\"");
    Ok(())
}

/// A path through an interface whose name needs quoting resolves to the function inside it.
#[test]
fn a_path_through_a_quoted_interface_resolves() -> TestResult {
    use crate::{SymbolNameComponent, SymbolPath, dialects::builtin::Function};

    let test = ParserTest::default();
    let world = test.parse_any("interface_resolution.hir", INTERFACE_SOURCE)?;
    let path = SymbolPath::from_iter([
        SymbolNameComponent::Root,
        SymbolNameComponent::Component("app".into()),
        SymbolNameComponent::Component("my-api".into()),
        SymbolNameComponent::Leaf("f".into()),
    ]);
    let world = world.try_downcast_op::<World>().expect("expected a world");
    let resolved = world.borrow().resolve(&path).expect("`::app::\"my-api\"::f` should resolve");
    assert!(resolved.borrow().as_symbol_operation().is::<Function>());
    assert_eq!(resolved.borrow().path(), path);
    Ok(())
}

/// A symbol-table op whose raw segment Miden Assembly cannot spell does not parse, a raw segment
/// wrapped in quotes included: it is not read as the segment it quotes.
#[test]
fn an_unspellable_symbol_table_name_is_reported() {
    let test = ParserTest::default();
    for (source, kind, segment) in [
        ("builtin.component private @\"q\\\"u\" {};", "component", "q\"u"),
        ("builtin.component private @\"\\\"x-y\\\"\" {};", "component", "\"x-y\""),
        ("builtin.component private @app::@\"a b\" {};", "component", "a b"),
        ("builtin.module public @\"a\\\\b\" {};", "module", "a\\b"),
        ("builtin.module public @\"\\\"m\\\"\" {};", "module", "\"m\""),
        ("builtin.module public @\"a#b\" {};", "module", "a#b"),
        ("builtin.interface @\"a\\\\b\" {};", "interface", "a\\b"),
    ] {
        let err = test
            .parse_any("unspellable.hir", source)
            .expect_err("an unspellable name must not parse")
            .to_string();
        assert_eq!(
            err,
            format!(
                "invalid {kind} name: the segment `{segment}` cannot be spelled in Miden Assembly"
            ),
            "{source}"
        );
    }
}

/// The diagnostic for a component name underlines the whole symbol path: from its root `::`, or
/// from its first identifier (after the `@` sigil) when it has no root, to the end of its last
/// identifier.
#[test]
fn a_symbol_path_diagnostic_spans_the_whole_path() {
    let test = ParserTest::default();
    // The path, and where its span starts
    for (path, first) in [("@app::@\"a b\"", "app"), ("::@app::@\"a b\"", "::")] {
        let source = format!("builtin.component private {path} {{}};");
        let err = test
            .parse_any("span.hir", &source)
            .expect_err("an unspellable name must not parse");
        let labels = err
            .labels()
            .expect("the diagnostic is labelled")
            .map(|label| (label.offset(), label.offset() + label.len()))
            .collect::<alloc::vec::Vec<_>>();
        let path_start = source.find(path).unwrap();
        let start = path_start + path.find(first).unwrap();
        assert_eq!(labels, [(start, path_start + path.len())], "{source}");
    }
}

/// A lone leaf naming a symbol table is its raw last segment, which `Symbol::path` gives the
/// table and which resolves to it, from its parent and as a standalone root.
#[test]
fn a_lone_leaf_naming_a_symbol_table_resolves() -> TestResult {
    use crate::{
        Op, SymbolName, SymbolNameComponent, SymbolPath,
        dialects::builtin::{Component, WorldBuilder},
    };

    let test = ParserTest::default();
    let leaf = |segments: &[&str]| {
        let (last, rest) = segments.split_last().unwrap();
        SymbolPath::from_iter(
            core::iter::once(SymbolNameComponent::Root)
                .chain(rest.iter().map(|s| SymbolNameComponent::Component(SymbolName::intern(s))))
                .chain([SymbolNameComponent::Leaf(SymbolName::intern(last))]),
        )
    };

    // A module whose name needs quoting, in a world
    let source = "\
builtin.world {
    builtin.module public @\"my-mod\" {
    };
};";
    let world = test.parse_any("leaf_module.hir", source)?;
    let world = world.try_downcast_op::<World>().expect("expected a world");
    let module = world.borrow().resolve(&leaf(&["my-mod"])).expect("`::\"my-mod\"` resolves");
    assert!(module.borrow().as_symbol_operation().is::<Module>());
    let path = module.borrow().path();
    assert_eq!(path, leaf(&["my-mod"]));
    let flags = Default::default();
    let mut printer = AsmPrinter::new(test.context_rc(), &flags);
    printer.print_symbol_path(&path);
    assert_eq!(printer.finish().to_string(), "::@\"my-mod\"");

    // A component named by several segments, in a world
    let world = test.context_rc().builder().create::<World, ()>(SourceSpan::UNKNOWN)()?;
    let component = WorldBuilder::new(world).define_component("miden::a::b".into())?;
    let path = component.borrow().path();
    assert_eq!(path, leaf(&["miden", "a", "b"]));
    let resolved = world.borrow().resolve(&path).expect("`::miden::a::b` resolves");
    assert_eq!(
        resolved.borrow().as_symbol_operation().as_operation_ref(),
        component.as_operation_ref()
    );

    // A standalone component, the root itself, named by its raw segment only
    let source = "builtin.component private @\"hir_ns:test@1.0.0\" {\n};";
    let component = test.parse::<Component>("leaf_root.hir", source)?;
    let root = component.as_operation_ref();
    let resolve = |path: &SymbolPath| {
        component
            .borrow()
            .resolve(path)
            .map(|symbol| symbol.borrow().as_symbol_operation().as_operation_ref())
    };
    assert_eq!(resolve(&leaf(&["hir_ns:test@1.0.0"])), Some(root));
    assert_eq!(resolve(&leaf(&["\"hir_ns:test@1.0.0\""])), None);
    assert_eq!(resolve(&component.borrow().path()), Some(root));
    Ok(())
}

/// A component whose path is the path of a function in a module tree is rejected, while a
/// component at another path is accepted: paths identify symbols uniquely.
#[test]
fn a_component_at_the_path_of_a_module_tree_function_is_reported() -> TestResult {
    let source = |component: &str| {
        format!(
            "\
builtin.world {{
    builtin.module public @miden {{
        builtin.module public @a {{
            builtin.function public extern(\"C\") @f() {{
                builtin.ret;
            }};
        }};
    }};
    builtin.component private {component} {{
    }};
}};"
        )
    };
    let test = ParserTest::default();
    let err = test
        .parse_any("shadow.hir", &source("@miden::@a::@f"))
        .expect_err("the component shadows the function `f`")
        .to_string();
    assert_eq!(
        err,
        "component `miden::a::f` and `builtin.function` `::miden::a::f` have the same path; a \
         component must not share its path with another symbol"
    );
    test.parse_any("shadow_ctrl.hir", &source("@other"))?;
    Ok(())
}

/// A component whose path is the path of a function in a world-level interface is rejected,
/// whichever of the two is defined first.
#[test]
fn a_component_at_the_path_of_an_interface_function_is_reported() {
    let interface = "\
    builtin.interface @api {
        builtin.function public extern(\"C\") @f() {
            builtin.ret;
        };
    };";
    let component = "\
    builtin.component private @api::@f {
    };";
    let test = ParserTest::default();
    for (first, second) in [(interface, component), (component, interface)] {
        let source = format!("builtin.world {{\n{first}\n{second}\n}};");
        let err = test
            .parse_any("iface_shadow.hir", &source)
            .expect_err("the component shadows the interface function `f`")
            .to_string();
        assert_eq!(
            err,
            "component `api::f` and `builtin.function` `::api::f` have the same path; a component \
             must not share its path with another symbol",
            "{source}"
        );
    }
}

/// A module and a function whose raw names are equal have the same path, so they cannot both be
/// defined in one symbol table, whatever their definition order.
#[test]
fn a_module_and_a_function_with_one_raw_name_are_reported() {
    let module = "\
        builtin.module private @\"my-mod\" {
            builtin.function public extern(\"C\") @g() {
                builtin.ret;
            };
        };";
    let function = "\
        builtin.function public extern(\"C\") @\"my-mod\"() {
            builtin.ret;
        };";
    let test = ParserTest::default();
    let (module_kind, function_kind) = ("module \"my-mod\"", "function my-mod");
    for (first, second, expected) in [
        (module, function, (function_kind, module_kind)),
        (function, module, (module_kind, function_kind)),
    ] {
        let source = format!(
            "builtin.world {{\n    builtin.component private @app {{\n{first}\n{second}\n    \
             }};\n}};"
        );
        let err = test
            .parse_any("crosskind.hir", &source)
            .expect_err("the module and the function share the path `::app::my-mod`")
            .to_string();
        let (new, existing) = expected;
        assert_eq!(
            err,
            format!("`{new}` and `{existing}` have the same path `::app::\"my-mod\"`"),
            "{source}"
        );
    }
}

/// A reference spells only raw segments: a segment wrapped in quotes is not read as the segment
/// it quotes.
#[test]
fn a_quoted_raw_segment_in_a_reference_does_not_resolve() -> TestResult {
    use crate::{SymbolName, SymbolNameComponent, SymbolPath};

    let source = "\
builtin.world {
    builtin.component private @app {
        builtin.module private @\"my-mod\" {
            builtin.global_variable public @v : i32;
        };
    };
};";
    let test = ParserTest::default();
    let world = test.parse_any("modq.hir", source)?;
    let world = world.try_downcast_op::<World>().expect("expected a world");
    let resolves = |module: &str| {
        let path = SymbolPath::from_iter([
            SymbolNameComponent::Root,
            SymbolNameComponent::Component("app".into()),
            SymbolNameComponent::Component(SymbolName::intern(module)),
            SymbolNameComponent::Leaf("v".into()),
        ]);
        world.borrow().resolve(&path).is_some()
    };
    assert!(resolves("my-mod"));
    assert!(!resolves("\"my-mod\""));
    Ok(())
}

/// A generic-form operation with more successors than the 16-bit operand-group index can address
/// is rejected by the parser instead of wrapping a successor onto an earlier operand group.
#[test]
fn parse_generic_operation_with_too_many_successors_is_rejected() {
    let test = ParserTest::default();
    test.context().get_or_register_dialect::<TestDialect>();
    let name = test.context().get_registered_name::<VariadicSuccessors>();
    // The op's own operands take group 0, so the successors fill the remaining 65,535 groups
    // and the last one needs group 65,536
    let successors = alloc::vec!["^dest"; u16::MAX as usize + 1].join(", ");
    let source = format!(
        "\
builtin.function public extern(\"C\") @entrypoint(%a: i32) -> i32 {{
    \"{name}\"()[{successors}] : () -> ();
^dest:
    builtin.ret %a : (i32);
}};"
    );

    let error = test
        .parse::<Function>("parse_generic_too_many_successors.hir", &source)
        .err()
        .expect("expected the operation to be rejected");
    let message = format!("{error}");
    assert!(message.contains("too many operand groups"), "{message}");
}
