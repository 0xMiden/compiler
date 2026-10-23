;; RUN: midenc %s --entrypoint=entrypoint --debug full --emit=hir=- -Canalyze-only > %t.hir
;; RUN: filecheck %s < %t.hir
;; RUN: hir-opt %t.hir | filecheck %s
;;
;; Regenerate the WAT/DWARF with:
;; cargo test -p midenc-frontend-wasm --lib write_debug_location_lit_fixtures -- --ignored
;;
;; The default br_table edge skips the location-list end. Both switch destinations
;; must invalidate tracked before control reaches the common xor.
;;
;; CHECK-LABEL: builtin.function public extern("C") @entrypoint(
;; CHECK: di.debug_declare {{.*}}name = "tracked"
;; CHECK: cf.switch {{.*}} [#builtin.u32<0> -> ^block[[CASE:[0-9]+]]], ^block[[DEFAULT:[0-9]+]]
;; CHECK: ^block[[RETURN:[0-9]+]](
;; CHECK-NEXT: di.debug_kill {{.*}}name = "tracked"
;; CHECK-NEXT: builtin.ret
;; CHECK: ^block[[DEFAULT]]:
;; CHECK-NEXT: di.debug_kill {{.*}}name = "tracked"
;; CHECK-NOT: di.debug_declare
;; CHECK: arith.bxor
;; CHECK: ^block[[CASE]]:
;; CHECK-NEXT: di.debug_kill {{.*}}name = "tracked"
;; CHECK-NOT: di.debug_declare
;; CHECK: cf.br ^block[[DEFAULT]]

(module
  (type (;0;) (func (param i32) (result i32)))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;0;) (type 0) (param i32) (result i32)
    (local i32)
    i32.const 7
    local.set 1
    block ;; label = @1
      block ;; label = @2
        local.get 0
        br_table 0 (;@2;) 1 (;@1;)
      end
      nop
    end
    local.get 0
    i32.const 42
    i32.xor
  )
  (@custom ".debug_abbrev" (after code) "\01\11\01\11\01\00\00\02.\01\03\08\11\01\12\01\00\00\034\00\03\08\02\17\00\00\00")
  (@custom ".debug_loc" (after code) "\ff\ff\ff\ff\00\00\00\00\0d\00\00\00\14\00\00\00\04\00\ed\00\01\9f\00\00\00\00\00\00\00\00")
  (@custom ".debug_info" (after code) "/\00\00\00\04\00\00\00\00\00\04\01\00\00\00\00\02entrypoint\00\02\00\00\00\1c\00\00\00\03tracked\00\00\00\00\00\00\00")
)
