//! Integration tests that are deploying code and runnning test scenarior on a local Miden node instance or testnet

pub mod local_node;
pub mod scenario;

#[cfg(test)]
mod node_tests;
#[cfg(test)]
mod scenario_testnet_tests;
#[cfg(test)]
mod testnet_tests;
