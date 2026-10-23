//! Allocation-free note attachments with validated schemes, indexes and content lengths.
//!
//! Attachment content is public, including attachments of private notes. A scheme only identifies
//! the content's intended meaning; callers must validate any scheme-specific content themselves.

use miden_field_repr::{
    FeltReader, FeltReprError, FeltReprResult, FeltWriter, FromFeltRepr, ToFeltRepr,
};
use miden_stdlib_sys::{Felt, Word};

use super::{
    MAX_ATTACHMENT_WORDS, MAX_ATTACHMENTS_PER_NOTE, NoteIdx, output_note,
    types::{felt_to_u8, felt_to_u16},
};

/// An error produced while constructing a note attachment, its scheme or an attachment index.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NoteAttachmentError {
    /// The scheme is `0` or `65535`, outside the valid range `1..=65534`.
    InvalidScheme(u16),
    /// The scheme felt exceeds `u16::MAX`.
    SchemeExceedsU16,
    /// The attachment does not contain any words.
    EmptyContent,
    /// The attachment contains more than 256 words.
    TooManyWords(usize),
    /// The attachment index is in `4..=255`, above `3`, the last index a note can have.
    InvalidIndex(u8),
    /// The attachment index value exceeds `u8::MAX`.
    IndexExceedsU8,
}

impl core::fmt::Display for NoteAttachmentError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidScheme(scheme) => {
                write!(
                    f,
                    "note attachment scheme {scheme} must be between 1 and {}",
                    NoteAttachmentScheme::MAX.as_u16()
                )
            }
            Self::SchemeExceedsU16 => f.write_str("note attachment scheme exceeds u16"),
            Self::EmptyContent => f.write_str("note attachment cannot be empty"),
            Self::TooManyWords(num_words) => {
                write!(
                    f,
                    "note attachment contains {num_words} words, exceeding the maximum \
                     {MAX_ATTACHMENT_WORDS}"
                )
            }
            Self::InvalidIndex(index) => {
                write!(
                    f,
                    "note attachment index {index} must be at most {}",
                    AttachmentIdx::MAX.as_u8()
                )
            }
            Self::IndexExceedsU8 => f.write_str("note attachment index exceeds u8"),
        }
    }
}

impl core::error::Error for NoteAttachmentError {}

/// A validated note attachment scheme in the range `1..=65534`.
///
/// Schemes identify attachment content without validating it. [`Self::none`] denotes unspecified
/// content and is the default. The attachment writers and lookups, such as
/// [`output_note::add_word_attachment`] and [`output_note::find_attachment`], take this type
/// directly. [`Self::as_u16`] returns the protocol value for raw bindings and other encodings.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct NoteAttachmentScheme(u16);

impl NoteAttachmentScheme {
    /// The maximum valid scheme, `65534`.
    pub const MAX: Self = Self(65534);

    /// Creates a scheme from its protocol value.
    ///
    /// # Errors
    ///
    /// Returns [`NoteAttachmentError::InvalidScheme`] for `0` or `65535`.
    pub const fn new(scheme: u16) -> Result<Self, NoteAttachmentError> {
        if scheme == 0 || scheme > Self::MAX.as_u16() {
            return Err(NoteAttachmentError::InvalidScheme(scheme));
        }
        Ok(Self(scheme))
    }

    /// Creates a scheme from its protocol value, for defining scheme constants:
    ///
    /// ```rust,ignore
    /// const SCHEME: NoteAttachmentScheme = NoteAttachmentScheme::new_const(42);
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if `scheme` is `0` or `65535`; in a constant, this is a compile-time error.
    pub const fn new_const(scheme: u16) -> Self {
        match Self::new(scheme) {
            Ok(scheme) => scheme,
            // A const `panic!` cannot format the bound, so the literal is checked against `MAX`
            // below.
            Err(_) => panic!("note attachment scheme must be between 1 and 65534"),
        }
    }

    /// Returns scheme `1`, which denotes an unspecified attachment scheme.
    pub const fn none() -> Self {
        Self(1)
    }

    /// Returns `true` if this is scheme `1`, the unspecified attachment scheme.
    pub const fn is_none(&self) -> bool {
        self.0 == Self::none().0
    }

