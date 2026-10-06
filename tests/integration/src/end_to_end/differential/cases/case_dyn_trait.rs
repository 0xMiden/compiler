// `dyn Trait` dispatch: the vtables live in `.rodata` as arrays of funcref
// table indices, and each method call loads its slot and dispatches via
// `call_indirect`. The concrete impl is picked through a runtime-indexed array
// of trait-object references so LLVM cannot devirtualize, and two methods per
// trait exercise two distinct vtable slots per object.

trait Mix {
    fn scale(&self, x: u32) -> u32;
    fn fold(&self, x: u32, y: u32) -> u32;
}

struct Affine(u32);
struct Xor(u32);

impl Mix for Affine {
    #[inline(never)]
    fn scale(&self, x: u32) -> u32 {
        x.wrapping_mul(self.0 | 1).wrapping_add(0x9e37)
    }

    #[inline(never)]
    fn fold(&self, x: u32, y: u32) -> u32 {
        x.rotate_left(self.0 & 31) ^ y
    }
}

impl Mix for Xor {
    #[inline(never)]
    fn scale(&self, x: u32) -> u32 {
        (x ^ self.0).wrapping_sub(x >> 5)
    }

    #[inline(never)]
    fn fold(&self, x: u32, y: u32) -> u32 {
        (x | y).wrapping_mul(2654435761).wrapping_add(self.0)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = Affine(input2);
    let b = Xor(input1.wrapping_add(input2));
    // Runtime-indexed fat-pointer loads defeat devirtualization, the same way
    // the fn-pointer array idiom does.
    let objs: [&dyn Mix; 2] = [&a, &b];
    let first = objs[(input1 & 1) as usize];
    let second = objs[((input2 >> 2) & 1) as usize];
    let scaled = first.scale(input1);
    second.fold(scaled, input2).wrapping_add(first.fold(input2, input1))
}
