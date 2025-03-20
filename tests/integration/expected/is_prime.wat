(module $is_prime.wasm
  (type (;0;) (func (param i32) (result i32)))
  (func $entrypoint (;0;) (type 0) (param i32) (result i32)
    (local i32 i32 i32)
    block ;; label = @1
      local.get 0
      i32.const 2
      i32.lt_u
      br_if 0 (;@1;)
      i32.const 1
      local.set 1
      block ;; label = @2
        block ;; label = @3
          local.get 0
          i32.const 4
          i32.lt_u
          br_if 0 (;@3;)
          local.get 0
          i32.const 3
          i32.rem_u
          local.set 2
          local.get 0
          i32.const 1
          i32.and
          i32.eqz
          br_if 2 (;@1;)
          i32.const 0
          local.set 1
          local.get 2
          i32.eqz
          br_if 0 (;@3;)
          i32.const 5
          local.set 2
          loop ;; label = @4
            local.get 2
            local.get 2
            i32.mul
            local.get 0
            i32.gt_u
            local.tee 1
            br_if 1 (;@3;)
            local.get 2
            i32.eqz
            br_if 2 (;@2;)
            local.get 0
            local.get 2
            i32.rem_u
            i32.eqz
            br_if 1 (;@3;)
            local.get 2
            i32.const 2
            i32.add
            local.tee 3
            i32.eqz
            br_if 2 (;@2;)
            local.get 3
            i32.const 4
            i32.add
            local.set 2
            local.get 0
            local.get 3
            i32.rem_u
            br_if 0 (;@4;)
          end
        end
        local.get 1
        return
      end
      unreachable
    end
    i32.const 0
  )
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
)