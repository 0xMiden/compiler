// W2 — `Rc<RefCell<u32>>` shared by two handles: the strong-count arithmetic
// in the `RcInner` header (a `Cell<usize>` — 8 bytes natively, 4 on wasm,
// which is why only the COUNT VALUE is hashed and never a size), `RefCell`'s
// borrow-flag state machine, `Rc::clone`/`drop` around a mutation, and
// `Rc::try_unwrap` in both its outcomes (unique -> `Ok`, shared -> `Err`).
//
// `Rc` drop glue is iterative (no recursive data here), so the assembler's
// call-graph check is not involved.
//
// Bulk-op evidence: 1 `memory.copy` (the `Vec<Rc<..>>` realloc), no
// `memory.fill`, no libcall. Arena: 64 KiB.

extern crate alloc;

use alloc::{rc::Rc, vec::Vec};
use core::{
    alloc::{GlobalAlloc, Layout},
    cell::{RefCell, UnsafeCell},
};

const ARENA_SIZE: usize = 1 << 16;

#[repr(align(16))]
struct Arena(UnsafeCell<[u8; ARENA_SIZE]>);
unsafe impl Sync for Arena {}
static ARENA: Arena = Arena(UnsafeCell::new([0; ARENA_SIZE]));
static mut NEXT: usize = 0;

struct Bump;

unsafe impl GlobalAlloc for Bump {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let base = ARENA.0.get() as *mut u8;
        let start = (unsafe { NEXT } + layout.align() - 1) & !(layout.align() - 1);
        let end = start + layout.size();
        if end > ARENA_SIZE {
            return core::ptr::null_mut();
        }
        unsafe { NEXT = end };
        unsafe { base.add(start) }
    }

    unsafe fn dealloc(&self, _p: *mut u8, _l: Layout) {}
}

#[global_allocator]
static GLOBAL: Bump = Bump;

fn mix(h: u32, v: u32) -> u32 {
    (h ^ v).wrapping_mul(16777619).rotate_left(7)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    unsafe { NEXT = 0 };
    let n = (input1 % 12) as usize;
    let mut h: u32 = 0x811c_9dc5;

    let cell = Rc::new(RefCell::new(input2));
    let second = Rc::clone(&cell);
    h = mix(h, Rc::strong_count(&cell) as u32);

    // Mutate through one handle, read through the other. The read is bound to
    // a local FIRST: a `borrow()` temporary inside an assignment lives to the
    // end of the statement, so inlining it would double-borrow and panic.
    let current = *cell.borrow();
    *cell.borrow_mut() = current.wrapping_mul(2246822519).wrapping_add(input1);
    h = mix(h, *second.borrow());

    // A vector of clones: the count climbs, then falls as the vector's
    // elements are dropped one by one.
    let mut handles: Vec<Rc<RefCell<u32>>> = Vec::new();
    for i in 0..n {
        handles.push(Rc::clone(&cell));
        let rotated = second.borrow().rotate_left((i & 31) as u32);
        *handles[i].borrow_mut() = rotated;
    }
    h = mix(h, Rc::strong_count(&cell) as u32);
    while let Some(handle) = handles.pop() {
        h = mix(h, *handle.borrow());
        h = mix(h, Rc::strong_count(&cell) as u32);
    }

    // try_unwrap: `Err` while `second` is alive, `Ok` once it is gone.
    let shared = match Rc::try_unwrap(Rc::clone(&cell)) {
        Ok(inner) => mix(1, inner.into_inner()),
        Err(rc) => mix(2, *rc.borrow()),
    };
    h = mix(h, shared);
    drop(second);
    let unique = Rc::new(RefCell::new(input1 ^ input2));
    let owned = match Rc::try_unwrap(unique) {
        Ok(inner) => mix(3, inner.into_inner()),
        Err(rc) => mix(4, *rc.borrow()),
    };
    h = mix(h, owned);
    h.wrapping_add(Rc::strong_count(&cell) as u32)
}
