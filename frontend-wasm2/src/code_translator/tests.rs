// TODO: remove when completed
#![allow(unused)]

use core::fmt::Write;

use expect_test::expect;
use midenc_hir::{Ident, Module};

use crate::{test_utils::test_context, translate, WasmTranslationConfig};

/// Check IR generated for a Wasm op(s).
/// Wrap Wasm ops in a function and check the IR generated for the entry block of that function.
fn check_op(wat_op: &str, expected_ir: expect_test::Expect) {
    let context = test_context();

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
    // let module = translate(&wasm, &WasmTranslationConfig::default(), &context.session)
    //     .unwrap()
    //     .unwrap_one_module();
    let module: Box<Module> = todo!();
    let func = module.function(Ident::from("test_wrapper")).unwrap();
    // let fref = module.get_funcref_by_name("test_wrapper").unwrap();
    // let func = module.get_function(fref).unwrap();
    let entry_block = func.dfg.entry_block();
    // let entry_block_data = func.dfg.block_data(entry_block);
    let entry_block_data = func.dfg.block(entry_block);
    let mut w = String::new();
    // print instructions up to the branch to the exit block
    for inst in entry_block_data
        .insts()
        .take_while(|inst| !func.dfg[*inst].opcode().is_branch())
    {
        let inst_printer = midenc_hir::InstPrettyPrinter {
            current_function: func.id,
            id: inst,
            dfg: &func.dfg,
        };
        writeln!(&mut w, "{inst_printer}").unwrap();
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
            (let (v0 i32) (const.i32 1))
            (let (v1 u32) (bitcast v0))
            (let (v2 i32) (memory.grow v1))
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
            (let (v0 u32) (memory.size))
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
            (let (v0 i32) (const.i32 20))
            (let (v1 i32) (const.i32 10))
            (let (v2 i32) (const.i32 1))
            (let (v3 u32) (bitcast v2))
            (let (v4 u32) (bitcast v0))
            (let (v5 (ptr u8)) (inttoptr v4))
            (let (v6 u32) (bitcast v1))
            (let (v7 (ptr u8)) (inttoptr v6))
            (memcpy v7 v5 v3)
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
            (let (v0 i32) (const.i32 1024))
            (let (v1 i32) (const.i32 1))
            (let (v2 u32) (bitcast v1))
            (let (v3 u8) (trunc v2))
            (let (v4 u32) (bitcast v0))
            (let (v5 (ptr u8)) (inttoptr v4))
            (store v5 v3)
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
            (let (v0 i32) (const.i32 1))
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
            (let (v0 i64) (const.i64 1))
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
            (let (v0 i32) (const.i32 1))
            (let (v1 u32) (popcnt v0))
            (let (v2 i32) (bitcast v1))
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
            (let (v0 i32) (const.i32 1))
            (let (v1 u32) (clz v0))
            (let (v2 i32) (bitcast v1))
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
            (let (v0 i64) (const.i64 1))
            (let (v1 u32) (clz v0))
            (let (v2 i32) (bitcast v1))
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
            (let (v0 i32) (const.i32 1))
            (let (v1 u32) (ctz v0))
            (let (v2 i32) (bitcast v1))
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
            (let (v0 i64) (const.i64 1))
            (let (v1 u32) (ctz v0))
            (let (v2 i32) (bitcast v1))
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
            (let (v0 i32) (const.i32 1))
            (let (v1 i64) (sext v0))
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
            (let (v0 i32) (const.i32 1))
            (let (v1 u32) (bitcast v0))
            (let (v2 u64) (zext v1))
            (let (v3 i64) (bitcast v2))
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
            (let (v0 i64) (const.i64 1))
            (let (v1 i32) (trunc v0))
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
            (let (v0 i32) (const.i32 3))
            (let (v1 i32) (const.i32 1))
            (let (v2 i32) (add.wrapping v0 v1))
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
            (let (v0 i64) (const.i64 3))
            (let (v1 i64) (const.i64 1))
            (let (v2 i64) (add.wrapping v0 v1))
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
            (let (v0 i32) (const.i32 2))
            (let (v1 i32) (const.i32 1))
            (let (v2 i32) (band v0 v1))
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
            (let (v0 i64) (const.i64 2))
            (let (v1 i64) (const.i64 1))
            (let (v2 i64) (band v0 v1))
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
            (let (v0 i32) (const.i32 2))
            (let (v1 i32) (const.i32 1))
            (let (v2 i32) (bor v0 v1))
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
            (let (v0 i64) (const.i64 2))
            (let (v1 i64) (const.i64 1))
            (let (v2 i64) (bor v0 v1))
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
            (let (v0 i32) (const.i32 3))
            (let (v1 i32) (const.i32 1))
            (let (v2 i32) (sub.wrapping v0 v1))
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
            (let (v0 i64) (const.i64 3))
            (let (v1 i64) (const.i64 1))
            (let (v2 i64) (sub.wrapping v0 v1))
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
            (let (v0 i32) (const.i32 2))
            (let (v1 i32) (const.i32 1))
            (let (v2 i32) (bxor v0 v1))
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
            (let (v0 i64) (const.i64 2))
            (let (v1 i64) (const.i64 1))
            (let (v2 i64) (bxor v0 v1))
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
            (let (v0 i32) (const.i32 2))
            (let (v1 i32) (const.i32 1))
            (let (v2 u32) (bitcast v1))
            (let (v3 i32) (shl.wrapping v0 v2))
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
            (let (v0 i64) (const.i64 2))
            (let (v1 i64) (const.i64 1))
            (let (v2 u32) (cast v1))
            (let (v3 i64) (shl.wrapping v0 v2))
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
            (let (v0 i32) (const.i32 2))
            (let (v1 i32) (const.i32 1))
            (let (v2 u32) (bitcast v0))
            (let (v3 u32) (bitcast v1))
            (let (v4 u32) (shr.wrapping v2 v3))
            (let (v5 i32) (bitcast v4))
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
            (let (v0 i64) (const.i64 2))
            (let (v1 i64) (const.i64 1))
            (let (v2 u64) (bitcast v0))
            (let (v3 u32) (cast v1))
            (let (v4 u64) (shr.wrapping v2 v3))
            (let (v5 i64) (bitcast v4))
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
            (let (v0 i32) (const.i32 2))
            (let (v1 i32) (const.i32 1))
            (let (v2 u32) (bitcast v1))
            (let (v3 i32) (shr.wrapping v0 v2))
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
            (let (v0 i64) (const.i64 2))
            (let (v1 i64) (const.i64 1))
            (let (v2 u32) (cast v1))
            (let (v3 i64) (shr.wrapping v0 v2))
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
            (let (v0 i32) (const.i32 2))
            (let (v1 i32) (const.i32 1))
            (let (v2 u32) (bitcast v1))
            (let (v3 i32) (rotl.wrapping v0 v2))
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
            (let (v0 i64) (const.i64 2))
            (let (v1 i64) (const.i64 1))
            (let (v2 u32) (cast v1))
            (let (v3 i64) (rotl.wrapping v0 v2))
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
            (let (v0 i32) (const.i32 2))
            (let (v1 i32) (const.i32 1))
            (let (v2 u32) (bitcast v1))
            (let (v3 i32) (rotr.wrapping v0 v2))
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
            (let (v0 i64) (const.i64 2))
            (let (v1 i64) (const.i64 1))
            (let (v2 u32) (cast v1))
            (let (v3 i64) (rotr.wrapping v0 v2))
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
            (let (v0 i32) (const.i32 2))
            (let (v1 i32) (const.i32 1))
            (let (v2 i32) (mul.wrapping v0 v1))
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
            (let (v0 i64) (const.i64 2))
            (let (v1 i64) (const.i64 1))
            (let (v2 i64) (mul.wrapping v0 v1))
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
            (let (v0 i32) (const.i32 2))
            (let (v1 i32) (const.i32 1))
            (let (v2 u32) (bitcast v0))
            (let (v3 u32) (bitcast v1))
            (let (v4 i1) (lt v2 v3))
            (let (v5 i32) (sext v4))
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
            (let (v0 i64) (const.i64 2))
            (let (v1 i64) (const.i64 1))
            (let (v2 u64) (bitcast v0))
            (let (v3 u64) (bitcast v1))
            (let (v4 i1) (lt v2 v3))
            (let (v5 i32) (sext v4))
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
            (let (v0 i32) (const.i32 2))
            (let (v1 i32) (const.i32 1))
            (let (v2 i1) (lt v0 v1))
            (let (v3 i32) (sext v2))
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
            (let (v0 i64) (const.i64 2))
            (let (v1 i64) (const.i64 1))
            (let (v2 i1) (lt v0 v1))
            (let (v3 i32) (sext v2))
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
            (let (v0 i32) (const.i32 2))
            (let (v1 i32) (const.i32 1))
            (let (v2 u32) (bitcast v0))
            (let (v3 u32) (bitcast v1))
            (let (v4 i1) (lte v2 v3))
            (let (v5 i32) (sext v4))
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
            (let (v0 i64) (const.i64 2))
            (let (v1 i64) (const.i64 1))
            (let (v2 u64) (bitcast v0))
            (let (v3 u64) (bitcast v1))
            (let (v4 i1) (lte v2 v3))
            (let (v5 i32) (sext v4))
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
            (let (v0 i32) (const.i32 2))
            (let (v1 i32) (const.i32 1))
            (let (v2 i1) (lte v0 v1))
            (let (v3 i32) (sext v2))
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
            (let (v0 i64) (const.i64 2))
            (let (v1 i64) (const.i64 1))
            (let (v2 i1) (lte v0 v1))
            (let (v3 i32) (sext v2))
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
            (let (v0 i32) (const.i32 2))
            (let (v1 i32) (const.i32 1))
            (let (v2 u32) (bitcast v0))
            (let (v3 u32) (bitcast v1))
            (let (v4 i1) (gt v2 v3))
            (let (v5 i32) (sext v4))
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
            (let (v0 i64) (const.i64 2))
            (let (v1 i64) (const.i64 1))
            (let (v2 u64) (bitcast v0))
            (let (v3 u64) (bitcast v1))
            (let (v4 i1) (gt v2 v3))
            (let (v5 i32) (sext v4))
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
            (let (v0 i32) (const.i32 2))
            (let (v1 i32) (const.i32 1))
            (let (v2 i1) (gt v0 v1))
            (let (v3 i32) (zext v2))
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
            (let (v0 i64) (const.i64 2))
            (let (v1 i64) (const.i64 1))
            (let (v2 i1) (gt v0 v1))
            (let (v3 i32) (zext v2))
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
            (let (v0 i32) (const.i32 2))
            (let (v1 i32) (const.i32 1))
            (let (v2 u32) (bitcast v0))
            (let (v3 u32) (bitcast v1))
            (let (v4 i1) (gte v2 v3))
            (let (v5 i32) (zext v4))
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
            (let (v0 i64) (const.i64 2))
            (let (v1 i64) (const.i64 1))
            (let (v2 u64) (bitcast v0))
            (let (v3 u64) (bitcast v1))
            (let (v4 i1) (gte v2 v3))
            (let (v5 i32) (zext v4))
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
            (let (v0 i32) (const.i32 2))
            (let (v1 i32) (const.i32 1))
            (let (v2 i1) (gte v0 v1))
            (let (v3 i32) (zext v2))
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
            (let (v0 i64) (const.i64 2))
            (let (v1 i64) (const.i64 1))
            (let (v2 i1) (gte v0 v1))
            (let (v3 i32) (zext v2))
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
            (let (v0 i32) (const.i32 2))
            (let (v1 i1) (eq v0 0))
            (let (v2 i32) (zext v1))
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
            (let (v0 i64) (const.i64 2))
            (let (v1 i1) (eq v0 0))
            (let (v2 i32) (zext v1))
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
            (let (v0 i32) (const.i32 2))
            (let (v1 i32) (const.i32 1))
            (let (v2 i1) (eq v0 v1))
            (let (v3 i32) (zext v2))
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
            (let (v0 i64) (const.i64 2))
            (let (v1 i64) (const.i64 1))
            (let (v2 i1) (eq v0 v1))
            (let (v3 i32) (zext v2))
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
            (let (v0 i32) (const.i32 2))
            (let (v1 i32) (const.i32 1))
            (let (v2 i1) (neq v0 v1))
            (let (v3 i32) (zext v2))
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
            (let (v0 i64) (const.i64 2))
            (let (v1 i64) (const.i64 1))
            (let (v2 i1) (neq v0 v1))
            (let (v3 i32) (zext v2))
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
            (let (v0 i64) (const.i64 3))
            (let (v1 i64) (const.i64 7))
            (let (v2 i32) (const.i32 1))
            (let (v3 i1) (neq v2 0))
            (let (v4 i64) (select v3 v0 v1))
        "#]],
    )
}
