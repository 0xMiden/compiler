;; RUN: midenc %s --entrypoint=entrypoint --debug full --emit=hir=- -Canalyze-only > %t.hir
;; RUN: filecheck %s < %t.hir
;; RUN: hir-opt %t.hir | filecheck %s
;;
;; Regenerate the WAT/DWARF with:
;; cargo test -p midenc-frontend-wasm --lib write_debug_location_lit_fixtures -- --ignored
;;
;; The location ends in one arm of a value-producing if. Neither the other arm nor
;; the synthetic return block may retain it after leaving its valid range.
;;
;; CHECK-LABEL: builtin.function public extern("C") @entrypoint(
;; CHECK: di.debug_declare {{.*}}name = "tracked"
;; CHECK: cf.cond_br {{.*}} ^block[[THEN:[0-9]+]], ^block[[ELSE:[0-9]+]]
;; CHECK: ^block[[RETURN:[0-9]+]](
;; CHECK-NEXT: di.debug_kill {{.*}}name = "tracked"
;; CHECK-NEXT: builtin.ret
;; CHECK: ^block[[THEN]]:
;; CHECK-NEXT: di.debug_kill {{.*}}name = "tracked"
;; CHECK-NOT: di.debug_declare
;; CHECK: arith.constant 42
;; CHECK: cf.br ^block[[JOIN:[0-9]+]]
;; CHECK: ^block[[JOIN]](
;; CHECK-NEXT: cf.br ^block[[RETURN]]
;; CHECK: ^block[[ELSE]]:
;; CHECK-NEXT: di.debug_kill {{.*}}name = "tracked"
;; CHECK-NOT: di.debug_declare
;; CHECK: arith.constant 43
;; CHECK: cf.br ^block[[JOIN]]

(module
  (type (;0;) (func (param i32) (result i32)))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;0;) (type 0) (param i32) (result i32)
    (local i32)
    i32.const 7
    local.set 1
    local.get 0
    if (result i32) ;; label = @1
      nop
      i32.const 42
    else
      i32.const 43
    end
  )
  (@custom ".debug_abbrev" (after code) "\01\11\01\11\01\00\00\02.\01\03\08\11\01\12\01\00\00\034\00\03\08\02\17\00\00\00")
  (@custom ".debug_loc" (after code) "\ff\ff\ff\ff\00\00\00\00\09\00\00\00\0d\00\00\00\04\00\ed\00\01\9f\00\00\00\00\00\00\00\00")
  (@custom ".debug_info" (after code) "/\00\00\00\04\00\00\00\00\00\04\01\00\00\00\00\02entrypoint\00\02\00\00\00\15\00\00\00\03tracked\00\00\00\00\00\00\00")
)
