use alloc::{format, rc::Rc, string::ToString};
use core::ops::{Deref, DerefMut};

use litcheck_filecheck::filecheck;
use pretty_assertions::assert_eq;

use crate::{
    BuilderExt, CallConv, Context, FunctionType, OpParser, OpRegistration, OperationRef, Symbol,
    SymbolTable, Type, UnsafeIntrusiveEntityRef, ValueRef, Visibility,
    diagnostics::{Report, SourceSpan, Uri},
    dialects::builtin::{
        BuiltinOpBuilder, Function, Module, Ret, UnrealizedConversionCast, WorldRef,
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
