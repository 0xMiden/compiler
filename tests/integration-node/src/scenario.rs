//! This module provides infrastructure for writing integration tests that execute against the
//! Miden testnet.
use std::{collections::BTreeMap, sync::Arc};

use miden_client::{
    account::{
        component::{BasicWallet, RpoFalcon512},
        Account, AccountId, AccountStorageMode, AccountType,
    },
    asset::Asset,
    builder::ClientBuilder,
    crypto::FeltRng,
    note::{Note, NoteType},
    rpc::{Endpoint, NodeRpcClient, TonicRpcClient},
    sync::SyncSummary,
    transaction::{TransactionId, TransactionRequestBuilder},
    Client, Felt,
};
use miden_core::{utils::Deserializable, FieldElement, Word};
use miden_objects::{
    account::{
        AccountBuilder, AccountComponent, AccountComponentMetadata, AccountComponentTemplate,
        InitStorageData,
    },
    //transaction::TransactionScript,
};
use rand::RngCore;

/// 10s
const RPC_TIMEOUT: u64 = 10_000;

/// Represents an integration test that executes against the Miden testnet
///
/// A scenario consists of one or more steps/actions which build up some desired state, and then
/// asserts facts about that state, before continuing to make further changes, or terminating
/// successfully.
///
/// A scenario can fail for a number of different reasons:
///
/// * Invalid parameters provided to the client
/// * Invalid account/note creation parameters
/// * A transaction request failed, or the transaction could not be submitted
/// * An attempt to perform an action that is not valid at that point (e.g. creating an account
///   that already exists).
/// * An assertion about the state of the network doesn't hold
///
/// Currently, these all result in asserts/panics, so that we can pinpoint the source of errors,
/// but as a result, a failed tests may be due to factors out of your control, such as the testnet
/// being temporarily unavailable. You must assess the panic output to determine if the failure
/// is truly a failed test, or something spurious.
///
/// NOTE: Created transactions can be viewed online via MidenScan given the transaction id, using
/// the URL `https://testnet.midenscan.com/tx/{id}`.
pub struct Scenario {
    dir: temp_dir::TempDir,
    rpc_client: Arc<dyn NodeRpcClient + Send>,
    actions: Vec<Action>,
    accounts: BTreeMap<&'static str, AccountId>,
    notes: Vec<Note>,
    transactions: Vec<TransactionId>,
    sync_summary: Option<miden_client::sync::SyncSummary>,
}

impl Default for Scenario {
    fn default() -> Self {
        let endpoint =
            Endpoint::new("https".to_string(), "rpc.testnet.miden.io".to_string(), Some(443));
        Self::with_endpoint(endpoint)
    }
}

impl Scenario {
    /// Create a new, empty test scenario targeting the public testnet
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new, empty test scenario targeting the testnet at the specified endpoint
    pub fn with_endpoint(endpoint: Endpoint) -> Self {
        let dir = temp_dir::TempDir::new().unwrap();
        let rpc_client = Arc::new(TonicRpcClient::new(&endpoint, RPC_TIMEOUT));

        Self {
            dir,
            rpc_client,
            actions: Default::default(),
            accounts: Default::default(),
            notes: Default::default(),
            transactions: Default::default(),
            sync_summary: None,
        }
    }

    /// An existing account will be made available to subsequent scenario steps via `alias`
    pub fn with_existing_account(&mut self, id: AccountId, alias: &'static str) -> &mut Self {
        self.accounts.insert(alias, id);
        self
    }

