//! Differential cases. One `#[test]` per file under `cases/`, driven by
//! `run_case`, grouped into thematic modules. File a new case by the surface
//! its doc comment says it exercises; `_repro`/`_edges` companions stay next
//! to their base case.

mod arith;
mod boundaries;
mod calls;
mod canon;
mod compose;
mod control_flow;
mod corelib;
mod cse;
mod debug_info;
mod frames;
mod interact;
mod memorder;
mod memory;
mod opt_levels;
mod pressure;
mod programs;
mod programs_oz;
mod scale;
mod sccp;
mod signed;
mod spills;
mod traps;
mod wide;
