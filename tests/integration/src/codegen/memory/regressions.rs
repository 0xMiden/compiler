use super::*;

/// Regression test for `store_double_word_int` handling immediate-address stores.
///
/// Global variable initializers are lowered using `store_imm`, which passes an immediate native
/// pointer (element address + byte offset) to the store helpers, i.e. the pointer is **not**
/// present on the operand stack.
#[test]
fn global_u64_initializer_uses_immediate_store_dw() {
    setup::enable_compiler_instrumentation();

    let init_value = 0x0123_4567_89ab_cdef_u64;

    let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);
    let link_output = setup::build_empty_component_for_test(context.clone());

    // Define `test` module.
    let module = {
        let mut component_builder =
            midenc_hir::dialects::builtin::ComponentBuilder::new(link_output.component.unwrap());
        component_builder
            .define_module(midenc_hir::Ident::with_empty_span("test".into()))
            .unwrap()
    };

    // Define a u64 global with an initializer that returns a u64 literal.
    let mut gv = {
        let mut module_builder = midenc_hir::dialects::builtin::ModuleBuilder::new(module);
        module_builder
            .define_global_variable(
                midenc_hir::Ident::with_empty_span("gv_u64".into()),
                midenc_hir::Visibility::Private,
                Type::U64,
            )
            .unwrap()
    };
    {
        let init_region_ref = {
            let mut global_var = gv.borrow_mut();
            global_var.initializer_mut().as_region_ref()
        };
        let mut op_builder = midenc_hir::OpBuilder::new(context.clone());
        op_builder.create_block(init_region_ref, None, &[]);
        op_builder.ret_imm(init_value.into(), SourceSpan::default()).unwrap();
    }

    // Entrypoint: load the global and return it.
    let signature = Signature::new(&context, [], [Type::U64]);
    let function = {
        let mut module_builder = midenc_hir::dialects::builtin::ModuleBuilder::new(module);
        module_builder
            .define_function(
                midenc_hir::Ident::with_empty_span("main".into()),
                midenc_hir::Visibility::Public,
                signature.clone(),
            )
            .unwrap()
    };
    {
        let mut builder = midenc_hir::OpBuilder::new(context.clone());
        let mut builder =
            midenc_hir::dialects::builtin::FunctionBuilder::new(function, &mut builder);
        let loaded = builder.load_global(gv, SourceSpan::default()).unwrap();
        builder.ret(Some(loaded), SourceSpan::default()).unwrap();
    }

    let output = eval_miden_component::<u64, _, _>(
        link_output,
        std::iter::empty::<Initializer<'_>>(),
        &[],
        context.session(),
        |_| Ok(()),
    )
    .unwrap();

    assert_eq!(output, init_value);
}

/// A `u64` global initialized from two felts that do not fit in 32 bits holds them intact.
///
/// The initializer stores the value at the global's constant address, which moves the two elements
/// as they are, as a store to a dynamic address does: there is no `u32` range check to trap on. The
/// entrypoint loads the global, from its address on the operand stack, and returns its two limbs.
#[test]
fn global_u64_initializer_stores_a_felt_pair_intact() {
    setup::enable_compiler_instrumentation();

    let lo = Felt::new_unchecked(u64::MAX - u64::from(u32::MAX));
    let hi = Felt::new_unchecked(1 << 40);

    let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);
    let link_output = setup::build_empty_component_for_test(context.clone());

    let module = {
        let mut component_builder =
            midenc_hir::dialects::builtin::ComponentBuilder::new(link_output.component.unwrap());
        component_builder
            .define_module(midenc_hir::Ident::with_empty_span("test".into()))
            .unwrap()
    };

    // A u64 global whose initializer joins the two felts as its limbs.
    let mut gv = {
        let mut module_builder = midenc_hir::dialects::builtin::ModuleBuilder::new(module);
        module_builder
            .define_global_variable(
                midenc_hir::Ident::with_empty_span("gv_felt_pair".into()),
                midenc_hir::Visibility::Private,
                Type::U64,
            )
            .unwrap()
    };
    {
        let init_region_ref = {
            let mut global_var = gv.borrow_mut();
            global_var.initializer_mut().as_region_ref()
        };
        let span = SourceSpan::default();
        let mut op_builder = midenc_hir::OpBuilder::new(context.clone());
        op_builder.create_block(init_region_ref, None, &[]);
        let hi = op_builder.felt(hi, span);
        let lo = op_builder.felt(lo, span);
        let pair = op_builder.join2(hi, lo, Type::U64, span).unwrap();
        op_builder.ret(Some(pair), span).unwrap();
    }

    // Entrypoint: load the global and return its limbs, the low one on top.
    let signature = Signature::new(&context, [], [Type::Felt, Type::Felt]);
    let function = {
        let mut module_builder = midenc_hir::dialects::builtin::ModuleBuilder::new(module);
        module_builder
            .define_function(
                midenc_hir::Ident::with_empty_span("main".into()),
                midenc_hir::Visibility::Public,
                signature.clone(),
            )
            .unwrap()
    };
    {
        let span = SourceSpan::default();
        let mut builder = midenc_hir::OpBuilder::new(context.clone());
        let mut builder =
            midenc_hir::dialects::builtin::FunctionBuilder::new(function, &mut builder);
        let loaded = builder.load_global(gv, span).unwrap();
        let (loaded_hi, loaded_lo) = builder.split2(loaded, Type::Felt, span).unwrap();
        builder.ret([loaded_lo, loaded_hi], span).unwrap();
    }

    eval_miden_component::<Felt, _, _>(
        link_output,
        std::iter::empty::<Initializer<'_>>(),
        &[],
        context.session(),
        |trace| {
            assert_eq!(trace.outputs().get_num_elements(2), [lo, hi]);
            Ok(())
        },
    )
    .unwrap();
}
