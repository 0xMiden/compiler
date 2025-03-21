mod block_merging;
mod dce;
mod drop_redundant_args;

#[derive(Debug, Copy, Clone)]
pub struct RegionTransformFailed;
