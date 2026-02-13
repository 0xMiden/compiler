(module $hash_elements.wasm
  (type (;0;) (func (param i32 i32 i32)))
  (type (;1;) (func (param i32) (result f32)))
  (type (;2;) (func (param f32 f32)))
  (type (;3;) (func (param i64) (result f32)))
  (type (;4;) (func (param i32 i32 i32) (result i32)))
  (type (;5;) (func (param i32 i32 i32 i32)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 17)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $__rustc::__rust_dealloc (;0;) (type 0) (param i32 i32 i32))
  (func $entrypoint (;1;) (type 1) (param i32) (result f32)
    (local i32 i32 i32 f32)
    global.get $__stack_pointer
    i32.const 32
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
    i64.extend_i32_u
    call $intrinsics::felt::from_u64_unchecked
    i64.const 0
    call $intrinsics::felt::from_u64_unchecked
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
        i32.const 16
        i32.add
        call $miden::core::crypto::hashes::rpo256::hash_elements
        br 1 (;@1;)
      end
      local.get 3
      local.get 3
      local.get 2
      i32.add
      local.get 1
      i32.const 16
      i32.add
      call $miden::core::crypto::hashes::rpo256::hash_words
    end
    local.get 1
    local.get 1
    i64.load offset=16
    i64.const 32
    i64.rotl
    i64.store offset=8
    local.get 1
    local.get 1
    i64.load offset=24
    i64.const 32
    i64.rotl
    i64.store
    local.get 0
    i32.const 4
    i32.const 4
    call $<alloc::raw_vec::RawVecInner>::deallocate
    local.get 1
    i32.const 0
    i32.const 1048588
    call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
    f32.load
    local.set 4
    local.get 1
    i32.const 32
    i32.add
    global.set $__stack_pointer
    local.get 4
  )
  (func $intrinsics::felt::assert_eq (;2;) (type 2) (param f32 f32)
    unreachable
  )
  (func $intrinsics::felt::from_u64_unchecked (;3;) (type 3) (param i64) (result f32)
    unreachable
  )
  (func $miden::core::crypto::hashes::rpo256::hash_elements (;4;) (type 0) (param i32 i32 i32)
    unreachable
  )
  (func $miden::core::crypto::hashes::rpo256::hash_words (;5;) (type 0) (param i32 i32 i32)
    unreachable
  )
  (func $<miden_field::word::Word as core::ops::index::Index<usize>>::index (;6;) (type 4) (param i32 i32 i32) (result i32)
    block ;; label = @1
      local.get 1
      i32.const 3
      i32.gt_u
      br_if 0 (;@1;)
      local.get 0
      local.get 1
      i32.const 2
      i32.shl
      i32.add
      return
    end
    unreachable
  )
  (func $<alloc::raw_vec::RawVecInner>::deallocate (;7;) (type 0) (param i32 i32 i32)
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
  (func $<alloc::raw_vec::RawVecInner>::current_memory (;8;) (type 5) (param i32 i32 i32 i32)
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
  (func $<alloc::alloc::Global as core::alloc::Allocator>::deallocate (;9;) (type 0) (param i32 i32 i32)
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
  (data $.rodata (;0;) (i32.const 1048576) "<redacted>\00\00\00\00\10\00\0a\00\00\00\00\00\00\00\00\00\00\00")
)
