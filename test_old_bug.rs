// This demonstrates the OLD BUGGY code that would overflow
// Run with: rustc -O test_old_bug.rs && ./test_old_bug.exe

fn main() {
    println!("Demonstrating the OLD BUGGY code behavior...\n");

    let n = 1u32;
    let reserved = 32 - n;  // reserved = 31
    
    println!("n={}, reserved={}", n, reserved);
    println!("Attempting: 2u32.pow({}) = 2u32.pow(32)", reserved + 1);
    println!("This causes overflow in release mode!\n");
    
    // OLD BUGGY CODE:
    // In debug mode: panics with "attempt to multiply with overflow"
    // In release mode: wraps to 0, then subtracting 1 wraps to 0xFFFFFFFF
    let result = 2u32.pow(reserved + 1);
    println!("Result in release mode (wraps): {:#010x}", result);
    
    let mask = (result - 1) << (n - 1);
    println!("Old mask (accidentally correct due to double-wrap): {:#010x}", mask);
    println!("\n⚠️  This 'works' in release mode only by accident (double overflow)");
    println!("⚠️  In debug mode, this would panic!");
}
