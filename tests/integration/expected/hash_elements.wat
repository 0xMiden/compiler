(module $hash_elements.wasm
  (type (;0;) (func (param i32) (result f32)))
  (type (;1;) (func (param f32 f32)))
  (type (;2;) (func (param i32 i32 i32)))
  (type (;3;) (func (param i32 i32 i32 i32)))
  (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "from-u32" (func $miden_stdlib_sys::intrinsics::felt::extern_from_u32 (;0;) (type 0)))
  (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "assert-eq" (func $miden_stdlib_sys::intrinsics::felt::extern_assert_eq (;1;) (type 1)))
  (import "miden:core-import/stdlib-crypto-hashes-rpo@1.0.0" "hash-memory" (func $miden_stdlib_sys::stdlib::crypto::hashes::extern_hash_memory (;2;) (type 2)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;3;) (type 0) (param i32) (result f32)
    (local i32 i32 f32)
    global.get $__stack_pointer
    i32.const 16
    i32.sub
    local.tee 1
    global.set $__stack_pointer
    local.get 0
    i32.load offset=4
    i32.const 2
    i32.shr_u
    local.tee 2
    i32.const 3
    i32.and
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    i32.const 0
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    local.get 2
    local.get 0
    i32.load offset=8
    local.get 1
    call $miden_stdlib_sys::stdlib::crypto::hashes::extern_hash_memory
    local.get 1
    f32.load offset=12
    local.set 3
    local.get 0
    i32.const 4
    i32.const 4
    call $alloc::raw_vec::RawVecInner<A>::deallocate
    local.get 1
    i32.const 16
    i32.add
    global.set $__stack_pointer
    local.get 3
  )
  (func $__rustc::__rust_dealloc (;4;) (type 2) (param i32 i32 i32))
  (func $alloc::raw_vec::RawVecInner<A>::deallocate (;5;) (type 2) (param i32 i32 i32)
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
  (func $alloc::raw_vec::RawVecInner<A>::current_memory (;6;) (type 3) (param i32 i32 i32 i32)
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
  (func $<alloc::alloc::Global as core::alloc::Allocator>::deallocate (;7;) (type 2) (param i32 i32 i32)
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
)
