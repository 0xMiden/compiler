#![no_std]

#[cfg(not(test))]
#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

/// Returns 1 if integer is prime
fn is_prime_reduced(n: u32) -> bool {
    let mut i = 5;
    while i <= n {
        if n % i == 0 {
            return false;
        }
        i += 6;
    }
    true
}

/// https://www.math.utah.edu/~pa/MDS/primes.html
#[no_mangle]
fn entrypoint(n: u32) -> bool {
    return is_prime_reduced(n);
}
