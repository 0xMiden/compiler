// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

// Native account the transaction script runs against. The `#[account(...)]`
// attribute exposes the `add-contract` account component's methods on this
// wrapper (the path is `<package>::<Interface>` with `-` replaced by `_`).
#[account(add_contract::AddContract)]
pub struct NativeAccount;

#[tx_script]
fn run(_arg: Word, account: &mut NativeAccount) {
    // Call the `add` function exposed by the `add-contract` account component.
    let a = felt!(1);
    let b = felt!(2);
    account.add(a, b);
}
