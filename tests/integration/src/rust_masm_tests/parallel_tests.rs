//! Example tests that demonstrate using the shared node infrastructure

use super::local_node;

/// Test 1: Simple test that gets the shared node
#[tokio::test]
async fn test_parallel_node_usage_1() {
    let handle = local_node::get_shared_node()
        .await
        .expect("Failed to get shared node");
    
    eprintln!("Test 1: Using node at {}", handle.rpc_url());
    
    // Simulate some work
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    
    eprintln!("Test 1: Completed");
}

/// Test 2: Another test that uses the shared node
#[tokio::test]
async fn test_parallel_node_usage_2() {
    let handle = local_node::get_shared_node()
        .await
        .expect("Failed to get shared node");
    
    eprintln!("Test 2: Using node at {}", handle.rpc_url());
    
    // Simulate some work
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    
    eprintln!("Test 2: Completed");
}

/// Test 3: Yet another test that uses the shared node
#[tokio::test]
async fn test_parallel_node_usage_3() {
    let handle = local_node::get_shared_node()
        .await
        .expect("Failed to get shared node");
    
    eprintln!("Test 3: Using node at {}", handle.rpc_url());
    
    // Simulate some work
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    
    eprintln!("Test 3: Completed");
}