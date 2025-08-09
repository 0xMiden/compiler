(module $mem_intrinsics_heap_base.wasm
  (type (;0;) (func (result i32)))
  (type (;1;) (func (param i32 i32)))
  (type (;2;) (func (param i32 i32) (result i32)))
  (type (;3;) (func))
  (type (;4;) (func (param i32 i32 i32) (result i32)))
  (import "miden:core-intrinsics/intrinsics-mem@1.0.0" "heap-base" (func $miden_sdk_alloc::heap_base (;0;) (type 0)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 17)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;1;) (type 1) (param i32 i32)
    (local i32)
    call $__rustc::__rust_no_alloc_shim_is_unstable_v2
    block ;; label = @1
      i32.const 4
      i32.const 4
      call $__rustc::__rust_alloc
      local.tee 2
      br_if 0 (;@1;)
      i32.const 4
      i32.const 4
      call $alloc::alloc::handle_alloc_error
      unreachable
    end
    local.get 0
    i32.const 1
    i32.store offset=8
    local.get 0
    local.get 2
    i32.store offset=4
    local.get 0
    i32.const 1
    i32.store
    local.get 2
    local.get 1
    i32.const 1
    i32.shl
    i32.store
  )
  (func $__rustc::__rust_alloc (;2;) (type 2) (param i32 i32) (result i32)
    i32.const 1048576
    local.get 1
    local.get 0
    call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
  )
  (func $__rustc::__rust_no_alloc_shim_is_unstable_v2 (;3;) (type 3)
    return
  )
  (func $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc (;4;) (type 4) (param i32 i32 i32) (result i32)
    (local i32 i32)
    block ;; label = @1
      local.get 1
      i32.const 16
      local.get 1
      i32.const 16
      i32.gt_u
      select
      local.tee 3
      local.get 3
      i32.const -1
      i32.add
      i32.and
      br_if 0 (;@1;)
      local.get 2
      i32.const -2147483648
      local.get 1
      local.get 3
      call $core::ptr::alignment::Alignment::max
      local.tee 1
      i32.sub
      i32.gt_u
      br_if 0 (;@1;)
      i32.const 0
      local.set 3
      local.get 2
      local.get 1
      i32.add
      i32.const -1
      i32.add
      i32.const 0
      local.get 1
      i32.sub
      i32.and
      local.set 2
      block ;; label = @2
        local.get 0
        i32.load
        br_if 0 (;@2;)
        local.get 0
        call $miden_sdk_alloc::heap_base
        memory.size
        i32.const 16
        i32.shl
        i32.add
        i32.store
      end
      block ;; label = @2
        i32.const -2
        local.get 0
        i32.load
        local.tee 4
        i32.sub
        local.get 2
        i32.lt_u
        br_if 0 (;@2;)
        local.get 0
        local.get 4
        local.get 2
        i32.add
        i32.store
        local.get 4
        local.get 1
        i32.add
        local.set 3
      end
      local.get 3
      return
    end
    unreachable
  )
  (func $alloc::alloc::handle_alloc_error (;5;) (type 1) (param i32 i32)
    unreachable
  )
  (func $core::ptr::alignment::Alignment::max (;6;) (type 2) (param i32 i32) (result i32)
    local.get 0
    local.get 1
    local.get 0
    local.get 1
    i32.gt_u
    select
  )
)
