//! Test scenario infrastructure for mock-chain integration tests.
//!
//! Provides a typestate builder for constructing and executing test scenarios
//! against a [`MockChainBuilder`].
//!
//! The build steps are currently as follows:
//! ScenarioBuilder -> ScenarioPackageSetup -> ScenarioAccountSetup -> Scenario
//!
//! Afterwards, one can call `Scenario::build()` to get the resulting `MockChain`.

use std::{collections::BTreeMap, sync::Arc};

use miden_client::{
    account::Account,
    testing::{AccountState, Auth, MockChain, MockChainBuilder},
};
use miden_integration_tests::CompilerTestBuilder;
use miden_mast_package::Package;
use midenc_frontend_wasm::WasmTranslationConfig;

use super::helpers::build_existing_basic_wallet_account_builder;

// ================================= Step 1 ====================================

/// First build phase: create the scenario builder.
///
/// Call [`next`](Self::next) to transition to the package-setup phase.
pub struct ScenarioBuilder {
    chain_builder: MockChainBuilder,
}

impl ScenarioBuilder {
    /// Create a new scenario builder with default settings.
    pub fn new() -> Self {
        Self {
            chain_builder: MockChainBuilder::new(),
        }
    }

    /// Escape hatch for when a more detailed mock chain setup is required.
    pub fn get_mock_chain(&mut self) -> &mut MockChainBuilder {
        &mut self.chain_builder
    }

    pub fn next(self) -> ScenarioPackageSetup {
        ScenarioPackageSetup {
            chain_builder: self.chain_builder,
            packages: BTreeMap::new(),
        }
    }
}

// ====================== Step 2: Package compilation =========================

/// Second build phase: compile and register packages by alias.
pub struct ScenarioPackageSetup {
    chain_builder: MockChainBuilder,
    packages: BTreeMap<String, Arc<Package>>,
}

impl ScenarioPackageSetup {
    /// Escape hatch for when a more detailed mock chain setup is required.
    pub fn get_mock_chain(&mut self) -> &mut MockChainBuilder {
        &mut self.chain_builder
    }

    /// Compile a crate in release mode and register it under the given alias.
    pub fn add_package(mut self, path: &str, alias: impl AsRef<str>) -> Self {
        let config = WasmTranslationConfig::default();
        let mut builder = CompilerTestBuilder::rust_source_cargo_miden(path, config, []);
        builder.with_release(true);
        let mut test = builder.build();
        let package = test.compiled_package();
        self.packages.insert(alias.as_ref().to_string(), package);
        self
    }

    pub fn next(self) -> ScenarioAccountSetup {
        ScenarioAccountSetup {
            chain_builder: self.chain_builder,
            packages: self.packages,
            accounts: BTreeMap::new(),
        }
    }
}

// ========================= Step 3: Account setup ============================

/// Third build phase: add accounts and genesis notes to the chain.
pub struct ScenarioAccountSetup {
    chain_builder: MockChainBuilder,
    packages: BTreeMap<String, Arc<Package>>,
    accounts: BTreeMap<String, Account>,
}

impl ScenarioAccountSetup {
    /// Escape hatch for when a more detailed mock chain setup is required.
    pub fn get_mock_chain(&mut self) -> &mut MockChainBuilder {
        &mut self.chain_builder
    }

    /// Register an account under an alias.
    pub fn add_account(mut self, wallet_account: WalletAccountConfig) -> Self {
        let alias = wallet_account.alias;
        let package = self.packages.get(&wallet_account.wallet_package).unwrap_or_else(|| {
            panic!("Failed to find package under alias {}", wallet_account.wallet_package)
        });
        let created_account = self
            .chain_builder
            .add_account_from_builder(
                wallet_account.auth,
                build_existing_basic_wallet_account_builder(
                    package.clone(),
                    wallet_account.with_std_basic_wallet,
                    wallet_account.seed,
                ),
                wallet_account.state,
            )
            .unwrap();

        self.accounts.insert(alias, created_account);
        self
    }

    /// Register a faucet account on the mock chain.
    pub fn add_faucet(mut self, faucet: FaucetAccountConfig) -> Self {
        let faucet_account = self
            .chain_builder
            .add_existing_basic_faucet(faucet.auth, &faucet.token_symbol, faucet.max_supply, None)
            .unwrap();

        self.accounts.insert(faucet.alias, faucet_account);
        self
    }

    /// Transition to the scenario phase.
    pub fn next(self) -> Scenario {
        Scenario {
            packages: self.packages,
            chain_builder: self.chain_builder,
            accounts: self.accounts,
        }
    }
}

/// Configuration for creating a wallet account on the mock chain.
pub struct WalletAccountConfig {
    /// Alias used to store the wallet account in the scenario's account map.
    alias: String,
    /// Authentication scheme for the wallet.
    auth: Auth,
    /// Whether the account exists on-chain or is new.
    state: AccountState,
    /// Whether to include the standard basic wallet component.
    with_std_basic_wallet: bool,
    /// Seed bytes used for account creation.
    seed: [u8; 32],
    /// Package alias for the wallet.
    wallet_package: String,
}

