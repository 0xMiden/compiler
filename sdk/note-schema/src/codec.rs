//! String codecs for named WIT leaf types.

use std::{collections::BTreeMap, sync::Arc};

use miden_field::{Felt, Word};
use miden_field_repr::{FeltReader, FeltWriter, FromFeltRepr, ToFeltRepr};
use miden_protocol::{account::AccountId, address::NetworkId, asset::AssetAmount};

use crate::{Error, Result};

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

/// Parses, displays, and validates one fully-qualified WIT leaf type.
pub trait ConsumerTypeCodec: Send + Sync {
    /// Parses a string into its structural felt representation.
    fn parse(&self, value: &str) -> Result<Vec<Felt>>;

    /// Displays a structurally valid felt representation.
    fn display(&self, felts: &[Felt]) -> Result<String>;

    /// Validates the felt representation and any semantic type constraints.
    fn validate(&self, felts: &[Felt]) -> Result<()>;
}

/// A registry of codecs keyed by canonical WIT fully-qualified type name.
///
/// The canonical form is `<namespace>:<package>/<interface>@<version>.<type>`. The version follows
/// the interface, matching WIT interface identifiers. When a package has no version, the
/// `@<version>` part is omitted, for example `miden:base/core-types.account-id`.
/// The standard account ID codec displays the canonical mainnet bech32 form because the network
/// identifier is not part of an account ID's felt representation.
#[derive(Clone)]
pub struct CodecRegistry {
    codecs: BTreeMap<String, Arc<dyn ConsumerTypeCodec>>,
}

impl CodecRegistry {
    /// Creates an empty codec registry.
    pub fn empty() -> Self {
        Self {
            codecs: BTreeMap::new(),
        }
    }

    /// Creates a registry containing all standard note storage codecs.
    pub fn with_standard_codecs() -> Self {
        let mut registry = Self::empty();
        for leaf in StandardLeaf::ALL {
            match leaf {
                StandardLeaf::Felt => registry.register(leaf.fqn(), FeltCodec),
                StandardLeaf::Word => registry.register(leaf.fqn(), WordCodec),
                StandardLeaf::AccountId => registry.register(leaf.fqn(), AccountIdCodec),
                StandardLeaf::AssetAmount => registry.register(leaf.fqn(), AssetAmountCodec),
            }
        }
        registry
    }

    /// Registers or replaces a codec under its canonical WIT FQN.
    pub fn register(&mut self, fqn: impl Into<String>, codec: impl ConsumerTypeCodec + 'static) {
        self.register_shared(fqn, Arc::new(codec));
    }

    /// Registers or replaces a shared codec under its canonical WIT FQN.
    pub fn register_shared(&mut self, fqn: impl Into<String>, codec: Arc<dyn ConsumerTypeCodec>) {
        self.codecs.insert(fqn.into(), codec);
    }

    /// Returns the codec registered for a canonical WIT FQN.
    pub fn codec(&self, fqn: &str) -> Option<&dyn ConsumerTypeCodec> {
        self.codecs.get(fqn).map(Arc::as_ref)
    }

    /// Returns true when the canonical WIT FQN has a codec.
    pub fn contains(&self, fqn: &str) -> bool {
        self.codecs.contains_key(fqn)
    }
}

impl Default for CodecRegistry {
    fn default() -> Self {
        Self::with_standard_codecs()
    }
}

/// Parses and displays one field element.
struct FeltCodec;

impl ConsumerTypeCodec for FeltCodec {
    fn parse(&self, value: &str) -> Result<Vec<Felt>> {
        let felt = parse_felt(value)?;
        Ok(write_repr(&felt))
    }

    fn display(&self, felts: &[Felt]) -> Result<String> {
        read_repr::<Felt>(felts).map(|felt| felt.as_canonical_u64().to_string())
    }

    fn validate(&self, felts: &[Felt]) -> Result<()> {
        read_repr::<Felt>(felts).map(|_| ())
    }
}

/// Parses and displays a four-felt word.
struct WordCodec;

