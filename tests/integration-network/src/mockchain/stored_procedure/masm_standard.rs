//! A dispatcher component calling a MASM standards-library procedure through a stored root.

use miden_client::{
    account::{
        AccountComponent,
        component::{BasicWallet, InitStorageData, StorageValueName},
    },
    note::NoteTag,
    transaction::RawOutputNote,
};
use miden_core::{Felt, Word};
use miden_protocol::{
    account::{AccountBuilder, AccountComponentCode, AccountId, AccountType, auth::AuthScheme},
    crypto::rand::RandomCoin,
};
use miden_standards::{account::access::RoleBasedAccessControl, testing::note::NoteBuilder};
use miden_testing::{AccountState, Auth, MockChain};
use midenc_expect_test::expect;

use super::{
    super::support::{execute_tx_measurements, note_script_root, single_note_cycles},
    common::{
        DispatchProjectNames, assert_word_value_slots, build_dispatcher_package, build_note_package,
    },
};

/// Deploys one account holding the standards `RoleBasedAccessControl` component, whose
/// `has_role` procedure root is stored in a dispatcher slot declared with the procedure's stack
/// contract (`role_symbol` on top, then the account id suffix and prefix). Two notes query the
/// role of the admin member and of a non-member and check the answers, proving that a MASM
/// account procedure is a valid `dyncall` target and pinning the argument order against the
/// standards ABI.
#[test]
fn masm_standard() {
    let names = DispatchProjectNames::new("stored_procedure_masm_standard");
    let (dispatcher_project, dispatcher_package) =
        build_dispatcher_package(&names, DISPATCHER_SOURCE);
    let note_package = build_note_package(&names, "note", dispatcher_project.root(), NOTE_SOURCE);

    let has_role_slot = names.dispatcher_slot("has_role");
    assert_word_value_slots(&dispatcher_package, std::slice::from_ref(&has_role_slot));

    let has_role_root = standards_procedure_root(RoleBasedAccessControl::code(), "has_role");

    let mut builder = MockChain::builder();
    let member = builder.add_existing_wallet(Auth::Noop).unwrap().id();
    let non_member = builder.add_existing_wallet(Auth::Noop).unwrap().id();

    let rbac_component: AccountComponent =
        RoleBasedAccessControl::with_admins([member]).unwrap().into();
    let dispatcher_component = {
        let mut init_storage_data = InitStorageData::default();
        init_storage_data
            .insert_value(StorageValueName::from_slot_name(&has_role_slot), has_role_root)
            .unwrap();
        AccountComponent::from_package(&dispatcher_package, &init_storage_data).unwrap()
    };

    let account_builder = AccountBuilder::new([2_u8; 32])
        .account_type(AccountType::Public)
        .with_component(BasicWallet)
        .with_component(rbac_component)
        .with_component(dispatcher_component);
    let account = builder
        .add_account_from_builder(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            account_builder,
            AccountState::Exists,
        )
        .expect("failed to add the dispatch account to the mock chain builder");

    let admin_role: Felt = RoleBasedAccessControl::admin_role().into();
    let mut notes = Vec::new();
    for (queried, expected) in [(member, Felt::ONE), (non_member, Felt::ZERO)] {
        let rng = RandomCoin::new(note_script_root(note_package.as_ref()));
        let note = NoteBuilder::new(account.id(), rng)
            .package((*note_package).clone())
            .note_storage(role_check_storage(admin_role, queried, expected))
            .unwrap()
            .tag(NoteTag::with_account_target(account.id()).into())
            .build()
            .unwrap();
        builder.add_output_note(RawOutputNote::Full(note.clone()));
        notes.push(note);
    }

    let mut chain = builder.build().expect("failed to build mock chain");
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    let mut note_cycles = Vec::new();
    for note in notes {
        let account = chain.committed_account(account.id()).unwrap().clone();
        let mock_tx = chain
            .build_transaction(account)
            .authenticated_input_note(note.id())
            .build()
            .unwrap();
        let tx_measurements = execute_tx_measurements(&mut chain, mock_tx);
        note_cycles.push(single_note_cycles(&tx_measurements).to_string());
    }
    // Member query, then non-member query
    expect![[r#"
        [
            "3602",
            "3507",
        ]
    "#]]
    .assert_debug_eq(&note_cycles);
}

/// Returns the MAST root of the procedure `leaf` exported by a standards component.
///
/// The host obtains MASM procedure roots from the standards crate's component code, exactly as a
/// deployment populating the slot would; the export is selected by leaf name, as the module path
/// under which the standards library exports it is an implementation detail.
fn standards_procedure_root(code: &AccountComponentCode, leaf: &str) -> Word {
    let suffix = format!("::{leaf}");
    let matches = code
        .exports()
        .filter(|export| export.path.as_ref().as_str().ends_with(&suffix))
        .collect::<Vec<_>>();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one standards export named `{leaf}`, got {:?}",
        code.exports()
            .map(|export| export.path.as_ref().as_str().to_string())
            .collect::<Vec<_>>()
    );
    matches[0].digest
}

/// Note storage layout of the role-check note: the role symbol, the queried account id in the
/// note's `AccountId` field order (prefix, suffix), and the expected answer.
fn role_check_storage(role: Felt, queried: AccountId, expected: Felt) -> Vec<Felt> {
    vec![role, queried.prefix().as_felt(), queried.suffix(), expected]
}

/// Dispatcher component holding the root of the standards `has_role` procedure.
///
/// The slot signature spells the MASM stack contract of `has_role`: the callee sees the first
/// parameter on top of the stack, so the account id is passed as its suffix then prefix felts,
/// as `rbac.masm` documents (`[role_symbol, account_suffix, account_prefix, ..]`).
const DISPATCHER_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, Felt, StorageValue, StoredProcedure};

/// Storage holding the root of the role query procedure.
#[component_storage]
struct DispatcherStorage {
    #[storage(description = "root of the standards rbac::has_role procedure")]
    has_role: StorageValue<StoredProcedure<fn(role: Felt, account_suffix: Felt, account_prefix: Felt) -> bool>>,
}

/// Account component querying role membership through the stored procedure.
#[component]
trait Dispatcher {
    /// Returns whether the account holds `role`, as answered by the stored procedure.
    #[account_procedure]
    fn check_role(&self, role: Felt, account_suffix: Felt, account_prefix: Felt) -> bool;
}

#[component]
impl Dispatcher for DispatcherStorage {
    fn check_role(&self, role: Felt, account_suffix: Felt, account_prefix: Felt) -> bool {
        self.has_role.get().call(role, account_suffix, account_prefix)
    }
}
"#;

/// Note script querying a role through the dispatcher and checking the expected answer.
const NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native (active) account of the note: the dispatcher component account.
#[account(stored_procedure_masm_standard_dispatcher_account::Dispatcher)]
struct Account;

/// Role query carried in the note storage.
#[note]
struct RoleCheckNote {
    /// The role symbol to query.
    role: Felt,
    /// The account whose membership is queried.
    account: AccountId,
    /// The expected answer: 1 when the account holds the role, 0 otherwise.
    expected: Felt,
}

#[note]
impl RoleCheckNote {
    /// Queries the role through the stored procedure and checks the answer.
    #[note_script]
    pub fn run(self, _arg: Word, account: &mut Account) {
        let has_role = account.check_role(self.role, self.account.suffix, self.account.prefix);
        let answer = if has_role { felt!(1) } else { felt!(0) };
        assert_eq(answer, self.expected);
    }
}
"#;
