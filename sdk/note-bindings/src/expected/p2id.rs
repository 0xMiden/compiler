#[doc(hidden)]
mod __miden_note_bindings_f74ea5e7a6e77b2d {
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
                            stringify!(::miden_field::Felt),
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
                            stringify!(::miden_field::Word),
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
    ///Rust binding for WIT type `example:p2id-schema/note-storage@1.0.0.p2id-note`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct P2idNote {
        ///Value of the WIT `target-account-id` field.
        pub target_account_id: ::miden_note_bindings::__private::miden_protocol::account::AccountId,
    }
    impl P2idNote {
        /// The canonical fully-qualified WIT name for this type.
        pub const WIT_FQN: &'static str = "example:p2id-schema/note-storage@1.0.0.p2id-note";
    }
    impl __MidenNoteEncode for P2idNote {
        fn __write_note_felts(
            &self,
            writer: &mut ::miden_note_bindings::__private::miden_field_repr::FeltWriter<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<()> {
            self.target_account_id.__write_note_felts(writer)?;
            Ok(())
        }
    }
    impl __MidenNoteDecode for P2idNote {
        fn __read_note_felts(
            reader: &mut ::miden_note_bindings::__private::miden_field_repr::FeltReader<
                '_,
            >,
        ) -> ::miden_note_bindings::__private::miden_note_schema::Result<Self> {
            Ok(Self {
                target_account_id: <::miden_note_bindings::__private::miden_protocol::account::AccountId as __MidenNoteDecode>::__read_note_felts(
                    reader,
                )?,
            })
        }
    }
    #[doc(hidden)]
    const __MIDEN_NOTE_STORAGE_SCHEMA_WIT: &str = "\npackage example:p2id-schema@1.0.0;\n\nuse miden:base/core-types@1.0.0;\n\ninterface note-storage {\n    use core-types.{account-id};\n\n    record p2id-note {\n        target-account-id: account-id,\n    }\n\n    type storage = p2id-note;\n}\n\npackage miden:base@1.0.0 {\n    interface core-types {\n        record felt { inner: f32 }\n        record account-id { prefix: felt, suffix: felt }\n    }\n}\n";
    #[doc(hidden)]
    fn __miden_note_storage_schema() -> ::miden_note_bindings::__private::miden_note_schema::Result<
        ::miden_note_bindings::__private::miden_note_schema::NoteStorageSchema,
    > {
        ::miden_note_bindings::__private::miden_note_schema::NoteStorageSchema::from_wit_text(
            __MIDEN_NOTE_STORAGE_SCHEMA_WIT,
        )
    }
    impl P2idNote {
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
pub use __miden_note_bindings_f74ea5e7a6e77b2d::P2idNote;