    /// Returns the scheme's protocol value.
    pub const fn as_u16(&self) -> u16 {
        self.0
    }

    /// Decodes a scheme from a note metadata header slot.
    ///
    /// The kernel encodes an absent attachment slot as `0`, which maps to `None`, mirroring the
    /// protocol's `NoteAttachmentHeader::scheme()`.
    ///
    /// # Panics
    ///
    /// Panics if `value` is `65535`. The metadata of notes the kernel created or the protocol
    /// validated never holds it; a hand-built metadata word can.
    pub(super) fn from_header_u16(value: u16) -> Option<Self> {
        if value == 0 {
            return None;
        }
        Some(Self::new(value).expect("invalid attachment scheme in note metadata"))
    }
}

// Keeps the `new_const` panic literal and the protocol's limit in sync with `MAX`.
const _: () = assert!(
    NoteAttachmentScheme::MAX.as_u16() == 65534
        && NoteAttachmentScheme::MAX.as_u16() == crate::raw::protocol::note::MAX_ATTACHMENT_SCHEME
);

impl Default for NoteAttachmentScheme {
    /// Returns [`NoteAttachmentScheme::none`].
    fn default() -> Self {
        Self::none()
    }
}

impl TryFrom<u16> for NoteAttachmentScheme {
    type Error = NoteAttachmentError;

    /// Same as [`NoteAttachmentScheme::new`].
    fn try_from(scheme: u16) -> Result<Self, Self::Error> {
        Self::new(scheme)
    }
}

impl TryFrom<Felt> for NoteAttachmentScheme {
    type Error = NoteAttachmentError;

    /// Converts a felt holding a scheme's protocol value.
    ///
    /// # Errors
    ///
    /// Returns [`NoteAttachmentError::SchemeExceedsU16`] for a felt above `u16::MAX` and
    /// [`NoteAttachmentError::InvalidScheme`] for `0` or `65535`.
    fn try_from(value: Felt) -> Result<Self, Self::Error> {
        let scheme = felt_to_u16(value).ok_or(NoteAttachmentError::SchemeExceedsU16)?;
        Self::new(scheme)
    }
}

impl From<NoteAttachmentScheme> for u16 {
    /// Returns the scheme's protocol value, as [`NoteAttachmentScheme::as_u16`] does.
    fn from(scheme: NoteAttachmentScheme) -> Self {
        scheme.as_u16()
    }
}

/// Decodes a scheme from one felt holding its protocol value, rejecting invalid schemes, for
/// script arguments and derived felt layouts.
impl FromFeltRepr for NoteAttachmentScheme {
    const FIXED_LEN: Option<usize> = Some(1);

    #[inline(always)]
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> FeltReprResult<Self> {
        let pos = reader.pos();
        let len = reader.len();
        let felt = reader.read()?;
        // `0` is below the range; the error type has no distinct variant for it.
        Self::try_from(felt).map_err(|_| FeltReprError::ValueOutOfRange {
            pos,
            len,
            ty: "NoteAttachmentScheme",
            value: felt.as_canonical_u64(),
            max: u64::from(Self::MAX.as_u16()),
        })
    }
}

/// Encodes a scheme as one felt holding its protocol value, for script arguments and derived felt
/// layouts.
impl ToFeltRepr for NoteAttachmentScheme {
    #[inline(always)]
    fn write_felt_repr(&self, writer: &mut FeltWriter<'_>) {
        writer.write(Felt::from(self.as_u16()));
    }
}

/// Decodes a scheme from exactly one felt, as [`FromFeltRepr`] does.
impl TryFrom<&[Felt]> for NoteAttachmentScheme {
    type Error = FeltReprError;

    #[inline(always)]
    fn try_from(felts: &[Felt]) -> Result<Self, Self::Error> {
        let mut reader = FeltReader::new(felts);
        let scheme = Self::from_felt_repr(&mut reader)?;
        reader.ensure_eof()?;
        Ok(scheme)
    }
}

/// A validated attachment index within a note, `0..=3`.
///
/// The attachment lookups, such as [`output_note::find_attachment`], return this type, and the
/// attachment readers, such as [`output_note::write_attachment_to_memory`], take it.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AttachmentIdx(u8);

