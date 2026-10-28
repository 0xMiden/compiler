#[doc(hidden)]
mod __miden_bindings_dbbbaf8198e47b5a {
    #[doc(hidden)]
    trait __MidenSchemaEncode {
        fn __write_felts(
            &self,
            writer: &mut ::miden_bindings::__private::miden_field_repr::FeltWriter<'_>,
        ) -> ::miden_bindings::__private::miden_schema::Result<()>;
    }
    #[doc(hidden)]
    trait __MidenSchemaDecode: ::core::marker::Sized {
        fn __read_felts(
            reader: &mut ::miden_bindings::__private::miden_field_repr::FeltReader<'_>,
        ) -> ::miden_bindings::__private::miden_schema::Result<Self>;
    }
    impl __MidenSchemaEncode for u64 {
        fn __write_felts(
            &self,
            writer: &mut ::miden_bindings::__private::miden_field_repr::FeltWriter<'_>,
        ) -> ::miden_bindings::__private::miden_schema::Result<()> {
            ::miden_bindings::__private::miden_field_repr::ToFeltRepr::write_felt_repr(
                self,
                writer,
            );
            Ok(())
        }
    }
    impl __MidenSchemaDecode for u64 {
        fn __read_felts(
            reader: &mut ::miden_bindings::__private::miden_field_repr::FeltReader<'_>,
        ) -> ::miden_bindings::__private::miden_schema::Result<Self> {
            ::miden_bindings::__private::miden_field_repr::FromFeltRepr::from_felt_repr(
                    reader,
                )
                .map_err(|error| {
                    ::miden_bindings::__private::miden_schema::Error::new(
                        format!(
                            "failed to decode {} from its felt encoding: {error}",
                            stringify!(u64),
                        ),
                    )
                })
        }
    }
    impl __MidenSchemaEncode for u32 {
        fn __write_felts(
            &self,
            writer: &mut ::miden_bindings::__private::miden_field_repr::FeltWriter<'_>,
        ) -> ::miden_bindings::__private::miden_schema::Result<()> {
            ::miden_bindings::__private::miden_field_repr::ToFeltRepr::write_felt_repr(
                self,
                writer,
            );
            Ok(())
        }
    }
    impl __MidenSchemaDecode for u32 {
        fn __read_felts(
            reader: &mut ::miden_bindings::__private::miden_field_repr::FeltReader<'_>,
        ) -> ::miden_bindings::__private::miden_schema::Result<Self> {
            ::miden_bindings::__private::miden_field_repr::FromFeltRepr::from_felt_repr(
                    reader,
                )
                .map_err(|error| {
                    ::miden_bindings::__private::miden_schema::Error::new(
                        format!(
                            "failed to decode {} from its felt encoding: {error}",
                            stringify!(u32),
                        ),
                    )
                })
        }
    }
    impl __MidenSchemaEncode for u8 {
        fn __write_felts(
            &self,
            writer: &mut ::miden_bindings::__private::miden_field_repr::FeltWriter<'_>,
        ) -> ::miden_bindings::__private::miden_schema::Result<()> {
            ::miden_bindings::__private::miden_field_repr::ToFeltRepr::write_felt_repr(
                self,
                writer,
            );
            Ok(())
        }
    }
    impl __MidenSchemaDecode for u8 {
        fn __read_felts(
            reader: &mut ::miden_bindings::__private::miden_field_repr::FeltReader<'_>,
        ) -> ::miden_bindings::__private::miden_schema::Result<Self> {
            ::miden_bindings::__private::miden_field_repr::FromFeltRepr::from_felt_repr(
                    reader,
                )
                .map_err(|error| {
                    ::miden_bindings::__private::miden_schema::Error::new(
                        format!(
                            "failed to decode {} from its felt encoding: {error}",
                            stringify!(u8),
                        ),
                    )
                })
        }
    }
    impl __MidenSchemaEncode for bool {
        fn __write_felts(
            &self,
            writer: &mut ::miden_bindings::__private::miden_field_repr::FeltWriter<'_>,
        ) -> ::miden_bindings::__private::miden_schema::Result<()> {
            ::miden_bindings::__private::miden_field_repr::ToFeltRepr::write_felt_repr(
                self,
                writer,
            );
            Ok(())
        }
    }
    impl __MidenSchemaDecode for bool {
        fn __read_felts(
            reader: &mut ::miden_bindings::__private::miden_field_repr::FeltReader<'_>,
        ) -> ::miden_bindings::__private::miden_schema::Result<Self> {
            ::miden_bindings::__private::miden_field_repr::FromFeltRepr::from_felt_repr(
                    reader,
                )
                .map_err(|error| {
                    ::miden_bindings::__private::miden_schema::Error::new(
                        format!(
                            "failed to decode {} from its felt encoding: {error}",
                            stringify!(bool),
                        ),
                    )
                })
        }
    }
    impl __MidenSchemaEncode for ::miden_bindings::__private::miden_field::Felt {
        fn __write_felts(
            &self,
            writer: &mut ::miden_bindings::__private::miden_field_repr::FeltWriter<'_>,
        ) -> ::miden_bindings::__private::miden_schema::Result<()> {
            ::miden_bindings::__private::miden_field_repr::ToFeltRepr::write_felt_repr(
                self,
                writer,
            );
            Ok(())
        }
    }
    impl __MidenSchemaDecode for ::miden_bindings::__private::miden_field::Felt {
        fn __read_felts(
            reader: &mut ::miden_bindings::__private::miden_field_repr::FeltReader<'_>,
        ) -> ::miden_bindings::__private::miden_schema::Result<Self> {
            ::miden_bindings::__private::miden_field_repr::FromFeltRepr::from_felt_repr(
                    reader,
                )
                .map_err(|error| {
                    ::miden_bindings::__private::miden_schema::Error::new(
                        format!(
                            "failed to decode {} from its felt encoding: {error}",
                            stringify!(::miden_bindings::__private::miden_field::Felt),
                        ),
                    )
                })
        }
    }
    impl __MidenSchemaEncode for ::miden_bindings::__private::miden_field::Word {
        fn __write_felts(
            &self,
            writer: &mut ::miden_bindings::__private::miden_field_repr::FeltWriter<'_>,
        ) -> ::miden_bindings::__private::miden_schema::Result<()> {
            ::miden_bindings::__private::miden_field_repr::ToFeltRepr::write_felt_repr(
                self,
                writer,
            );
            Ok(())
        }
    }
    impl __MidenSchemaDecode for ::miden_bindings::__private::miden_field::Word {
        fn __read_felts(
            reader: &mut ::miden_bindings::__private::miden_field_repr::FeltReader<'_>,
        ) -> ::miden_bindings::__private::miden_schema::Result<Self> {
            ::miden_bindings::__private::miden_field_repr::FromFeltRepr::from_felt_repr(
                    reader,
                )
                .map_err(|error| {
                    ::miden_bindings::__private::miden_schema::Error::new(
                        format!(
                            "failed to decode {} from its felt encoding: {error}",
                            stringify!(::miden_bindings::__private::miden_field::Word),
                        ),
                    )
                })
        }
    }
    impl __MidenSchemaEncode for u16 {
        fn __write_felts(
            &self,
            writer: &mut ::miden_bindings::__private::miden_field_repr::FeltWriter<'_>,
        ) -> ::miden_bindings::__private::miden_schema::Result<()> {
            ::miden_bindings::__private::miden_field_repr::ToFeltRepr::write_felt_repr(
                &u32::from(*self),
                writer,
            );
            Ok(())
        }
    }
    impl __MidenSchemaDecode for u16 {
        fn __read_felts(
            reader: &mut ::miden_bindings::__private::miden_field_repr::FeltReader<'_>,
        ) -> ::miden_bindings::__private::miden_schema::Result<Self> {
            let value = reader
                .read_u32()
                .map_err(|error| {
                    ::miden_bindings::__private::miden_schema::Error::new(
                        format!("failed to decode u16 from its felt encoding: {error}"),
                    )
                })?;
            u16::try_from(value)
                .map_err(|_| {
                    ::miden_bindings::__private::miden_schema::Error::new(
                        format!(
                            "failed to decode u16 from its felt encoding: value {value} does not fit"
                        ),
                    )
                })
        }
    }
    impl __MidenSchemaEncode
    for ::miden_bindings::__private::miden_protocol::account::AccountId {
        fn __write_felts(
            &self,
            writer: &mut ::miden_bindings::__private::miden_field_repr::FeltWriter<'_>,
        ) -> ::miden_bindings::__private::miden_schema::Result<()> {
            writer.write(self.prefix().as_felt());
            writer.write(self.suffix());
            Ok(())
        }
    }
    impl __MidenSchemaDecode
    for ::miden_bindings::__private::miden_protocol::account::AccountId {
        fn __read_felts(
            reader: &mut ::miden_bindings::__private::miden_field_repr::FeltReader<'_>,
        ) -> ::miden_bindings::__private::miden_schema::Result<Self> {
            let prefix = reader
                .read()
                .map_err(|error| {
                    ::miden_bindings::__private::miden_schema::Error::new(
                        format!("failed to decode account-id prefix: {error}"),
                    )
                })?;
            let suffix = reader
                .read()
                .map_err(|error| {
                    ::miden_bindings::__private::miden_schema::Error::new(
                        format!("failed to decode account-id suffix: {error}"),
                    )
                })?;
            ::miden_bindings::__private::miden_protocol::account::AccountId::try_from_elements(
                    suffix,
                    prefix,
                )
                .map_err(|error| {
                    ::miden_bindings::__private::miden_schema::Error::new(
                        format!("invalid account-id: {error}"),
                    )
                })
        }
    }
    impl __MidenSchemaEncode
    for ::miden_bindings::__private::miden_protocol::asset::AssetAmount {
        fn __write_felts(
            &self,
            writer: &mut ::miden_bindings::__private::miden_field_repr::FeltWriter<'_>,
        ) -> ::miden_bindings::__private::miden_schema::Result<()> {
            writer.write(::miden_bindings::__private::miden_field::Felt::from(*self));
            Ok(())
        }
    }
    impl __MidenSchemaDecode
    for ::miden_bindings::__private::miden_protocol::asset::AssetAmount {
        fn __read_felts(
            reader: &mut ::miden_bindings::__private::miden_field_repr::FeltReader<'_>,
        ) -> ::miden_bindings::__private::miden_schema::Result<Self> {
            let value = reader
                .read()
                .map_err(|error| {
                    ::miden_bindings::__private::miden_schema::Error::new(
                        format!("failed to decode asset-amount: {error}"),
                    )
                })?;
            ::miden_bindings::__private::miden_protocol::asset::AssetAmount::try_from(
                    value,
                )
                .map_err(|error| {
                    ::miden_bindings::__private::miden_schema::Error::new(
                        format!("invalid asset-amount: {error}"),
                    )
                })
        }
    }
    impl<T> __MidenSchemaEncode for ::core::option::Option<T>
    where
        T: __MidenSchemaEncode,
    {
        fn __write_felts(
            &self,
            writer: &mut ::miden_bindings::__private::miden_field_repr::FeltWriter<'_>,
        ) -> ::miden_bindings::__private::miden_schema::Result<()> {
            match self {
                None => {
                    writer.write(::miden_bindings::__private::miden_field::Felt::ZERO)
                }
                Some(value) => {
                    writer.write(::miden_bindings::__private::miden_field::Felt::ONE);
                    value.__write_felts(writer)?;
                }
            }
            Ok(())
        }
    }
    impl<T> __MidenSchemaDecode for ::core::option::Option<T>
    where
        T: __MidenSchemaDecode,
    {
        fn __read_felts(
            reader: &mut ::miden_bindings::__private::miden_field_repr::FeltReader<'_>,
        ) -> ::miden_bindings::__private::miden_schema::Result<Self> {
            let tag = reader
                .read()
                .map_err(|error| {
                    ::miden_bindings::__private::miden_schema::Error::new(
                        format!("failed to decode option tag: {error}"),
                    )
                })?;
            match tag.as_canonical_u64() {
                0 => Ok(None),
                1 => Ok(Some(T::__read_felts(reader)?)),
                tag => {
                    Err(
                        ::miden_bindings::__private::miden_schema::Error::new(
                            format!("invalid option tag {tag}; expected 0 or 1"),
                        ),
                    )
                }
            }
        }
    }
    ///Rust binding for WIT type `example:p2id-schema/note-storage.p2id-note`.
    #[derive(
        ::core::clone::Clone,
        ::core::fmt::Debug,
        ::core::cmp::PartialEq,
        ::core::cmp::Eq,
    )]
    pub struct P2idNote {
        ///Value of the WIT `target-account-id` field.
        pub target_account_id: ::miden_bindings::__private::miden_protocol::account::AccountId,
    }
    impl P2idNote {
        /// The canonical fully-qualified WIT name for this type.
        pub const WIT_FQN: &'static str = "example:p2id-schema/note-storage.p2id-note";
    }
    impl __MidenSchemaEncode for P2idNote {
        fn __write_felts(
            &self,
            writer: &mut ::miden_bindings::__private::miden_field_repr::FeltWriter<'_>,
        ) -> ::miden_bindings::__private::miden_schema::Result<()> {
            self.target_account_id.__write_felts(writer)?;
            Ok(())
        }
    }
    impl __MidenSchemaDecode for P2idNote {
        fn __read_felts(
            reader: &mut ::miden_bindings::__private::miden_field_repr::FeltReader<'_>,
        ) -> ::miden_bindings::__private::miden_schema::Result<Self> {
            Ok(Self {
                target_account_id: <::miden_bindings::__private::miden_protocol::account::AccountId as __MidenSchemaDecode>::__read_felts(
                    reader,
                )?,
            })
        }
    }
    #[doc(hidden)]
    const __MIDEN_NOTE_STORAGE_SCHEMA_WIT: &str = "\npackage example:p2id-schema;\n\nuse miden:base/core-types@1.0.0;\n\ninterface note-storage {\n    use core-types.{account-id};\n\n    record p2id-note {\n        target-account-id: account-id,\n    }\n\n    type storage = p2id-note;\n}\n\npackage miden:base@1.0.0 {\n    interface core-types {\n        record felt { inner: f32 }\n        record account-id { prefix: felt, suffix: felt }\n    }\n}\n";
    /// Returns the resolved schema, which is parsed once for the whole process.
    #[doc(hidden)]
    fn __miden_note_storage_schema() -> ::miden_bindings::__private::miden_note_schema::Result<
        &'static ::miden_bindings::__private::miden_note_schema::NoteStorageSchema,
    > {
        static __MIDEN_NOTE_STORAGE_SCHEMA: ::std::sync::OnceLock<
            ::miden_bindings::__private::miden_note_schema::Result<
                ::miden_bindings::__private::miden_note_schema::NoteStorageSchema,
            >,
        > = ::std::sync::OnceLock::new();
        __MIDEN_NOTE_STORAGE_SCHEMA
            .get_or_init(|| {
                ::miden_bindings::__private::miden_note_schema::NoteStorageSchema::from_wit_text(
                    __MIDEN_NOTE_STORAGE_SCHEMA_WIT,
                )
            })
            .as_ref()
            .map_err(::core::clone::Clone::clone)
    }
    impl P2idNote {
        /// Encodes this typed value as note storage in WIT declaration order.
        pub fn to_note_storage(
            &self,
        ) -> ::miden_bindings::__private::miden_note_schema::Result<
            ::miden_bindings::__private::miden_note_schema::NoteStorage,
        > {
            let mut felts = ::std::vec::Vec::new();
            self.__write_felts(
                &mut ::miden_bindings::__private::miden_field_repr::FeltWriter::new(
                    &mut felts,
                ),
            )?;
            ::miden_bindings::__private::miden_note_schema::NoteStorage::new(felts)
                .map_err(|error| {
                    ::miden_bindings::__private::miden_note_schema::Error::new(
                        format!("failed to create note storage: {error}"),
                    )
                })
        }
        /// Decodes this typed value from complete note storage.
        pub fn from_note_storage(
            storage: &::miden_bindings::__private::miden_note_schema::NoteStorage,
        ) -> ::miden_bindings::__private::miden_note_schema::Result<Self> {
            let mut reader = ::miden_bindings::__private::miden_field_repr::FeltReader::new(
                storage.items(),
            );
            let value = Self::__read_felts(&mut reader)?;
            reader
                .ensure_eof()
                .map_err(|error| {
                    ::miden_bindings::__private::miden_note_schema::Error::new(
                        format!("note storage has trailing data: {error}"),
                    )
                })?;
            Ok(value)
        }
        /// Builds a typed value with a caller-provided codec registry.
        pub fn from_str_values_with(
            values: &::std::collections::BTreeMap<
                ::std::string::String,
                ::std::string::String,
            >,
            codecs: &::miden_bindings::__private::miden_note_schema::CodecRegistry,
        ) -> ::miden_bindings::__private::miden_note_schema::Result<Self> {
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
            values: &::std::collections::BTreeMap<
                ::std::string::String,
                ::std::string::String,
            >,
        ) -> ::miden_bindings::__private::miden_note_schema::Result<Self> {
            let codecs = ::miden_bindings::__private::miden_note_schema::CodecRegistry::with_standard_codecs();
            Self::from_str_values_with(values, &codecs)
        }
        /// Validates this value with structural rules and caller-provided codecs.
        pub fn validate_with(
            &self,
            codecs: &::miden_bindings::__private::miden_note_schema::CodecRegistry,
        ) -> ::miden_bindings::__private::miden_note_schema::Result<()> {
            let storage = self.to_note_storage()?;
            __miden_note_storage_schema()?.decode_with_registry(&storage, codecs)?;
            Ok(())
        }
        /// Validates this value with the standard codec registry.
        pub fn validate(
            &self,
        ) -> ::miden_bindings::__private::miden_note_schema::Result<()> {
            let codecs = ::miden_bindings::__private::miden_note_schema::CodecRegistry::with_standard_codecs();
            self.validate_with(&codecs)
        }
        /// Displays this value with caller-provided codecs and structural fallbacks.
        pub fn display_with(
            &self,
            codecs: &::miden_bindings::__private::miden_note_schema::CodecRegistry,
        ) -> ::miden_bindings::__private::miden_note_schema::Result<
            ::std::string::String,
        > {
            let storage = self.to_note_storage()?;
            let decoded = __miden_note_storage_schema()?
                .decode_with_registry(&storage, codecs)?;
            Ok(decoded.to_string())
        }
        /// Displays this value with standard codecs and structural fallbacks.
        pub fn display(
            &self,
        ) -> ::miden_bindings::__private::miden_note_schema::Result<
            ::std::string::String,
        > {
            let codecs = ::miden_bindings::__private::miden_note_schema::CodecRegistry::with_standard_codecs();
            self.display_with(&codecs)
        }
    }
}
pub use __miden_bindings_dbbbaf8198e47b5a::P2idNote;
