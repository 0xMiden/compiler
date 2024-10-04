#![no_std]

extern crate alloc;

mod layout;

use alloc::{boxed::Box, vec::Vec};
use core::{fmt, num::NonZeroU16, str::FromStr};

use miden_formatting::prettier::PrettyPrint;

pub use self::layout::Alignable;

/// Represents the type of a value
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Type {
    /// This indicates a failure to type a value, or a value which is untypable
    Unknown,
    /// This type is used to indicate the absence of a value, such as a function which returns
    /// nothing
    Unit,
    /// This type is the bottom type, and represents divergence, akin to Rust's Never/! type
    Never,
    I1,
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    I128,
    U128,
    U256,
    F64,
    /// Field element
    Felt,
    /// A pointer to a value in the default byte-addressable address space used by the IR.
    ///
    /// Pointers of this type will be translated to an appropriate address space during code
    /// generation.
    Ptr(Box<Type>),
    /// A pointer to a valude in Miden's native word-addressable address space.
    ///
    /// In the type system, we represent the type of the pointee, as well as an address space
    /// identifier.
    ///
    /// This pointer type is represented on Miden's operand stack as a u64 value, consisting of
    /// two 32-bit elements, the most-significant bits being on top of the stack:
    ///
    /// 1. Metadata for the pointer (in the upper 32-bit limb):
    ///   * The least-significant 2 bits represent a zero-based element index (range is 0-3)
    ///   * The next most significant 4 bits represent a zero-based byte index (range is 0-31)
    ///   * The remaining 26 bits represent an address space identifier
    /// 2. The lower 32-bit limb contains the word-aligned address, which forms the base address of
    ///    the pointer.
    ///
    /// Dereferencing a pointer of this type involves popping the pointer metadata, and determining
    /// what type of load to issue based on the size of the value being loaded, and where the
    /// start of the data is according to the metadata. Then the word-aligned address is popped
    /// and the value is loaded.
    ///
    /// If the load is naturally aligned, i.e. the element index and byte offset are zero, and the
    /// size is exactly one element or word; then a mem_load or mem_loadw are issued and no
    /// further action is required. If the load is not naturally aligned, then either one or
    /// two words will be loaded, depending on the type being loaded, unused elements will be
    /// dropped, and if the byte offset is non-zero, the data will be shifted bitwise into
    /// alignment on an element boundary.
    NativePtr(Box<Type>, AddressSpace),
    /// A compound type of fixed shape and size
    Struct(StructType),
    /// A vector of fixed size
    Array(Box<Type>, usize),
    /// A dynamically sized list of values of the given type
    ///
    /// NOTE: Currently this only exists to support the Wasm Canonical ABI,
    /// but it has no defined represenation yet, so in practice cannot be
    /// used in most places except during initial translation in the Wasm frontend.
    List(Box<Type>),
}
impl Type {
    /// Returns true if this type is a zero-sized type, which includes:
    ///
    /// * Types with no size, e.g. `Type::Unit`
    /// * Zero-sized arrays
    /// * Arrays with a zero-sized element type
    /// * Structs composed of nothing but zero-sized fields
    pub fn is_zst(&self) -> bool {
        match self {
            Self::Unknown => false,
            Self::Never | Self::Unit => true,
            Self::Array(_, 0) => true,
            Self::Array(ref elem_ty, _) => elem_ty.is_zst(),
            Self::Struct(ref struct_ty) => struct_ty.fields.iter().all(|f| f.ty.is_zst()),
            Self::I1
            | Self::I8
            | Self::U8
            | Self::I16
            | Self::U16
            | Self::I32
            | Self::U32
            | Self::I64
            | Self::U64
            | Self::I128
            | Self::U128
            | Self::U256
            | Self::F64
            | Self::Felt
            | Self::Ptr(_)
            | Self::NativePtr(..)
            | Self::List(_) => false,
        }
    }

    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            Self::I1
                | Self::I8
                | Self::U8
                | Self::I16
                | Self::U16
                | Self::I32
                | Self::U32
                | Self::I64
                | Self::U64
                | Self::I128
                | Self::U128
                | Self::U256
                | Self::F64
                | Self::Felt
        )
    }

    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            Self::I1
                | Self::I8
                | Self::U8
                | Self::I16
                | Self::U16
                | Self::I32
                | Self::U32
                | Self::I64
                | Self::U64
                | Self::I128
                | Self::U128
                | Self::U256
                | Self::Felt
        )
    }

    pub fn is_signed_integer(&self) -> bool {
        matches!(self, Self::I8 | Self::I16 | Self::I32 | Self::I64 | Self::I128)
    }

    pub fn is_unsigned_integer(&self) -> bool {
        matches!(self, Self::I1 | Self::U8 | Self::U16 | Self::U32 | Self::U64 | Self::U128)
    }

    /// Get this type as its unsigned integral twin, e.g. i32 becomes u32.
    ///
    /// This function will panic if the type is not an integer type, or has no unsigned
    /// representation
    pub fn as_unsigned(&self) -> Type {
        match self {
            Self::I8 | Self::U8 => Self::U8,
            Self::I16 | Self::U16 => Self::U16,
            Self::I32 | Self::U32 => Self::U32,
            Self::I64 | Self::U64 => Self::U64,
            Self::I128 | Self::U128 => Self::U128,
            Self::Felt => Self::Felt,
            ty => panic!("invalid conversion to unsigned integer type: {ty} is not an integer"),
        }
    }

    /// Get this type as its signed integral twin, e.g. u32 becomes i32.
    ///
    /// This function will panic if the type is not an integer type, or has no signed representation
    pub fn as_signed(&self) -> Type {
        match self {
            Self::I8 | Self::U8 => Self::I8,
            Self::I16 | Self::U16 => Self::I16,
            Self::I32 | Self::U32 => Self::I32,
            Self::I64 | Self::U64 => Self::I64,
            Self::I128 | Self::U128 => Self::I128,
            Self::Felt => {
                panic!("invalid conversion to signed integer type: felt has no signed equivalent")
            }
            ty => panic!("invalid conversion to signed integer type: {ty} is not an integer"),
        }
    }

    #[inline]
    pub fn is_float(&self) -> bool {
        matches!(self, Self::F64)
    }

    #[inline]
    pub fn is_felt(&self) -> bool {
        matches!(self, Self::Felt)
    }

    #[inline]
    pub fn is_pointer(&self) -> bool {
        matches!(self, Self::Ptr(_) | Self::NativePtr(_, _))
    }

    #[inline]
    pub fn is_struct(&self) -> bool {
        matches!(self, Self::Struct(_))
    }

    #[inline]
    pub fn is_array(&self) -> bool {
        matches!(self, Self::Array(_, _))
    }

    #[inline]
    pub fn is_list(&self) -> bool {
        matches!(self, Self::List(_))
    }

    /// Returns true if `self` and `other` are compatible operand types for a binary operator, e.g.
    /// `add`
    ///
    /// In short, the rules are as follows:
    ///
    /// * The operand order is assumed to be `self <op> other`, i.e. `op` is being applied to `self`
    ///   using `other`. The left-hand operand is used as the "controlling" type for the operator,
    ///   i.e. it determines what instruction will be used to perform the operation.
    /// * The operand types must be numeric, or support being manipulated numerically
    /// * If the controlling type is unsigned, it is never compatible with signed types, because
    ///   Miden instructions for unsigned types use a simple unsigned binary encoding, thus they
    ///   will not handle signed operands using two's complement correctly.
    /// * If the controlling type is signed, it is compatible with both signed and unsigned types,
    ///   as long as the values fit in the range of the controlling type, e.g. adding a `u16` to an
    ///   `i32` is fine, but adding a `u32` to an `i32` is not.
    /// * Pointer types are permitted to be the controlling type, and since they are represented
    ///   using u32, they have the same compatibility set as u32 does. In all other cases, pointer
    ///   types are treated the same as any other non-numeric type.
    /// * Non-numeric types are always incompatible, since no operators support these types
    pub fn is_compatible_operand(&self, other: &Type) -> bool {
        match (self, other) {
            (Type::I1, Type::I1) => true,
            (Type::I8, Type::I8) => true,
            (Type::U8, Type::U8) => true,
            (Type::I16, Type::I8 | Type::U8 | Type::I16) => true,
            (Type::U16, Type::U8 | Type::U16) => true,
            (Type::I32, Type::I8 | Type::U8 | Type::I16 | Type::U16 | Type::I32) => true,
            (Type::U32, Type::U8 | Type::U16 | Type::U32) => true,
            (
                Type::I64,
                Type::I8 | Type::U8 | Type::I16 | Type::U16 | Type::I32 | Type::U32 | Type::I64,
            ) => true,
            (Type::U64, Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::Felt) => true,
            (
                Type::I128,
                Type::I8
                | Type::U8
                | Type::I16
                | Type::U16
                | Type::I32
                | Type::U32
                | Type::I64
                | Type::U64
                | Type::Felt
                | Type::I128,
            ) => true,
            (
                Type::U128,
                Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::Felt | Type::U128,
            ) => true,
            (Type::U256, rty) => rty.is_unsigned_integer(),
            (Type::Felt, Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::Felt) => true,
            (Type::F64, Type::F64) => true,
            (Type::Ptr(_) | Type::NativePtr(..), Type::U8 | Type::U16 | Type::U32) => true,
            _ => false,
        }
    }

    #[inline]
    pub fn pointee(&self) -> Option<&Type> {
        use core::ops::Deref;
        match self {
            Self::Ptr(ty) | Self::NativePtr(ty, _) => Some(ty.deref()),
            _ => None,
        }
    }
}
impl From<StructType> for Type {
    #[inline]
    fn from(ty: StructType) -> Type {
        Type::Struct(ty)
    }
}
impl fmt::Display for Type {
    /// Print this type for display using the provided module context
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use core::fmt::Write;
        match self {
            Self::Unknown => f.write_str("?"),
            Self::Unit => f.write_str("()"),
            Self::Never => f.write_char('!'),
            Self::I1 => f.write_str("i1"),
            Self::I8 => f.write_str("i8"),
            Self::U8 => f.write_str("u8"),
            Self::I16 => f.write_str("i16"),
            Self::U16 => f.write_str("u16"),
            Self::I32 => f.write_str("i32"),
            Self::U32 => f.write_str("u32"),
            Self::I64 => f.write_str("i64"),
            Self::U64 => f.write_str("u64"),
            Self::I128 => f.write_str("i128"),
            Self::U128 => f.write_str("u128"),
            Self::U256 => f.write_str("u256"),
            Self::F64 => f.write_str("f64"),
            Self::Felt => f.write_str("felt"),
            Self::Ptr(inner) => write!(f, "(ptr {inner})"),
            Self::NativePtr(inner, AddressSpace::Unknown) => {
                write!(f, "(ptr (addrspace ?) {inner})")
            }
            Self::NativePtr(inner, AddressSpace::Root) => {
                write!(f, "(ptr (addrspace 0) {inner})")
            }
            Self::NativePtr(inner, AddressSpace::Id(id)) => {
                write!(f, "(ptr (addrspace {id}) {inner})")
            }
            Self::Struct(sty) => write!(f, "{sty}"),
            Self::Array(element_ty, arity) => write!(f, "(array {element_ty} {arity})"),
            Self::List(ty) => write!(f, "(list {ty})"),
        }
    }
}
impl PrettyPrint for Type {
    fn render(&self) -> miden_formatting::prettier::Document {
        use miden_formatting::prettier::*;

        display(self)
    }
}