impl AttachmentIdx {
    /// The last valid index, `3`.
    pub const MAX: Self = Self((MAX_ATTACHMENTS_PER_NOTE - 1) as u8);

    /// Creates an attachment index.
    ///
    /// # Errors
    ///
    /// Returns [`NoteAttachmentError::InvalidIndex`] for an index above [`Self::MAX`].
    pub const fn new(index: u8) -> Result<Self, NoteAttachmentError> {
        if index > Self::MAX.0 {
            return Err(NoteAttachmentError::InvalidIndex(index));
        }
        Ok(Self(index))
    }

    /// Creates an attachment index, for defining index constants:
    ///
    /// ```rust,ignore
    /// const IDX: AttachmentIdx = AttachmentIdx::new_const(2);
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if `index` is above `3`; in a constant, this is a compile-time error.
    pub const fn new_const(index: u8) -> Self {
        match Self::new(index) {
            Ok(index) => index,
            // A const `panic!` cannot format the bound, so the literal is checked against `MAX`
            // below.
            Err(_) => panic!("note attachment index must be at most 3"),
        }
    }

    /// Wraps an index a lookup procedure reported as found.
    ///
    /// The procedures scan the four header slots, so a found index is below 4; only the
    /// `find_attachment` bindings and `note::find_attachment_idx` call this.
    pub(super) const fn from_kernel(index: u8) -> Self {
        Self(index)
    }

    /// Returns the index as a `u8`.
    pub const fn as_u8(&self) -> u8 {
        self.0
    }
}

// Keeps the `new_const` panic literal in sync with `MAX`.
const _: () = assert!(AttachmentIdx::MAX.as_u8() == 3);

impl TryFrom<u32> for AttachmentIdx {
    type Error = NoteAttachmentError;

    /// Converts an index.
    ///
    /// # Errors
    ///
    /// Returns [`NoteAttachmentError::IndexExceedsU8`] for a value above `u8::MAX` and
    /// [`NoteAttachmentError::InvalidIndex`] for a value in `4..=255`.
    fn try_from(index: u32) -> Result<Self, Self::Error> {
        let index = u8::try_from(index).map_err(|_| NoteAttachmentError::IndexExceedsU8)?;
        Self::new(index)
    }
}

impl TryFrom<Felt> for AttachmentIdx {
    type Error = NoteAttachmentError;

    /// Converts a felt holding an index.
    ///
    /// # Errors
    ///
    /// Returns [`NoteAttachmentError::IndexExceedsU8`] for a felt above `u8::MAX` and
    /// [`NoteAttachmentError::InvalidIndex`] for a felt in `4..=255`.
    #[inline]
    fn try_from(value: Felt) -> Result<Self, Self::Error> {
        let index = felt_to_u8(value).ok_or(NoteAttachmentError::IndexExceedsU8)?;
        Self::new(index)
    }
}

impl From<AttachmentIdx> for u8 {
    /// Returns the index, as [`AttachmentIdx::as_u8`] does.
    fn from(index: AttachmentIdx) -> Self {
        index.as_u8()
    }
}

impl From<AttachmentIdx> for u32 {
    /// Returns the index.
    fn from(index: AttachmentIdx) -> Self {
        index.as_u8().into()
    }
}

impl From<AttachmentIdx> for usize {
    /// Returns the index.
    fn from(index: AttachmentIdx) -> Self {
        index.as_u8().into()
    }
}

/// Decodes an index from one felt holding its value, rejecting indexes above `3`, for script
/// arguments and derived felt layouts.
impl FromFeltRepr for AttachmentIdx {
    const FIXED_LEN: Option<usize> = Some(1);

    #[inline(always)]
    fn from_felt_repr(reader: &mut FeltReader<'_>) -> FeltReprResult<Self> {
        let pos = reader.pos();
        let len = reader.len();
        let felt = reader.read()?;
        Self::try_from(felt).map_err(|_| FeltReprError::ValueOutOfRange {
            pos,
            len,
            ty: "AttachmentIdx",
            value: felt.as_canonical_u64(),
            max: u64::from(Self::MAX.as_u8()),
        })
    }
}

