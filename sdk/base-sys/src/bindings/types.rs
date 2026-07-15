extern crate alloc;

use miden_field_repr::FromFeltRepr;
use miden_stdlib_sys::{Felt, Word, felt};

/// Packs a scalar felt into the leading limb of a protocol word.
pub fn padded_word_from_felt(value: Felt) -> Word {
    Word::new([value, felt!(0), felt!(0), felt!(0)])
}

/// Extracts a scalar felt from a protocol word with zero-padded trailing limbs.
pub fn felt_from_padded_word(value: Word) -> Result<Felt, &'static str> {
    if value[1] != felt!(0) || value[2] != felt!(0) || value[3] != felt!(0) {
        return Err("expected zero padding in the trailing three felts");
    }

    Ok(value[0])
}

/// Unique identifier for a Miden account, composed of two field elements.
#[derive(Copy, Clone, Debug, PartialEq, Eq, FromFeltRepr)]
pub struct AccountId {
    pub prefix: Felt,
    pub suffix: Felt,
}

impl AccountId {
    /// Creates a new AccountId from prefix and suffix Felt values.
    pub fn new(prefix: Felt, suffix: Felt) -> Self {
        Self { prefix, suffix }
    }
}

/// Raw protocol return layout for account identifiers.
/// The protocol MASM procedures are returning [suffix, prefix]
#[derive(Copy, Clone)]
#[repr(C)]
pub(crate) struct RawAccountId {
    pub suffix: Felt,
    pub prefix: Felt,
}

impl RawAccountId {
    /// Converts the protocol return layout into the Rust [`AccountId`] layout.
    pub(crate) fn into_account_id(self) -> AccountId {
        AccountId::new(self.prefix, self.suffix)
    }
}

impl From<AccountId> for Word {
    #[inline]
    fn from(value: AccountId) -> Self {
        Word::from([felt!(0), felt!(0), value.suffix, value.prefix])
    }
}

impl TryFrom<Word> for AccountId {
    type Error = &'static str;

    #[inline]
    fn try_from(value: Word) -> Result<Self, Self::Error> {
        if value[0] != felt!(0) || value[1] != felt!(0) {
            return Err("expected zero padding in the upper two felts");
        }

        Ok(Self {
            prefix: value[3],
            suffix: value[2],
        })
    }
}

/// A fungible or non-fungible asset encoded as separate vault key and value words.
///
/// The `key` identifies the asset in the account vault and the `value` stores the corresponding
/// asset contents. This matches the v0.14 protocol/base ABI.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Asset {
    /// The asset's vault key.
    pub key: Word,
    /// The asset's vault value.
    pub value: Word,
}

impl Asset {
    /// Creates a new [`Asset`] from its key and value words.
    pub fn new(key: impl Into<Word>, value: impl Into<Word>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }

    /// Returns this asset's amount when its key and value have a valid fungible encoding.
    ///
    /// Returns `None` for non-fungible assets and malformed fungible asset encodings.
    pub fn amount(&self) -> Option<AssetAmount> {
        // Bit layout of the vault-key metadata byte (the low byte of the faucet-id suffix limb),
        // mirroring `miden_protocol::asset::AssetVaultKey`: bits 0-1 encode the asset
        // composition, bit 2 the callback flag, and bits 3-7 are reserved and must be zero.
        const METADATA_BYTE_MASK: u64 = 0xff;
        const COMPOSITION_MASK: u8 = 0b11;
        const FUNGIBLE_COMPOSITION: u8 = 0b01;
        const RESERVED_METADATA_MASK: u8 = 0b1111_1000;

        let metadata = (self.key[2].as_canonical_u64() & METADATA_BYTE_MASK) as u8;
        let is_fungible = metadata & COMPOSITION_MASK == FUNGIBLE_COMPOSITION;
        let has_reserved_metadata = metadata & RESERVED_METADATA_MASK != 0;
        let has_asset_id = self.key[0] != Felt::ZERO || self.key[1] != Felt::ZERO;
        let has_value_padding = self.value[1] != Felt::ZERO
            || self.value[2] != Felt::ZERO
            || self.value[3] != Felt::ZERO;

        if !is_fungible || has_reserved_metadata || has_asset_id || has_value_padding {
            return None;
        }

        AssetAmount::try_from(self.value[0]).ok()
    }
}

