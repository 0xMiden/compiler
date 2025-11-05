(module $fibonacci.wasm
  (type (;0;) (func (param i32) (result i32)))
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;0;) (type 0) (param i32) (result i32)
    (local i32 i32 i32)
    i32.const 0
    local.set 1
    i32.const 1
    local.set 2
    block ;; label = @1
      loop ;; label = @2
        local.get 2
        local.set 3
        local.get 0
        i32.eqz
        br_if 1 (;@1;)
        local.get 0
        i32.const -1
        i32.add
        local.set 0
        local.get 1
        local.get 3
        i32.add
        local.set 2
        local.get 3
        local.set 1
        br 0 (;@2;)
      end
    end
    local.get 1
  )
)
