;; RUN: midenc %s --entrypoint=entrypoint --debug full --emit=hir=- -Canalyze-only > %t.hir
;; RUN: filecheck %s < %t.hir
;; RUN: hir-opt %t.hir | filecheck %s
;;
;; Regenerate the WAT/DWARF with:
;; cargo test -p midenc-frontend-wasm --lib write_debug_location_lit_fixtures -- --ignored
;;
;; The taken arm kills tracked and branches backward into its valid range; the header
;; must restore it. The other arm exits and must not carry that restored value to the xor.
;;
;; CHECK-LABEL: builtin.function public extern("C") @entrypoint(
;; CHECK: cf.br ^block[[LOOP:[0-9]+]]
;; CHECK: ^block[[RETURN:[0-9]+]](
;; CHECK-NEXT: di.debug_kill {{.*}}name = "tracked"
;; CHECK-NEXT: builtin.ret
;; CHECK: ^block[[EXIT:[0-9]+]]:
;; CHECK: arith.bxor
;; CHECK: ^block[[LOOP]]:
;; CHECK: di.debug_kill {{.*}}name = "tracked"
;; CHECK-NEXT: di.debug_declare {{.*}}name = "tracked"{{.*}}DI_OP_local_slot(1), DW_OP_stack_value
;; CHECK-NEXT: {{%[0-9]+}} = hir.load_local <{ local = #builtin.local_variable<0, i32>
;; CHECK: cf.cond_br {{.*}} ^block[[TAKEN:[0-9]+]], ^block[[ELSE:[0-9]+]]
;; CHECK: ^block[[TAKEN]]:
;; CHECK-NEXT: di.debug_kill {{.*}}name = "tracked"
;; CHECK-NOT: di.debug_declare
;; CHECK: cf.br ^block[[LOOP]]
;; CHECK: ^block[[LEAVE:[0-9]+]]:
;; CHECK-NEXT: cf.br ^block[[EXIT]]
;; CHECK: ^block[[ELSE]]:
;; CHECK-NEXT: di.debug_kill {{.*}}name = "tracked"
;; CHECK-NOT: di.debug_declare
;; CHECK: cf.br ^block[[LEAVE]]

(module
  (type (;0;) (func (param i32) (result i32)))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;0;) (type 0) (param i32) (result i32)
    (local i32)
    i32.const 7
    local.set 1
    block ;; label = @1
      loop ;; label = @2
        local.get 0
        if ;; label = @3
          nop
          i32.const 0
          local.set 0
          br 1 (;@2;)
        else
          nop
        end
        br 1 (;@1;)
      end
    end
    local.get 0
    i32.const 42
    i32.xor
  )
  (@custom ".debug_abbrev" (after code) "\01\11\01\11\01\00\00\02.\01\03\08\11\01\12\01\00\00\034\00\03\08\02\17\00\00\00")
  (@custom ".debug_loc" (after code) "\ff\ff\ff\ff\00\00\00\00\0d\00\00\00\11\00\00\00\04\00\ed\00\01\9f\00\00\00\00\00\00\00\00")
  (@custom ".debug_info" (after code) "/\00\00\00\04\00\00\00\00\00\04\01\00\00\00\00\02entrypoint\00\02\00\00\00%\00\00\00\03tracked\00\00\00\00\00\00\00")
)
