//! Allocation owns its analyses independently of verification.
pub(crate) mod analysis;
pub(crate) mod edges;
pub(crate) mod normalize;
pub(crate) mod state;
pub(crate) mod storage;

#[cfg(test)]
mod tests;