impl From<Asset> for (Word, Word) {
    fn from(val: Asset) -> Self {
        (val.key, val.value)
    }
}

/// An error produced while constructing an [`AssetAmount`] from an out-of-range value.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AssetAmountError {
    /// The amount exceeds [`AssetAmount::MAX_U64`].
    AmountTooBig(u64),
}

impl core::fmt::Display for AssetAmountError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::AmountTooBig(amount) => {
                write!(f, "asset amount {amount} exceeds the maximum {}", AssetAmount::MAX_U64)
            }
        }
    }
}

impl core::error::Error for AssetAmountError {}

/// A validated fungible asset amount.
///
/// Values created through this type's constructors, conversions, and arithmetic operations wrap
/// a [`Felt`] whose canonical value is at most [`AssetAmount::MAX_U64`]. The API mirrors
/// `miden_protocol::asset::AssetAmount` so that on-chain and off-chain code handle amounts the
/// same way, while the felt representation avoids integer/felt conversions around the
/// transaction kernel procedures.
///
/// Unlike a raw [`Felt`], an amount only offers integer semantics: addition and subtraction
/// panic on overflow and underflow instead of wrapping, and comparison follows the canonical
/// integer value. Finite field arithmetic (wrapping at the field modulus, division via the
/// multiplicative inverse) is intentionally unavailable; convert with [`AssetAmount::as_u64`]
/// when full integer functionality is needed.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct AssetAmount {
    /// The raw field representation.
    ///
    /// Assigning this field directly bypasses amount validation; the checked arithmetic rejects
    /// out-of-range operands. It is public only because component-model bindings construct WIT
    /// records by field — use the checked constructors and accessors instead.
    #[doc(hidden)]
    pub inner: Felt,
}

impl AssetAmount {
    /// The maximum value an asset amount can represent, equal to `2^63 - 2^31`.
    ///
    /// Matches `miden_protocol::asset::AssetAmount::MAX`, which is chosen so that an amount fits
    /// in a field element as both a positive and a negative value.
    // Felt constants on the Miden target are limited to 32-bit values, so the maximum amount
    // cannot be an associated `AssetAmount` constant; see `Self::max`.
    pub const MAX_U64: u64 = (1u64 << 63) - (1u64 << 31);
    /// The zero amount.
    pub const ZERO: Self = Self { inner: Felt::ZERO };

    /// Returns the maximum representable asset amount, equal to [`Self::MAX_U64`].
    #[inline]
    pub fn max() -> Self {
        Self {
            inner: Self::max_inner(),
        }
    }

    /// Returns a new asset amount if `amount` does not exceed [`Self::MAX_U64`].
    ///
    /// # Errors
    ///
    /// Returns an error if `amount` is greater than [`Self::MAX_U64`].
    pub fn new(amount: u64) -> Result<Self, AssetAmountError> {
        if amount > Self::MAX_U64 {
            return Err(AssetAmountError::AmountTooBig(amount));
        }
        // The bound check above also guarantees the value is below the field modulus.
        Ok(Self {
            inner: Felt::new_unchecked(amount),
        })
    }

    /// Returns the amount as a `u64` value.
    #[inline]
    pub fn as_u64(&self) -> u64 {
        self.inner.as_canonical_u64()
    }

    /// Returns the amount as a raw [`Felt`] for advanced use.
    #[inline]
    pub fn as_felt(&self) -> Felt {
        self.inner
    }

    /// Returns the maximum amount as a raw felt.
    #[inline(always)]
    fn max_inner() -> Felt {
        // MAX_U64 is below the field modulus, so no reduction occurs.
        Felt::new_unchecked(Self::MAX_U64)
    }

