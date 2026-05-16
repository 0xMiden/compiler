;; In this sequence, `i32.wrap_i64` produced an invalid bitcast from `i64` to `i32`.
;;
;; TODO stop producing the invalid bitcast + make this test assert valid ops are generated
;;
;; RUN: /bin/sh -c "TMPDIR=\$(mktemp -d) && midenc %s --entrypoint=width_changing_bitcast::wrap_i64 --emit=hir=\"\$TMPDIR\" --emit=masm=\"\$TMPDIR\" -o \"\$TMPDIR/width_changing_bitcast.masp\" && cat \"\$TMPDIR/root.hir\"" | filecheck %s

;; CHECK-LABEL: builtin.function public {{.*}}@wrap_i64
;; CHECK: [[C64:[%v][0-9]+]] = arith.constant 64 : i64;
;; CHECK-NEXT: {{[%v][0-9]+}} = hir.bitcast [[C64]] <{ ty = #builtin.type<i32> }>;

(module
  (func $wrap_i64 (export "wrap_i64") (param i64 i64 i64) (result i32)
    local.get 0
    i64.clz
    local.get 1
    i64.clz
    i64.const 64
    i64.add
    local.get 2
    i64.const 0
    i64.ne
    select
    i32.wrap_i64
  )
)