    /// A new account will be created with the given alias and code.
    ///
    /// Subsequent actions in this scenario may refer to this account using the provided alias.
    ///
    /// Returns a builder that can be used to customize how the account is created.
    pub fn create_account(
        &mut self,
        alias: &'static str,
        code: Arc<miden_mast_package::Package>,
    ) -> ScenarioAccountBuilder<'_> {
        ScenarioAccountBuilder::new(self, alias, code)
    }

    /// A new note will be created with the given code, to be sent from the account aliased by
    /// `sender`, to the account aliased by `recipient`.
    ///
    /// NOTE: The sender/recipient aliases must correspond to accounts which are created, or are
    /// mapped to existing accounts, when the scenario executes this step, or an error will occur.
    ///
    /// Returns a builder that can be used to customize the note that will be created.
    pub fn create_note(
        &mut self,
        code: Arc<miden_mast_package::Package>,
        sender: &'static str,
        recipient: &'static str,
    ) -> ScenarioNoteBuilder<'_> {
        ScenarioNoteBuilder::new(self, code, sender, recipient)
    }

    /// A new transaction will be created against the account aliased by `to`, containing all of
    /// the notes created so far (and consuming them in the process).
    ///
    /// NOTE: The provided account alias must correspond to an account which was created, or was
    /// mapped to an existing account, when the scenario executes this step, or an error will occur.
    ///
    /// When this step is executed, the transaction will be submitted to the network, and the client
    /// state will be synchronized so that transaction effects are visible.
    pub fn submit_transaction(&mut self, to: &'static str) -> &mut Self {
        self.actions.push(Action::SubmitTransaction { account: to });
        self
    }

    /// The scenario will assert that the storage map at `index` of `account`, contains `expected`
    /// as the value of `key` when this step is executed.
    pub fn assert_account_storage_map_entry_eq(
        &mut self,
        account: &'static str,
        index: u8,
        key: Word,
        expected: Word,
    ) -> &mut Self {
        self.actions.push(Action::AssertStorageMapEq {
            account,
            index,
            key,
            expected,
        });
        self
    }

    /// Get a reference to the temporary directory used for this scenario
    pub fn temp_dir(&self) -> &temp_dir::TempDir {
        &self.dir
    }

    /// Get a new [Client] for interacting with the testnet
    async fn get_client(&self) -> Client {
        let keystore_dir = self.dir.child("keystore");
        ClientBuilder::new()
            .rpc(self.rpc_client.clone())
            .filesystem_keystore(keystore_dir.as_path().to_str().unwrap())
            .in_debug_mode(true)
            .build()
            .await
            .unwrap_or_else(|err| panic!("failed to create testnet client: {err}"))
    }

    /// Run this scenario to completion.
    ///
    /// Returns the sync summary of the client if successful
    pub fn run(mut self) -> Option<SyncSummary> {
        let mut builder = tokio::runtime::Builder::new_current_thread();
        let rt = builder.enable_all().build().unwrap();
        rt.block_on(async move {
            let mut client = self.get_client().await;

            // Synchronize with the network before continuing
            let sync_summary = client.sync_state().await.unwrap();
            self.sync_summary = Some(sync_summary);

            let actions = core::mem::take(&mut self.actions);
            for action in actions {
                action.execute(&mut client, &mut self).await;
            }

            self.sync_summary.take()
        })
    }
}

enum Action {
    /// Create an account with the given alias, code, and initial storage contents
    CreateAccount(CreateAccountParams),
    /// Create a note to be sent from one account to another
    CreateNote(CreateNoteParams),
    /// Submit all notes created so far to the given account alias
    SubmitTransaction {
        /// The account alias we're submitting the transaction against
        account: &'static str,
    },
    /// Assert that the given storage map entry matches an expected value
    AssertStorageMapEq {
        /// The account alias whose storage we're asserting against
        account: &'static str,
        /// The storage slot index
        ///
        /// NOTE: The referenced slot must be a storage map slot
        index: u8,
        /// The storage map key to assert against
        key: Word,
        /// The expected value stored under the key
        expected: Word,
    },
}

/// A builder pattern struct for customizing the creation of an account during a given scenario
pub struct ScenarioAccountBuilder<'a> {
    scenario: &'a mut Scenario,
    params: CreateAccountParams,
}

