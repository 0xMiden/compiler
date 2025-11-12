(module $rust_sdk_stdlib_sha256_hash.wasm
  (type (;0;) (func (param i32)))
  (type (;1;) (func (param i32 i32)))
  (type (;2;) (func (param i32 i32 i32 i32 i32 i32 i32 i32 i32)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $core::slice::<impl [T]>::reverse (;0;) (type 0) (param i32)
    (local i32 i32 i32 i32)
    local.get 0
    i32.const 3
    i32.add
    local.set 1
    i32.const 0
    local.set 2
    block ;; label = @1
      loop ;; label = @2
        local.get 2
        i32.const 2
        i32.eq
        br_if 1 (;@1;)
        local.get 0
        local.get 2
        i32.add
        local.tee 3
        i32.load8_u
        local.set 4
        local.get 3
        local.get 1
        i32.load8_u
        i32.store8
        local.get 1
        local.get 4
        i32.store8
        local.get 1
        i32.const -1
        i32.add
        local.set 1
        local.get 2
        i32.const 1
        i32.add
        local.set 2
        br 0 (;@2;)
      end
    end
  )
  (func $entrypoint (;1;) (type 1) (param i32 i32)
    (local i32 i32)
    global.get $__stack_pointer
    local.tee 2
    local.set 3
    local.get 2
    i32.const 64
    i32.sub
    i32.const -32
    i32.and
    local.tee 2
    global.set $__stack_pointer
    local.get 2
    i32.const 32
    i32.add
    i32.const 24
    i32.add
    local.get 1
    i32.const 24
    i32.add
    i64.load align=1
    i64.store
    local.get 2
    i32.const 32
    i32.add
    i32.const 16
    i32.add
    local.get 1
    i32.const 16
    i32.add
    i64.load align=1
    i64.store
    local.get 2
    i32.const 32
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
    i64.store offset=32
    i32.const 0
    local.set 1
    block ;; label = @1
      loop ;; label = @2
        local.get 1
        i32.const 32
        i32.eq
        br_if 1 (;@1;)
        local.get 2
        i32.const 32
        i32.add
        local.get 1
        i32.add
        call $core::slice::<impl [T]>::reverse
        local.get 1
        i32.const 4
        i32.add
        local.set 1
        br 0 (;@2;)
      end
    end
    local.get 2
    i32.load offset=32
    local.get 2
    i32.load offset=36
    local.get 2
    i32.load offset=40
    local.get 2
    i32.load offset=44
    local.get 2
    i32.load offset=48
    local.get 2
    i32.load offset=52
    local.get 2
    i32.load offset=56
    local.get 2
    i32.load offset=60
    local.get 2
    call $std::crypto::hashes::sha256::hash_1to1
    local.get 2
    local.get 2
    i64.load offset=24
    i64.store offset=56
    local.get 2
    local.get 2
    i64.load offset=16
    i64.store offset=48
    local.get 2
    local.get 2
    i64.load offset=8
    i64.store offset=40
    local.get 2
    local.get 2
    i64.load
    i64.store offset=32
    i32.const 0
    local.set 1
    block ;; label = @1
      loop ;; label = @2
        local.get 1
        i32.const 32
        i32.eq
        br_if 1 (;@1;)
        local.get 2
        i32.const 32
        i32.add
        local.get 1
        i32.add
        call $core::slice::<impl [T]>::reverse
        local.get 1
        i32.const 4
        i32.add
        local.set 1
        br 0 (;@2;)
      end
    end
    local.get 0
    local.get 2
    i64.load offset=32
    i64.store align=1
    local.get 0
    i32.const 24
    i32.add
    local.get 2
    i64.load offset=56
    i64.store align=1
    local.get 0
    i32.const 16
    i32.add
    local.get 2
    i64.load offset=48
    i64.store align=1
    local.get 0
    i32.const 8
    i32.add
    local.get 2
    i64.load offset=40
    i64.store align=1
    local.get 3
    global.set $__stack_pointer
  )
  (func $std::crypto::hashes::sha256::hash_1to1 (;2;) (type 2) (param i32 i32 i32 i32 i32 i32 i32 i32 i32)
    unreachable
  )
)
