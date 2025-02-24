(module $abi_transform_tx_kernel_get_id.wasm
  (type (;0;) (func (result f32)))
  (import "miden:core-import/account@1.0.0" "get-id" (func $miden_base_sys::bindings::account::extern_account_get_id (;0;) (type 0)))
  (func $entrypoint (;1;) (type 0) (result f32)
    call $miden_base_sys::bindings::account::get_id
  )
  (func $miden_base_sys::bindings::account::get_id (;2;) (type 0) (result f32)
    call $miden_base_sys::bindings::account::extern_account_get_id
  )
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
)