#[doc(hidden)]
mod __miden_bindings_485c0640bbf662e5 {
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
    #[doc(hidden)]
    const __MIDEN_ACCOUNT_STORAGE_SCHEMA_WIT: &str = "\npackage miden:storage-example-storage-schema;\n\nuse miden:base/core-types@1.0.0;\n\ninterface account-storage {\n    use core-types.{asset-amount, word};\n\n    /// An example account demonstrating storage value and map usage.\n    record storage {\n        /// Public key authorized to update the stored asset quantities.\n        %owner-public-key: word,\n        /// A map from asset id word to quantity held by the account.\n        %asset-qty-map: list<tuple<word, asset-amount>>,\n    }\n}\n\npackage miden:base@1.0.0 {\n    interface core-types {\n        record felt { inner: f32 }\n        record word { a: felt, b: felt, c: felt, d: felt }\n        record asset-amount { inner: felt }\n    }\n}\n";
    #[doc(hidden)]
    const __MIDEN_ACCOUNT_COMPONENT_METADATA: &[u8] = b"\x1Fstorage-example\x01\x0B1.0.0\x02\0-miden::storage_example::foo::owner_public_key\0\x01!owner public key\0\tword\0*miden::storage_example::foo::asset_qty_map\x01\x01%asset quantity map\0\0\tword\0\0\tfelt\0";
    /// Packs a typed slot value, map key, or map value into its storage word.
    #[doc(hidden)]
    fn __to_word<T: __MidenSchemaEncode>(
        value: &T,
    ) -> ::miden_bindings::__private::miden_account_storage_schema::Result<
        ::miden_bindings::__private::miden_account_storage_schema::Word,
    > {
        let mut felts = ::std::vec::Vec::new();
        value
            .__write_felts(
                &mut ::miden_bindings::__private::miden_field_repr::FeltWriter::new(
                    &mut felts,
                ),
            )?;
        ::miden_bindings::__private::miden_account_storage_schema::word_from_felts(
            &felts,
        )
    }
    /// Reads a typed slot value, map key, or map value from its storage word.
    #[doc(hidden)]
    fn __from_word<T: __MidenSchemaDecode>(
        word: ::miden_bindings::__private::miden_account_storage_schema::Word,
    ) -> ::miden_bindings::__private::miden_account_storage_schema::Result<T> {
        let felts = ::miden_bindings::__private::miden_account_storage_schema::felts_from_word(
            word,
        );
        let mut reader = ::miden_bindings::__private::miden_field_repr::FeltReader::new(
            &felts,
        );
        let value = T::__read_felts(&mut reader)?;
        if let Some(index) = felts[reader.pos()..]
            .iter()
            .position(|felt| {
                *felt != ::miden_bindings::__private::miden_field::Felt::ZERO
            })
        {
            return Err(
                ::miden_bindings::__private::miden_account_storage_schema::Error::new(
                    format!(
                        "the word has a nonzero padding felt at index {}", reader.pos() +
                        index
                    ),
                ),
            );
        }
        Ok(value)
    }
    /// The initial values of the account component's storage slots, one field per slot.
    #[derive(
        ::core::clone::Clone,
        ::core::fmt::Debug,
        ::core::cmp::PartialEq,
        ::core::cmp::Eq,
    )]
    pub struct Storage {
        ///Public key authorized to update the stored asset quantities.
        pub owner_public_key: ::miden_bindings::__private::miden_field::Word,
        ///A map from asset id word to quantity held by the account.
        pub asset_qty_map: ::std::vec::Vec<
            (
                ::miden_bindings::__private::miden_field::Word,
                ::miden_bindings::__private::miden_protocol::asset::AssetAmount,
            ),
        >,
    }
    impl Storage {
        ///The full protocol name of the `owner_public_key` slot.
        pub const OWNER_PUBLIC_KEY_SLOT: &'static str = "miden::storage_example::foo::owner_public_key";
        ///The full protocol name of the `asset_qty_map` slot.
        pub const ASSET_QTY_MAP_SLOT: &'static str = "miden::storage_example::foo::asset_qty_map";
        /// Returns the resolved schema with full slot names, which is parsed once for the
        /// whole process.
        pub fn schema() -> ::miden_bindings::__private::miden_account_storage_schema::Result<
            &'static ::miden_bindings::__private::miden_account_storage_schema::AccountStorageSchema,
        > {
            static __MIDEN_ACCOUNT_STORAGE_SCHEMA: ::std::sync::OnceLock<
                ::miden_bindings::__private::miden_account_storage_schema::Result<
                    ::miden_bindings::__private::miden_account_storage_schema::AccountStorageSchema,
                >,
            > = ::std::sync::OnceLock::new();
            __MIDEN_ACCOUNT_STORAGE_SCHEMA
                .get_or_init(|| {
                    let metadata = <::miden_bindings::__private::miden_protocol::account::AccountComponentMetadata as ::miden_bindings::__private::miden_protocol::utils::serde::Deserializable>::read_from_bytes(
                            __MIDEN_ACCOUNT_COMPONENT_METADATA,
                        )
                        .map_err(|error| {
                            ::miden_bindings::__private::miden_account_storage_schema::Error::new(
                                format!("failed to decode the component metadata: {error}"),
                            )
                        })?;
                    ::miden_bindings::__private::miden_account_storage_schema::AccountStorageSchema::from_wit_text_and_metadata(
                        __MIDEN_ACCOUNT_STORAGE_SCHEMA_WIT,
                        &metadata,
                    )
                })
                .as_ref()
                .map_err(::core::clone::Clone::clone)
        }
        /// Encodes every slot as the init storage data of the account component.
        pub fn to_init_storage_data(
            &self,
        ) -> ::miden_bindings::__private::miden_account_storage_schema::Result<
            ::miden_bindings::__private::miden_account_storage_schema::InitStorageData,
        > {
            let mut builder = Self::schema()?.init_builder();
            builder = builder
                .set_word("owner_public_key", __to_word(&self.owner_public_key)?)?;
            for (key, value) in &self.asset_qty_map {
                builder = builder
                    .set_entry_words(
                        "asset_qty_map",
                        __to_word(key)?,
                        __to_word(value)?,
                    )?;
            }
            builder.build()
        }
        ///Decodes a word of the `owner_public_key` slot; the felts after the value must be zero.
        pub fn decode_owner_public_key(
            word: ::miden_bindings::__private::miden_account_storage_schema::Word,
        ) -> ::miden_bindings::__private::miden_account_storage_schema::Result<
            ::miden_bindings::__private::miden_field::Word,
        > {
            __from_word(word)
        }
        ///Decodes the key and value words of one `asset_qty_map` map entry; the felts after each must be zero.
        pub fn decode_asset_qty_map_entry(
            key: ::miden_bindings::__private::miden_account_storage_schema::Word,
            value: ::miden_bindings::__private::miden_account_storage_schema::Word,
        ) -> ::miden_bindings::__private::miden_account_storage_schema::Result<
            (
                ::miden_bindings::__private::miden_field::Word,
                ::miden_bindings::__private::miden_protocol::asset::AssetAmount,
            ),
        > {
            Ok((__from_word(key)?, __from_word(value)?))
        }
    }
}
pub use __miden_bindings_485c0640bbf662e5::Storage;