impl<'a> ScenarioAccountBuilder<'a> {
    fn new(
        scenario: &'a mut Scenario,
        alias: &'static str,
        account: Arc<miden_mast_package::Package>,
    ) -> Self {
        Self {
            scenario,
            params: CreateAccountParams {
                alias,
                account,
                init_storage_data: None,
            },
        }
    }

    /// Provide the initial storage data for the account when it is created
    pub fn with_init_storage_data(&mut self, data: InitStorageData) -> &mut Self {
        self.params.init_storage_data = Some(data);
        self
    }

    /// Finalizes the account creation parameters and returns to the current [Scenario]
    pub fn then(self) -> &'a mut Scenario {
        let ScenarioAccountBuilder { scenario, params } = self;
        scenario.actions.push(Action::CreateAccount(params));
        scenario
    }
}

/// A builder pattern struct for customizing the creation of a note during a given scenario
pub struct ScenarioNoteBuilder<'a> {
    scenario: &'a mut Scenario,
    params: CreateNoteParams,
}

impl<'a> ScenarioNoteBuilder<'a> {
    fn new(
        scenario: &'a mut Scenario,
        note: Arc<miden_mast_package::Package>,
        sender: &'static str,
        recipient: &'static str,
    ) -> Self {
        Self {
            scenario,
            params: CreateNoteParams {
                note,
                note_ty: NoteType::Public,
                inputs: Default::default(),
                assets: Default::default(),
                sender,
                recipient,
            },
        }
    }

    /// Override the default note type of `Public`
    pub fn with_note_type(&mut self, ty: NoteType) -> &mut Self {
        self.params.note_ty = ty;
        self
    }

    /// Specify the note inputs
    pub fn with_inputs(&mut self, inputs: impl IntoIterator<Item = Felt>) -> &mut Self {
        self.params.inputs.extend(inputs);
        self
    }

    /// Specify the assets attached to the note
    pub fn with_assets(&mut self, assets: impl IntoIterator<Item = Asset>) -> &mut Self {
        self.params.assets.extend(assets);
        self
    }

    /// Finalizes the note creation parameters and returns to the current [Scenario]
    pub fn then(self) -> &'a mut Scenario {
        let ScenarioNoteBuilder { scenario, params } = self;
        scenario.actions.push(Action::CreateNote(params));
        scenario
    }
}

struct CreateAccountParams {
    /// The friendly name by which this account can be referenced before and after it is
    /// created within the context of a [Scenario]
    alias: &'static str,
    /// The code for the account
    account: Arc<miden_mast_package::Package>,
    /// The initial storage data to populate the account with
    init_storage_data: Option<InitStorageData>,
    // TODO: Support overriding account components
    //components: Vec<AccountComponent>,
}

struct CreateNoteParams {
    note: Arc<miden_mast_package::Package>,
    note_ty: NoteType,
    inputs: Vec<Felt>,
    assets: Vec<Asset>,
    sender: &'static str,
    recipient: &'static str,
}

impl Action {
    pub async fn execute(&self, client: &mut Client, scenario: &mut Scenario) {
        match self {
            Self::CreateAccount(CreateAccountParams {
                alias,
                account,
                init_storage_data,
            }) => {
                let account = match init_storage_data.as_ref() {
                    Some(data) => create_account(client, account, data).await,
                    None => {
                        let init_storage_data = InitStorageData::default();
                        create_account(client, account, &init_storage_data).await
                    }
                };

                scenario.accounts.insert(*alias, account.id());
            }
            Self::CreateNote(params) => {
                let note = create_note(client, scenario, params).await;

                scenario.notes.push(note);
            }
            Self::SubmitTransaction { account } => {
                let id = submit_transaction(client, scenario, account).await;

                scenario.transactions.push(id);

                // Sync the client with the network so we can observe the transaction effects
                let summary = client.sync_state().await.unwrap();

                match scenario.sync_summary.as_mut() {
                    Some(current_summary) => {
                        current_summary.combine_with(summary);
                    }
                    None => {
                        scenario.sync_summary = Some(summary);
                    }
                }
            }
            Self::AssertStorageMapEq {
                account,
                index,
                key,
                expected,
            } => {
                assert_account_storage_map_eq(client, scenario, account, *index, *key, *expected)
                    .await;
            }
        }
    }
}