impl ConsumerTypeCodec for WordCodec {
    fn parse(&self, value: &str) -> Result<Vec<Felt>> {
        let value = value.trim();
        let word = if value.starts_with("0x") || value.starts_with("0X") {
            Word::parse(value).map_err(|err| Error::new(format!("invalid word hex: {err}")))?
        } else {
            let values = value.trim_matches(['[', ']']);
            let felts = values.split(',').map(parse_felt).collect::<Result<Vec<_>>>()?;
            let elements: [Felt; 4] = felts.try_into().map_err(|felts: Vec<Felt>| {
                Error::new(format!(
                    "a word needs four comma-separated felts, found {}",
                    felts.len()
                ))
            })?;
            Word::new(elements)
        };
        Ok(write_repr(&word))
    }

    fn display(&self, felts: &[Felt]) -> Result<String> {
        read_repr::<Word>(felts).map(|word| word.to_hex())
    }

    fn validate(&self, felts: &[Felt]) -> Result<()> {
        read_repr::<Word>(felts).map(|_| ())
    }
}

/// Parses and displays a protocol account ID.
struct AccountIdCodec;

impl ConsumerTypeCodec for AccountIdCodec {
    fn parse(&self, value: &str) -> Result<Vec<Felt>> {
        let (account_id, _) = AccountId::parse(value)
            .map_err(|err| Error::new(format!("invalid account-id: {err}")))?;
        let mut felts = Vec::with_capacity(2);
        let mut writer = FeltWriter::new(&mut felts);
        writer.write(account_id.prefix().as_felt());
        writer.write(account_id.suffix());
        Ok(felts)
    }

    fn display(&self, felts: &[Felt]) -> Result<String> {
        read_account_id(felts).map(|account_id| account_id.to_bech32(NetworkId::Mainnet))
    }

    fn validate(&self, felts: &[Felt]) -> Result<()> {
        read_account_id(felts).map(|_| ())
    }
}

/// Parses and displays a validated asset amount.
struct AssetAmountCodec;

impl ConsumerTypeCodec for AssetAmountCodec {
    fn parse(&self, value: &str) -> Result<Vec<Felt>> {
        let amount = value
            .trim()
            .parse::<u64>()
            .map_err(|err| Error::new(format!("invalid asset amount: {err}")))?;
        let amount = AssetAmount::new(amount)
            .map_err(|err| Error::new(format!("invalid asset amount: {err}")))?;
        let felt = Felt::from(amount);
        Ok(write_repr(&felt))
    }

    fn display(&self, felts: &[Felt]) -> Result<String> {
        read_asset_amount(felts).map(|amount| amount.to_string())
    }

    fn validate(&self, felts: &[Felt]) -> Result<()> {
        read_asset_amount(felts).map(|_| ())
    }
}

/// Parses a canonical felt from decimal or hexadecimal text.
pub(crate) fn parse_felt(value: &str) -> Result<Felt> {
    let value = parse_unsigned(value, "felt")?;
    Felt::new(value).map_err(|err| Error::new(format!("invalid felt: {err}")))
}

/// Parses a decimal or hexadecimal unsigned integer.
pub(crate) fn parse_unsigned(value: &str, ty: &str) -> Result<u64> {
    let value = value.trim();
    let parsed = match value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")) {
        Some(digits) => u64::from_str_radix(digits, 16),
        None => value.parse::<u64>(),
    };
    parsed.map_err(|err| Error::new(format!("invalid {ty}: {err}")))
}

/// Encodes a felt-repr value through the shared writer.
pub(crate) fn write_repr(value: &impl ToFeltRepr) -> Vec<Felt> {
    let mut felts = Vec::new();
    value.write_felt_repr(&mut FeltWriter::new(&mut felts));
    felts
}

/// Decodes one felt-repr value and rejects trailing elements.
fn read_repr<T: FromFeltRepr>(felts: &[Felt]) -> Result<T> {
    let mut reader = FeltReader::new(felts);
    let value = T::from_felt_repr(&mut reader)
        .map_err(|err| Error::new(format!("invalid felt representation: {err}")))?;
    reader
        .ensure_eof()
        .map_err(|err| Error::new(format!("invalid felt representation: {err}")))?;
    Ok(value)
}

