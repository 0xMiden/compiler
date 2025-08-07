(module $vec_alloc_vec.wasm
  (type (;0;) (func (param i32) (result f32)))
  (type (;1;) (func (result i32)))
  (type (;2;) (func (param i32 i32) (result i32)))
  (type (;3;) (func (param i32 i32 i32)))
  (type (;4;) (func))
  (type (;5;) (func (param i32 i32 i32) (result i32)))
  (type (;6;) (func (param i32 i32 i32 i32)))
  (type (;7;) (func (param i32 i32)))
  (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "from-u32" (func $miden_stdlib_sys::intrinsics::felt::extern_from_u32 (;0;) (type 0)))
  (import "miden:core-intrinsics/intrinsics-mem@1.0.0" "heap-base" (func $miden_sdk_alloc::heap_base (;1;) (type 1)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 17)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;2;) (type 0) (param i32) (result f32)
    (local i32 i32 f32 f32)
    global.get $__stack_pointer
    i32.const 16
    i32.sub
    local.tee 1
    global.set $__stack_pointer
    call $__rustc::__rust_no_alloc_shim_is_unstable_v2
    block ;; label = @1
      block ;; label = @2
        i32.const 12
        i32.const 4
        call $__rustc::__rust_alloc
        local.tee 2
        i32.eqz
        br_if 0 (;@2;)
        i32.const 1
        call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
        local.set 3
        i32.const 2
        call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
        local.set 4
        local.get 2
        i32.const 3
        call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
        f32.store offset=8
        local.get 2
        local.get 4
        f32.store offset=4
        local.get 2
        local.get 3
        f32.store
        local.get 1
        i32.const 3
        i32.store offset=12
        local.get 1
        local.get 2
        i32.store offset=8
        local.get 1
        i32.const 3
        i32.store offset=4
        local.get 0
        i32.const 3
        i32.ge_u
        br_if 1 (;@1;)
        local.get 2
        local.get 0
        i32.const 2
        i32.shl
        i32.add
        f32.load
        local.set 3
        local.get 1
        i32.const 4
        i32.add
        i32.const 4
        i32.const 4
        call $alloc::raw_vec::RawVecInner<A>::deallocate
        local.get 1
        i32.const 16
        i32.add
        global.set $__stack_pointer
        local.get 3
        return
      end
      i32.const 4
      i32.const 12
      call $alloc::alloc::handle_alloc_error
    end
    unreachable
  )
  (func $__rustc::__rust_alloc (;3;) (type 2) (param i32 i32) (result i32)
    i32.const 1048576
    local.get 1
    local.get 0
    call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
  )
  (func $__rustc::__rust_dealloc (;4;) (type 3) (param i32 i32 i32))
  (func $__rustc::__rust_no_alloc_shim_is_unstable_v2 (;5;) (type 4)
    return
  )
  (func $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc (;6;) (type 5) (param i32 i32 i32) (result i32)
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
  (func $alloc::raw_vec::RawVecInner<A>::deallocate (;7;) (type 3) (param i32 i32 i32)
    (local i32)
    global.get $__stack_pointer
    i32.const 16
    i32.sub
    local.tee 3
    global.set $__stack_pointer
    local.get 3
    i32.const 4
    i32.add
    local.get 0
    local.get 1
    local.get 2
    call $alloc::raw_vec::RawVecInner<A>::current_memory
    block ;; label = @1
      local.get 3
      i32.load offset=8
      local.tee 2
      i32.eqz
      br_if 0 (;@1;)
      local.get 3
      i32.load offset=4
      local.get 2
      local.get 3
      i32.load offset=12
      call $<alloc::alloc::Global as core::alloc::Allocator>::deallocate
    end
    local.get 3
    i32.const 16
    i32.add
    global.set $__stack_pointer
  )
  (func $alloc::raw_vec::RawVecInner<A>::current_memory (;8;) (type 6) (param i32 i32 i32 i32)
    (local i32 i32 i32)
    i32.const 0
    local.set 4
    i32.const 4
    local.set 5
    block ;; label = @1
      local.get 3
      i32.eqz
      br_if 0 (;@1;)
      local.get 1
      i32.load
      local.tee 6
      i32.eqz
      br_if 0 (;@1;)
      local.get 0
      local.get 2
      i32.store offset=4
      local.get 0
      local.get 1
      i32.load offset=4
      i32.store
      local.get 6
      local.get 3
      i32.mul
      local.set 4
      i32.const 8
      local.set 5
    end
    local.get 0
    local.get 5
    i32.add
    local.get 4
    i32.store
  )
  (func $<alloc::alloc::Global as core::alloc::Allocator>::deallocate (;9;) (type 3) (param i32 i32 i32)
    block ;; label = @1
      local.get 2
      i32.eqz
      br_if 0 (;@1;)
      local.get 0
      local.get 2
      local.get 1
      call $__rustc::__rust_dealloc
    end
  )
  (func $alloc::alloc::handle_alloc_error (;10;) (type 7) (param i32 i32)
    unreachable
  )
  (func $core::ptr::alignment::Alignment::max (;11;) (type 2) (param i32 i32) (result i32)
    local.get 0
    local.get 1
    local.get 0
    local.get 1
    i32.gt_u
    select
  )
)
