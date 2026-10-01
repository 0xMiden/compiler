use super::*;
use crate::{
    Op, OpBuilder, Spanned,
    dialects::{
        builtin::BuiltinOpBuilder,
        debuginfo::attributes::{
            INLINE_CALL_CHAIN_ATTR_NAME, InlineCallChain, InlineCallChainAttr, InlineCallFrame,
        },
        test::TestOpBuilder,
    },
    patterns::NoopRewriterListener,
    testing::Test,
};

fn mark_inline(test: &Test, value: ValueRef) -> OperationRef {
    let chain = test
        .context_rc()
        .create_attribute::<InlineCallChainAttr, _>(InlineCallChain::new(alloc::vec![
            InlineCallFrame {
                name: "callee".into(),
                linkage_name: None,
                file: "callee.rs".into(),
                line: 1,
                column: 1,
                call_file: "caller.rs".into(),
                call_line: 7,
                call_column: 1,
            }
        ]))
        .as_attribute_ref();
    let mut op = value.borrow().get_defining_op().unwrap();
    op.borrow_mut().set_attribute(INLINE_CALL_CHAIN_ATTR_NAME, chain);
    op
}

fn assert_location_erased(op: OperationRef) {
    let op = op.borrow();
    assert_eq!(op.span(), SourceSpan::UNKNOWN);
    assert!(!op.has_attribute(INLINE_CALL_CHAIN_ATTR_NAME));
}

#[test]
fn deduplicated_constant_loses_inline_provenance() {
    let mut test = Test::new("deduplicated_constant", &[], &[Type::U32, Type::U32]);
    let (first, second, ret) = {
        let mut builder = test.function_builder();
        let first = builder.u32(42, SourceSpan::SYNTHETIC).unwrap();
        let second = builder.u32(42, SourceSpan::SYNTHETIC).unwrap();
        let ret = builder.ret([first, second], SourceSpan::UNKNOWN).unwrap();
        (first, second, ret)
    };
    let first_op = mark_inline(&test, first);
    let second_op = mark_inline(&test, second);
    let mut folder = OperationFolder::new(test.context_rc(), NoopRewriterListener);
    assert!(folder.insert_known_constant(first_op, None));
    assert!(first_op.borrow().has_attribute(INLINE_CALL_CHAIN_ATTR_NAME));
    assert!(!folder.insert_known_constant(second_op, None));
    assert_location_erased(first_op);
    for operand in ret.borrow().as_operation().operands().all() {
        assert_eq!(operand.borrow().as_value_ref(), first);
    }
}

#[test]
fn hoisted_constant_loses_inline_provenance() {
    let mut test = Test::new("hoisted_constant", &[], &[Type::U32]);
    let value = {
        let mut builder = test.function_builder();
        builder.u32(1, SourceSpan::UNKNOWN).unwrap();
        let value = builder.u32(42, SourceSpan::SYNTHETIC).unwrap();
        builder.ret([value], SourceSpan::UNKNOWN).unwrap();
        value
    };
    let op = mark_inline(&test, value);
    let mut folder = OperationFolder::new(test.context_rc(), NoopRewriterListener);
    assert!(folder.insert_known_constant(op, None));
    assert_eq!(op.parent().unwrap().borrow().front().unwrap(), op);
    assert_location_erased(op);
}

#[test]
fn rehoisted_constant_loses_inline_provenance() {
    for via_fold in [false, true] {
        let mut test = Test::new("rehoisted_constant", &[Type::U32], &[Type::U32]);
        let value = {
            let mut builder = test.function_builder();
            let value = builder.u32(42, SourceSpan::SYNTHETIC).unwrap();
            builder.ret([value], SourceSpan::UNKNOWN).unwrap();
            value
        };
        let op = mark_inline(&test, value);
        let mut folder = OperationFolder::new(test.context_rc(), NoopRewriterListener);
        assert!(folder.insert_known_constant(op, None));
        // Registering a constant already in place must preserve its original location.
        assert!(folder.insert_known_constant(op, None));
        assert_eq!(op.borrow().span(), SourceSpan::SYNTHETIC);
        assert!(op.borrow().has_attribute(INLINE_CALL_CHAIN_ATTR_NAME));
        let mut builder = OpBuilder::new(test.context_rc());
        builder.set_insertion_point_before(op);
        let input = op.parent().unwrap().borrow().arguments()[0].upcast();
        builder.add(input, input, SourceSpan::UNKNOWN).unwrap();
        if via_fold {
            assert!(matches!(folder.try_fold(op), FoldResult::Failed));
        } else {
            assert!(folder.insert_known_constant(op, None));
        }
        assert_eq!(op.parent().unwrap().borrow().front().unwrap(), op);
        assert_location_erased(op);
    }
}

#[test]
fn reused_constant_with_unknown_span_loses_inline_provenance() {
    check_reused_constant(false);
}

#[test]
fn cross_dialect_reused_constant_loses_inline_provenance() {
    check_reused_constant(true);
}

fn check_reused_constant(cross_dialect: bool) {
    let mut test = Test::new("reused_constant", &[], &[Type::U32]);
    let value = {
        let mut builder = test.function_builder();
        let value = builder.u32(42, SourceSpan::UNKNOWN).unwrap();
        builder.ret([value], SourceSpan::UNKNOWN).unwrap();
        value
    };
    let op = mark_inline(&test, value);
    let mut folder = OperationFolder::new(test.context_rc(), NoopRewriterListener);
    assert!(folder.insert_known_constant(op, None));
    let dialect: Rc<dyn Dialect> = if cross_dialect {
        test.context_rc().get_or_register_dialect::<ForwardingDialect>()
    } else {
        op.borrow().dialect()
    };
    let attr = crate::matchers::constant().matches(&op.borrow()).unwrap();
    let reused = folder.get_or_create_constant(op.parent().unwrap(), dialect, attr, Type::U32);
    assert_eq!(reused, Some(value));
    assert_location_erased(op);
}

/// Materializes constants in another dialect to exercise the folder's alternate cache key.
#[derive(crate::derive::DialectRegistration, Debug)]
struct ForwardingDialect {
    #[dialect(info)]
    info: crate::DialectInfo,
}

impl From<crate::DialectInfo> for ForwardingDialect {
    fn from(info: crate::DialectInfo) -> Self {
        Self { info }
    }
}

impl Dialect for ForwardingDialect {
    fn info(&self) -> &crate::DialectInfo {
        &self.info
    }

    fn materialize_constant(
        &self,
        builder: &mut dyn Builder,
        value: AttributeRef,
        ty: &Type,
        span: SourceSpan,
    ) -> Option<OperationRef> {
        builder
            .context_rc()
            .get_or_register_dialect::<crate::dialects::test::TestDialect>()
            .materialize_constant(builder, value, ty, span)
    }
}
