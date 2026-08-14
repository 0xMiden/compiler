// Standalone test to verify the int32_to_int overflow fix
// This reproduces the bug and validates the fix

fn main() {
    println!("Testing int32_to_int mask calculation fix...\n");

    // Test case 1: n=1 (the overflow case)
    test_mask_calculation(1, 0xFFFFFFFF);
    
    // Test case 2: other bit widths for completeness
    test_mask_calculation(2, 0xFFFFFFFE);
    test_mask_calculation(8, 0xFFFFFF80);
    test_mask_calculation(16, 0xFFFF8000);
    test_mask_calculation(32, 0x80000000);

    println!("\n✅ All tests passed! The fix correctly handles all edge cases.");
}

fn test_mask_calculation(n: u32, expected: u32) {
    let reserved = 32 - n;
    
    // OLD BUGGY CODE (would panic in debug, overflow in release for n=1):
    // let mask_old = (2u32.pow(reserved + 1) - 1) << (n - 1);
    
    // FIXED CODE using wrapping operations:
    let mask_fixed = (2u32.wrapping_pow(reserved + 1).wrapping_sub(1)) << (n - 1);
    
    if mask_fixed == expected {
        println!("✓ n={:2} → mask={:#010x} (expected={:#010x}) PASS", n, mask_fixed, expected);
    } else {
        panic!("✗ n={:2} → mask={:#010x} (expected={:#010x}) FAIL", n, mask_fixed, expected);
    }
}