/// This represents metadata about how a structured type will be represented in memory
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TypeRepr {
    /// This corresponds to the C ABI representation for a given type
    #[default]
    Default,
    /// This modifies the default representation, by raising the minimum alignment.
    ///
    /// The alignment must be a power of two, e.g. 32, and values from 1 to 2^16 are allowed.
    ///
    /// The alignment must be greater than the default minimum alignment of the type
    /// or this representation has no effect.
    Align(NonZeroU16),
    /// This modifies the default representation, by lowering the minimum alignment of
    /// a type, and in the case of structs, changes the alignments of the fields to be
    /// the smaller of the specified alignment and the default alignment. This has the
    /// effect of changing the layout of a struct.
    ///
    /// Notably, `Packed(1)` will result in a struct that has no alignment requirement,
    /// and no padding between fields.
    ///
    /// The alignment must be a power of two, e.g. 32, and values from 1 to 2^16 are allowed.
    ///
    /// The alignment must be smaller than the default alignment, or this representation
    /// has no effect.
    Packed(NonZeroU16),
    /// This may only be used on structs with no more than one non-zero sized field, and
    /// indicates that the representation of that field should be used for the struct.
    Transparent,
}
impl TypeRepr {
    #[inline]
    pub fn packed(align: u16) -> Self {
        Self::Packed(
            NonZeroU16::new(align).expect("invalid alignment: expected value in range 1..=65535"),
        )
    }