/// Encodes an index as one felt holding its value, for script arguments and derived felt layouts.
impl ToFeltRepr for AttachmentIdx {
    #[inline(always)]
    fn write_felt_repr(&self, writer: &mut FeltWriter<'_>) {
        writer.write(Felt::from(self.as_u8()));
    }
}

/// Decodes an index from exactly one felt, as [`FromFeltRepr`] does.
impl TryFrom<&[Felt]> for AttachmentIdx {
    type Error = FeltReprError;

    #[inline(always)]
    fn try_from(felts: &[Felt]) -> Result<Self, Self::Error> {
        let mut reader = FeltReader::new(felts);
        let index = Self::from_felt_repr(&mut reader)?;
        reader.ensure_eof()?;
        Ok(index)
    }
}

/// A note attachment containing one owned word or between 1 and 256 borrowed words.
///
/// Construction neither allocates nor hashes the content. Content is public even when the note is
/// private. The scheme and content length are validated; scheme-specific content requirements and
/// per-note limits remain the caller's responsibility and are checked by the kernel where applicable.
///
/// # Examples
///
/// Create an output note and attach application data from an account procedure, with the native
/// account active:
///
/// ```rust,ignore
/// use miden::{NoteAttachment, NoteAttachmentScheme, NoteIdx, NoteType, Recipient, Tag, Word,
///             output_note};
///
/// fn create_note(tag: Tag, recipient: Recipient, data: Word) -> NoteIdx {
///     let attachment = NoteAttachment::with_word(NoteAttachmentScheme::none(), data);
///     let note_idx = output_note::create(tag, NoteType::from(1u8), recipient);
///     attachment.add_to(note_idx);
///     note_idx
/// }
/// ```
#[derive(Clone, Debug)]
#[must_use = "the attachment is only added to a note by `add_to`"]
pub struct NoteAttachment<'a> {
    attachment_scheme: NoteAttachmentScheme,
    content: NoteAttachmentContent<'a>,
}

impl PartialEq for NoteAttachment<'_> {
    /// Two attachments are equal when their schemes and words are equal, however the words are
    /// stored.
    fn eq(&self, other: &Self) -> bool {
        self.attachment_scheme == other.attachment_scheme && self.as_words() == other.as_words()
    }
}

impl Eq for NoteAttachment<'_> {}

/// The owned or borrowed words of a validated attachment.
#[derive(Clone, Debug)]
enum NoteAttachmentContent<'a> {
    /// A single word stored in the attachment itself.
    Word(Word),
    /// A nonempty slice containing at most 256 words.
    Words(&'a [Word]),
}

impl<'a> NoteAttachment<'a> {
    /// Creates an attachment holding a single word by value, without allocating or hashing.
    pub fn with_word(attachment_scheme: NoteAttachmentScheme, word: Word) -> Self {
        Self {
            attachment_scheme,
            content: NoteAttachmentContent::Word(word),
        }
    }

    /// Creates an attachment borrowing its words, without allocating or hashing.
    ///
    /// # Errors
    ///
    /// Returns [`NoteAttachmentError::EmptyContent`] for an empty slice or
    /// [`NoteAttachmentError::TooManyWords`] for more than 256 words.
    pub fn with_words(
        attachment_scheme: NoteAttachmentScheme,
        words: &'a [Word],
    ) -> Result<Self, NoteAttachmentError> {
        if words.is_empty() {
            return Err(NoteAttachmentError::EmptyContent);
        }
        if words.len() > MAX_ATTACHMENT_WORDS {
            return Err(NoteAttachmentError::TooManyWords(words.len()));
        }
        Ok(Self {
            attachment_scheme,
            content: NoteAttachmentContent::Words(words),
        })
    }

    /// Returns the attachment's scheme.
    pub fn attachment_scheme(&self) -> NoteAttachmentScheme {
        self.attachment_scheme
    }

    /// Returns the attachment's content as words.
    pub fn as_words(&self) -> &[Word] {
        match &self.content {
            NoteAttachmentContent::Word(word) => core::slice::from_ref(word),
            NoteAttachmentContent::Words(words) => words,
        }
    }

