use std::rc::Rc;

use midenc_compile::{CodegenOutput, compile_link_output_to_masm_with_pre_assembly_stage};
use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    PointerType, SourceSpan, Type,
    dialects::builtin::{BuiltinOpBuilder, attributes::Signature},
};

use crate::testing::setup;

// TODO seems like this test setup doesn't fit well. Try `hir-opt` or just rely on e2e test

#[test]
fn println_lowers_to_trace_nop_and_cleanup() {
    setup::enable_compiler_instrumentation();

    let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);
    let link_output = setup::build_empty_component_for_test(context.clone());

    let signature = Signature::new(&context, [], []);
    setup::build_entrypoint(link_output.component, &signature, |builder| {
        let span = SourceSpan::default();
        let ptr_ty = Type::from(PointerType::new(Type::U8));
        let addr = builder.u32(256, span); // TODO page 17
        let ptr = builder.inttoptr(addr, ptr_ty, span).unwrap();
        let len = builder.u32(1, span);
        builder.println(ptr, len, span).unwrap();
        builder.ret(None, span).unwrap();
    });

    let masm_src = compile_to_masm_src(link_output, context);

    // TODO don't hardcode number
    let trace_idx = masm_src
        .find("trace.42")
        .expect("expected hir.println lowering to emit trace.42");
    let nop_idx = masm_src[trace_idx..]
        .find("nop")
        .map(|idx| trace_idx + idx)
        .expect("expected hir.println lowering to emit nop after trace");
    let first_drop_idx = masm_src[nop_idx..]
        .find("drop")
        .map(|idx| nop_idx + idx)
        .expect("expected hir.println lowering to drop ptr after nop");
    let second_drop_idx = masm_src[first_drop_idx + 1..]
        .find("drop")
        .map(|idx| first_drop_idx + 1 + idx)
        .expect("expected hir.println lowering to drop len after nop");

    assert!(trace_idx < nop_idx);
    assert!(nop_idx < first_drop_idx);
    assert!(first_drop_idx < second_drop_idx);
}

fn compile_to_masm_src(
    link_output: midenc_compile::LinkOutput,
    context: Rc<midenc_hir::Context>,
) -> String {
    let mut masm_src = None;
    let mut stage = |output: CodegenOutput, _context: Rc<midenc_hir::Context>| {
        masm_src = Some(output.component.to_string());
        Ok(output)
    };

    compile_link_output_to_masm_with_pre_assembly_stage(link_output, &mut stage)
        .unwrap_or_else(|err| panic!("{}", crate::testing::format_report(err)))
        .unwrap_mast();

    let _ = context;
    masm_src.expect("expected MASM source to be captured")
}