    #[inline]
    pub fn align(align: u16) -> Self {
        Self::Align(
            NonZeroU16::new(align).expect("invalid alignment: expected value in range 1..=65535"),
        )
    }

    /// Return true if this type representation is transparent
    pub fn is_transparent(&self) -> bool {
        matches!(self, Self::Transparent)
    }

    /// Return true if this type representation is packed
    pub fn is_packed(&self) -> bool {
        matches!(self, Self::Packed(_))
    }

    /// Get the custom alignment given for this type representation, if applicable
    pub fn min_alignment(&self) -> Option<usize> {
        match self {
            Self::Packed(align) | Self::Align(align) => Some(align.get() as usize),
            _ => None,
        }
    }
}

/// This represents metadata about a field of a [StructType]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StructField {
    /// The index of this field in the final layout
    pub index: u8,
    /// The specified alignment for this field
    pub align: u16,
    /// The offset of this field relative to the base of the struct
    pub offset: u32,
    /// The type of this field
    pub ty: Type,
}
impl fmt::Display for StructField {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", &self.ty)
    }
}

/// This represents a structured aggregate type
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StructType {
    /// The representation to use for this type
    pub(crate) repr: TypeRepr,
    /// The computed size of this struct
    pub(crate) size: u32,
    /// The fields of this struct, in the original order specified
    ///
    /// The actual order of fields in the final layout is determined by the index
    /// associated with each field, not the index in this vector, although for `repr(C)`
    /// structs they will be the same
    pub(crate) fields: Vec<StructField>,
}
impl StructType {
    /// Create a new struct with default representation, i.e. a struct with representation of
    /// `TypeRepr::Packed(1)`.
    #[inline]
    pub fn new<I: IntoIterator<Item = Type>>(fields: I) -> Self {
        Self::new_with_repr(TypeRepr::Default, fields)
    }

