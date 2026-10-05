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

/// A `u32` global initialized from a felt that does not fit in 32 bits holds it intact.
///
/// The initializer stores the value at the global's constant address, which moves the element as
/// it is, as a store to a dynamic address does: there is no `u32` range check to trap on. The
/// entrypoint loads the global, from its address on the operand stack, and returns it as a felt.
#[test]
fn global_u32_initializer_stores_a_felt_intact() {
    setup::enable_compiler_instrumentation();

    let value = Felt::new_unchecked(u64::MAX - u64::from(u32::MAX));

    let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);
    let link_output = setup::build_empty_component_for_test(context.clone());

    let module = {
        let mut component_builder =
            midenc_hir::dialects::builtin::ComponentBuilder::new(link_output.component.unwrap());
        component_builder
            .define_module(midenc_hir::Ident::with_empty_span("test".into()))
            .unwrap()
    };

    // A u32 global whose initializer reinterprets the felt as a `u32`.
    let mut gv = {
        let mut module_builder = midenc_hir::dialects::builtin::ModuleBuilder::new(module);
        module_builder
            .define_global_variable(
                midenc_hir::Ident::with_empty_span("gv_felt".into()),
                midenc_hir::Visibility::Private,
                Type::U32,
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
        let value = op_builder.felt(value, span);
        let value = op_builder.bitcast(value, Type::U32, span).unwrap();
        op_builder.ret(Some(value), span).unwrap();
    }

    // Entrypoint: load the global and return it as a felt.
    let signature = Signature::new(&context, [], [Type::Felt]);
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
        let loaded = builder.bitcast(loaded, Type::Felt, span).unwrap();
        builder.ret(Some(loaded), span).unwrap();
    }

    eval_miden_component::<Felt, _, _>(
        link_output,
        std::iter::empty::<Initializer<'_>>(),
        &[],
        context.session(),
        |trace| {
            assert_eq!(trace.outputs().get_num_elements(1), [value]);
            Ok(())
        },
    )
    .unwrap();
}

/// A `u128` global initialized from four felts that do not fit in 32 bits holds them intact.
///
/// The global is word-aligned, so the initializer stores the value at its constant address with
/// one word store, which moves the four elements as they are, as a store to a dynamic address
/// does: there is no `u32` range check to trap on. The entrypoint loads the global, from its
/// address on the operand stack, and returns its four limbs.
#[test]
fn global_u128_initializer_stores_four_felts_intact() {
    setup::enable_compiler_instrumentation();

    // Least significant first
    let limbs = [
        Felt::new_unchecked(u64::MAX - u64::from(u32::MAX)),
        Felt::new_unchecked(1 << 40),
        Felt::new_unchecked((1 << 33) + 1),
        Felt::new_unchecked(1 << 63),
    ];

    let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);
    let link_output = setup::build_empty_component_for_test(context.clone());

    let module = {
        let mut component_builder =
            midenc_hir::dialects::builtin::ComponentBuilder::new(link_output.component.unwrap());
        component_builder
            .define_module(midenc_hir::Ident::with_empty_span("test".into()))
            .unwrap()
    };

    // A u128 global whose initializer joins the four felts as its limbs.
    let mut gv = {
        let mut module_builder = midenc_hir::dialects::builtin::ModuleBuilder::new(module);
        module_builder
            .define_global_variable(
                midenc_hir::Ident::with_empty_span("gv_felt_quad".into()),
                midenc_hir::Visibility::Private,
                Type::U128,
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
        // `arith.join` takes the most significant limb first
        let x3 = op_builder.felt(limbs[3], span);
        let x2 = op_builder.felt(limbs[2], span);
        let x1 = op_builder.felt(limbs[1], span);
        let x0 = op_builder.felt(limbs[0], span);
        let quad = op_builder.join4([x3, x2, x1, x0], Type::U128, span).unwrap();
        op_builder.ret(Some(quad), span).unwrap();
    }

    // Entrypoint: load the global and return its limbs, the least significant one on top.
    let signature = Signature::new(&context, [], [Type::Felt, Type::Felt, Type::Felt, Type::Felt]);
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
        let [x3, x2, x1, x0] = builder.split4(loaded, Type::Felt, span).unwrap();
        builder.ret([x0, x1, x2, x3], span).unwrap();
    }

    eval_miden_component::<Felt, _, _>(
        link_output,
        std::iter::empty::<Initializer<'_>>(),
        &[],
        context.session(),
        |trace| {
            assert_eq!(trace.outputs().get_num_elements(4), limbs);
            Ok(())
        },
    )
    .unwrap();
}