    /// Builds the out-of-range error for the provided felt.
    #[inline]
    fn amount_too_big(value: Felt) -> AssetAmountError {
        // The felt-to-integer conversion only runs on error paths.
        AssetAmountError::AmountTooBig(value.as_canonical_u64())
    }
}

// Two maximal amounts must sum to exactly the field modulus minus one; the checked arithmetic
// below relies on this to rule out field wrap-around for validated operands.
const _: () = assert!(AssetAmount::MAX_U64 * 2 == Felt::ORDER - 1);

impl core::ops::Add for AssetAmount {
    type Output = Self;

    /// Adds two asset amounts, staying in the field domain.
    ///
    /// # Panics
    ///
    /// Panics if either operand or the sum exceeds [`AssetAmount::MAX_U64`].
    fn add(self, other: Self) -> Self {
        let max = Self::max_inner();
        // Reject an out-of-range operand (possible via direct `inner` assignment) before
        // relying on its value.
        assert!(self.inner <= max, "asset amount exceeds the maximum allowed amount");
        // `self` is in range, so this felt subtraction is exact and the headroom is at most
        // MAX_U64. One comparison then proves both that `other` is in range
        // (other <= headroom <= MAX_U64) and that the sum stays in range
        // (self + other <= MAX_U64), so the felt addition below cannot wrap around.
        let headroom = max - self.inner;
        assert!(other.inner <= headroom, "asset amount addition overflow");
        Self {
            inner: self.inner + other.inner,
        }
    }
}

impl core::ops::Sub for AssetAmount {
    type Output = Self;

    /// Subtracts `other` from `self`, staying in the field domain.
    ///
    /// # Panics
    ///
    /// Panics if either operand exceeds [`AssetAmount::MAX_U64`] or if `other` is greater than
    /// `self`.
    fn sub(self, other: Self) -> Self {
        let max = Self::max_inner();
        // An out-of-range minuend (possible via direct `inner` assignment) could otherwise
        // produce an out-of-range result.
        assert!(self.inner <= max, "asset amount exceeds the maximum allowed amount");
        // When this check passes, other <= self <= MAX_U64, so `other` is in range, the felt
        // subtraction cannot wrap around, and the result needs no validation.
        assert!(other.inner <= self.inner, "asset amount subtraction underflow");
        Self {
            inner: self.inner - other.inner,
        }
    }
}

impl Default for AssetAmount {
    fn default() -> Self {
        Self::ZERO
    }
}

impl From<u8> for AssetAmount {
    fn from(value: u8) -> Self {
        Self {
            inner: Felt::from(value),
        }
    }
}

impl From<u16> for AssetAmount {
    fn from(value: u16) -> Self {
        Self {
            inner: Felt::from(value),
        }
    }
}

impl From<u32> for AssetAmount {
    fn from(value: u32) -> Self {
        // Any u32 value is below the maximum amount.
        Self {
            inner: Felt::from_u32(value),
        }
    }
}

impl TryFrom<u64> for AssetAmount {
    type Error = AssetAmountError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<Felt> for AssetAmount {
    type Error = AssetAmountError;

    fn try_from(value: Felt) -> Result<Self, Self::Error> {
        if value > Self::max_inner() {
            return Err(Self::amount_too_big(value));
        }
        Ok(Self { inner: value })
    }
}

impl From<AssetAmount> for u64 {
    fn from(amount: AssetAmount) -> Self {
        amount.as_u64()
    }
}

impl From<AssetAmount> for Felt {
    fn from(amount: AssetAmount) -> Self {
        amount.inner
    }
}

impl core::fmt::Display for AssetAmount {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.as_u64())
    }
}

/// A note recipient digest.
#[derive(Clone, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Recipient {
    pub inner: Word,
}

/// The note metadata returned by `*_note::get_metadata` procedures.
///
/// In the Miden protocol, metadata retrieval returns a single metadata header word. Note
/// attachments are retrieved separately via the `*_note::get_attachments_commitment`,
/// `find_attachment`, and `write_attachment_*` procedures.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct NoteMetadata {
    /// The metadata header of the note.
    pub header: Word,
}

