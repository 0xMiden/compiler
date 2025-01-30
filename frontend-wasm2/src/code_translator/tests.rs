// TODO: remove when completed
#![allow(unused)]

use core::fmt::Write;
use std::{any::Any, rc::Rc};

use expect_test::expect;
use midenc_hir2::{
    dialects::builtin::{self, Module},
    Op,
};

use crate::{translate, WasmTranslationConfig};

/// Check IR generated for a Wasm op(s).
/// Wrap Wasm ops in a function and check the IR generated for the entry block of that function.
fn check_op(wat_op: &str, expected_ir: expect_test::Expect) {
    let ctx = midenc_hir2::Context::default();
    let context = Rc::new(ctx);

    let wat = format!(
        r#"
        (module
            (memory (;0;) 16384)
            (func $test_wrapper
                {wat_op}
            )
        )"#,
    );
    let wasm = wat::parse_str(wat).unwrap();
    let component_ref = translate(&wasm, &WasmTranslationConfig::default(), context.clone())
        .map_err(|e| {
            let labels = e.labels().unwrap().collect::<Vec<midenc_hir::diagnostics::LabeledSpan>>();
            if let Some(labels) = e.labels() {
                for label in labels {
                    eprintln!("{}", label.label().unwrap());
                }
            }
            let report = midenc_hir::diagnostics::PrintDiagnostic::new(e).to_string();
            eprintln!("{report}");
        })
        .unwrap();

    let borrow = component_ref.borrow();
    let body = borrow.body();
    let mut w = String::new();
    for item in body.entry().body() {
        if let Some(module) = item.downcast_ref::<builtin::Module>() {
            let module_body = module.body();

            let module_body = module_body.entry();
            for item in module_body.body() {
                if let Some(function) = item.downcast_ref::<builtin::Function>() {
                    let function_str = function.as_operation().to_string();
                    writeln!(&mut w, "{function_str}");
                }
            }
        }
    }

    expected_ir.assert_eq(&w);
}

