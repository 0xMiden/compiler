(module $hash_elements_smoke.wasm
  (type (;0;) (func (param i32) (result f32)))
  (type (;1;) (func (param f32 f32)))
  (type (;2;) (func (param i32 i32 i32)))
  (type (;3;) (func (result i32)))
  (type (;4;) (func (param f32 f32 f32 f32 f32 f32 f32 f32) (result f32)))
  (type (;5;) (func (param i32 i32) (result i32)))
  (type (;6;) (func (param i32 i32 i32) (result i32)))
  (type (;7;) (func (param i32 i32)))
  (type (;8;) (func (param i32 i32 i32 i32)))
  (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "from-u32" (func $miden_stdlib_sys::intrinsics::felt::extern_from_u32 (;0;) (type 0)))
  (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "assert-eq" (func $miden_stdlib_sys::intrinsics::felt::extern_assert_eq (;1;) (type 1)))
  (import "miden:core-import/stdlib-crypto-hashes-rpo@1.0.0" "hash-memory" (func $miden_stdlib_sys::stdlib::crypto::hashes::extern_hash_memory (;2;) (type 2)))
  (import "miden:core-intrinsics/intrinsics-mem@1.0.0" "heap-base" (func $miden_sdk_alloc::heap_base (;3;) (type 3)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 17)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;4;) (type 4) (param f32 f32 f32 f32 f32 f32 f32 f32) (result f32)
    (local i32 i32 i32)
    global.get $__stack_pointer
    local.tee 8
    local.set 9
    local.get 8
    i32.const 64
    i32.sub
    i32.const -32
    i32.and
    local.tee 10
    global.set $__stack_pointer
    local.get 0
    i32.const 0
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    local.get 1
    i32.const 1
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    local.get 2
    i32.const 2
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    local.get 3
    i32.const 3
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    local.get 4
    i32.const 4
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    local.get 5
    i32.const 5
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    local.get 6
    i32.const 6
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    local.get 7
    i32.const 7
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    i32.const 0
    i32.load8_u offset=1048580
    drop
    block ;; label = @1
      i32.const 32
      i32.const 4
      call $__rustc::__rust_alloc
      local.tee 8
      br_if 0 (;@1;)
      i32.const 4
      i32.const 32
      call $alloc::alloc::handle_alloc_error
      unreachable
    end
    local.get 8
    local.get 7
    f32.store offset=28
    local.get 8
    local.get 6
    f32.store offset=24
    local.get 8
    local.get 5
    f32.store offset=20
    local.get 8
    local.get 4
    f32.store offset=16
    local.get 8
    local.get 3
    f32.store offset=12
    local.get 8
    local.get 2
    f32.store offset=8
    local.get 8
    local.get 1
    f32.store offset=4
    local.get 8
    local.get 0
    f32.store
    local.get 0
    i32.const 0
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    local.get 8
    f32.load offset=4
    i32.const 1
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    local.get 8
    f32.load offset=8
    i32.const 2
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    local.get 8
    f32.load offset=12
    i32.const 3
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    local.get 8
    f32.load offset=16
    i32.const 4
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    local.get 8
    f32.load offset=20
    i32.const 5
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    local.get 8
    f32.load offset=24
    i32.const 6
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    local.get 8
    f32.load offset=28
    i32.const 7
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    local.get 10
    i32.const 8
    i32.store offset=28
    local.get 10
    local.get 8
    i32.store offset=24
    local.get 10
    i32.const 8
    i32.store offset=20
    local.get 8
    f32.load
    i32.const 0
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    local.get 8
    f32.load offset=16
    i32.const 4
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    local.get 8
    f32.load offset=20
    i32.const 5
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    local.get 8
    f32.load offset=24
    i32.const 6
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    local.get 8
    f32.load offset=28
    i32.const 7
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    i32.const 8
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    i32.const 8
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    local.get 8
    i32.const 2
    i32.shr_u
    local.tee 8
    i32.const 3
    i32.and
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    i32.const 0
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    local.get 8
    i32.const 8
    local.get 10
    i32.const 32
    i32.add
    call $miden_stdlib_sys::stdlib::crypto::hashes::extern_hash_memory
    local.get 10
    f32.load offset=44
    local.set 0
    local.get 10
    i32.const 20
    i32.add
    i32.const 4
    i32.const 4
    call $alloc::raw_vec::RawVecInner<A>::deallocate
    local.get 9
    global.set $__stack_pointer
    local.get 0
  )
  (func $__rustc::__rust_alloc (;5;) (type 5) (param i32 i32) (result i32)
    i32.const 1048576
    local.get 1
    local.get 0
    call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
  )
  (func $__rustc::__rust_dealloc (;6;) (type 2) (param i32 i32 i32))
  (func $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc (;7;) (type 6) (param i32 i32 i32) (result i32)
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
        i32.const 268435456
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
  (func $alloc::alloc::handle_alloc_error (;8;) (type 7) (param i32 i32)
    unreachable
  )
  (func $alloc::raw_vec::RawVecInner<A>::deallocate (;9;) (type 2) (param i32 i32 i32)
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
  (func $alloc::raw_vec::RawVecInner<A>::current_memory (;10;) (type 8) (param i32 i32 i32 i32)
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
  (func $<alloc::alloc::Global as core::alloc::Allocator>::deallocate (;11;) (type 2) (param i32 i32 i32)
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
  (func $core::ptr::alignment::Alignment::max (;12;) (type 5) (param i32 i32) (result i32)
    local.get 0
    local.get 1
    local.get 0
    local.get 1
    i32.gt_u
    select
  )
)