impl NoteMetadata {
    /// Creates a new [`NoteMetadata`] from the metadata header word.
    pub fn new(header: Word) -> Self {
        Self { header }
    }
}

/// Result of searching note metadata for an attachment scheme.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct AttachmentLocation {
    /// Non-zero when the attachment scheme was found.
    pub is_found: Felt,
    /// The matching attachment index, valid only when `is_found` is non-zero.
    pub index: Felt,
}

impl AttachmentLocation {
    /// Returns whether the attachment scheme was found.
    #[inline]
    pub fn found(&self) -> bool {
        self.is_found != Felt::new(0).unwrap()
    }
}

impl From<[Felt; 4]> for Recipient {
    fn from(value: [Felt; 4]) -> Self {
        Recipient {
            inner: Word::from(value),
        }
    }
}

impl From<Word> for Recipient {
    fn from(value: Word) -> Self {
        Recipient { inner: value }
    }
}

impl From<Recipient> for Word {
    #[inline]
    fn from(value: Recipient) -> Self {
        value.inner
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Tag {
    pub inner: Felt,
}

impl From<Felt> for Tag {
    fn from(value: Felt) -> Self {
        Tag { inner: value }
    }
}

impl From<Tag> for Word {
    #[inline]
    fn from(value: Tag) -> Self {
        padded_word_from_felt(value.inner)
    }
}

impl TryFrom<Word> for Tag {
    type Error = &'static str;

    #[inline]
    fn try_from(value: Word) -> Result<Self, Self::Error> {
        Ok(Tag {
            inner: felt_from_padded_word(value)?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct NoteIdx {
    pub inner: Felt,
}

impl From<NoteIdx> for Word {
    #[inline]
    fn from(value: NoteIdx) -> Self {
        padded_word_from_felt(value.inner)
    }
}

impl TryFrom<Word> for NoteIdx {
    type Error = &'static str;

    #[inline]
    fn try_from(value: Word) -> Result<Self, Self::Error> {
        Ok(NoteIdx {
            inner: felt_from_padded_word(value)?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct NoteType {
    pub inner: Felt,
}

impl From<Felt> for NoteType {
    fn from(value: Felt) -> Self {
        NoteType { inner: value }
    }
}

impl From<NoteType> for Word {
    #[inline]
    fn from(value: NoteType) -> Self {
        padded_word_from_felt(value.inner)
    }
}

impl TryFrom<Word> for NoteType {
    type Error = &'static str;

    #[inline]
    fn try_from(value: Word) -> Result<Self, Self::Error> {
        Ok(NoteType {
            inner: felt_from_padded_word(value)?,
        })
    }
}

/// The partial hash of a storage slot name.
///
/// A slot id consists of two field elements: a `prefix` and a `suffix`.
///
/// Slot ids uniquely identify slots in account storage and are used by the host functions exposed
/// via `miden::protocol::*`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct StorageSlotId {
    suffix: Felt,
    prefix: Felt,
}

impl StorageSlotId {
    /// Creates a new [`StorageSlotId`] from the provided felts.
    ///
    /// Note: this constructor takes `(suffix, prefix)` to match the values returned by
    /// `miden_protocol::account::StorageSlotId::{suffix,prefix}`.
    pub fn new(suffix: Felt, prefix: Felt) -> Self {
        Self { suffix, prefix }
    }

    /// Creates a new [`StorageSlotId`] from the provided felts in host-call order.
    ///
    /// Host functions take the `prefix` first and then the `suffix`.
    pub fn from_prefix_suffix(prefix: Felt, suffix: Felt) -> Self {
        Self { suffix, prefix }
    }

    /// Returns the `(prefix, suffix)` pair in host-call order.
    pub fn to_prefix_suffix(&self) -> (Felt, Felt) {
        (self.prefix, self.suffix)
    }

    /// Returns the `(suffix, prefix)` pair in storage-slot order.
    pub fn to_suffix_prefix(&self) -> (Felt, Felt) {
        (self.suffix, self.prefix)
    }

    /// Returns the suffix of the [`StorageSlotId`].
    pub fn suffix(&self) -> Felt {
        self.suffix
    }

    /// Returns the prefix of the [`StorageSlotId`].
    pub fn prefix(&self) -> Felt {
        self.prefix
    }
}

#[cfg(test)]
mod tests {
    use miden_stdlib_sys::{Felt, Word, felt};

    use super::{
        Asset, AssetAmount, AssetAmountError, felt_from_padded_word, padded_word_from_felt,
    };

    /// Ensures `padded_word_from_felt` zero-pads the trailing three limbs.
    #[test]
    fn padded_word_from_felt_zero_pads_trailing_limbs() {
        assert_eq!(
            padded_word_from_felt(felt!(7)),
            Word::new([felt!(7), felt!(0), felt!(0), felt!(0)])
        );
    }

    /// Ensures `felt_from_padded_word` rejects words with non-zero trailing padding.
    #[test]
    fn felt_from_padded_word_rejects_non_zero_padding() {
        let err =
            felt_from_padded_word(Word::new([felt!(7), felt!(1), felt!(0), felt!(0)])).unwrap_err();

        assert_eq!(err, "expected zero padding in the trailing three felts");
    }

    /// Ensures the felt-padding helpers form a lossless roundtrip for scalar values.
    #[test]
    fn felt_padding_helpers_roundtrip() {
        let value = felt!(42);

        assert_eq!(felt_from_padded_word(padded_word_from_felt(value)), Ok(value));
    }

    /// Ensures amounts within the bound construct successfully and convert back losslessly.
    #[test]
    fn asset_amount_valid_amounts() {
        assert_eq!(AssetAmount::new(0).unwrap().as_u64(), 0);
        assert_eq!(AssetAmount::new(1000).unwrap().as_u64(), 1000);
        assert_eq!(AssetAmount::new(AssetAmount::MAX_U64).unwrap(), AssetAmount::max());
    }

    /// Ensures amounts above the bound are rejected with the offending value.
    #[test]
    fn asset_amount_exceeds_max() {
        assert_eq!(
            AssetAmount::new(AssetAmount::MAX_U64 + 1),
            Err(AssetAmountError::AmountTooBig(AssetAmount::MAX_U64 + 1))
        );
        assert_eq!(AssetAmount::new(u64::MAX), Err(AssetAmountError::AmountTooBig(u64::MAX)));
    }

    /// Ensures the maximum amount constant matches its documented value.
    #[test]
    fn asset_amount_max_value() {
        assert_eq!(AssetAmount::MAX_U64, 2u64.pow(63) - 2u64.pow(31));
        assert_eq!(AssetAmount::max().as_u64(), AssetAmount::MAX_U64);
    }

    /// Ensures the infallible conversions from small integer types.
    #[test]
    fn asset_amount_from_small_types() {
        assert_eq!(AssetAmount::from(42u8).as_u64(), 42);
        assert_eq!(AssetAmount::from(1000u16).as_u64(), 1000);
        assert_eq!(AssetAmount::from(u32::MAX).as_u64(), u32::MAX as u64);
    }

    /// Ensures the fallible conversions from `u64` and `Felt` enforce the bound.
    #[test]
    fn asset_amount_try_from() {
        assert!(AssetAmount::try_from(AssetAmount::MAX_U64).is_ok());
        assert!(AssetAmount::try_from(AssetAmount::MAX_U64 + 1).is_err());
        assert!(AssetAmount::try_from(Felt::new(AssetAmount::MAX_U64).unwrap()).is_ok());
        assert!(AssetAmount::try_from(Felt::new(AssetAmount::MAX_U64 + 1).unwrap()).is_err());
        // The largest canonical felt is far above the bound and must be rejected.
        assert_eq!(
            AssetAmount::try_from(Felt::new(Felt::ORDER - 1).unwrap()),
            Err(AssetAmountError::AmountTooBig(Felt::ORDER - 1))
        );
    }

    /// Ensures addition computes exact integer sums for in-range amounts.
    #[test]
    fn asset_amount_add() {
        let a = AssetAmount::new(100).unwrap();
        let b = AssetAmount::new(200).unwrap();

        assert_eq!((a + b).as_u64(), 300);
        assert_eq!(AssetAmount::ZERO + AssetAmount::ZERO, AssetAmount::ZERO);
        assert_eq!(AssetAmount::max() + AssetAmount::ZERO, AssetAmount::max());
    }

    /// Ensures addition panics when the sum exceeds the maximum amount.
    #[test]
    #[should_panic(expected = "asset amount addition overflow")]
    fn asset_amount_add_panics_on_overflow() {
        let _ = AssetAmount::max() + AssetAmount::new(1).unwrap();
    }

    /// Ensures addition rejects an out-of-range left operand built via direct field assignment
    /// instead of laundering it into a valid-looking sum; this operand would wrap the field to
    /// zero if it were not rejected.
    #[test]
    #[should_panic(expected = "asset amount exceeds the maximum allowed amount")]
    fn asset_amount_add_panics_on_forged_lhs() {
        let wrapping = AssetAmount {
            inner: Felt::new(Felt::ORDER - 1).unwrap(),
        };

        let _ = wrapping + AssetAmount::new(1).unwrap();
    }

    /// Ensures addition rejects an out-of-range right operand built via direct field
    /// assignment (reported as an overflowing sum).
    #[test]
    #[should_panic(expected = "asset amount addition overflow")]
    fn asset_amount_add_panics_on_forged_rhs() {
        let forged = AssetAmount {
            inner: Felt::new(AssetAmount::MAX_U64 + 1).unwrap(),
        };

        let _ = AssetAmount::new(1).unwrap() + forged;
    }

    /// Ensures subtraction computes exact integer differences for in-range amounts.
    #[test]
    fn asset_amount_sub() {
        let a = AssetAmount::new(300).unwrap();
        let b = AssetAmount::new(100).unwrap();

        assert_eq!((a - b).as_u64(), 200);
        assert_eq!(AssetAmount::ZERO - AssetAmount::ZERO, AssetAmount::ZERO);
        assert_eq!(AssetAmount::max() - AssetAmount::max(), AssetAmount::ZERO);
    }

    /// Ensures subtraction panics when the subtrahend exceeds the minuend.
    #[test]
    #[should_panic(expected = "asset amount subtraction underflow")]
    fn asset_amount_sub_panics_on_underflow() {
        let _ = AssetAmount::ZERO - AssetAmount::new(1).unwrap();
    }

    /// Ensures subtraction rejects an out-of-range minuend built via direct field assignment,
    /// which could otherwise produce an out-of-range result.
    #[test]
    #[should_panic(expected = "asset amount exceeds the maximum allowed amount")]
    fn asset_amount_sub_panics_on_forged_minuend() {
        let forged = AssetAmount {
            inner: Felt::new(AssetAmount::MAX_U64 + 1).unwrap(),
        };

        let _ = forged - AssetAmount::new(1).unwrap();
    }

    /// Ensures the SDK amount arithmetic agrees with the off-chain protocol implementation
    /// whenever the protocol operation succeeds (the SDK panics where the protocol errors).
    #[test]
    fn asset_amount_differential_vs_protocol() {
        use miden_protocol::asset::AssetAmount as ProtocolAmount;

        let values = [
            0u64,
            1,
            2,
            31,
            u32::MAX as u64,
            1 << 40,
            AssetAmount::MAX_U64 / 2,
            AssetAmount::MAX_U64 - 1,
            AssetAmount::MAX_U64,
        ];
        for &a in &values {
            for &b in &values {
                let ours = (AssetAmount::new(a).unwrap(), AssetAmount::new(b).unwrap());
                let theirs = (ProtocolAmount::new(a).unwrap(), ProtocolAmount::new(b).unwrap());

                if let Ok(sum) = theirs.0 + theirs.1 {
                    assert_eq!(
                        (ours.0 + ours.1).as_u64(),
                        sum.as_u64(),
                        "sum mismatch for {a} + {b}"
                    );
                }

                if let Ok(difference) = theirs.0 - theirs.1 {
                    assert_eq!(
                        (ours.0 - ours.1).as_u64(),
                        difference.as_u64(),
                        "difference mismatch for {a} - {b}"
                    );
                }
            }
        }
    }

    /// Ensures comparison follows canonical integer ordering.
    #[test]
    fn asset_amount_ordering() {
        assert!(AssetAmount::new(1).unwrap() < AssetAmount::new(2).unwrap());
        assert!(AssetAmount::max() > AssetAmount::ZERO);
        assert_eq!(AssetAmount::default(), AssetAmount::ZERO);
    }

    /// Ensures the amount displays as a decimal integer.
    #[test]
    fn asset_amount_display() {
        extern crate alloc;
        use alloc::string::ToString;

        assert_eq!(AssetAmount::new(12345).unwrap().to_string(), "12345");
    }

    /// Ensures the felt accessor and conversions roundtrip the underlying value.
    #[test]
    fn asset_amount_felt_roundtrip() {
        let amount = AssetAmount::new(500).unwrap();

        assert_eq!(amount.as_felt(), felt!(500));
        assert_eq!(Felt::from(amount), felt!(500));
        assert_eq!(u64::from(amount), 500);
    }

    /// Creates a raw fungible asset encoding (composition bits `0b01` in the key metadata byte)
    /// for amount tests.
    fn fungible_asset(amount: Felt) -> Asset {
        Asset::new(
            Word::new([felt!(0), felt!(0), felt!(1), felt!(0)]),
            Word::new([amount, felt!(0), felt!(0), felt!(0)]),
        )
    }

    /// Ensures fungible asset amounts are decoded from valid key/value encodings.
    #[test]
    fn asset_amount_decodes_valid_fungible_assets() {
        let asset = fungible_asset(felt!(42));
        // Metadata byte 0b101: fungible composition with the callback flag set.
        let callback_asset =
            Asset::new(Word::new([felt!(0), felt!(0), felt!(5), felt!(0)]), asset.value);

        assert_eq!(asset.amount(), Some(AssetAmount::new(42).unwrap()));
        assert_eq!(callback_asset.amount(), Some(AssetAmount::new(42).unwrap()));
    }

    /// Ensures non-fungible and malformed asset encodings have no amount.
    #[test]
    fn asset_amount_rejects_invalid_asset_encodings() {
        let amount = felt!(42);
        let non_fungible = Asset::new(
            Word::new([felt!(1), felt!(0), felt!(0), felt!(0)]),
            Word::new([amount, felt!(0), felt!(0), felt!(0)]),
        );
        let non_zero_asset_id = Asset::new(
            Word::new([felt!(1), felt!(0), felt!(1), felt!(0)]),
            Word::new([amount, felt!(0), felt!(0), felt!(0)]),
        );
        // Metadata byte 0b1001: fungible composition but a reserved bit is set.
        let reserved_metadata = Asset::new(
            Word::new([felt!(0), felt!(0), felt!(9), felt!(0)]),
            Word::new([amount, felt!(0), felt!(0), felt!(0)]),
        );
        let non_zero_value_padding = Asset::new(
            Word::new([felt!(0), felt!(0), felt!(1), felt!(0)]),
            Word::new([amount, felt!(1), felt!(0), felt!(0)]),
        );
        let excessive_amount = fungible_asset(Felt::new(AssetAmount::MAX_U64 + 1).unwrap());

        assert_eq!(non_fungible.amount(), None);
        assert_eq!(non_zero_asset_id.amount(), None);
        assert_eq!(reserved_metadata.amount(), None);
        assert_eq!(non_zero_value_padding.amount(), None);
        assert_eq!(excessive_amount.amount(), None);
    }
}
