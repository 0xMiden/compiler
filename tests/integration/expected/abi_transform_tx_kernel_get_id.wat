(module $abi_transform_tx_kernel_get_id.wasm
  (type (;0;) (func (param i32)))
  (import "miden:core-base/account@1.0.0" "get-id" (func $miden_base_sys::bindings::account::extern_account_get_id (;0;) (type 0)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;1;) (type 0) (param i32)
    (local i32)
    global.get $__stack_pointer
    i32.const 16
    i32.sub
    local.tee 1
    global.set $__stack_pointer
    local.get 1
    i32.const 8
    i32.add
    call $miden_base_sys::bindings::account::get_id
    local.get 0
    local.get 1
    i64.load offset=8
    i64.store align=4
    local.get 1
    i32.const 16
    i32.add
    global.set $__stack_pointer
  )
  (func $miden_base_sys::bindings::account::get_id (;2;) (type 0) (param i32)
    (local i32)
    global.get $__stack_pointer
    i32.const 16
    i32.sub
    local.tee 1
    global.set $__stack_pointer
    local.get 1
    i32.const 8
    i32.add
    call $miden_base_sys::bindings::account::extern_account_get_id
    local.get 0
    local.get 1
    i64.load offset=8 align=4
    i64.store
    local.get 1
    i32.const 16
    i32.add
    global.set $__stack_pointer
  )
)
