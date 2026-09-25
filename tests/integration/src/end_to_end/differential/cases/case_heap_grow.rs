// W5b — `memory.grow` / `memory.size` semantics after the fixed-heap model.
//
// Miden's heap intrinsics (codegen/masm/intrinsics/mem.masm) model a DYNAMIC
// heap that starts at ZERO pages, based at the first page boundary past all
// static memory (`codegen/masm/src/linker.rs`), and refuses to grow past
// `HEAP_END = (2^30 - 1) * 4` bytes; `memory_grow` returns the PREVIOUS page
// count on success and `-1` (`usize::MAX`) on failure, leaving the metadata
// untouched. A real wasm engine instead counts the whole linear memory, so
// the absolute page numbers differ by target — this case therefore observes
// only what BOTH models agree on:
//
//   * a small growth succeeds, and `memory.size` rises by EXACTLY the number
//     of pages requested (a delta, not an absolute);
//   * an impossible growth (2^20 pages = 64 GiB, past `HEAP_END` under the
//     Miden model and past the 32-bit address space under any other) returns
//     `usize::MAX` and leaves `memory.size` UNCHANGED;
//   * growing by zero pages succeeds and changes nothing.
//
// The page count itself never escapes: the result is built from success
// flags and size DELTAS only.
//
// The native arm mirrors the same model with a static page counter, which —
// like every allocator in this module — is reset at entry, because the native
// `cdylib` is reused across input pairs while the VM starts fresh.

#[cfg(target_arch = "wasm32")]
mod pages {
    pub fn reset() {}

    pub fn size() -> u32 {
        core::arch::wasm32::memory_size(0) as u32
    }

    pub fn grow(delta: u32) -> u32 {
        core::arch::wasm32::memory_grow(0, delta as usize) as u32
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod pages {
    // The largest growth the Miden model can accept is bounded by HEAP_END
    // minus the heap base; any request anywhere near 2^20 pages fails on both
    // sides, so an approximate ceiling is enough to mirror the decision.
    const MAX_PAGES: u32 = 60000;
    static mut PAGES: u32 = 0;

    pub fn reset() {
        unsafe { PAGES = 0 };
    }

    pub fn size() -> u32 {
        unsafe { PAGES }
    }

    pub fn grow(delta: u32) -> u32 {
        let previous = unsafe { PAGES };
        match previous.checked_add(delta) {
            Some(next) if next <= MAX_PAGES => {
                unsafe { PAGES = next };
                previous
            }
            _ => u32::MAX,
        }
    }
}

fn mix(h: u32, v: u32) -> u32 {
    (h ^ v).wrapping_mul(2654435761).rotate_left(5)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    pages::reset();
    let mut h: u32 = 0x9e37_79b9;

    // A growth that must succeed: 1..=5 pages.
    let small = (input1 % 5) + 1;
    let before = pages::size();
    let first = pages::grow(small);
    let after_small = pages::size();
    h = mix(h, (first != u32::MAX) as u32);
    h = mix(h, after_small.wrapping_sub(before));

    // A growth that must fail, leaving the size alone.
    let huge = 1u32 << 20;
    let second = pages::grow(huge);
    let after_huge = pages::size();
    h = mix(h, (second == u32::MAX) as u32);
    h = mix(h, after_huge.wrapping_sub(after_small));

    // A zero-page growth: succeeds, changes nothing.
    let third = pages::grow(0);
    let after_zero = pages::size();
    h = mix(h, (third != u32::MAX) as u32);
    h = mix(h, after_zero.wrapping_sub(after_huge));

    // A second small growth, so the accumulated delta is checked too.
    let more = (input2 % 3) + 1;
    let fourth = pages::grow(more);
    h = mix(h, (fourth != u32::MAX) as u32);
    h = mix(h, pages::size().wrapping_sub(after_zero));
    h = mix(h, pages::size().wrapping_sub(before));

    h.wrapping_add(small).wrapping_add(more << 8) ^ input2.rotate_left(small & 31)
}
