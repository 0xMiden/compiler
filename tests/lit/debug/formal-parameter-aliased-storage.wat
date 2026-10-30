;; RUN: midenc %s --entrypoint=entrypoint --debug full --emit=hir=- -Canalyze-only > %t.hir
;; RUN: filecheck %s < %t.hir
;; RUN: hir-opt %t.hir --verify | filecheck %s
;; CHECK-LABEL: builtin.function public extern("C") @entrypoint(
;; CHECK: di.debug_declare {{.*}}name = "shadowed"{{.*}}line = 99{{.*}}arg = 0{{.*}}DI_OP_local_slot(1), DW_OP_deref
;; CHECK-NOT: di.debug_value {{.*}}name = "{{shadowed|storage}}"
;; CHECK: hir.store_local
;; CHECK-NOT: di.debug_value {{.*}}name = "{{shadowed|storage}}"
;; CHECK: hir.store_local
;; CHECK-NOT: di.debug_value {{.*}}name = "{{shadowed|storage}}"
;; CHECK: builtin.ret

(module
  (type (;0;) (func (param i32) (result i32)))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;0;) (type 0) (param i32) (result i32)
    (local $storage i32) (local $shadowed i32)
    i32.const 7
    local.set $storage
    i32.const 9
    local.set $shadowed
    local.get $storage
  )
  (@custom ".debug_abbrev" (after code) "\01\11\01\00\00\02.\01\03\08\00\00\03\05\00\03\08;\0f\02\18\00\00\00")
  (@custom ".debug_info" (after code) "%\00\00\00\04\00\00\00\00\00\04\01\02entrypoint\00\03shadowed\00c\03\ed\00\01\00\00")
)