/// Decodes and validates an account ID in WIT record field order.
fn read_account_id(felts: &[Felt]) -> Result<AccountId> {
    let mut reader = FeltReader::new(felts);
    let prefix = reader
        .read()
        .map_err(|err| Error::new(format!("invalid account-id representation: {err}")))?;
    let suffix = reader
        .read()
        .map_err(|err| Error::new(format!("invalid account-id representation: {err}")))?;
    reader
        .ensure_eof()
        .map_err(|err| Error::new(format!("invalid account-id representation: {err}")))?;
    AccountId::try_from_elements(suffix, prefix)
        .map_err(|err| Error::new(format!("invalid account-id representation: {err}")))
}

/// Decodes and validates an asset amount.
fn read_asset_amount(felts: &[Felt]) -> Result<AssetAmount> {
    let felt = read_repr::<Felt>(felts)?;
    AssetAmount::try_from(felt)
        .map_err(|err| Error::new(format!("invalid asset amount representation: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Returns a valid account ID and its mainnet bech32 form.
    fn account_id() -> (AccountId, String) {
        let account_id =
            AccountId::try_from(0xaa00_0000_0000_bc11_0000_bc00_0000_de00u128).unwrap();
        let bech32 = account_id.to_bech32(NetworkId::Mainnet);
        (account_id, bech32)
    }

    #[test]
    fn standard_registry_contains_canonical_versioned_fqns() {
        let registry = CodecRegistry::default();

        for leaf in StandardLeaf::ALL {
            assert!(registry.contains(leaf.fqn()));
        }
        assert!(!CodecRegistry::empty().contains(FELT_FQN));
    }

    #[test]
    fn standard_leaf_definition_is_pinned() {
        assert_eq!(
            StandardLeaf::ALL.map(StandardLeaf::fqn),
            [FELT_FQN, WORD_FQN, ACCOUNT_ID_FQN, ASSET_AMOUNT_FQN]
        );
        for leaf in StandardLeaf::ALL {
            assert_eq!(StandardLeaf::from_fqn(leaf.fqn()), Some(leaf));
            assert!(CodecRegistry::default().contains(leaf.fqn()));
        }
        assert_eq!(StandardLeaf::from_fqn("miden:base/core-types@1.0.0.digest"), None);
    }

    #[test]
    fn felt_codec_accepts_decimal_and_hex() {
        let codec = FeltCodec;

        assert_eq!(codec.parse("42").unwrap(), codec.parse("0x2a").unwrap());
        assert_eq!(codec.display(&codec.parse("42").unwrap()).unwrap(), "42");
    }

    #[test]
    fn word_codec_accepts_hex_and_four_felts() {
        let codec = WordCodec;
        let from_felts = codec.parse("[1, 2, 3, 4]").unwrap();
        let from_hex = codec.parse(&codec.display(&from_felts).unwrap()).unwrap();

        assert_eq!(from_hex, from_felts);
    }

    #[test]
    fn account_id_codec_uses_prefix_suffix_field_order() {
        let codec = AccountIdCodec;
        let (account_id, bech32) = account_id();
        let felts = codec.parse(&bech32).unwrap();
        let hex_felts = codec.parse(&account_id.to_hex()).unwrap();

        assert_eq!(felts, [account_id.prefix().as_felt(), account_id.suffix()]);
        assert_eq!(hex_felts, felts);
        assert_eq!(codec.display(&felts).unwrap(), bech32);
    }

    #[test]
    fn asset_amount_codec_enforces_protocol_limit() {
        let codec = AssetAmountCodec;

        assert!(codec.parse(&AssetAmount::MAX.as_u64().to_string()).is_ok());
        assert!(codec.parse(&(AssetAmount::MAX.as_u64() + 1).to_string()).is_err());
    }
}