    /// Create a new struct with the given representation.
    ///
    /// This function will panic if the rules of the given representation are violated.
    pub fn new_with_repr<I: IntoIterator<Item = Type>>(repr: TypeRepr, fields: I) -> Self {
        let tys = fields.into_iter().collect::<Vec<_>>();
        let mut fields = Vec::with_capacity(tys.len());
        let size = match repr {
            TypeRepr::Transparent => {
                let mut offset = 0u32;
                for (index, ty) in tys.into_iter().enumerate() {
                    let index: u8 =
                        index.try_into().expect("invalid struct: expected no more than 255 fields");
                    let field_size: u32 = ty
                        .size_in_bytes()
                        .try_into()
                        .expect("invalid type: size is larger than 2^32 bytes");
                    if field_size == 0 {
                        fields.push(StructField {
                            index,
                            align: 1,
                            offset,
                            ty,
                        });
                    } else {
                        let align = ty.min_alignment().try_into().expect(
                            "invalid struct field alignment: expected power of two between 1 and \
                             2^16",
                        );
                        assert_eq!(
                            offset, 0,
                            "invalid transparent representation for struct: repr(transparent) is \
                             only valid for structs with a single non-zero sized field"
                        );
                        fields.push(StructField {
                            index,
                            align,
                            offset,
                            ty,
                        });
                        offset += field_size;
                    }
                }
                offset
            }
            repr => {
                let mut offset = 0u32;
                let default_align: u16 =
                    tys.iter().map(|t| t.min_alignment()).max().unwrap_or(1).try_into().expect(
                        "invalid struct field alignment: expected power of two between 1 and 2^16",
                    );
                let align = match repr {
                    TypeRepr::Align(align) => core::cmp::max(align.get(), default_align),
                    TypeRepr::Packed(align) => core::cmp::min(align.get(), default_align),
                    TypeRepr::Transparent | TypeRepr::Default => default_align,
                };

                for (index, ty) in tys.into_iter().enumerate() {
                    let index: u8 =
                        index.try_into().expect("invalid struct: expected no more than 255 fields");
                    let field_size: u32 = ty
                        .size_in_bytes()
                        .try_into()
                        .expect("invalid type: size is larger than 2^32 bytes");
                    let default_align: u16 = ty.min_alignment().try_into().expect(
                        "invalid struct field alignment: expected power of two between 1 and 2^16",
                    );
                    let align: u16 = match repr {
                        TypeRepr::Packed(align) => core::cmp::min(align.get(), default_align),
                        _ => default_align,
                    };
                    offset += offset.align_offset(align as u32);
                    fields.push(StructField {
                        index,
                        align,
                        offset,
                        ty,
                    });
                    offset += field_size;
                }
                offset.align_up(align as u32)
            }
        };
        Self { repr, size, fields }
    }

