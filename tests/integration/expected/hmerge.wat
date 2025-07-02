(module $hmerge.wasm
  (type (;0;) (func (param i32) (result f32)))
  (type (;1;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 i32)))
  (type (;2;) (func (param i32 i32)))
  (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "from-u32" (func $miden_stdlib_sys::intrinsics::felt::extern_from_u32 (;0;) (type 0)))
  (import "miden:core-intrinsics/intrinsics-crypto@1.0.0" "hmerge" (func $miden_stdlib_sys::intrinsics::crypto::extern_hmerge (;1;) (type 1)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;2;) (type 2) (param i32 i32)
    (local i32 i32 f32)
    global.get $__stack_pointer
    i32.const 16
    i32.sub
    local.tee 2
    global.set $__stack_pointer
    i32.const 0
    local.set 3
    i32.const 0
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    local.set 4
    block ;; label = @1
      loop ;; label = @2
        local.get 3
        i32.const 16
        i32.eq
        br_if 1 (;@1;)
        local.get 2
        local.get 3
        i32.add
        local.get 4
        f32.store
        local.get 3
        i32.const 4
        i32.add
        local.set 3
        br 0 (;@2;)
      end
    end
    local.get 1
    f32.load
    local.get 1
    f32.load offset=4
    local.get 1
    f32.load offset=8
    local.get 1
    f32.load offset=12
    local.get 1
    f32.load offset=32
    local.get 1
    f32.load offset=36
    local.get 1
    f32.load offset=40
    local.get 1
    f32.load offset=44
    local.get 2
    call $miden_stdlib_sys::intrinsics::crypto::extern_hmerge
    local.get 0
    local.get 2
    i64.load offset=8 align=4
    i64.store offset=8
    local.get 0
    local.get 2
    i64.load align=4
    i64.store
    local.get 2
    i32.const 16
    i32.add
    global.set $__stack_pointer
  )
)
