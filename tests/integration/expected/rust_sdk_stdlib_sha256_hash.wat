(module $rust_sdk_stdlib_sha256_hash.wasm
  (type (;0;) (func (param i32 i32)))
  (type (;1;) (func (param i32 i32 i32 i32 i32 i32 i32 i32 i32)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $<[u8]>::reverse (;0;) (type 0) (param i32 i32)
    (local i32 i32)
    local.get 1
    i32.const 1
    i32.shr_u
    local.set 2
    local.get 1
    local.get 0
    i32.add
    i32.const -1
    i32.add
    local.set 1
    block ;; label = @1
      loop ;; label = @2
        local.get 2
        i32.eqz
        br_if 1 (;@1;)
        local.get 0
        i32.load8_u
        local.set 3
        local.get 0
        local.get 1
        i32.load8_u
        i32.store8
        local.get 1
        local.get 3
        i32.store8
        local.get 2
        i32.const -1
        i32.add
        local.set 2
        local.get 0
        i32.const 1
        i32.add
        local.set 0
        local.get 1
        i32.const -1
        i32.add
        local.set 1
        br 0 (;@2;)
      end
    end
  )
  (func $<core::slice::iter::ChunksExactMut<u8> as core::iter::traits::iterator::Iterator>::next (;1;) (type 0) (param i32 i32)
    (local i32 i32 i32 i32)
    local.get 1
    i32.load offset=16
    local.set 2
    i32.const 0
    local.set 3
    block ;; label = @1
      local.get 1
      i32.load offset=8
      local.tee 4
      i32.eqz
      br_if 0 (;@1;)
      local.get 1
      i32.load offset=12
      local.tee 5
      local.get 2
      i32.lt_u
      br_if 0 (;@1;)
      local.get 1
      local.get 5
      local.get 2
      i32.sub
      i32.store offset=12
      local.get 1
      local.get 4
      local.get 2
      i32.add
      i32.store offset=8
      local.get 4
      local.set 3
    end
    local.get 0
    local.get 2
    i32.store offset=4
    local.get 0
    local.get 3
    i32.store
  )
  (func $entrypoint (;2;) (type 0) (param i32 i32)
    (local i32 i32)
    global.get $__stack_pointer
    local.tee 2
    local.set 3
    local.get 2
    i32.const 128
    i32.sub
    i32.const -32
    i32.and
    local.tee 2
    global.set $__stack_pointer
    local.get 2
    i32.const 64
    i32.add
    i32.const 24
    i32.add
    local.get 1
    i32.const 24
    i32.add
    i64.load align=1
    i64.store
    local.get 2
    i32.const 64
    i32.add
    i32.const 16
    i32.add
    local.get 1
    i32.const 16
    i32.add
    i64.load align=1
    i64.store
    local.get 2
    i32.const 64
    i32.add
    i32.const 8
    i32.add
    local.get 1
    i32.const 8
    i32.add
    i64.load align=1
    i64.store
    local.get 2
    local.get 1
    i64.load align=1
    i64.store offset=64
    local.get 2
    i64.const 17179869216
    i64.store offset=44 align=4
    local.get 2
    i32.const 0
    i32.store offset=36
    local.get 2
    local.get 2
    i32.const 96
    i32.add
    i32.store offset=32
    local.get 2
    local.get 2
    i32.const 64
    i32.add
    i32.store offset=40
    block ;; label = @1
      loop ;; label = @2
        local.get 2
        i32.const 24
        i32.add
        local.get 2
        i32.const 32
        i32.add
        call $<core::slice::iter::ChunksExactMut<u8> as core::iter::traits::iterator::Iterator>::next
        local.get 2
        i32.load offset=24
        local.tee 1
        i32.eqz
        br_if 1 (;@1;)
        local.get 1
        local.get 2
        i32.load offset=28
        call $<[u8]>::reverse
        br 0 (;@2;)
      end
    end
    local.get 2
    i32.load offset=64
    local.get 2
    i32.load offset=68
    local.get 2
    i32.load offset=72
    local.get 2
    i32.load offset=76
    local.get 2
    i32.load offset=80
    local.get 2
    i32.load offset=84
    local.get 2
    i32.load offset=88
    local.get 2
    i32.load offset=92
    local.get 2
    i32.const 32
    i32.add
    call $std::crypto::hashes::sha256::hash_1to1
    local.get 2
    local.get 2
    i64.load offset=56
    i64.store offset=88
    local.get 2
    local.get 2
    i64.load offset=48
    i64.store offset=80
    local.get 2
    local.get 2
    i64.load offset=40
    i64.store offset=72
    local.get 2
    local.get 2
    i64.load offset=32
    i64.store offset=64
    local.get 2
    i64.const 17179869216
    i64.store offset=120 align=4
    local.get 2
    i32.const 0
    i32.store offset=112
    local.get 2
    local.get 2
    i32.const 96
    i32.add
    i32.store offset=108
    local.get 2
    local.get 2
    i32.const 64
    i32.add
    i32.store offset=116
    block ;; label = @1
      loop ;; label = @2
        local.get 2
        i32.const 16
        i32.add
        local.get 2
        i32.const 108
        i32.add
        call $<core::slice::iter::ChunksExactMut<u8> as core::iter::traits::iterator::Iterator>::next
        local.get 2
        i32.load offset=16
        local.tee 1
        i32.eqz
        br_if 1 (;@1;)
        local.get 1
        local.get 2
        i32.load offset=20
        call $<[u8]>::reverse
        br 0 (;@2;)
      end
    end
    local.get 0
    local.get 2
    i64.load offset=64
    i64.store align=1
    local.get 0
    i32.const 24
    i32.add
    local.get 2
    i64.load offset=88
    i64.store align=1
    local.get 0
    i32.const 16
    i32.add
    local.get 2
    i64.load offset=80
    i64.store align=1
    local.get 0
    i32.const 8
    i32.add
    local.get 2
    i64.load offset=72
    i64.store align=1
    local.get 3
    global.set $__stack_pointer
  )
  (func $std::crypto::hashes::sha256::hash_1to1 (;3;) (type 1) (param i32 i32 i32 i32 i32 i32 i32 i32 i32)
    unreachable
  )
)