/// 8-, 16- and 1-bit global initializers store into their own bytes of the element they share
/// with their neighbours, and consume their value.
///
/// Globals are laid out in definition order, each at the next offset aligned for its type, from an
/// element-aligned base. So the three `u8`s and the `i1` fill one element, and their initializers
/// store at byte offsets 0 to 3 of a constant address; the `u16` starts the next element, at offset
/// 0, followed by two more `u8`s at offsets 2 and 3. The entrypoint loads every global back and
/// returns them: a store that wrote the wrong bits, or into a neighbour, shows in its output. The
/// operand stack below the outputs must still hold the zeros the program started with: a store that
/// left its value on the stack would show there.
#[test]
fn global_small_initializers_store_into_their_own_bytes() {
    setup::enable_compiler_instrumentation();

    let globals = [
        (Type::U8, Immediate::U8(0x11)),
        (Type::U8, Immediate::U8(0x22)),
        (Type::U8, Immediate::U8(0x33)),
        (Type::I1, Immediate::I1(true)),
        (Type::U16, Immediate::U16(0xbeef)),
        (Type::U8, Immediate::U8(0x44)),
        (Type::U8, Immediate::U8(0x55)),
    ];

    let context = setup::dummy_context(&["--test-harness", "--entrypoint", "test::main"]);
    let link_output = setup::build_empty_component_for_test(context.clone());

    let module = {
        let mut component_builder =
            midenc_hir::dialects::builtin::ComponentBuilder::new(link_output.component.unwrap());
        component_builder
            .define_module(midenc_hir::Ident::with_empty_span("test".into()))
            .unwrap()
    };

    // Each global's initializer returns its value as a literal.
    let gvs = globals
        .iter()
        .enumerate()
        .map(|(i, (ty, value))| {
            let mut gv = {
                let mut module_builder = midenc_hir::dialects::builtin::ModuleBuilder::new(module);
                let name = midenc_hir::interner::Symbol::intern(format!("gv_small_{i}"));
                module_builder
                    .define_global_variable(
                        midenc_hir::Ident::with_empty_span(name),
                        midenc_hir::Visibility::Private,
                        ty.clone(),
                    )
                    .unwrap()
            };
            let init_region_ref = {
                let mut global_var = gv.borrow_mut();
                global_var.initializer_mut().as_region_ref()
            };
            let mut op_builder = midenc_hir::OpBuilder::new(context.clone());
            op_builder.create_block(init_region_ref, None, &[]);
            op_builder.ret_imm(*value, SourceSpan::default()).unwrap();
            gv
        })
        .collect::<Vec<_>>();

    // Entrypoint: load every global and return them, the first on top.
    let signature = Signature::new(&context, [], globals.iter().map(|(ty, _)| ty.clone()));
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
        let loaded =
            gvs.iter().map(|gv| builder.load_global(*gv, span).unwrap()).collect::<Vec<_>>();
        builder.ret(loaded, span).unwrap();
    }

    let mut expected = [Felt::ZERO; 16];
    for (output, (_, value)) in expected.iter_mut().zip(&globals) {
        *output = value.as_felt().unwrap();
    }
    eval_miden_component::<Felt, _, _>(
        link_output,
        std::iter::empty::<Initializer<'_>>(),
        &[],
        context.session(),
        |trace| {
            assert_eq!(trace.outputs().get_num_elements(16), expected);
            Ok(())
        },
    )
    .unwrap();
}