impl Default for WalletAccountConfig {
    fn default() -> Self {
        Self {
            alias: String::new(),
            auth: Auth::BasicAuth,
            state: AccountState::Exists,
            with_std_basic_wallet: false,
            seed: [0u8; 32],
            wallet_package: String::new(),
        }
    }
}

impl WalletAccountConfig {
    /// Set the alias used to store the wallet account in the scenario's account map.
    pub fn with_alias(mut self, alias: impl AsRef<str>) -> WalletAccountConfig {
        self.alias = alias.as_ref().to_string();
        self
    }

    /// Set the authentication scheme for the wallet.
    pub fn with_auth(mut self, auth: Auth) -> WalletAccountConfig {
        self.auth = auth;
        self
    }

    /// Set whether the account exists on-chain or is new.
    pub fn with_state(mut self, state: AccountState) -> WalletAccountConfig {
        self.state = state;
        self
    }

    /// Set whether to include the standard basic wallet component.
    pub fn with_std_basic_wallet(mut self, std_basic_wallet: bool) -> WalletAccountConfig {
        self.with_std_basic_wallet = std_basic_wallet;
        self
    }

    /// Set the seed bytes used for account creation.
    pub fn with_seed(mut self, seed: [u8; 32]) -> WalletAccountConfig {
        self.seed = seed;
        self
    }

    /// Set the package alias for the wallet.
    pub fn with_wallet_package(mut self, wallet_package: impl AsRef<str>) -> WalletAccountConfig {
        self.wallet_package = wallet_package.as_ref().to_string();
        self
    }
}

/// Configuration for creating a faucet account on the mock chain.
pub struct FaucetAccountConfig {
    /// Alias used to store the faucet account in the scenario's account map.
    alias: String,
    /// Authentication scheme for the faucet.
    auth: Auth,
    /// Token symbol for the faucet's fungible asset.
    token_symbol: String,
    /// Maximum supply of the fungible asset.
    max_supply: u64,
    /// Total issuance of the fungible asset.
    total_issuance: u64,
}

impl Default for FaucetAccountConfig {
    fn default() -> Self {
        Self {
            alias: String::new(),
            auth: Auth::BasicAuth,
            token_symbol: "TEST".to_string(),
            max_supply: 1_000_000_000,
            total_issuance: 0,
        }
    }
}

impl FaucetAccountConfig {
    /// Set the alias used to store the faucet account in the scenario's account map.
    pub fn with_alias(mut self, alias: impl AsRef<str>) -> FaucetAccountConfig {
        self.alias = alias.as_ref().to_string();
        self
    }

    /// Set the authentication scheme for the faucet.
    pub fn with_auth(mut self, auth: Auth) -> FaucetAccountConfig {
        self.auth = auth;
        self
    }

    /// Set the token symbol for the faucet's fungible asset.
    pub fn with_token_symbol(mut self, token_symbol: impl AsRef<str>) -> FaucetAccountConfig {
        self.token_symbol = token_symbol.as_ref().to_string();
        self
    }

    /// Set the maximum supply of the fungible asset.
    pub fn with_max_supply(mut self, max_supply: u64) -> FaucetAccountConfig {
        self.max_supply = max_supply;
        self
    }

    /// Set the total issuance of the fungible asset.
    pub fn with_total_issuance(mut self, total_issuance: u64) -> FaucetAccountConfig {
        self.total_issuance = total_issuance;
        self
    }

    /// Get the total issuance. Returns `None` if issuance is 0.
    pub fn get_issuance(&self) -> Option<u64> {
        if self.total_issuance == 0 {
            None
        } else {
            Some(self.total_issuance)
        }
    }
}

// ========================= Step 4: Final scenario ============================

/// Final phase: build the mock chain and execute transactions.
pub struct Scenario {
    packages: BTreeMap<String, Arc<Package>>,
    chain_builder: MockChainBuilder,
    accounts: BTreeMap<String, Account>,
}

impl Scenario {
    /// Escape hatch for when a more detailed mock chain setup is required.
    pub fn get_mock_chain(&mut self) -> &mut MockChainBuilder {
        &mut self.chain_builder
    }

    /// Get a reference to a stored account by alias.
    pub fn get_account(&self, alias: &str) -> &Account {
        self.accounts
            .get(alias)
            .unwrap_or_else(|| panic!("No account found under alias '{alias}'"))
    }

    /// Get a stored package by alias.
    pub fn get_package(&self, alias: &str) -> Arc<Package> {
        self.packages
            .get(alias)
            .unwrap_or_else(|| panic!("No package found under alias '{alias}'"))
            .clone()
    }

    /// Build the mock chain.
    pub fn build(self) -> MockChain {
        self.chain_builder.build().expect("failed to build mock chain")
    }
}
