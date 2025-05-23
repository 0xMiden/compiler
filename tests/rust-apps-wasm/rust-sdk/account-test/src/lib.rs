extern crate alloc;

use miden::*;

#[global_allocator]
static ALLOC: BumpAlloc = BumpAlloc::new();

pub struct Account;

impl Account {
    #[no_mangle]
    pub fn get_wallet_magic_number() -> Felt {
        let acc_id = miden::account::get_id();
        let magic = felt!(42);
        magic + acc_id.into()
    }

    #[no_mangle]
    pub fn test_add_asset() -> Felt {
        let asset_in = Asset::new([felt!(1), felt!(2), felt!(3), felt!(4)]);
        let asset_out = miden::account::add_asset(asset_in);
        asset_out.as_word()[0]
    }

    #[no_mangle]
    pub fn test_felt_ops_smoke(a: Felt, b: Felt) -> Felt {
        let d = a.as_u64();
        if a > b {
            a.inv() + b
        } else if a < b {
            a.exp(b) - b
        } else if a <= b {
            a.pow2() * b
        } else if a >= b {
            b / a
        } else if a == b {
            miden::assert_eq(a, b);
            a + Felt::from_u64_unchecked(d)
        } else if a != b {
            -a
        } else if b.is_odd() {
            assert(a);
            b
        } else {
            assertz(b);
            a
        }
    }
}

pub struct Note;

impl Note {
    #[no_mangle]
    pub fn note_script() -> Felt {
        let mut sum = Felt::new(0).unwrap();
        for input in miden::note::get_inputs() {
            sum = sum + input;
        }
        sum
    }
}

#[no_mangle]
pub fn test_blake3_hash_1to1(input: [u8; 32]) -> [u8; 32] {
    blake3_hash_1to1(input)
}

#[no_mangle]
pub fn test_blake3_hash_2to1(input: [u8; 64]) -> [u8; 32] {
    blake3_hash_2to1(input)
}

#[no_mangle]
pub fn test_rpo_falcon512_verify(pk: Word, msg: Word) {
    rpo_falcon512_verify(pk, msg)
}

#[no_mangle]
pub fn test_pipe_words_to_memory(num_words: Felt) -> (Word, Vec<Felt>) {
    pipe_words_to_memory(num_words)
}

#[no_mangle]
pub fn test_pipe_double_words_to_memory(num_words: Felt) -> (Word, Vec<Felt>) {
    pipe_double_words_to_memory(num_words)
}

#[no_mangle]
pub fn test_remove_asset(asset: Asset) -> Felt {
    let asset_out = miden::account::remove_asset(asset);
    asset_out.as_word()[0]
}

#[no_mangle]
pub fn test_create_note(
    asset: Asset,
    tag: Tag,
    note_type: NoteType,
    recipient: Recipient,
) -> NoteId {
    miden::tx::create_note(asset, tag, note_type, recipient)
}
