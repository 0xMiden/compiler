# Lessons

- When CLI lint behavior differs from frontend unit tests, check the analysis root. The CLI lint stage analyzes the lifted `builtin.world`, while many MASM advice-taint tests analyze only the target `builtin.module`; interprocedural call predecessor resolution can differ across those roots.
- When diagnosing compiler pipeline regressions, prefer the user's direct CLI reproduction before reasoning from integration-test harness behavior; generated test crates can introduce separate build-path artifacts that mask the actual issue.
- After a user fixes suspected root causes, re-run the exact reproduction before relying on earlier conclusions; similar Rust/Cargo failures can move to a different dependency edge after pipeline fixes.
- When using HIR effects for analysis, check both declaration-level summaries and operation-level `EffectOpInterface` implementations; native HIR ops can carry more precise per-result effect information than external function attributes.
