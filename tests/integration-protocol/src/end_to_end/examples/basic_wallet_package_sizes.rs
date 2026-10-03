use std::path::Path;

use midenc_expect_test::expect;
use midenc_integration_test_support::{compile_project, testing::stripped_mast_size_str};

#[test]
fn basic_wallet_and_p2id() {
    let account_package = compile_project(Path::new("../../examples/basic-wallet"));
    assert!(account_package.is_library(), "expected library");
    // History: 8505 with the hand bindings; 10155 once `miden-base-sys` called the protocol
    // through its generated bindings, whose integer parameters made the hand layer range-check
    // each felt newtype per call; 8895 since `Tag`, `NoteIdx` and `NoteType` wrap the manifest's
    // integers and those checks are gone.
    //
    // The 390 bytes that remain over the hand bindings: `output_note::create` converts the
    // `NoteType` to the protocol's `u8` enum (a `u32lt` and a branch to a trap for anything but
    // private `0` or public `1`); the frontend masks the `u8` and the `u16` it passes the kernel
    // to their declared types (`push.255 u32and`, `push.65535 u32and`); and — new with the
    // integer newtypes — the component masks the `u8` note type and the `u16` note index as it
    // lifts them from its arguments, where it lifted felts before.
    expect!["8895"].assert_eq(stripped_mast_size_str(&account_package).as_str());

    let tx_script_package = compile_project(Path::new("../../examples/basic-wallet-tx-script"));
    assert!(tx_script_package.is_library(), "expected library");
    // 384 bytes more (13784 before) since `miden-stdlib-sys` calls the core library through its
    // generated bindings: `adv_load_preimage` (via `miden-tx-script-args`) checks the buffer
    // address for element alignment (`ElementPtr::from_ptr`) and passes the word count to
    // `mem::pipe_preimage_to_memory` as the `u32` the manifest declares rather than the felt the
    // hand binding passed.
    //
    // The first generated bindings came to 13941 bytes: their wrapper also range-checked the end
    // pointer the procedure returns, which is ignored (`ElementPtr::to_ptr`). Without that branch
    // after the call, LLVM lays out the inlined `decode_commitment` differently: the empty
    // pre-image path moves from the head of the procedure to its tail, and the decode result
    // becomes a three-way status matched after the join. The instruction count is the same, with
    // one more branch; putting the range check back gives 13941 again.
    //
    // A further 63 bytes (14168 before) since `mem::pipe_preimage_to_memory` resolves from the
    // core manifest instead of the deleted transitional table, which declared its address as
    // `i32`: the casts that give the stub's `i32` carriers the manifest's element-space pointer
    // type cost the stub a redundant `swap.1 swap.1` before the `exec`. That is the only change
    // to the MASM.
    //
    // 1147 bytes more (14231 before) since `Tag` and `NoteType` wrap the manifest's `u32` and
    // `u8`: `TxScriptArgs` now checks the two fields it used to read unchecked. Each felt-repr
    // reader splits its felt (`u32split`), compares the canonical `u64` with the bound through
    // the core library's `u64::gt` and branches to the decode-error exit; the value stays in a
    // 64-bit local until the `create_note` call. The same script reading the two fields as felts
    // and converting them unchecked comes to 14261 bytes, so all but 30 of the bytes are the two
    // checks — and most of that is structure rather than instructions: the two branches add 17
    // MAST nodes (2 split, 5 block, 10 join) and 7 op batches.
    expect!["15378"].assert_eq(stripped_mast_size_str(&tx_script_package).as_str());

    let note_package = compile_project(Path::new("../../examples/p2id-note"));
    assert!(note_package.is_library(), "expected library");
    // 74 bytes more than the hand tables produced: `note::compute_and_store_recipient` and
    // `note::compute_storage_commitment` resolve from the protocol manifest, which types their
    // count parameter `u16`, so the frontend masks the `i32` the SDK passes
    // (`push.65535; u32and` plus stack shuffling) to declare the import with the callee's type.
    expect!["21871"].assert_eq(stripped_mast_size_str(&note_package).as_str());
    // The note package exports both the note script and the `build-recipient` constructor; the
    // constructor must not interfere with the `@note_script`-attributed export selection.
    assert!(
        note_package.manifest.exports().any(|export| export.name() == "build-recipient"),
        "expected the p2id note package to export the `build-recipient` constructor"
    );
    miden_protocol::note::NoteScript::from_package(&note_package)
        .expect("expected the p2id note package to contain exactly one note script export");

    let p2ide_package = compile_project(Path::new("../../examples/p2ide-note"));
    assert!(p2ide_package.is_library(), "expected library");
    expect!["16436"].assert_eq(stripped_mast_size_str(&p2ide_package).as_str());
}