    /// Returns the attachment's word count, between 1 and 256.
    pub fn num_words(&self) -> u16 {
        // The constructors bound the length by 256, so the cast is exact.
        self.as_words().len() as u16
    }

    /// Appends this attachment to an output note, preserving existing attachment order.
    ///
    /// Duplicate schemes are allowed; lookup returns the first matching attachment. For a
    /// commitment whose preimage is already available in the advice map, call the advice-map writer
    /// [`output_note::add_attachment`] directly; it takes a [`NoteAttachmentScheme`] as well.
    ///
    /// # Panics
    ///
    /// The transaction kernel rejects a missing or sealed output note, a non-native active account,
    /// or an append that exceeds four attachments or 512 total attachment words for the note.
    pub fn add_to(&self, note_idx: NoteIdx) {
        let words = self.as_words();
        // Single-word content, including a one-word borrowed slice, uses the word writer; longer
        // content uses the memory writer.
        if let [word] = words {
            output_note::add_word_attachment(note_idx, self.attachment_scheme, *word);
        } else {
            output_note::add_attachment_from_memory(note_idx, self.attachment_scheme, words);
        }
    }
}

#[cfg(test)]
/// Native checks of SDK attachment invariants against the pinned protocol.
mod tests {
    use miden_field_repr::{FeltReprError, FromFeltRepr, ToFeltRepr};
    use miden_protocol::note::{
        NoteAttachment as ProtocolAttachment, NoteAttachmentScheme as ProtocolScheme,
    };
    use miden_stdlib_sys::{Felt, Word, felt};

    use super::{AttachmentIdx, NoteAttachment, NoteAttachmentError, NoteAttachmentScheme};

    /// Checks every scheme value against the protocol and preserves accepted values in conversions.
    #[test]
    fn schemes_match_protocol() {
        for value in 0..=u16::MAX {
            let ours = NoteAttachmentScheme::new(value);
            let theirs = ProtocolScheme::new(value);
            assert_eq!(ours.is_ok(), theirs.is_ok(), "scheme {value}");
            assert_eq!(ours, NoteAttachmentScheme::try_from(value));
            match ours {
                Ok(scheme) => {
                    assert_eq!(scheme.as_u16(), value);
                    assert_eq!(u16::from(scheme), value);
                    assert_eq!(scheme.is_none(), theirs.unwrap().is_none(), "scheme {value}");
                }
                Err(error) => assert_eq!(error, NoteAttachmentError::InvalidScheme(value)),
            }
        }
        assert_eq!(NoteAttachmentScheme::none().as_u16(), ProtocolScheme::none().as_u16());
        assert_eq!(NoteAttachmentScheme::MAX.as_u16(), ProtocolScheme::MAX.as_u16());
        assert_eq!(NoteAttachmentScheme::default().as_u16(), ProtocolScheme::default().as_u16());
    }

    /// Checks that `new_const` keeps a valid scheme value.
    #[test]
    fn new_const_keeps_a_valid_scheme() {
        const SCHEME: NoteAttachmentScheme = NoteAttachmentScheme::new_const(42);
        assert_eq!(SCHEME.as_u16(), 42);
    }

    /// Checks that `new_const` rejects scheme `0`.
    #[test]
    #[should_panic(expected = "note attachment scheme must be between 1 and 65534")]
    fn new_const_rejects_zero() {
        let _ = NoteAttachmentScheme::new_const(core::hint::black_box(0));
    }

    /// Checks that `new_const` rejects scheme `65535`.
    #[test]
    #[should_panic(expected = "note attachment scheme must be between 1 and 65534")]
    fn new_const_rejects_65535() {
        let _ = NoteAttachmentScheme::new_const(core::hint::black_box(u16::MAX));
    }

