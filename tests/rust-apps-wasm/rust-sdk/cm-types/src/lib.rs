#![no_std]

extern crate alloc;
use alloc::vec::Vec;

#[global_allocator]
static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

use bindings::exports::miden::cm_types::cm_types::Guest;

miden::generate!();
bindings::export!(MyAccountComponent);

struct MyAccountComponent;

impl Guest for MyAccountComponent {
    // fn func_enum(e: TheEnum) -> TheEnum {
    //     match e {
    //         TheEnum::VariantA => TheEnum::VariantB,
    //         TheEnum::VariantB => TheEnum::VariantC,
    //         TheEnum::VariantC => TheEnum::VariantA,
    //     }
    // }

    // fn func_flags(f: TheFlags) -> TheFlags {
    //     if f.contains(TheFlags::FLAG_A) {
    //         f | TheFlags::FLAG_B
    //     } else {
    //         TheFlags::FLAG_C
    //     }
    // }

    // fn func_record(mut r: TheRecord) -> TheRecord {
    //     if r.rec_flags.contains(TheFlags::FLAG_B) {
    //         r.optional_enum = None;
    //     }
    //     r
    // }

    // fn func_option(o: Option<TheRecord>) -> Option<TheEnum> {
    //     if let Some(r) = o {
    //         r.optional_enum
    //     } else {
    //         None
    //     }
    // }

    // fn func_tuple(t: (TheEnum, u64)) -> (u64, TheRecord) {
    //     (
    //         t.1,
    //         TheRecord {
    //             rec_flags: TheFlags::FLAG_A,
    //             optional_enum: Some(t.0),
    //         },
    //     )
    // }

    // fn func_list(l: Vec<u8>) -> Vec<u16> {
    //     let mut sq = Vec::with_capacity(l.len());
    //     for b in l {
    //         sq.push(b as u16 * b as u16);
    //     }
    //     sq
    // }

    // fn func_result_small(r: Result<u64, u32>) -> Result<u16, u8> {
    //     match r {
    //         Ok(n) => Ok(n as u16 - 11),
    //         Err(e) => Err(e as u8 + 22),
    //     }
    // }

    // fn func_result_enum(e: TheEnum) -> Result<TheEnum, bool> {
    //     match e {
    //         TheEnum::VariantA => Err(true),
    //         TheEnum::VariantB => Ok(e),
    //         TheEnum::VariantC => Err(false),
    //     }
    // }

    // fn func_result_large(r: Result<(u64, u32), TheRecord>) -> Result<(u64, u64, u64), ()> {
    //     if let Ok((n, m)) = r {
    //         Ok((n + 11, n + 22, m as u64))
    //     } else {
    //         Err(())
    //     }
    // }

    fn func_list(list: Vec<u8>) -> Vec<u16> {
        list.into_iter().map(|n| n as u16 * 2).collect()
    }
}
