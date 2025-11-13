#![no_std]

// extern crate alloc;
// use alloc::vec::Vec;

// Global allocator to use heap memory in no-std environment
#[global_allocator]
static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

#[cfg(not(test))]
#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

use bindings::{exports::miden::base::note_script::Guest, miden::cm_types::cm_types::*};
use miden::*;

miden::generate!();
bindings::export!(MyNote);

struct MyNote;

impl Guest for MyNote {
    fn run(_arg: Word) {
        // // Enum.
        // let next_enum = func_enum(TheEnum::VariantB);
        // assert_eq!(next_enum, TheEnum::VariantC);

        // // Flags.
        // let flags = func_flags(TheFlags::FLAG_A | TheFlags::FLAG_C);
        // assert!(flags.contains(TheFlags::FLAG_B));
        //
        // let flags = func_flags(TheFlags::FLAG_B);
        // assert!(flags == TheFlags::FLAG_C);

        // // Records.
        // let rec = TheRecord {
        //     rec_flags: TheFlags::FLAG_A,
        //     optional_enum: Some(TheEnum::VariantA),
        // };
        //
        // let mut rec = func_record(rec);
        // assert!(rec.optional_enum.unwrap() == TheEnum::VariantA);
        //
        // rec.rec_flags = TheFlags::FLAG_B;
        // let rec = func_record(rec);
        // assert!(rec.optional_enum.is_none());

        // // Options.
        // let rec = TheRecord {
        //     rec_flags: TheFlags::FLAG_B,
        //     optional_enum: Some(TheEnum::VariantB),
        // };
        //
        // let opt_enum = func_option(Some(rec));
        // assert!(opt_enum.is_some());
        // assert_eq!(opt_enum.unwrap(), TheEnum::VariantB);
        //
        // let none_enum = func_option(None);
        // assert!(none_enum.is_none());

        // // Tuples.
        // let (n, rec) = func_tuple((TheEnum::VariantC, 11));
        // assert_eq!(n, 11);
        // assert_eq!(rec.rec_flags, TheFlags::FLAG_A);
        // assert!(rec.optional_enum.is_some());
        // assert_eq!(rec.optional_enum.unwrap(), TheEnum::VariantC);
        //
        // // Small results.
        // let res_ok = func_result_small(Ok(33_u64));
        // assert!(res_ok.is_ok());
        // assert_eq!(res_ok.unwrap(), 22_u16);
        //
        // let res_err = func_result_small(Err(44_u32));
        // assert!(res_err.is_err());
        // assert_eq!(res_err.err().unwrap(), 66_u8);

        // // Enum results.
        // let res_enum_a = func_result_enum(TheEnum::VariantA);
        // assert!(res_enum_a.is_err());
        // assert_eq!(res_enum_a.err().unwrap(), true);
        //
        // let res_enum_b = func_result_enum(TheEnum::VariantB);
        // assert!(res_enum_b.is_ok());
        // assert_eq!(res_enum_b.unwrap(), TheEnum::VariantB);
        //
        // let res_enum_c = func_result_enum(TheEnum::VariantC);
        // assert!(res_enum_c.is_err());
        // assert_eq!(res_enum_c.err().unwrap(), false);
        //
        // // XXX: Need to test an enum as the error.
        //
        // // Large results.
        // let res_ok = func_result_large(Ok((55_u64, 66_u32)));
        // assert!(res_ok.is_ok());
        // assert_eq!(res_ok.unwrap(), (66_u64, 77_u64, 66_u64));
        //
        // let res_err = func_result_large(Err(rec));
        // assert!(res_err.is_err());
        // assert_eq!(res_err.err().unwrap(), ());

        // List.
        let res_list_a = func_list(&[11, 22, 33]);
        assert!(res_list_a.len() == 3);
        assert!(res_list_a[0] == 22);
        assert!(res_list_a[1] == 44);
        assert!(res_list_a[2] == 66);
    }
}
