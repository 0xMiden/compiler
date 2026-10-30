;; RUN: midenc %s --entrypoint=entrypoint --debug full --emit=hir=- -Canalyze-only > %t.hir
;; RUN: filecheck %s < %t.hir
;; RUN: hir-opt %t.hir | filecheck %s
;;
;; Regenerate the WAT/DWARF with:
;; cargo test -p midenc-frontend-wasm --lib write_debug_location_lit_fixtures -- --ignored
;;
;; DWARF 5 .debug_loclists offset pairs use the compilation unit base, just like DWARF 4.
;; A procedure's nonzero low_pc must not shift the variable's range.
;;
;; CHECK-LABEL: builtin.function public extern("C") @entrypoint(
;; CHECK: di.debug_declare {{.*}}name = "tracked"{{.*}}DI_OP_local_slot(1), DW_OP_stack_value
;; CHECK-NEXT: {{%[0-9]+}} = hir.load_local <{ local = #builtin.local_variable<0, i32>
;; CHECK-NEXT: {{%[0-9]+}} = arith.constant 0 : i32;
;; CHECK-NEXT: {{%[0-9]+}} = arith.neq
;; CHECK-NEXT: cf.cond_br

(module
  (type (;0;) (func (param i32) (result i32)))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;0;) (type 0) (param i32) (result i32)
    (local i32)
    i32.const 7
    local.set 1
    local.get 0
    if ;; label = @1
      nop
    else
      nop
    end
    local.get 0
    i32.const 42
    i32.xor
  )
  (@custom ".debug_abbrev" (after code) "\01\11\01\11\01\00\00\02.\01\03\08\11\01\12\01\00\00\034\00\03\08\02\17\00\00\00")
  (@custom ".debug_loclists" (after code) "\11\00\00\00\05\00\04\00\00\00\00\00\04\09\0d\04\ed\00\01\9f\00")
  (@custom ".debug_info" (after code) "0\00\00\00\05\00\01\04\00\00\00\00\01\00\00\00\00\02entrypoint\00\02\00\00\00\17\00\00\00\03tracked\00\0c\00\00\00\00\00")
)
