use alloc::{format, rc::Rc, string::ToString};
use core::ops::{Deref, DerefMut};

use litcheck_filecheck::{filecheck, litcheck};
use pretty_assertions::assert_eq;

use crate::{
    BuilderExt, CallConv, Context, FunctionType, Immediate, OpParser, OpRegistration, OperationRef,
    Symbol, SymbolTable, Type, UnsafeIntrusiveEntityRef, ValueRef, Visibility,
    attributes::IntegerLikeAttr,
    diagnostics::{Report, SourceSpan, Uri},
    dialects::builtin::{
        BuiltinOpBuilder, Function, Module, Ret, RetImm, UnrealizedConversionCast, WorldRef,
        attributes::{AbiParam, Signature},
    },
    parse::{self, ParseResult, ParserConfig},
    print::AsmPrinter,
    testing::Test,
};

type TestResult<T = ()> = Result<T, Report>;

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
            builtin.function_table_entry 1 @callee;
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
    assert_eq!(table.entries().entry().body().iter().count(), 1);

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
}
