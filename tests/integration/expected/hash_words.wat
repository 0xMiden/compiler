(module $hash_words.wasm
  (type (;0;) (func (param i32) (result f32)))
  (type (;1;) (func (param i32 i32 i32)))
  (type (;2;) (func (param i32 i32)))
  (type (;3;) (func (param f32 f32)))
  (type (;4;) (func (param i32 i32 i32 i32)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;0;) (type 0) (param i32) (result f32)
    (local i32 i32 i32 f32)
    global.get $__stack_pointer
    i32.const 48
    i32.sub
    local.tee 1
    global.set $__stack_pointer
    local.get 0
    i32.load offset=4
    local.set 2
    local.get 0
    i32.load offset=8
    local.set 3
    i32.const 0
    call $intrinsics::felt::from_u32
    i32.const 0
    call $intrinsics::felt::from_u32
    call $intrinsics::felt::assert_eq
    local.get 2
    i32.const 2
    i32.shr_u
    local.tee 2
    local.get 2
    local.get 3
    i32.const 2
    i32.shl
    i32.add
    local.get 1
    i32.const 16
    i32.add
    call $std::crypto::hashes::rpo::hash_memory_words
    local.get 1
    local.get 1
    i64.load offset=24
    i64.store offset=40
    local.get 1
    local.get 1
    i64.load offset=16
    i64.store offset=32
    local.get 1
    local.get 1
    i32.const 32
    i32.add
    call $miden_stdlib_sys::intrinsics::word::Word::reverse
    local.get 1
    f32.load
    local.set 4
    local.get 0
    i32.const 16
    i32.const 16
    call $alloc::raw_vec::RawVecInner<A>::deallocate
    local.get 1
    i32.const 48
    i32.add
    global.set $__stack_pointer
    local.get 4
  )
  (func $__rustc::__rust_dealloc (;1;) (type 1) (param i32 i32 i32))
  (func $miden_stdlib_sys::intrinsics::word::Word::reverse (;2;) (type 2) (param i32 i32)
    (local i32 i32 i32 f32)
    global.get $__stack_pointer
    i32.const 16
    i32.sub
    local.tee 2
    local.get 1
    i64.load offset=8
    i64.store offset=8 align=4
    local.get 2
    local.get 1
    i64.load
    i64.store align=4
    local.get 2
    i32.const 12
    i32.add
    local.set 3
    i32.const 0
    local.set 1
    block ;; label = @1
      loop ;; label = @2
        local.get 1
        i32.const 8
        i32.eq
        br_if 1 (;@1;)
        local.get 2
        local.get 1
        i32.add
        local.tee 4
        f32.load
        local.set 5
        local.get 4
        local.get 3
        i32.load
        i32.store
        local.get 3
        local.get 5
        f32.store
        local.get 1
        i32.const 4
        i32.add
        local.set 1
        local.get 3
        i32.const -4
        i32.add
        local.set 3
        br 0 (;@2;)
      end
    end
    local.get 0
    local.get 2
    i64.load offset=8 align=4
    i64.store offset=8
    local.get 0
    local.get 2
    i64.load align=4
    i64.store
  )
  (func $intrinsics::felt::from_u32 (;3;) (type 0) (param i32) (result f32)
    unreachable
  )
  (func $intrinsics::felt::assert_eq (;4;) (type 3) (param f32 f32)
    unreachable
  )
  (func $std::crypto::hashes::rpo::hash_memory_words (;5;) (type 1) (param i32 i32 i32)
    unreachable
  )
  (func $alloc::raw_vec::RawVecInner<A>::deallocate (;6;) (type 1) (param i32 i32 i32)
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
  (func $alloc::raw_vec::RawVecInner<A>::current_memory (;7;) (type 4) (param i32 i32 i32 i32)
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
  (func $<alloc::alloc::Global as core::alloc::Allocator>::deallocate (;8;) (type 1) (param i32 i32 i32)
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
