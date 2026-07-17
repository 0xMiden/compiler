// Unreachable-terminator CF shapes: a statically-infinite loop behind an
// impossible guard (a cycle with zero exit edges — the
// `create_unreachable_terminator` path of cfg-to-scf), and two planted wasm
// `unreachable` sites (loop body + match arm) giving mixed return-like exit
// kinds that force deep discriminator index_switch chains and
// `ub.unreachable`-terminated regions through structuring and lowering.

/// Trap that lowers to a genuine wasm `unreachable` instruction (the panic
/// handler is `loop {}`, so `panic!` never produces one). Never executed:
/// callers guard it with dynamically-impossible conditions.
#[inline(always)]
fn trap() -> ! {
    #[cfg(target_arch = "wasm32")]
    core::arch::wasm32::unreachable();
    #[cfg(not(target_arch = "wasm32"))]
    loop {}
}

// A: statically-infinite loop behind an impossible guard — a cycle with zero
// exit edges (create_unreachable_terminator path in cfg-to-scf).
#[inline(never)]
fn infinite_arm(a: u32, b: u32) -> u32 {
    let h = a ^ b.rotate_left(13);
    // h % 6 == 5 implies h % 3 == 2, contradicting h % 3 == 0.
    if h % 6 == 5 && h % 3 == 0 {
        let mut s = a;
        loop {
            // Keep the loop body non-empty so it is not trivially collapsed.
            s = s.wrapping_mul(3).wrapping_add(1);
            core::hint::black_box(s);
        }
    }
    h.wrapping_add(a)
}

// B: two distinct wasm-unreachable sites (loop body + match arm) — multiple
// ub.unreachable return-like ops merged by combine_exit, and a br_table arm
// plus loop exits with mixed return-like kinds.
#[inline(never)]
fn trap_sites(a: u32, b: u32) -> u32 {
    let mut x = a | 1;
    let mut i = 0u32;
    loop {
        x = x.wrapping_mul(0x0100_0193) ^ i;
        let h = x ^ b;
        if h % 10 == 7 && h % 5 == 1 {
            trap(); // site 1: inside the loop
        }
        if x & 0xfff == 0x421 {
            break;
        }
        i = i.wrapping_add(1);
        if i >= (b % 89).wrapping_add(2) {
            break;
        }
    }
    match x % 6 {
        0 => x ^ b,
        1 => x.wrapping_add(i),
        2 => {
            let h = x ^ i;
            if h % 6 == 5 && h % 3 == 0 {
                trap(); // site 2: inside a match arm
            }
            h >> 1
        }
        3 => x.rotate_right(3),
        4 => x.wrapping_sub(b),
        _ => x ^ 0x5a5a_5a5a,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    infinite_arm(input1, input2) ^ trap_sites(input2, input1)
}