    /// Checks felt conversion of valid, invalid and out-of-range scheme values.
    #[test]
    fn schemes_convert_from_felts() {
        assert_eq!(NoteAttachmentScheme::try_from(felt!(42)), NoteAttachmentScheme::new(42));
        assert_eq!(
            NoteAttachmentScheme::try_from(felt!(0)),
            Err(NoteAttachmentError::InvalidScheme(0))
        );
        assert_eq!(NoteAttachmentScheme::try_from(felt!(65534)), NoteAttachmentScheme::new(65534));
        assert_eq!(
            NoteAttachmentScheme::try_from(felt!(65535)),
            Err(NoteAttachmentError::InvalidScheme(65535))
        );
        assert_eq!(
            NoteAttachmentScheme::try_from(Felt::from_u32(u32::from(u16::MAX) + 1)),
            Err(NoteAttachmentError::SchemeExceedsU16)
        );
        assert_eq!(
            NoteAttachmentScheme::try_from(Felt::new(Felt::ORDER - 1).unwrap()),
            Err(NoteAttachmentError::SchemeExceedsU16)
        );
    }

    /// Checks that a metadata slot holding the invalid scheme `65535` panics.
    #[test]
    #[should_panic(expected = "invalid attachment scheme in note metadata")]
    fn header_slot_rejects_invalid_scheme() {
        let _ = NoteAttachmentScheme::from_header_u16(u16::MAX);
    }

    /// Checks that an absent metadata slot decodes to `None` and other values keep their scheme.
    #[test]
    fn header_slots_decode_to_optional_schemes() {
        assert_eq!(NoteAttachmentScheme::from_header_u16(0), None);
        assert_eq!(NoteAttachmentScheme::from_header_u16(1), Some(NoteAttachmentScheme::none()));
        assert_eq!(
            NoteAttachmentScheme::from_header_u16(42),
            Some(NoteAttachmentScheme::new(42).unwrap())
        );
    }

    /// Checks content length boundaries against the protocol's owned attachment type.
    #[test]
    fn content_lengths_match_protocol() {
        extern crate alloc;

        let words = [Word::from([felt!(0); 4]); 257];
        for length in [0, 1, 256, 257] {
            let ours = NoteAttachment::with_words(NoteAttachmentScheme::none(), &words[..length]);
            let theirs = ProtocolAttachment::with_words(
                ProtocolScheme::none(),
                alloc::vec![miden_protocol::Word::default(); length],
            );
            assert_eq!(ours.is_ok(), theirs.is_ok(), "length {length}");
            match ours {
                Ok(attachment) => {
                    assert_eq!(attachment.num_words(), theirs.unwrap().num_words());
                    assert_eq!(attachment.as_words(), &words[..length]);
                }
                Err(error) if length == 0 => assert_eq!(error, NoteAttachmentError::EmptyContent),
                Err(error) => assert_eq!(error, NoteAttachmentError::TooManyWords(length)),
            }
        }
    }

    /// Checks scheme and content access for owned words and borrowed slices.
    #[test]
    fn accessors_preserve_content() {
        let scheme = NoteAttachmentScheme::new(42).unwrap();
        let words = [
            Word::from([felt!(1), felt!(2), felt!(3), felt!(4)]),
            Word::from([felt!(5), felt!(6), felt!(7), felt!(8)]),
        ];
        let owned = NoteAttachment::with_word(scheme, words[0]);
        let borrowed_one = NoteAttachment::with_words(scheme, &words[..1]).unwrap();
        let borrowed_many = NoteAttachment::with_words(scheme, &words).unwrap();
        for attachment in [&owned, &borrowed_one, &borrowed_many] {
            assert_eq!(attachment.attachment_scheme(), scheme);
            assert_eq!(attachment.as_words(), &words[..usize::from(attachment.num_words())]);
        }
        assert_eq!(owned.num_words(), 1);
        assert_eq!(borrowed_one.num_words(), 1);
        assert_eq!(borrowed_many.num_words(), 2);
        assert_eq!(borrowed_many.as_words().as_ptr(), words.as_ptr());
    }

