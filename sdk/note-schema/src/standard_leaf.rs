//! The standard leaf types of a note storage schema, and their canonical WIT names.

/// The canonical WIT FQN for `felt`.
pub const FELT_FQN: &str = "miden:base/core-types@1.0.0.felt";
/// The canonical WIT FQN for `word`.
pub const WORD_FQN: &str = "miden:base/core-types@1.0.0.word";
/// The canonical WIT FQN for `account-id`.
pub const ACCOUNT_ID_FQN: &str = "miden:base/core-types@1.0.0.account-id";
/// The canonical WIT FQN for `asset-amount`.
pub const ASSET_AMOUNT_FQN: &str = "miden:base/core-types@1.0.0.asset-amount";

/// A protocol type whose schema leaf maps directly to an existing host type and standard codec.
///
/// This is the canonical standard-leaf definition used by schema traversal, Rust code generation,
/// and author-codec registration. Named types outside this set remain schema-owned, including
/// other records in the `miden:base/core-types` interface.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum StandardLeaf {
    /// The one-element Miden base-field type.
    Felt,
    /// A group of four Miden base-field elements.
    Word,
    /// A two-element protocol account identifier.
    AccountId,
    /// A validated fungible-asset amount.
    AssetAmount,
}

impl StandardLeaf {
    /// Every standard leaf in canonical registry order.
    pub const ALL: [Self; 4] = [Self::Felt, Self::Word, Self::AccountId, Self::AssetAmount];

    /// Returns the canonical WIT FQN for this standard leaf.
    pub const fn fqn(self) -> &'static str {
        match self {
            Self::Felt => FELT_FQN,
            Self::Word => WORD_FQN,
            Self::AccountId => ACCOUNT_ID_FQN,
            Self::AssetAmount => ASSET_AMOUNT_FQN,
        }
    }

    /// Classifies a canonical WIT FQN as a standard leaf.
    pub fn from_fqn(fqn: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|leaf| leaf.fqn() == fqn)
    }
}
