#![no_std]
#![feature(alloc_error_handler)]

#[cfg(not(test))]
#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[cfg(not(test))]
#[alloc_error_handler]
fn alloc_failed(_layout: core::alloc::Layout) -> ! {
    loop {}
}

/// Returns 1 if integer is prime
fn is_prime(n: u32) -> bool {
    if n <= 1 {
        return false;
    }
    if n <= 3 {
        return true;
    }
    if n % 2 == 0 || n % 3 == 0 {
        return false;
    }
    let mut i = 5;
    while i * i <= n {
        if n % i == 0 || n % (i + 2) == 0 {
            return false;
        }
        i += 6;
    }
    true
}

/// https://www.math.utah.edu/~pa/MDS/primes.html
#[unsafe(no_mangle)]
fn entrypoint(n: u32) -> bool {
    return is_prime(n);
}