    /// Checks that equality compares schemes and words, not how the words are stored.
    #[test]
    fn equality_ignores_word_storage() {
        let scheme = NoteAttachmentScheme::new(42).unwrap();
        let word = Word::from([felt!(1), felt!(2), felt!(3), felt!(4)]);
        let other_word = Word::from([felt!(5), felt!(6), felt!(7), felt!(8)]);
        let owned = NoteAttachment::with_word(scheme, word);
        let words = [word];
        assert_eq!(owned, NoteAttachment::with_words(scheme, &words).unwrap());
        assert_ne!(owned, NoteAttachment::with_word(NoteAttachmentScheme::none(), word));
        assert_ne!(owned, NoteAttachment::with_word(scheme, other_word));

        let pair = [word, other_word];
        let pair_copy = [word, other_word];
        let differing_second = [word, word];
        let two_words = NoteAttachment::with_words(scheme, &pair).unwrap();
        assert_eq!(two_words, NoteAttachment::with_words(scheme, &pair_copy).unwrap());
        assert_ne!(two_words, NoteAttachment::with_words(scheme, &differing_second).unwrap());
        assert_ne!(owned, two_words);
    }

    /// Checks the index bounds, the integer and felt conversions, the const constructor and the
    /// protocol's attachment count.
    #[test]
    fn indexes_are_bounded_by_attachment_count() {
        for value in 0..=3u8 {
            let index = AttachmentIdx::new(value).unwrap();
            assert_eq!(index.as_u8(), value);
            assert_eq!(AttachmentIdx::try_from(u32::from(value)), Ok(index));
            assert_eq!(AttachmentIdx::try_from(Felt::from(value)), Ok(index));
            assert_eq!(u8::from(index), value);
            assert_eq!(u32::from(index), u32::from(value));
            assert_eq!(usize::from(index), usize::from(value));
        }
        assert_eq!(AttachmentIdx::new(4), Err(NoteAttachmentError::InvalidIndex(4)));
        assert_eq!(AttachmentIdx::new(255), Err(NoteAttachmentError::InvalidIndex(255)));
        assert_eq!(AttachmentIdx::try_from(4u32), Err(NoteAttachmentError::InvalidIndex(4)));
        assert_eq!(AttachmentIdx::try_from(255u32), Err(NoteAttachmentError::InvalidIndex(255)));
        assert_eq!(AttachmentIdx::try_from(256u32), Err(NoteAttachmentError::IndexExceedsU8));
        assert_eq!(AttachmentIdx::try_from(u32::MAX), Err(NoteAttachmentError::IndexExceedsU8));
        assert_eq!(AttachmentIdx::try_from(felt!(3)), Ok(AttachmentIdx::MAX));
        assert_eq!(AttachmentIdx::try_from(felt!(4)), Err(NoteAttachmentError::InvalidIndex(4)));
        assert_eq!(
            AttachmentIdx::try_from(felt!(255)),
            Err(NoteAttachmentError::InvalidIndex(255))
        );
        assert_eq!(AttachmentIdx::try_from(felt!(256)), Err(NoteAttachmentError::IndexExceedsU8));
        assert_eq!(
            AttachmentIdx::try_from(Felt::new(Felt::ORDER - 1).unwrap()),
            Err(NoteAttachmentError::IndexExceedsU8)
        );
        const IDX: AttachmentIdx = AttachmentIdx::new_const(2);
        assert_eq!(IDX, AttachmentIdx::new(2).unwrap());
        assert_eq!(
            usize::from(AttachmentIdx::MAX) + 1,
            miden_protocol::note::NoteAttachments::MAX_COUNT
        );
        assert!(AttachmentIdx::new(1).unwrap() < AttachmentIdx::new(2).unwrap());
        assert_eq!(AttachmentIdx::from_kernel(2), AttachmentIdx::new(2).unwrap());
    }

    /// Checks that `new_const` rejects index `4`.
    #[test]
    #[should_panic(expected = "note attachment index must be at most 3")]
    fn new_const_rejects_index_4() {
        let _ = AttachmentIdx::new_const(core::hint::black_box(4));
    }