    /// Get the [TypeRepr] for this struct
    #[inline]
    pub const fn repr(&self) -> TypeRepr {
        self.repr
    }

    /// Get the minimum alignment for this struct
    pub fn min_alignment(&self) -> usize {
        self.repr
            .min_alignment()
            .unwrap_or_else(|| self.fields.iter().map(|f| f.align as usize).max().unwrap_or(1))
    }

    /// Get the total size in bytes required to hold this struct, including alignment padding
    #[inline]
    pub fn size(&self) -> usize {
        self.size as usize
    }

    /// Get the struct field at `index`, relative to declaration order.
    pub fn get(&self, index: usize) -> &StructField {
        &self.fields[index]
    }

    /// Get the struct fields as a slice
    pub fn fields(&self) -> &[StructField] {
        self.fields.as_slice()
    }

    /// Returns true if this struct has no fields
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Get the length of this struct (i.e. number of fields)
    pub fn len(&self) -> usize {
        self.fields.len()
    }
}
impl TryFrom<Type> for StructType {
    type Error = Type;

    fn try_from(ty: Type) -> Result<Self, Self::Error> {
        match ty {
            Type::Struct(ty) => Ok(ty),
            other => Err(other),
        }
    }
}
impl fmt::Display for StructType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.repr {
            TypeRepr::Default => f.write_str("(struct ")?,
            TypeRepr::Transparent => f.write_str("(struct (repr transparent) ")?,
            TypeRepr::Align(align) => write!(f, "(struct (repr (align {align})) ")?,
            TypeRepr::Packed(align) => write!(f, "(struct (repr (packed {align})) ")?,
        };
        for (i, field) in self.fields.iter().enumerate() {
            if i > 0 {
                write!(f, " {}", field)?;
            } else {
                write!(f, "{}", field)?;
            }
        }
        f.write_str(")")
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde_repr::Serialize_repr, serde_repr::Deserialize_repr)
)]
#[repr(u8)]
pub enum Abi {
    /// The type signature of a function in canonical form. Canonical in this context means that
    /// no special lowering is required between caller and callee - all the compiler needs to
    /// deal with are the details of the specific calling convention.
    #[default]
    Canonical,
    /// The type signature of a function expressed in terms of the Canonical ABI of the Wasm
    /// Component Model. This indicates that additional lowering/lifting code is required between
    /// caller and callee. It also dictates the calling convention for the callee.
    Wasm,
}

impl fmt::Display for Abi {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Canonical => f.write_str("canon"),
            Self::Wasm => f.write_str("wasm"),
        }
    }
}

