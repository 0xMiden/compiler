use alloc::rc::Rc;

use crate::{
    CallConv, Context, FunctionType, OpParser, OpRegistration, OperationRef, Symbol, Type,
    UnsafeIntrusiveEntityRef, Visibility,
    diagnostics::Uri,
    dialects::builtin::{
        Function, WorldRef,
        attributes::{AbiParam, Signature},
    },
    parse::{self, ParseResult, ParserConfig},
};

#[test]
fn parse_simple_function() -> ParseResult {
    let mut test = Test::default();

    let source = r#"\
function public @entrypoint(%a: !i32) -> !i32 {
^entry(%a: !i32):
    ret %a;
};"#;

    let entrypoint = test.parse::<Function>("parse_simple_function.hir", source)?;
    let entrypoint = entrypoint.borrow();

    assert_eq!(entrypoint.name().as_str(), "entrypoint");
    assert_eq!(
        &*entrypoint.get_signature(),
        &Signature::new([AbiParam::new(Type::I32)], [AbiParam::new(Type::I32)])
    );
    assert_eq!(entrypoint.num_locals(), 0);
    assert_eq!(entrypoint.body().entry().body().len(), 1);

    Ok(())
}

#[test]
fn parse_simple_function_generic() -> ParseResult {
    let mut test = Test::default();

    let source = r#"\
"builtin.function" {
^entry(%a: !i32):
    "builtin.ret" %a : (!i32) -> ();
} attributes {
    name: @entrypoint,
    signature: #builtin.signature<"(!i32) -> !i32">,
    visibility: public,
};"#;

    let world = test.parse_generic("parse_simple_function_generic.hir", source)?;
    let entrypoint = world.borrow().body().entry().front().unwrap();
    let entrypoint = entrypoint.borrow();
    let entrypoint = entrypoint.downcast_ref::<Function>().expect("expected to parse a function");

    assert_eq!(entrypoint.name().as_str(), "entrypoint");
    assert_eq!(
        &*entrypoint.get_signature(),
        &Signature::new([AbiParam::new(Type::I32)], [AbiParam::new(Type::I32)])
    );
    assert_eq!(entrypoint.num_locals(), 0);
    assert_eq!(entrypoint.body().entry().body().len(), 1);

    Ok(())
}

#[derive(Default)]
struct Test {
    context: Rc<Context>,
}

impl Test {
    #[allow(unused)]
    pub fn parse_generic(&self, name: &str, source: &str) -> ParseResult<WorldRef> {
        let config = ParserConfig::new(self.context.clone());
        parse::parse_generic(config, Uri::new(name), source)
    }

    pub fn parse<T: OpParser + OpRegistration>(
        &self,
        name: &str,
        source: &str,
    ) -> ParseResult<UnsafeIntrusiveEntityRef<T>> {
        let config = ParserConfig::new(self.context.clone());
        parse::parse::<T>(config, Uri::new(name), source)
    }
}
