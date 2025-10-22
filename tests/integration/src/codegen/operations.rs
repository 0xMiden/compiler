use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_hir::{
    dialects::builtin::BuiltinOpBuilder, AbiParam, Felt, Immediate, Signature, SourceSpan, Type,
    ValueRef,
};

use crate::testing::{compile_test_module, eval_package, Initializer};

fn run_select_test(ty: Type, a: Immediate, a_result: &[u64], b: Immediate, b_result: &[u64]) {
    let span = SourceSpan::default();

    // Wrap 'select' in a function which takes a bool and returns selection from consts.
    let signature = Signature::new([AbiParam::new(Type::I1)], [AbiParam::new(ty)]);

    let (package, context) = compile_test_module(signature, |builder| {
        let block = builder.current_block();
        let cond_val = block.borrow().arguments()[0] as ValueRef;

        let a_imm = builder.imm(a, span);
        let b_imm = builder.imm(b, span);

        let result_val = builder.select(cond_val, a_imm, b_imm, span).unwrap();

        builder.ret(Some(result_val), span).unwrap();
    });

    let run_test = |cond_val, expected: &[u64]| {
        // XXX: The initialisers can't be empty otherwise eval_package() will abort early.
        let inits = [Initializer::Value {
            addr: 0,
            value: Box::new(Felt::from(0_u32)),
        }];

        eval_package::<u32, _, _>(
            &package,
            inits,
            &[Felt::from(cond_val)],
            context.session(),
            |trace| {
                let outputs = trace.outputs().as_int_vec();
                let len = expected.len();

                // Ensure the expected values are at the top of the stack and the rest are zeroes.
                assert_eq!(&outputs.as_slice()[..len], expected);
                assert!(outputs[len..].iter().all(|el| *el == 0));

                Ok(())
            },
        )
        .unwrap();
    };

    run_test(true, a_result);
    run_test(false, b_result);
}

macro_rules! simple_select {
    ($ty: ident, $a: literal, $b: literal) => {
        // This is a bit basic, and will break if $a or $b are negative.
        run_select_test(
            Type::$ty,
            Immediate::$ty($a),
            &[$a as u64],
            Immediate::$ty($b),
            &[$b as u64],
        );
    };
}

#[test]
fn select_u32() {
    simple_select!(U32, 11111111, 22222222);
}

#[test]
fn select_i32() {
    simple_select!(I32, 11111111, 22222222);
}

#[test]
fn select_u16() {
    simple_select!(U16, 11111, 22222);
}

#[test]
fn select_i16() {
    simple_select!(I16, 11111, 22222);
}

#[test]
fn select_u8() {
    simple_select!(U8, 111, 222);
}

#[test]
fn select_i8() {
    simple_select!(I8, 11, 22);
}

#[test]
fn select_i1() {
    simple_select!(I1, true, false);
}

#[test]
fn select_felt() {
    run_select_test(
        Type::Felt,
        Immediate::Felt(Felt::new(1111111111111111)),
        &[1111111111111111_u64],
        Immediate::Felt(Felt::new(2222222222222222)),
        &[2222222222222222_u64],
    );
}

#[test]
fn select_u64() {
    // U64 is split into two 32bit limbs.
    run_select_test(
        Type::U64,
        Immediate::U64(1111111111111111),
        &[1111111111111111_u64 >> 32, 1111111111111111_u64 & 0xffffffff],
        Immediate::U64(2222222222222222),
        &[2222222222222222_u64 >> 32, 2222222222222222_u64 & 0xffffffff],
    );
}

#[test]
fn select_i64() {
    // I64 is split into two 32bit limbs.
    run_select_test(
        Type::I64,
        Immediate::I64(1111111111111111),
        &[1111111111111111_u64 >> 32, 1111111111111111_u64 & 0xffffffff],
        Immediate::I64(2222222222222222),
        &[2222222222222222_u64 >> 32, 2222222222222222_u64 & 0xffffffff],
    );
}

#[test]
fn select_u128() {
    // U128 is split into four 32bit limbs.
    //
    // It also mixes them up based on the virtual 64bit limbs.
    let ones = 1111111111111111111111111111111_u128;
    let twos = 2222222222222222222222222222222_u128;
    run_select_test(
        Type::U128,
        Immediate::U128(ones),
        &[
            ((ones >> 32) & 0xffffffff) as u64, // lo_mid
            (ones & 0xffffffff) as u64,         // lo_lo
            (ones >> 96) as u64,                // hi_hi
            ((ones >> 64) & 0xffffffff) as u64, // hi_mid
        ],
        Immediate::U128(twos),
        &[
            ((twos >> 32) & 0xffffffff) as u64, // lo_mid
            (twos & 0xffffffff) as u64,         // lo_lo
            (twos >> 96) as u64,                // hi_hi
            ((twos >> 64) & 0xffffffff) as u64, // hi_mid
        ],
    );
}

#[test]
fn select_i128() {
    // I128 is split into four 32bit limbs.
    //
    // It also mixes them up based on the virtual 64bit limbs.
    let ones = 1111111111111111111111111111111_i128;
    let twos = 2222222222222222222222222222222_i128;
    run_select_test(
        Type::I128,
        Immediate::I128(ones),
        &[
            ((ones >> 32) & 0xffffffff) as u64, // lo_mid
            (ones & 0xffffffff) as u64,         // lo_lo
            (ones >> 96) as u64,                // hi_hi
            ((ones >> 64) & 0xffffffff) as u64, // hi_mid
        ],
        Immediate::I128(twos),
        &[
            ((twos >> 32) & 0xffffffff) as u64, // lo_mid
            (twos & 0xffffffff) as u64,         // lo_lo
            (twos >> 96) as u64,                // hi_hi
            ((twos >> 64) & 0xffffffff) as u64, // hi_mid
        ],
    );
}
