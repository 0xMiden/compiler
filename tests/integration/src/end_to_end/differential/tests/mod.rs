//! Differential cases. One `#[test]` per file under `cases/`, driven by
//! `run_case`, grouped into thematic modules. File a new case by the surface
//! its doc comment says it exercises; `_repro`/`_edges` companions stay next
//! to their base case.

mod arith;
mod boundaries;
mod calls;
mod control_flow;
mod debug_info;
mod memory;
mod opt_levels;
mod scale;
mod signed;
mod spills;
mod wide;
