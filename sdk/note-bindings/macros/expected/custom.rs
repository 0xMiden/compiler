#[doc(hidden)]
mod __miden_note_bindings_a3280bdaca3ec21e {
    #[doc(hidden)]
    trait __MidenNoteEncode {
        fn __write_note_felts(
            &self,
            writer: &mut ::miden_note_bindings::__private::miden_field_repr::FeltWriter<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<()>;
    }
    #[doc(hidden)]
    trait __MidenNoteDecode: Sized {
        fn __read_note_felts(
            reader: &mut ::miden_note_bindings::__private::miden_field_repr::FeltReader<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<Self>;
    }
    impl __MidenNoteEncode for u64 {
        fn __write_note_felts(
            &self,
            writer: &mut ::miden_note_bindings::__private::miden_field_repr::FeltWriter<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<()> {
            ::miden_note_bindings::__private::miden_field_repr::ToFeltRepr::write_felt_repr(
                self,
                writer,
            );
            Ok(())
        }
    }
    impl __MidenNoteDecode for u64 {
        fn __read_note_felts(
            reader: &mut ::miden_note_bindings::__private::miden_field_repr::FeltReader<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<Self> {
            ::miden_note_bindings::__private::miden_field_repr::FromFeltRepr::from_felt_repr(
                    reader,
                )
                .map_err(|error| {
                    ::miden_note_bindings::__private::miden_note_schema::Error::new(
                        format!(
                            "failed to decode {} from note storage: {error}",
                            stringify!(u64),
                        ),
                    )
                })
        }
    }
    impl __MidenNoteEncode for u32 {
        fn __write_note_felts(
            &self,
            writer: &mut ::miden_note_bindings::__private::miden_field_repr::FeltWriter<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<()> {
            ::miden_note_bindings::__private::miden_field_repr::ToFeltRepr::write_felt_repr(
                self,
                writer,
            );
            Ok(())
        }
    }
    impl __MidenNoteDecode for u32 {
        fn __read_note_felts(
            reader: &mut ::miden_note_bindings::__private::miden_field_repr::FeltReader<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<Self> {
            ::miden_note_bindings::__private::miden_field_repr::FromFeltRepr::from_felt_repr(
                    reader,
                )
                .map_err(|error| {
                    ::miden_note_bindings::__private::miden_note_schema::Error::new(
                        format!(
                            "failed to decode {} from note storage: {error}",
                            stringify!(u32),
                        ),
                    )
                })
        }
    }
    impl __MidenNoteEncode for u8 {
        fn __write_note_felts(
            &self,
            writer: &mut ::miden_note_bindings::__private::miden_field_repr::FeltWriter<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<()> {
            ::miden_note_bindings::__private::miden_field_repr::ToFeltRepr::write_felt_repr(
                self,
                writer,
            );
            Ok(())
        }
    }
    impl __MidenNoteDecode for u8 {
        fn __read_note_felts(
            reader: &mut ::miden_note_bindings::__private::miden_field_repr::FeltReader<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<Self> {
            ::miden_note_bindings::__private::miden_field_repr::FromFeltRepr::from_felt_repr(
                    reader,
                )
                .map_err(|error| {
                    ::miden_note_bindings::__private::miden_note_schema::Error::new(
                        format!(
                            "failed to decode {} from note storage: {error}",
                            stringify!(u8),
                        ),
                    )
                })
        }
    }
    impl __MidenNoteEncode for bool {
        fn __write_note_felts(
            &self,
            writer: &mut ::miden_note_bindings::__private::miden_field_repr::FeltWriter<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<()> {
            ::miden_note_bindings::__private::miden_field_repr::ToFeltRepr::write_felt_repr(
                self,
                writer,
            );
            Ok(())
        }
    }
    impl __MidenNoteDecode for bool {
        fn __read_note_felts(
            reader: &mut ::miden_note_bindings::__private::miden_field_repr::FeltReader<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<Self> {
            ::miden_note_bindings::__private::miden_field_repr::FromFeltRepr::from_felt_repr(
                    reader,
                )
                .map_err(|error| {
                    ::miden_note_bindings::__private::miden_note_schema::Error::new(
                        format!(
                            "failed to decode {} from note storage: {error}",
                            stringify!(bool),
                        ),
                    )
                })
        }
    }
    impl __MidenNoteEncode for ::miden_note_bindings::__private::miden_field::Felt {
        fn __write_note_felts(
            &self,
            writer: &mut ::miden_note_bindings::__private::miden_field_repr::FeltWriter<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<()> {
            ::miden_note_bindings::__private::miden_field_repr::ToFeltRepr::write_felt_repr(
                self,
                writer,
            );
            Ok(())
        }
    }
    impl __MidenNoteDecode for ::miden_note_bindings::__private::miden_field::Felt {
        fn __read_note_felts(
            reader: &mut ::miden_note_bindings::__private::miden_field_repr::FeltReader<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<Self> {
            ::miden_note_bindings::__private::miden_field_repr::FromFeltRepr::from_felt_repr(
                    reader,
                )
                .map_err(|error| {
                    ::miden_note_bindings::__private::miden_note_schema::Error::new(
                        format!(
                            "failed to decode {} from note storage: {error}",
                            stringify!(::miden_note_bindings::__private::miden_field::Felt),
                        ),
                    )
                })
        }
    }
    impl __MidenNoteEncode for ::miden_note_bindings::__private::miden_field::Word {
        fn __write_note_felts(
            &self,
            writer: &mut ::miden_note_bindings::__private::miden_field_repr::FeltWriter<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<()> {
            ::miden_note_bindings::__private::miden_field_repr::ToFeltRepr::write_felt_repr(
                self,
                writer,
            );
            Ok(())
        }
    }
    impl __MidenNoteDecode for ::miden_note_bindings::__private::miden_field::Word {
        fn __read_note_felts(
            reader: &mut ::miden_note_bindings::__private::miden_field_repr::FeltReader<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<Self> {
            ::miden_note_bindings::__private::miden_field_repr::FromFeltRepr::from_felt_repr(
                    reader,
                )
                .map_err(|error| {
                    ::miden_note_bindings::__private::miden_note_schema::Error::new(
                        format!(
                            "failed to decode {} from note storage: {error}",
                            stringify!(::miden_note_bindings::__private::miden_field::Word),
                        ),
                    )
                })
        }
    }
    impl __MidenNoteEncode
    for ::miden_note_bindings::__private::miden_protocol::account::AccountId {
        fn __write_note_felts(
            &self,
            writer: &mut ::miden_note_bindings::__private::miden_field_repr::FeltWriter<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<()> {
            writer.write(self.prefix().as_felt());
            writer.write(self.suffix());
            Ok(())
        }
    }
    impl __MidenNoteDecode
    for ::miden_note_bindings::__private::miden_protocol::account::AccountId {
        fn __read_note_felts(
            reader: &mut ::miden_note_bindings::__private::miden_field_repr::FeltReader<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<Self> {
            let prefix = reader
                .read()
                .map_err(|error| {
                    ::miden_note_bindings::__private::miden_note_schema::Error::new(
                        format!("failed to decode account-id prefix: {error}"),
                    )
                })?;
            let suffix = reader
                .read()
                .map_err(|error| {
                    ::miden_note_bindings::__private::miden_note_schema::Error::new(
                        format!("failed to decode account-id suffix: {error}"),
                    )
                })?;
            ::miden_note_bindings::__private::miden_protocol::account::AccountId::try_from_elements(
                    suffix,
                    prefix,
                )
                .map_err(|error| {
                    ::miden_note_bindings::__private::miden_note_schema::Error::new(
                        format!("invalid account-id in note storage: {error}"),
                    )
                })
        }
    }
    impl __MidenNoteEncode
    for ::miden_note_bindings::__private::miden_protocol::asset::AssetAmount {
        fn __write_note_felts(
            &self,
            writer: &mut ::miden_note_bindings::__private::miden_field_repr::FeltWriter<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<()> {
            writer
                .write(::miden_note_bindings::__private::miden_field::Felt::from(*self));
            Ok(())
        }
    }
    impl __MidenNoteDecode
    for ::miden_note_bindings::__private::miden_protocol::asset::AssetAmount {
        fn __read_note_felts(
            reader: &mut ::miden_note_bindings::__private::miden_field_repr::FeltReader<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<Self> {
            let value = reader
                .read()
                .map_err(|error| {
                    ::miden_note_bindings::__private::miden_note_schema::Error::new(
                        format!("failed to decode asset-amount: {error}"),
                    )
                })?;
            ::miden_note_bindings::__private::miden_protocol::asset::AssetAmount::try_from(
                    value,
                )
                .map_err(|error| {
                    ::miden_note_bindings::__private::miden_note_schema::Error::new(
                        format!("invalid asset-amount in note storage: {error}"),
                    )
                })
        }
    }
    impl<T> __MidenNoteEncode for Option<T>
    where
        T: __MidenNoteEncode,
    {
        fn __write_note_felts(
            &self,
            writer: &mut ::miden_note_bindings::__private::miden_field_repr::FeltWriter<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<()> {
            match self {
                None => {
                    writer
                        .write(::miden_note_bindings::__private::miden_field::Felt::ZERO)
                }
                Some(value) => {
                    writer
                        .write(::miden_note_bindings::__private::miden_field::Felt::ONE);
                    value.__write_note_felts(writer)?;
                }
            }
            Ok(())
        }
    }
    impl<T> __MidenNoteDecode for Option<T>
    where
        T: __MidenNoteDecode,
    {
        fn __read_note_felts(
            reader: &mut ::miden_note_bindings::__private::miden_field_repr::FeltReader<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<Self> {
            let tag = reader
                .read()
                .map_err(|error| {
                    ::miden_note_bindings::__private::miden_note_schema::Error::new(
                        format!("failed to decode option tag: {error}"),
                    )
                })?;
            match tag.as_canonical_u64() {
                0 => Ok(None),
                1 => Ok(Some(T::__read_note_felts(reader)?)),
                tag => {
                    Err(
                        ::miden_note_bindings::__private::miden_note_schema::Error::new(
                            format!("invalid option tag {tag}; expected 0 or 1"),
                        ),
                    )
                }
            }
        }
    }
    ///Rust binding for WIT type `example:dex-schema/note-storage@1.0.0.dex-note`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct DexNote {
        ///Value of the WIT `target` field.
        pub target: ::miden_note_bindings::__private::miden_protocol::account::AccountId,
        ///Value of the WIT `kind` field.
        pub kind: OrderKind,
    }
    impl DexNote {
        /// The canonical fully-qualified WIT name for this type.
        pub const WIT_FQN: &'static str = "example:dex-schema/note-storage@1.0.0.dex-note";
    }
    impl __MidenNoteEncode for DexNote {
        fn __write_note_felts(
            &self,
            writer: &mut ::miden_note_bindings::__private::miden_field_repr::FeltWriter<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<()> {
            self.target.__write_note_felts(writer)?;
            self.kind.__write_note_felts(writer)?;
            Ok(())
        }
    }
    impl __MidenNoteDecode for DexNote {
        fn __read_note_felts(
            reader: &mut ::miden_note_bindings::__private::miden_field_repr::FeltReader<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<Self> {
            Ok(Self {
                target: <::miden_note_bindings::__private::miden_protocol::account::AccountId as __MidenNoteDecode>::__read_note_felts(
                    reader,
                )?,
                kind: <OrderKind as __MidenNoteDecode>::__read_note_felts(reader)?,
            })
        }
    }
    ///Selects order execution.
    #[derive(
        Clone,
        Debug,
        PartialEq,
        Eq,
        ::miden_note_bindings::__private::miden_field_repr::ToFeltRepr,
        ::miden_note_bindings::__private::miden_field_repr::FromFeltRepr,
    )]
    #[felt_repr(crate_path = "::miden_note_bindings::__private::miden_field_repr")]
    pub enum OrderKind {
        ///WIT `market` case.
        Market,
        ///WIT `limit` case.
        Limit(LimitPrice),
    }
    impl OrderKind {
        /// The canonical fully-qualified WIT name for this type.
        pub const WIT_FQN: &'static str = "example:dex-schema/note-storage@1.0.0.order-kind";
    }
    impl __MidenNoteEncode for OrderKind {
        fn __write_note_felts(
            &self,
            writer: &mut ::miden_note_bindings::__private::miden_field_repr::FeltWriter<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<()> {
            match self {
                Self::Market => {
                    writer
                        .write(
                            ::miden_note_bindings::__private::miden_field::Felt::from_u32(
                                0u32,
                            ),
                        );
                }
                Self::Limit(value) => {
                    writer
                        .write(
                            ::miden_note_bindings::__private::miden_field::Felt::from_u32(
                                1u32,
                            ),
                        );
                    value.__write_note_felts(writer)?;
                }
            }
            Ok(())
        }
    }
    impl __MidenNoteDecode for OrderKind {
        fn __read_note_felts(
            reader: &mut ::miden_note_bindings::__private::miden_field_repr::FeltReader<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<Self> {
            let tag = reader
                .read_u32()
                .map_err(|error| {
                    ::miden_note_bindings::__private::miden_note_schema::Error::new(
                        format!(
                            "failed to decode {} tag: {error}", stringify!(OrderKind),
                        ),
                    )
                })?;
            match tag {
                0u32 => Ok(Self::Market),
                1u32 => {
                    Ok(
                        Self::Limit(
                            <LimitPrice as __MidenNoteDecode>::__read_note_felts(reader)?,
                        ),
                    )
                }
                tag => {
                    Err(
                        ::miden_note_bindings::__private::miden_note_schema::Error::new(
                            format!(
                                "invalid {} tag {tag}; expected a declaration ordinal below {}",
                                stringify!(OrderKind), 2usize,
                            ),
                        ),
                    )
                }
            }
        }
    }
    ///A ratio used as an order limit.
    #[derive(
        Clone,
        Debug,
        PartialEq,
        Eq,
        ::miden_note_bindings::__private::miden_field_repr::ToFeltRepr,
        ::miden_note_bindings::__private::miden_field_repr::FromFeltRepr,
    )]
    #[felt_repr(crate_path = "::miden_note_bindings::__private::miden_field_repr")]
    pub struct LimitPrice {
        ///Value of the WIT `numerator` field.
        pub numerator: u64,
        ///Value of the WIT `denominator` field.
        pub denominator: u64,
    }
    impl LimitPrice {
        /// The canonical fully-qualified WIT name for this type.
        pub const WIT_FQN: &'static str = "example:dex-schema/note-storage@1.0.0.limit-price";
    }
    impl __MidenNoteEncode for LimitPrice {
        fn __write_note_felts(
            &self,
            writer: &mut ::miden_note_bindings::__private::miden_field_repr::FeltWriter<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<()> {
            self.numerator.__write_note_felts(writer)?;
            self.denominator.__write_note_felts(writer)?;
            Ok(())
        }
    }
    impl __MidenNoteDecode for LimitPrice {
        fn __read_note_felts(
            reader: &mut ::miden_note_bindings::__private::miden_field_repr::FeltReader<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<Self> {
            Ok(Self {
                numerator: <u64 as __MidenNoteDecode>::__read_note_felts(reader)?,
                denominator: <u64 as __MidenNoteDecode>::__read_note_felts(reader)?,
            })
        }
    }
    #[doc(hidden)]
    const __MIDEN_NOTE_STORAGE_SCHEMA_WIT: &str = "\npackage example:dex-schema@1.0.0;\n\nuse miden:base/core-types@1.0.0;\n\ninterface note-storage {\n    use core-types.{account-id};\n\n    /// A ratio used as an order limit.\n    record limit-price {\n        numerator: u64,\n        denominator: u64,\n    }\n\n    /// Selects order execution.\n    variant order-kind {\n        market,\n        limit(limit-price),\n    }\n\n    record dex-note {\n        target: account-id,\n        kind: order-kind,\n    }\n\n    type storage = dex-note;\n}\n\npackage miden:base@1.0.0 {\n    interface core-types {\n        record felt { inner: f32 }\n        record account-id { prefix: felt, suffix: felt }\n    }\n}\n";
    #[doc(hidden)]
    fn __miden_note_storage_schema() -> ::miden_note_bindings::__private::miden_note_schema::Result<
        ::miden_note_bindings::__private::miden_note_schema::NoteStorageSchema,
    > {
        ::miden_note_bindings::__private::miden_note_schema::NoteStorageSchema::from_wit_text(
            __MIDEN_NOTE_STORAGE_SCHEMA_WIT,
        )
    }
    impl DexNote {
        /// Encodes this typed value as note storage in WIT declaration order.
        pub fn to_note_storage(
            &self,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<
            ::miden_note_bindings::__private::miden_note_schema::NoteStorage,
        > {
            let mut felts = Vec::new();
            self.__write_note_felts(
                &mut ::miden_note_bindings::__private::miden_field_repr::FeltWriter::new(
                    &mut felts,
                ),
            )?;
            ::miden_note_bindings::__private::miden_note_schema::NoteStorage::new(felts)
                .map_err(|error| {
                    ::miden_note_bindings::__private::miden_note_schema::Error::new(
                        format!("failed to create note storage: {error}"),
                    )
                })
        }
        /// Decodes this typed value from complete note storage.
        pub fn from_note_storage(
            storage: &::miden_note_bindings::__private::miden_note_schema::NoteStorage,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<Self> {
            let mut reader = ::miden_note_bindings::__private::miden_field_repr::FeltReader::new(
                storage.items(),
            );
            let value = Self::__read_note_felts(&mut reader)?;
            reader
                .ensure_eof()
                .map_err(|error| {
                    ::miden_note_bindings::__private::miden_note_schema::Error::new(
                        format!("note storage has trailing data: {error}"),
                    )
                })?;
            Ok(value)
        }
        /// Builds a typed value with a caller-provided codec registry.
        pub fn from_str_values_with(
            values: &::std::collections::BTreeMap<String, String>,
            codecs: &::miden_note_bindings::__private::miden_note_schema::CodecRegistry,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<Self> {
            let schema = __miden_note_storage_schema()?;
            let mut builder = schema.builder_with_registry(codecs);
            for (path, value) in values {
                builder = builder.set(path, value)?;
            }
            let storage = builder.build()?;
            Self::from_note_storage(&storage)
        }
        /// Builds a typed value with the standard codec registry.
        pub fn from_str_values(
            values: &::std::collections::BTreeMap<String, String>,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<Self> {
            let codecs = ::miden_note_bindings::__private::miden_note_schema::CodecRegistry::with_standard_codecs();
            Self::from_str_values_with(values, &codecs)
        }
        /// Validates this value with structural rules and caller-provided codecs.
        pub fn validate_with(
            &self,
            codecs: &::miden_note_bindings::__private::miden_note_schema::CodecRegistry,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<()> {
            let storage = self.to_note_storage()?;
            __miden_note_storage_schema()?.decode_with_registry(&storage, codecs)?;
            Ok(())
        }
        /// Validates this value with the standard codec registry.
        pub fn validate(
            &self,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<()> {
            let codecs = ::miden_note_bindings::__private::miden_note_schema::CodecRegistry::with_standard_codecs();
            self.validate_with(&codecs)
        }
        /// Displays this value with caller-provided codecs and structural fallbacks.
        pub fn display_with(
            &self,
            codecs: &::miden_note_bindings::__private::miden_note_schema::CodecRegistry,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<String> {
            let storage = self.to_note_storage()?;
            let decoded = __miden_note_storage_schema()?
                .decode_with_registry(&storage, codecs)?;
            Ok(decoded.to_string())
        }
        /// Displays this value with standard codecs and structural fallbacks.
        pub fn display(
            &self,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<String> {
            let codecs = ::miden_note_bindings::__private::miden_note_schema::CodecRegistry::with_standard_codecs();
            self.display_with(&codecs)
        }
    }
}
pub use __miden_note_bindings_a3280bdaca3ec21e::{DexNote, OrderKind, LimitPrice};
