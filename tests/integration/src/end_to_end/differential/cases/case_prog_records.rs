// A small record arena driven entirely by `Option` / `Result` combinators
// (campaign 27, program 8): eight slots hold `Option<Record>`, and a fixed
// script of operations moves values between them with `take` / `replace` /
// `get_or_insert_with` / `map` / `and_then` / `filter` / `zip` / `ok_or` /
// `unwrap_or_else` / `?`, with `core::mem::swap` / `replace` / `take` doing
// the in-place moves and a derived-`Debug` error enum reporting failures.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Record {
    key: u16,
    weight: u32,
    flags: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotErr {
    Empty(usize),
    OutOfRange(usize),
    Underflow { key: u16 },
}

fn slot(arena: &mut [Option<Record>; 8], i: usize) -> Result<&mut Record, SlotErr> {
    if i >= arena.len() {
        return Err(SlotErr::OutOfRange(i));
    }
    arena[i].as_mut().ok_or(SlotErr::Empty(i))
}

fn charge(arena: &mut [Option<Record>; 8], from: usize, to: usize, amount: u32) -> Result<u32, SlotErr> {
    let src = slot(arena, from)?;
    let left = src.weight.checked_sub(amount).ok_or(SlotErr::Underflow { key: src.key })?;
    src.weight = left;
    let key = src.key;
    let dst = slot(arena, to)?;
    dst.weight = dst.weight.wrapping_add(amount);
    dst.flags |= 1;
    Ok((key as u32) ^ left)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut arena: [Option<Record>; 8] = core::array::from_fn(|i| {
        let v = input1.rotate_left(i as u32 * 4) ^ input2;
        (v % 5 != 0).then_some(Record {
            key: v as u16,
            weight: v >> 8,
            flags: v as u8,
        })
    });

    let mut acc = 0u32;
    let mut errors = 0u32;
    let mut step = 0usize;
    while step < 12 {
        // Index arithmetic stays in `u32` before the `as usize`: `usize` is
        // 64 bits natively and 32 bits on wasm, so a `usize` wrap-around
        // ahead of a non-power-of-two modulus is a false divergence.
        let a = (input2.wrapping_add(step as u32) % 9) as usize;
        let b = (input1.wrapping_add(step as u32 * 3) % 9) as usize;
        let amount = input1.rotate_right(step as u32) % 4096;
        acc = acc.wrapping_mul(31).wrapping_add(match charge(&mut arena, a, b, amount) {
            Ok(v) => v,
            Err(e) => {
                errors += 1;
                match e {
                    SlotErr::Empty(i) => 0x10 + i as u32,
                    SlotErr::OutOfRange(i) => 0x20 + i as u32,
                    SlotErr::Underflow { key } => 0x1000 | key as u32,
                }
            }
        });
        step += 1;
    }

    // Combinator chains over the slots themselves.
    let i = (input2 % 8) as usize;
    let j = (input1 % 8) as usize;
    acc = acc.wrapping_add(
        arena[i]
            .map(|r| r.weight >> 1)
            .and_then(|w| w.checked_mul(3))
            .filter(|w| w % 7 != 0)
            .unwrap_or_else(|| input1 ^ 0x5eed),
    );
    acc = acc.wrapping_add(
        arena[i]
            .zip(arena[j])
            .map(|(x, y)| x.key as u32 ^ y.weight)
            .unwrap_or(0x2bad),
    );
    acc = acc.wrapping_add(arena[j].take().map_or(3, |r| r.flags as u32));
    acc = acc.wrapping_add(arena[j].get_or_insert_with(Record::default).key as u32);
    acc = acc.wrapping_add(
        arena[i]
            .replace(Record {
                key: input2 as u16,
                weight: input1,
                flags: 0xaa,
            })
            .map_or(5, |r| r.weight),
    );
    acc = acc.wrapping_add(arena[i].is_some() as u32 + arena[j].is_none() as u32 * 2);
    acc = acc.wrapping_add(
        arena[j]
            .ok_or(SlotErr::Empty(j))
            .map(|r| r.weight)
            .unwrap_or_else(|e| match e {
                SlotErr::Empty(k) => k as u32,
                _ => 9,
            }),
    );

    // In-place moves.
    let (lo, hi) = arena.split_at_mut(4);
    core::mem::swap(&mut lo[i % 4], &mut hi[j % 4]);
    let pulled = core::mem::take(&mut lo[(i + 1) % 4]).unwrap_or_default();
    let displaced = core::mem::replace(&mut hi[(j + 1) % 4], Some(pulled)).unwrap_or_default();
    acc = acc.wrapping_add(pulled.weight ^ displaced.key as u32);

    let mut k = 0usize;
    while k < 8 {
        acc = acc.wrapping_mul(17).wrapping_add(match arena[k] {
            Some(r) => (r.key as u32).wrapping_add(r.weight).wrapping_add(r.flags as u32),
            None => 0xfeed,
        });
        k += 1;
    }
    acc.wrapping_add(errors * 101)
}
