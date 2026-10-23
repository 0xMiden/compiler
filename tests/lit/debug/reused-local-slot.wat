;; RUN: midenc %s --entrypoint=entrypoint --debug full --emit=hir=- -Canalyze-only > %t.hir
;; RUN: filecheck %s < %t.hir
;; RUN: hir-opt %t.hir | filecheck %s
;;
;; Regenerate the WAT/DWARF with:
;; cargo test -p midenc-frontend-wasm --lib write_debug_location_lit_fixtures -- --ignored
;;
;; Two distinct variable DIEs occupy local 1 during disjoint ranges. Keep both names and
;; scalar types, kill earlier on both branches, and declare later at the common continuation.
;;
;; CHECK-LABEL: builtin.function public extern("C") @entrypoint(
;; CHECK: di.debug_declare {{.*}}name = "earlier"{{.*}}ty = i32{{.*}}DI_OP_local_slot(1), DW_OP_stack_value
;; CHECK-NEXT: {{%[0-9]+}} = hir.load_local <{ local = #builtin.local_variable<0, i32>
;; CHECK: cf.cond_br {{.*}} ^block[[THEN:[0-9]+]], ^block[[ELSE:[0-9]+]]
;; CHECK: ^block[[THEN]]:
;; CHECK-NEXT: di.debug_kill {{.*}}name = "earlier"
;; CHECK-NOT: di.debug_declare
;; CHECK: cf.br ^block[[JOIN:[0-9]+]]
;; CHECK: ^block[[JOIN]]:
;; CHECK: di.debug_declare {{.*}}name = "later"{{.*}}ty = i32{{.*}}DI_OP_local_slot(1), DW_OP_stack_value
;; CHECK: {{%[0-9]+}} = hir.load_local <{ local = #builtin.local_variable<0, i32>
;; CHECK: arith.bxor
;; CHECK: ^block[[ELSE]]:
;; CHECK-NEXT: di.debug_kill {{.*}}name = "earlier"
;; CHECK-NOT: di.debug_declare
;; CHECK: cf.br ^block[[JOIN]]

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
  (@custom ".debug_loc" (after code) "\ff\ff\ff\ff\00\00\00\00\09\00\00\00\0d\00\00\00\04\00\ed\00\01\9f\00\00\00\00\00\00\00\00\11\00\00\00\17\00\00\00\04\00\ed\00\01\9f\00\00\00\00\00\00\00\00")
  (@custom ".debug_info" (after code) ":\00\00\00\04\00\00\00\00\00\04\01\00\00\00\00\02entrypoint\00\02\00\00\00\17\00\00\00\03earlier\00\00\00\00\00\03later\00\1e\00\00\00\00\00")
)