async fn create_account(
    client: &mut Client,
    account_package: &miden_mast_package::Package,
    init_storage_data: &InitStorageData,
) -> Account {
    let account_component = match account_package.account_component_metadata_bytes.as_deref() {
        None => todo!("unsupported account package: no account component metadata present"),
        Some(bytes) => {
            let metadata = AccountComponentMetadata::read_from_bytes(bytes).unwrap();
            let template = AccountComponentTemplate::new(
                metadata,
                account_package.unwrap_library().as_ref().clone(),
            );
            AccountComponent::from_template(&template, init_storage_data)
                .unwrap()
                .with_supported_type(AccountType::RegularAccountImmutableCode)
        }
    };

    // Init seed for the account
    let mut init_seed = [0_u8; 32];
    client.rng().fill_bytes(&mut init_seed);

    // Build the new `Account` with the component
    let key_pair = miden_client::crypto::SecretKey::with_rng(client.rng());
    let (account, seed) = AccountBuilder::new(init_seed)
        .account_type(AccountType::RegularAccountImmutableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_component(account_component.clone())
        .with_component(RpoFalcon512::new(key_pair.public_key()))
        .with_component(BasicWallet)
        .build()
        .unwrap_or_else(|err| panic!("failed to build account: {err}"));

    client
        .add_account(&account, Some(seed), false)
        .await
        .unwrap_or_else(|err| panic!("account creation failed: {err}"));

    account
}

/// Build a note
async fn create_note(client: &mut Client, scenario: &Scenario, params: &CreateNoteParams) -> Note {
    let note_program = params.note.unwrap_program();
    let note_script = miden_client::note::NoteScript::from_parts(
        note_program.mast_forest().clone(),
        note_program.entrypoint(),
    );

    let sender = scenario.accounts[params.sender];
    let recipient = scenario.accounts[params.recipient];

    let tag = miden_client::note::NoteTag::from_account_id(recipient);
    let inputs = miden_client::note::NoteInputs::new(params.inputs.clone()).unwrap();
    let serial_num = client.rng().draw_word();
    let vault = miden_client::note::NoteAssets::new(params.assets.clone()).unwrap();
    let metadata = miden_client::note::NoteMetadata::new(
        sender,
        params.note_ty,
        tag,
        miden_client::note::NoteExecutionHint::always(),
        Felt::ZERO,
    )
    .unwrap();
    let recipient = miden_client::note::NoteRecipient::new(serial_num, note_script, inputs);
    miden_client::note::Note::new(vault, metadata, recipient)
}

async fn submit_transaction(
    client: &mut Client,
    scenario: &mut Scenario,
    account: &'static str,
) -> TransactionId {
    let output_notes = core::mem::take(&mut scenario.notes)
        .into_iter()
        .map(miden_client::transaction::OutputNote::Full);
    // Build a transaction request
    let tx_request =
        TransactionRequestBuilder::new().own_output_notes(output_notes).build().unwrap();

    // Execute the transaction locally
    let target_account = scenario.accounts[account];
    let tx_result = client.new_transaction(target_account, tx_request).await.unwrap();

    let tx_id = tx_result.executed_transaction().id();

    // Submit transaction to the network
    let _ = client.submit_transaction(tx_result).await;

    tx_id
}

async fn assert_account_storage_map_eq(
    client: &Client,
    scenario: &Scenario,
    account: &'static str,
    index: u8,
    key: Word,
    expected: Word,
) {
    let account_id = scenario.accounts[account];
    let account = client
        .get_account(account_id)
        .await
        .unwrap_or_else(|err| panic!("failed to get account record: {err}"))
        .unwrap_or_else(|| panic!("no account found for the id associated with alias '{account}'"));

    let item = account.account().storage().get_map_item(index, key).unwrap();

    assert_eq!(item, expected);
}