/// This represents the type of a function, including the ABI, result types, and parameter types
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FunctionType {
    /// The ABI of this function type
    pub abi: Abi,
    /// The result types of this function
    pub results: Vec<Type>,
    /// The parameter types of this function
    pub params: Vec<Type>,
}
impl FunctionType {
    /// Create a new function type with the canonical ABI
    pub fn new<P: IntoIterator<Item = Type>, R: IntoIterator<Item = Type>>(
        params: P,
        results: R,
    ) -> Self {
        Self {
            abi: Abi::Canonical,
            results: results.into_iter().collect(),
            params: params.into_iter().collect(),
        }
    }

    /// Create a new function type with the Wasm Component Model ABI
    pub fn new_wasm<P: IntoIterator<Item = Type>, R: IntoIterator<Item = Type>>(
        params: P,
        results: R,
    ) -> Self {
        Self {
            abi: Abi::Wasm,
            results: results.into_iter().collect(),
            params: params.into_iter().collect(),
        }
    }

    /// Set the ABI for this function type
    pub fn with_abi(mut self, abi: Abi) -> Self {
        self.abi = abi;
        self
    }

    pub fn arity(&self) -> usize {
        self.params.len()
    }

    pub fn results(&self) -> &[Type] {
        self.results.as_slice()
    }

    pub fn params(&self) -> &[Type] {
        self.params.as_slice()
    }

    pub fn abi(&self) -> Abi {
        self.abi
    }
}
impl fmt::Display for FunctionType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use core::fmt::Write;

        f.write_str("(func")?;
        f.write_fmt(format_args!(" (abi {abi}) ", abi = self.abi))?;
        for ty in self.params.iter() {
            write!(f, " (param {ty})")?;
        }
        if !self.results.is_empty() {
            f.write_str(" (result")?;
            for ty in self.results.iter() {
                write!(f, " {ty}")?;
            }
            f.write_char(')')?;
        }
        f.write_char(')')
    }
}

/// This error is raised when parsing an [AddressSpace]
#[derive(Debug)]
pub enum InvalidAddressSpaceError {
    InvalidId,
    InvalidIdOverflow,
}
impl fmt::Display for InvalidAddressSpaceError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InvalidId => {
                f.write_str("invalid address space identifier: expected integer value or 'unknown'")
            }
            Self::InvalidIdOverflow => f.write_str(
                "invalid address space identifier: value is too large, expected range is 0..=65535",
            ),
        }
    }
}

/// This type uniquely identifies the address space associated with a native
/// Miden pointer value
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AddressSpace {
    /// The address space is not known statically, but is available
    /// at runtime in the pointer metadata.
    ///
    /// This is also the type associated with user contexts in Miden,
    /// as it cannot be known statically how many such contexts will
    /// be used at runtime.
    #[default]
    Unknown,
    /// This address space corresponds to the root context in Miden
    ///
    /// The root context is the default context in the program entrypoint,
    /// and for use cases outside the typical smart contract usage, may be
    /// the only context in use at any given time.
    ///
    /// This address space corresponds to an address space identifier of 0.
    Root,
    /// This address space corresponds to a statically allocated separate
    /// memory region. This can be used to represent things in separate
    /// linear memory regions which are accessible simultaneously.
    ///
    /// Any non-zero identifier can be used for these address spaces.
    ///
    /// NOTE: It is up to the user to ensure that there are no conflicts
    /// between address space identifiers.
    Id(NonZeroU16),
}
impl fmt::Display for AddressSpace {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Unknown => f.write_str("?"),
            Self::Root => f.write_str("0"),
            Self::Id(id) => write!(f, "{id}"),
        }
    }
}
impl FromStr for AddressSpace {
    type Err = InvalidAddressSpaceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "unknown" => Ok(Self::Unknown),
            id => {
                use core::num::IntErrorKind;
                match NonZeroU16::from_str(id) {
                    Ok(id) => Ok(Self::Id(id)),
                    Err(err) => match err.kind() {
                        IntErrorKind::Zero => Ok(Self::Root),
                        IntErrorKind::PosOverflow | IntErrorKind::NegOverflow => {
                            Err(InvalidAddressSpaceError::InvalidIdOverflow)
                        }
                        _ => Err(InvalidAddressSpaceError::InvalidId),
                    },
                }
            }
        }
    }
}