    /// Checks that schemes and indexes encode as one felt and decode back.
    #[test]
    fn felt_repr_round_trips() {
        assert_eq!(<NoteAttachmentScheme as FromFeltRepr>::FIXED_LEN, Some(1));
        assert_eq!(<AttachmentIdx as FromFeltRepr>::FIXED_LEN, Some(1));
        for scheme in [NoteAttachmentScheme::new(42).unwrap(), NoteAttachmentScheme::MAX] {
            let felts = scheme.to_felt_repr();
            assert_eq!(felts, [Felt::from(scheme.as_u16())]);
            assert_eq!(NoteAttachmentScheme::try_from(&felts[..]), Ok(scheme));
        }
        for index in [AttachmentIdx::new(0).unwrap(), AttachmentIdx::MAX] {
            let felts = index.to_felt_repr();
            assert_eq!(felts, [Felt::from(index.as_u8())]);
            assert_eq!(AttachmentIdx::try_from(&felts[..]), Ok(index));
        }
        assert_eq!(
            NoteAttachmentScheme::try_from(&[felt!(1)][..]),
            Ok(NoteAttachmentScheme::none())
        );
        assert_eq!(AttachmentIdx::try_from(&[felt!(0)][..]), Ok(AttachmentIdx::new(0).unwrap()));
    }

    /// Checks that decoding rejects invalid values, trailing felts and missing felts.
    #[test]
    fn felt_repr_decoding_validates() {
        let scheme_out_of_range = |value| FeltReprError::ValueOutOfRange {
            pos: 0,
            len: 1,
            ty: "NoteAttachmentScheme",
            value,
            max: 65534,
        };
        let index_out_of_range = |value| FeltReprError::ValueOutOfRange {
            pos: 0,
            len: 1,
            ty: "AttachmentIdx",
            value,
            max: 3,
        };
        let top = Felt::new(Felt::ORDER - 1).unwrap();
        assert_eq!(NoteAttachmentScheme::try_from(&[felt!(0)][..]), Err(scheme_out_of_range(0)));
        assert_eq!(
            NoteAttachmentScheme::try_from(&[felt!(65535)][..]),
            Err(scheme_out_of_range(65535))
        );
        assert_eq!(
            NoteAttachmentScheme::try_from(&[felt!(65536)][..]),
            Err(scheme_out_of_range(65536))
        );
        assert_eq!(
            NoteAttachmentScheme::try_from(&[top][..]),
            Err(scheme_out_of_range(Felt::ORDER - 1))
        );
        assert_eq!(AttachmentIdx::try_from(&[felt!(4)][..]), Err(index_out_of_range(4)));
        assert_eq!(AttachmentIdx::try_from(&[felt!(255)][..]), Err(index_out_of_range(255)));
        assert_eq!(AttachmentIdx::try_from(&[felt!(256)][..]), Err(index_out_of_range(256)));
        assert_eq!(AttachmentIdx::try_from(&[top][..]), Err(index_out_of_range(Felt::ORDER - 1)));
        assert_eq!(
            NoteAttachmentScheme::try_from(&[felt!(42), felt!(1)][..]),
            Err(FeltReprError::TrailingData { pos: 1, len: 2 })
        );
        assert_eq!(
            AttachmentIdx::try_from(&[felt!(1), felt!(1)][..]),
            Err(FeltReprError::TrailingData { pos: 1, len: 2 })
        );
        assert_eq!(
            NoteAttachmentScheme::try_from(&[][..]),
            Err(FeltReprError::UnexpectedEof { pos: 0, len: 0 })
        );
        assert_eq!(
            AttachmentIdx::try_from(&[][..]),
            Err(FeltReprError::UnexpectedEof { pos: 0, len: 0 })
        );
    }

    /// Checks that derived encodings compose with the scheme and index encodings.
    #[test]
    fn felt_repr_composes_with_derives() {
        /// A derived felt layout holding an attachment reference.
        #[derive(Debug, PartialEq, FromFeltRepr, ToFeltRepr)]
        struct AttachmentRef {
            scheme: NoteAttachmentScheme,
            index: AttachmentIdx,
        }

        let value = AttachmentRef {
            scheme: NoteAttachmentScheme::new(42).unwrap(),
            index: AttachmentIdx::MAX,
        };
        let felts = value.to_felt_repr();
        assert_eq!(felts, [felt!(42), felt!(3)]);
        assert_eq!(<AttachmentRef as FromFeltRepr>::FIXED_LEN, Some(2));
        assert_eq!(AttachmentRef::try_from(&felts[..]), Ok(value));
    }
}
