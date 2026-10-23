;; RUN: midenc %s --entrypoint=entrypoint --debug full --emit=hir=- -Canalyze-only > %t.hir
;; RUN: filecheck %s < %t.hir
;; RUN: hir-opt %t.hir | filecheck %s
;;
;; Regenerate the WAT/DWARF with:
;; cargo test -p midenc-frontend-wasm --lib write_debug_location_lit_fixtures -- --ignored
;;
;; Taking br_if skips the location-list end. Repair the direct destination as well as
;; the fallthrough so neither path leaves tracked available at the xor.
;;
;; CHECK-LABEL: builtin.function public extern("C") @entrypoint(
;; CHECK: di.debug_declare {{.*}}name = "tracked"
;; CHECK: cf.cond_br {{.*}} ^block[[JOIN:[0-9]+]], ^block[[FALLTHROUGH:[0-9]+]]
;; CHECK: ^block[[RETURN:[0-9]+]](
;; CHECK-NEXT: di.debug_kill {{.*}}name = "tracked"
;; CHECK-NEXT: builtin.ret
;; CHECK: ^block[[JOIN]]:
;; CHECK-NEXT: di.debug_kill {{.*}}name = "tracked"
;; CHECK-NOT: di.debug_declare
;; CHECK: arith.bxor
;; CHECK: ^block[[FALLTHROUGH]]:
;; CHECK-NEXT: di.debug_kill {{.*}}name = "tracked"
;; CHECK-NOT: di.debug_declare
;; CHECK: cf.br ^block[[JOIN]]

(module
  (type (;0;) (func (param i32) (result i32)))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;0;) (type 0) (param i32) (result i32)
    (local i32)
    i32.const 7
    local.set 1
    block ;; label = @1
      local.get 0
      br_if 0 (;@1;)
      nop
    end
    local.get 0
    i32.const 42
    i32.xor
  )
  (@custom ".debug_abbrev" (after code) "\01\11\01\11\01\00\00\02.\01\03\08\11\01\12\01\00\00\034\00\03\08\02\17\00\00\00")
  (@custom ".debug_loc" (after code) "\ff\ff\ff\ff\00\00\00\00\0b\00\00\00\0f\00\00\00\04\00\ed\00\01\9f\00\00\00\00\00\00\00\00")
  (@custom ".debug_info" (after code) "/\00\00\00\04\00\00\00\00\00\04\01\00\00\00\00\02entrypoint\00\02\00\00\00\17\00\00\00\03tracked\00\00\00\00\00\00\00")
)
