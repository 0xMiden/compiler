// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::Account;

#[note]
struct P2idNote {
    target_account_id: AccountId,
}

#[note]
impl P2idNote {
    #[entrypoint]
    pub fn run(self, _arg: Word, account: &mut Account) {
        let current_account = account.get_id();
        assert_eq!(current_account, self.target_account_id);

        let assets = active_note::get_assets();
        for asset in assets {
            account.receive_asset(asset);
        }
    }
}