#[test]
fn memory_grow() {
    check_op(
        r#"
            i32.const 1
            memory.grow
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 1 : i32;
                v1 = hir.bitcast v0 : ? #[ty = u32];
                v2 = hir.mem_grow v1 : ?;
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn memory_size() {
    check_op(
        r#"
            memory.size
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.mem_size  : ?;
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn memory_copy() {
    check_op(
        r#"
            i32.const 20 ;; dst
            i32.const 10 ;; src
            i32.const 1  ;; len
            memory.copy
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 20 : i32;
                v1 = hir.constant 10 : i32;
                v2 = hir.constant 1 : i32;
                v3 = hir.bitcast v2 : ? #[ty = u32];
                v4 = hir.bitcast v0 : ? #[ty = u32];
                v5 = hir.int_to_ptr v4 : ? #[ty = (ptr u8)];
                v6 = hir.bitcast v1 : ? #[ty = u32];
                v7 = hir.int_to_ptr v6 : ? #[ty = (ptr u8)];
                hir.mem_cpy v7, v5, v3;
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_load8_u() {
    check_op(
        r#"
            i32.const 1024
            i32.load8_u
            drop
        "#,
        expect![[r#"
            (let (v0 i32) (const.i32 1024))
            (let (v1 u32) (bitcast v0))
            (let (v2 (ptr u8)) (inttoptr v1))
            (let (v3 u8) (load v2))
            (let (v4 i32) (zext v3))
        "#]],
    )
}

#[test]
fn i32_load16_u() {
    check_op(
        r#"
            i32.const 1024
            i32.load16_u
            drop
        "#,
        expect![[r#"
            (let (v0 i32) (const.i32 1024))
            (let (v1 u32) (bitcast v0))
            (let (v2 u32) (mod.unchecked v1 2))
            (assertz 250 v2)
            (let (v3 (ptr u16)) (inttoptr v1))
            (let (v4 u16) (load v3))
            (let (v5 i32) (zext v4))
        "#]],
    )
}

#[test]
fn i32_load8_s() {
    check_op(
        r#"
            i32.const 1024
            i32.load8_s
            drop
        "#,
        expect![[r#"
            (let (v0 i32) (const.i32 1024))
            (let (v1 u32) (bitcast v0))
            (let (v2 (ptr i8)) (inttoptr v1))
            (let (v3 i8) (load v2))
            (let (v4 i32) (sext v3))
        "#]],
    )
}

#[test]
fn i32_load16_s() {
    check_op(
        r#"
            i32.const 1024
            i32.load16_s
            drop
        "#,
        expect![[r#"
            (let (v0 i32) (const.i32 1024))
            (let (v1 u32) (bitcast v0))
            (let (v2 u32) (mod.unchecked v1 2))
            (assertz 250 v2)
            (let (v3 (ptr i16)) (inttoptr v1))
            (let (v4 i16) (load v3))
            (let (v5 i32) (sext v4))
        "#]],
    )
}

#[test]
fn i64_load8_u() {
    check_op(
        r#"
            i32.const 1024
            i64.load8_u
            drop
        "#,
        expect![[r#"
            (let (v0 i32) (const.i32 1024))
            (let (v1 u32) (bitcast v0))
            (let (v2 (ptr u8)) (inttoptr v1))
            (let (v3 u8) (load v2))
            (let (v4 i64) (zext v3))
        "#]],
    )
}

#[test]
fn i64_load16_u() {
    check_op(
        r#"
            i32.const 1024
            i64.load16_u
            drop
        "#,
        expect![[r#"
            (let (v0 i32) (const.i32 1024))
            (let (v1 u32) (bitcast v0))
            (let (v2 u32) (mod.unchecked v1 2))
            (assertz 250 v2)
            (let (v3 (ptr u16)) (inttoptr v1))
            (let (v4 u16) (load v3))
            (let (v5 i64) (zext v4))
        "#]],
    )
}

#[test]
fn i64_load8_s() {
    check_op(
        r#"
            i32.const 1024
            i64.load8_s
            drop
        "#,
        expect![[r#"
            (let (v0 i32) (const.i32 1024))
            (let (v1 u32) (bitcast v0))
            (let (v2 (ptr i8)) (inttoptr v1))
            (let (v3 i8) (load v2))
            (let (v4 i64) (sext v3))
        "#]],
    )
}

#[test]
fn i64_load16_s() {
    check_op(
        r#"
            i32.const 1024
            i64.load16_s
            drop
        "#,
        expect![[r#"
            (let (v0 i32) (const.i32 1024))
            (let (v1 u32) (bitcast v0))
            (let (v2 u32) (mod.unchecked v1 2))
            (assertz 250 v2)
            (let (v3 (ptr i16)) (inttoptr v1))
            (let (v4 i16) (load v3))
            (let (v5 i64) (sext v4))
        "#]],
    )
}

#[test]
fn i64_load32_s() {
    check_op(
        r#"
            i32.const 1024
            i64.load32_s
            drop
        "#,
        expect![[r#"
            (let (v0 i32) (const.i32 1024))
            (let (v1 u32) (bitcast v0))
            (let (v2 u32) (mod.unchecked v1 4))
            (assertz 250 v2)
            (let (v3 (ptr i32)) (inttoptr v1))
            (let (v4 i32) (load v3))
            (let (v5 i64) (sext v4))
        "#]],
    )
}

#[test]
fn i64_load32_u() {
    check_op(
        r#"
            i32.const 1024
            i64.load32_u
            drop
        "#,
        expect![[r#"
            (let (v0 i32) (const.i32 1024))
            (let (v1 u32) (bitcast v0))
            (let (v2 u32) (mod.unchecked v1 4))
            (assertz 250 v2)
            (let (v3 (ptr u32)) (inttoptr v1))
            (let (v4 u32) (load v3))
            (let (v5 i64) (zext v4))
        "#]],
    )
}

#[test]
fn i32_load() {
    check_op(
        r#"
            i32.const 1024
            i32.load
            drop
        "#,
        expect![[r#"
            (let (v0 i32) (const.i32 1024))
            (let (v1 u32) (bitcast v0))
            (let (v2 u32) (mod.unchecked v1 4))
            (assertz 250 v2)
            (let (v3 (ptr i32)) (inttoptr v1))
            (let (v4 i32) (load v3))
        "#]],
    )
}

#[test]
fn i64_load() {
    check_op(
        r#"
            i32.const 1024
            i64.load
            drop
        "#,
        expect![[r#"
            (let (v0 i32) (const.i32 1024))
            (let (v1 u32) (bitcast v0))
            (let (v2 u32) (mod.unchecked v1 8))
            (assertz 250 v2)
            (let (v3 (ptr i64)) (inttoptr v1))
            (let (v4 i64) (load v3))
        "#]],
    )
}

#[test]
fn i32_store() {
    check_op(
        r#"
            i32.const 1024
            i32.const 1
            i32.store
        "#,
        expect![[r#"
            (let (v0 i32) (const.i32 1024))
            (let (v1 i32) (const.i32 1))
            (let (v2 u32) (bitcast v0))
            (let (v3 u32) (mod.unchecked v2 4))
            (assertz 250 v3)
            (let (v4 (ptr i32)) (inttoptr v2))
            (store v4 v1)
        "#]],
    )
}

#[test]
fn i64_store() {
    check_op(
        r#"
            i32.const 1024
            i64.const 1
            i64.store
        "#,
        expect![[r#"
            (let (v0 i32) (const.i32 1024))
            (let (v1 i64) (const.i64 1))
            (let (v2 u32) (bitcast v0))
            (let (v3 u32) (mod.unchecked v2 8))
            (assertz 250 v3)
            (let (v4 (ptr i64)) (inttoptr v2))
            (store v4 v1)
        "#]],
    )
}

#[test]
fn i32_store8() {
    check_op(
        r#"
            i32.const 1024
            i32.const 1
            i32.store8
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 1024 : i32;
                v1 = hir.constant 1 : i32;
                v2 = hir.bitcast v1 : ? #[ty = u32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_store16() {
    check_op(
        r#"
            i32.const 1024
            i32.const 1
            i32.store16
        "#,
        expect![[r#"
            (let (v0 i32) (const.i32 1024))
            (let (v1 i32) (const.i32 1))
            (let (v2 u32) (bitcast v1))
            (let (v3 u16) (trunc v2))
            (let (v4 u32) (bitcast v0))
            (let (v5 u32) (mod.unchecked v4 2))
            (assertz 250 v5)
            (let (v6 (ptr u16)) (inttoptr v4))
            (store v6 v3)
        "#]],
    )
}

#[test]
fn i64_store32() {
    check_op(
        r#"
            i32.const 1024
            i64.const 1
            i64.store32
        "#,
        expect![[r#"
            (let (v0 i32) (const.i32 1024))
            (let (v1 i64) (const.i64 1))
            (let (v2 u64) (bitcast v1))
            (let (v3 u32) (trunc v2))
            (let (v4 u32) (bitcast v0))
            (let (v5 u32) (mod.unchecked v4 4))
            (assertz 250 v5)
            (let (v6 (ptr u32)) (inttoptr v4))
            (store v6 v3)
        "#]],
    )
}

#[test]
fn i32_const() {
    check_op(
        r#"
            i32.const 1
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 1 : i32;
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_const() {
    check_op(
        r#"
            i64.const 1
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 1 : i64;
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_popcnt() {
    check_op(
        r#"
            i32.const 1
            i32.popcnt
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 1 : i32;
                v1 = hir.popcnt v0 : ?;
                v2 = hir.bitcast v1 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_clz() {
    check_op(
        r#"
            i32.const 1
            i32.clz
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 1 : i32;
                v1 = hir.clz v0 : ?;
                v2 = hir.bitcast v1 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_clz() {
    check_op(
        r#"
            i64.const 1
            i64.clz
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 1 : i64;
                v1 = hir.clz v0 : ?;
                v2 = hir.bitcast v1 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_ctz() {
    check_op(
        r#"
            i32.const 1
            i32.ctz
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 1 : i32;
                v1 = hir.ctz v0 : ?;
                v2 = hir.bitcast v1 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_ctz() {
    check_op(
        r#"
            i64.const 1
            i64.ctz
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 1 : i64;
                v1 = hir.ctz v0 : ?;
                v2 = hir.bitcast v1 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_extend_i32_s() {
    check_op(
        r#"
            i32.const 1
            i64.extend_i32_s
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 1 : i32;
                v1 = hir.sext v0 : ? #[ty = i64];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_extend_i32_u() {
    check_op(
        r#"
            i32.const 1
            i64.extend_i32_u
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 1 : i32;
                v1 = hir.bitcast v0 : ? #[ty = u32];
                v2 = hir.zext v1 : ? #[ty = u64];
                v3 = hir.bitcast v2 : ? #[ty = i64];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_wrap_i64() {
    check_op(
        r#"
            i64.const 1
            i32.wrap_i64
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 1 : i64;
                v1 = hir.trunc v0 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_add() {
    check_op(
        r#"
            i32.const 3
            i32.const 1
            i32.add
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 3 : i32;
                v1 = hir.constant 1 : i32;
                v2 = hir.add v0, v1 : i32 #[overflow = wrapping];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_add() {
    check_op(
        r#"
            i64.const 3
            i64.const 1
            i64.add
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 3 : i64;
                v1 = hir.constant 1 : i64;
                v2 = hir.add v0, v1 : i64 #[overflow = wrapping];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_and() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.and
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i32;
                v1 = hir.constant 1 : i32;
                v2 = hir.band v0, v1 : i32;
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_and() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.and
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i64;
                v1 = hir.constant 1 : i64;
                v2 = hir.band v0, v1 : i64;
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_or() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.or
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i32;
                v1 = hir.constant 1 : i32;
                v2 = hir.bor v0, v1 : i32;
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_or() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.or
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i64;
                v1 = hir.constant 1 : i64;
                v2 = hir.bor v0, v1 : i64;
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_sub() {
    check_op(
        r#"
            i32.const 3
            i32.const 1
            i32.sub
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 3 : i32;
                v1 = hir.constant 1 : i32;
                v2 = hir.sub v0, v1 : i32 #[overflow = wrapping];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_sub() {
    check_op(
        r#"
            i64.const 3
            i64.const 1
            i64.sub
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 3 : i64;
                v1 = hir.constant 1 : i64;
                v2 = hir.sub v0, v1 : i64 #[overflow = wrapping];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_xor() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.xor
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i32;
                v1 = hir.constant 1 : i32;
                v2 = hir.bxor v0, v1 : i32;
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_xor() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.xor
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i64;
                v1 = hir.constant 1 : i64;
                v2 = hir.bxor v0, v1 : i64;
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_shl() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.shl
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i32;
                v1 = hir.constant 1 : i32;
                v2 = hir.bitcast v1 : ? #[ty = u32];
                v3 = hir.shl v0, v2 : i32;
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_shl() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.shl
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i64;
                v1 = hir.constant 1 : i64;
                v2 = hir.cast v1 : ? #[ty = u32];
                v3 = hir.shl v0, v2 : i64;
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_shr_u() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.shr_u
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i32;
                v1 = hir.constant 1 : i32;
                v2 = hir.bitcast v0 : ? #[ty = u32];
                v3 = hir.bitcast v1 : ? #[ty = u32];
                v4 = hir.shr v2, v3 : ?;
                v5 = hir.bitcast v4 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_shr_u() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.shr_u
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i64;
                v1 = hir.constant 1 : i64;
                v2 = hir.bitcast v0 : ? #[ty = u64];
                v3 = hir.cast v1 : ? #[ty = u32];
                v4 = hir.shr v2, v3 : ?;
                v5 = hir.bitcast v4 : ? #[ty = i64];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_shr_s() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.shr_s
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i32;
                v1 = hir.constant 1 : i32;
                v2 = hir.bitcast v1 : ? #[ty = u32];
                v3 = hir.shr v0, v2 : i32;
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_shr_s() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.shr_s
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i64;
                v1 = hir.constant 1 : i64;
                v2 = hir.cast v1 : ? #[ty = u32];
                v3 = hir.shr v0, v2 : i64;
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_rotl() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.rotl
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i32;
                v1 = hir.constant 1 : i32;
                v2 = hir.bitcast v1 : ? #[ty = u32];
                v3 = hir.rotl v0, v2 : i32;
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_rotl() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.rotl
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i64;
                v1 = hir.constant 1 : i64;
                v2 = hir.cast v1 : ? #[ty = u32];
                v3 = hir.rotl v0, v2 : i64;
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_rotr() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.rotr
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i32;
                v1 = hir.constant 1 : i32;
                v2 = hir.bitcast v1 : ? #[ty = u32];
                v3 = hir.rotr v0, v2 : i32;
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_rotr() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.rotr
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i64;
                v1 = hir.constant 1 : i64;
                v2 = hir.cast v1 : ? #[ty = u32];
                v3 = hir.rotr v0, v2 : i64;
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_mul() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.mul
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i32;
                v1 = hir.constant 1 : i32;
                v2 = hir.mul v0, v1 : i32 #[overflow = wrapping];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_mul() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.mul
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i64;
                v1 = hir.constant 1 : i64;
                v2 = hir.mul v0, v1 : i64 #[overflow = wrapping];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_div_u() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.div_u
            drop
        "#,
        expect![[r#"
            (let (v0 i32) (const.i32 2))
            (let (v1 i32) (const.i32 1))
            (let (v2 u32) (bitcast v0))
            (let (v3 u32) (bitcast v1))
            (let (v4 u32) (div.unchecked v2 v3))
            (let (v5 i32) (bitcast v4))
        "#]],
    )
}

#[test]
fn i64_div_u() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.div_u
            drop
        "#,
        expect![[r#"
            (let (v0 i64) (const.i64 2))
            (let (v1 i64) (const.i64 1))
            (let (v2 u64) (bitcast v0))
            (let (v3 u64) (bitcast v1))
            (let (v4 u64) (div.unchecked v2 v3))
            (let (v5 i64) (bitcast v4))
        "#]],
    )
}

#[test]
fn i32_div_s() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.div_s
            drop
        "#,
        expect![[r#"
            (let (v0 i32) (const.i32 2))
            (let (v1 i32) (const.i32 1))
            (let (v2 i32) (div.unchecked v0 v1))
        "#]],
    )
}

#[test]
fn i64_div_s() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.div_s
            drop
        "#,
        expect![[r#"
            (let (v0 i64) (const.i64 2))
            (let (v1 i64) (const.i64 1))
            (let (v2 i64) (div.unchecked v0 v1))
        "#]],
    )
}

#[test]
fn i32_rem_u() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.rem_u
            drop
        "#,
        expect![[r#"
            (let (v0 i32) (const.i32 2))
            (let (v1 i32) (const.i32 1))
            (let (v2 u32) (bitcast v0))
            (let (v3 u32) (bitcast v1))
            (let (v4 u32) (mod.checked v2 v3))
            (let (v5 i32) (bitcast v4))
        "#]],
    )
}

#[test]
fn i64_rem_u() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.rem_u
            drop
        "#,
        expect![[r#"
            (let (v0 i64) (const.i64 2))
            (let (v1 i64) (const.i64 1))
            (let (v2 u64) (bitcast v0))
            (let (v3 u64) (bitcast v1))
            (let (v4 u64) (mod.checked v2 v3))
            (let (v5 i64) (bitcast v4))
        "#]],
    )
}

#[test]
fn i32_rem_s() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.rem_s
            drop
        "#,
        expect![[r#"
            (let (v0 i32) (const.i32 2))
            (let (v1 i32) (const.i32 1))
            (let (v2 i32) (mod.checked v0 v1))
        "#]],
    )
}

#[test]
fn i64_rem_s() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.rem_s
            drop
        "#,
        expect![[r#"
            (let (v0 i64) (const.i64 2))
            (let (v1 i64) (const.i64 1))
            (let (v2 i64) (mod.checked v0 v1))
        "#]],
    )
}

#[test]
fn i32_lt_u() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.lt_u
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i32;
                v1 = hir.constant 1 : i32;
                v2 = hir.bitcast v0 : ? #[ty = u32];
                v3 = hir.bitcast v1 : ? #[ty = u32];
                v4 = hir.lt v2, v3 : i1;
                v5 = hir.sext v4 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_lt_u() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.lt_u
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i64;
                v1 = hir.constant 1 : i64;
                v2 = hir.bitcast v0 : ? #[ty = u64];
                v3 = hir.bitcast v1 : ? #[ty = u64];
                v4 = hir.lt v2, v3 : i1;
                v5 = hir.sext v4 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_lt_s() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.lt_s
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i32;
                v1 = hir.constant 1 : i32;
                v2 = hir.lt v0, v1 : i1;
                v3 = hir.sext v2 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_lt_s() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.lt_s
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i64;
                v1 = hir.constant 1 : i64;
                v2 = hir.lt v0, v1 : i1;
                v3 = hir.sext v2 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_le_u() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.le_u
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i32;
                v1 = hir.constant 1 : i32;
                v2 = hir.bitcast v0 : ? #[ty = u32];
                v3 = hir.bitcast v1 : ? #[ty = u32];
                v4 = hir.lte v2, v3 : i1;
                v5 = hir.sext v4 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_le_u() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.le_u
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i64;
                v1 = hir.constant 1 : i64;
                v2 = hir.bitcast v0 : ? #[ty = u64];
                v3 = hir.bitcast v1 : ? #[ty = u64];
                v4 = hir.lte v2, v3 : i1;
                v5 = hir.sext v4 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_le_s() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.le_s
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i32;
                v1 = hir.constant 1 : i32;
                v2 = hir.lte v0, v1 : i1;
                v3 = hir.sext v2 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_le_s() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.le_s
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i64;
                v1 = hir.constant 1 : i64;
                v2 = hir.lte v0, v1 : i1;
                v3 = hir.sext v2 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_gt_u() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.gt_u
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i32;
                v1 = hir.constant 1 : i32;
                v2 = hir.bitcast v0 : ? #[ty = u32];
                v3 = hir.bitcast v1 : ? #[ty = u32];
                v4 = hir.gt v2, v3 : i1;
                v5 = hir.sext v4 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_gt_u() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.gt_u
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i64;
                v1 = hir.constant 1 : i64;
                v2 = hir.bitcast v0 : ? #[ty = u64];
                v3 = hir.bitcast v1 : ? #[ty = u64];
                v4 = hir.gt v2, v3 : i1;
                v5 = hir.sext v4 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_gt_s() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.gt_s
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i32;
                v1 = hir.constant 1 : i32;
                v2 = hir.gt v0, v1 : i1;
                v3 = hir.zext v2 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_gt_s() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.gt_s
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i64;
                v1 = hir.constant 1 : i64;
                v2 = hir.gt v0, v1 : i1;
                v3 = hir.zext v2 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_ge_u() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.ge_u
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i32;
                v1 = hir.constant 1 : i32;
                v2 = hir.bitcast v0 : ? #[ty = u32];
                v3 = hir.bitcast v1 : ? #[ty = u32];
                v4 = hir.gte v2, v3 : i1;
                v5 = hir.zext v4 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_ge_u() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.ge_u
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i64;
                v1 = hir.constant 1 : i64;
                v2 = hir.bitcast v0 : ? #[ty = u64];
                v3 = hir.bitcast v1 : ? #[ty = u64];
                v4 = hir.gte v2, v3 : i1;
                v5 = hir.zext v4 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_ge_s() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.ge_s
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i32;
                v1 = hir.constant 1 : i32;
                v2 = hir.gte v0, v1 : i1;
                v3 = hir.zext v2 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_ge_s() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.ge_s
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i64;
                v1 = hir.constant 1 : i64;
                v2 = hir.gte v0, v1 : i1;
                v3 = hir.zext v2 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_eqz() {
    check_op(
        r#"
            i32.const 2
            i32.eqz
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i32;
                v1 = hir.constant 0 : i32;
                v2 = hir.eq v0, v1 : i1;
                v3 = hir.zext v2 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_eqz() {
    check_op(
        r#"
            i64.const 2
            i64.eqz
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i64;
                v1 = hir.constant 0 : i64;
                v2 = hir.eq v0, v1 : i1;
                v3 = hir.zext v2 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_eq() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.eq
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i32;
                v1 = hir.constant 1 : i32;
                v2 = hir.eq v0, v1 : i1;
                v3 = hir.zext v2 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_eq() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.eq
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i64;
                v1 = hir.constant 1 : i64;
                v2 = hir.eq v0, v1 : i1;
                v3 = hir.zext v2 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i32_ne() {
    check_op(
        r#"
            i32.const 2
            i32.const 1
            i32.ne
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i32;
                v1 = hir.constant 1 : i32;
                v2 = hir.neq v0, v1 : i1;
                v3 = hir.zext v2 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn i64_ne() {
    check_op(
        r#"
            i64.const 2
            i64.const 1
            i64.ne
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 2 : i64;
                v1 = hir.constant 1 : i64;
                v2 = hir.neq v0, v1 : i1;
                v3 = hir.zext v2 : ? #[ty = i32];
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}

#[test]
fn select_i32() {
    check_op(
        r#"
            i64.const 3
            i64.const 7
            i32.const 1
            select
            drop
        "#,
        expect![[r#"
            builtin.function internal @test_wrapper() {
            ^block2:
                v0 = hir.constant 3 : i64;
                v1 = hir.constant 7 : i64;
                v2 = hir.constant 1 : i32;
                v3 = hir.neq_imm v2 : i1 #[rhs = 0];
                v4 = hir.select v3, v0, v1 : i64;
                hir.br block3 ;
            ^block3:
                hir.ret ;
            };
        "#]],
    )
}
