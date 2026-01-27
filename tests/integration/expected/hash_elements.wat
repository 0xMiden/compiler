(module $hash_elements.wasm
  (type (;0;) (func (param i32 i32 i32)))
  (type (;1;) (func (param i32) (result f32)))
  (type (;2;) (func (param f32 f32)))
  (type (;3;) (func (param i32 i32 i32 i32)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $__rustc::__rust_dealloc (;0;) (type 0) (param i32 i32 i32))
  (func $entrypoint (;1;) (type 1) (param i32) (result f32)
    (local i32 i32 i32 f32)
    global.get $__stack_pointer
    i32.const 16
    i32.sub
    local.tee 1
    global.set $__stack_pointer
    local.get 0
    i32.load offset=8
    local.set 2
    local.get 0
    i32.load offset=4
    i32.const 2
    i32.shr_u
    local.tee 3
    i32.const 3
    i32.and
    call $intrinsics::felt::from_u32
    i32.const 0
    call $intrinsics::felt::from_u32
    call $intrinsics::felt::assert_eq
    block ;; label = @1
      block ;; label = @2
        local.get 2
        i32.const 3
        i32.and
        i32.eqz
        br_if 0 (;@2;)
        local.get 3
        local.get 2
        local.get 1
        call $std::crypto::hashes::rpo::hash_memory
        br 1 (;@1;)
      end
      local.get 3
      local.get 3
      local.get 2
      i32.add
      local.get 1
      call $std::crypto::hashes::rpo::hash_memory_words
    end
    local.get 1
    f32.load offset=12
    local.set 4
    local.get 0
    i32.const 4
    i32.const 4
    call $<alloc::raw_vec::RawVecInner>::deallocate
    local.get 1
    i32.const 16
    i32.add
    global.set $__stack_pointer
    local.get 4
  )
  (func $intrinsics::felt::assert_eq (;2;) (type 2) (param f32 f32)
    unreachable
  )
  (func $intrinsics::felt::from_u32 (;3;) (type 1) (param i32) (result f32)
    unreachable
  )
  (func $std::crypto::hashes::rpo::hash_memory (;4;) (type 0) (param i32 i32 i32)
    unreachable
  )
  (func $std::crypto::hashes::rpo::hash_memory_words (;5;) (type 0) (param i32 i32 i32)
    unreachable
  )
  (func $<alloc::raw_vec::RawVecInner>::deallocate (;6;) (type 0) (param i32 i32 i32)
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
    call $<alloc::raw_vec::RawVecInner>::current_memory
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
  (func $<alloc::raw_vec::RawVecInner>::current_memory (;7;) (type 3) (param i32 i32 i32 i32)
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
  (func $<alloc::alloc::Global as core::alloc::Allocator>::deallocate (;8;) (type 0) (param i32 i32 i32)
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
