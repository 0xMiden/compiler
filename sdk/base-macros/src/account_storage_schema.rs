//! Account storage schema generation for `#[component_storage]` structs.

use std::collections::{BTreeSet, HashMap};

use midenc_frontend_wasm_metadata::WASM_ACCOUNT_STORAGE_SCHEMA_CUSTOM_SECTION_NAME;
use proc_macro2::TokenStream as TokenStream2;
use syn::{ItemStruct, Type, ext::IdentExt, spanned::Spanned};

use crate::{
    namespace::ComponentNamespace,
    schema_render::{
        PLACEHOLDER_COMPONENT_PACKAGE, RenderedSchema, SchemaKind, emit_schema,
        map_schema_field_type, referenced_custom_types, render_schema_wit,
        required_core_type_imports, validate_schema_definition, validate_schema_type_ref,
    },
    types::{
        ExportedField, ExportedTypeDef, ExportedTypeKind, TypeRef, doc_comments,
        registered_export_types,
    },
    wit_names::rust_ident_to_wit_name,
};

/// WIT name of the record the schema declares for the storage root.
const ACCOUNT_STORAGE_ROOT: &str = "storage";
/// The account storage schema kind.
const ACCOUNT_STORAGE_SCHEMA: SchemaKind = SchemaKind {
    macro_name: "#[component_storage]",
    subject: "`#[component_storage]`",
    schema_name: "account storage schema",
    interface: "account-storage",
    source_name: "account-storage-schema.wit",
    link_section: WASM_ACCOUNT_STORAGE_SCHEMA_CUSTOM_SECTION_NAME,
    supported_types: "`Word`, `Felt`, `u64`, `u32`, `u16`, `u8`, `bool`, SDK core-type records, \
                      `#[export_type]` records or enums, and `Option<T>` over a supported type",
    primitives: &["u64", "u32", "u16", "u8", "bool"],
};

/// One storage slot of a `#[component_storage]` struct, as the storage schema describes it.
pub(crate) struct StorageSlotSpec {
    /// The struct field of the slot.
    pub(crate) field_ident: syn::Ident,
    /// Doc comment lines of the field; the `description` argument when the field has none.
    pub(crate) docs: Vec<String>,
    /// Whether the `#[storage(...)]` attribute of the slot carries a `type` argument.
    pub(crate) has_type_override: bool,
    /// What the slot holds.
    pub(crate) kind: StorageSlotKind,
}

/// What a storage slot holds.
pub(crate) enum StorageSlotKind {
    /// A `StorageValue<ty>` slot.
    Value {
        /// The value type.
        ty: Box<Type>,
    },
    /// A `StorageMap<key, value>` slot.
    Map {
        /// The key type.
        key: Box<Type>,
        /// The value type.
        value: Box<Type>,
    },
    /// A `StorageValue<StoredProcedure<..>>` slot, which holds a procedure root word.
    StoredProcedure,
}

/// Generates the account storage schema custom-section static for a `#[component_storage]`
/// struct with the storage slots `slots`, or `None` when the struct has no slots.
pub(crate) fn expand_account_storage_schema(
    item_struct: &ItemStruct,
    namespace: Option<&ComponentNamespace>,
    slots: &[StorageSlotSpec],
) -> Result<Option<TokenStream2>, syn::Error> {
    // An empty WIT record is invalid, so a struct without slots gets no schema.
    if slots.is_empty() {
        return Ok(None);
    }
    let component_package = namespace
        .map_or_else(|| PLACEHOLDER_COMPONENT_PACKAGE.to_owned(), ComponentNamespace::wit_package);
    let registry = registered_export_types();
    let rendered =
        render_account_storage_schema_model(item_struct, slots, &component_package, &registry)?;
    emit_schema(
        &ACCOUNT_STORAGE_SCHEMA,
        rendered,
        &registry,
        "__MIDEN_ACCOUNT_STORAGE_SCHEMA_BYTES",
    )
    .map(Some)
}

/// Renders an account storage schema against one export-type registry snapshot and retains its
/// source-definition context.
fn render_account_storage_schema_model(
    item_struct: &ItemStruct,
    slots: &[StorageSlotSpec],
    component_package: &str,
    registry: &[ExportedTypeDef],
) -> Result<RenderedSchema, syn::Error> {
    let span = item_struct.ident.span();
    let registry_by_rust_name = registry
        .iter()
        .cloned()
        .map(|definition| (definition.rust_name.clone(), definition))
        .collect();
    let root = storage_root_type(item_struct, slots, &registry_by_rust_name)?;
    let custom_types = referenced_custom_types(&ACCOUNT_STORAGE_SCHEMA, &root, registry, span)?;
    for definition in &custom_types {
        validate_schema_definition(&ACCOUNT_STORAGE_SCHEMA, definition, span)?;
    }
    let core_imports = required_core_type_imports(&root, &custom_types);
    reject_storage_type_name_collisions(item_struct, &custom_types, &core_imports)?;
    let source = render_schema_wit(
        &ACCOUNT_STORAGE_SCHEMA,
        &storage_schema_package_name(component_package),
        &core_imports,
        &custom_types,
        &root,
        None,
    )?;

    let mut definitions = custom_types;
    definitions.push(root);
    Ok(RenderedSchema {
        span,
        source,
        definitions,
    })
}

/// Builds the `storage` root record: one field per slot, in declaration order.
fn storage_root_type(
    item_struct: &ItemStruct,
    slots: &[StorageSlotSpec],
    registry: &HashMap<String, ExportedTypeDef>,
) -> Result<ExportedTypeDef, syn::Error> {
    let mut fields = Vec::with_capacity(slots.len());
    for slot in slots {
        let ident = &slot.field_ident;
        let wit_name = rust_ident_to_wit_name(ident)?;
        let field_name = ident.unraw().to_string();
        // Hosts recover the slot name leaf from the WIT field name.
        if wit_name.replace('-', "_") != field_name {
            return Err(syn::Error::new(
                ident.span(),
                format!(
                    "storage field `{ident}` gets the WIT name `{wit_name}` in the account \
                     storage schema, which does not map back to the field name; rename the field \
                     to snake case"
                ),
            ));
        }
        let context = format!("slot `{field_name}` in struct `{}`", item_struct.ident);
        let ty = match &slot.kind {
            StorageSlotKind::Value { ty } => slot_type_ref(slot, ty, registry, &context)?,
            StorageSlotKind::Map { key, value } => map_type_ref(
                slot_type_ref(slot, key, registry, &format!("key type of {context}"))?,
                slot_type_ref(slot, value, registry, &format!("value type of {context}"))?,
            ),
            StorageSlotKind::StoredProcedure => procedure_root_type_ref(),
        };
        fields.push(ExportedField {
            docs: slot.docs.clone(),
            wit_name,
            ty,
        });
    }

    Ok(ExportedTypeDef {
        docs: doc_comments(&item_struct.attrs),
        rust_name: item_struct.ident.to_string(),
        wit_name: ACCOUNT_STORAGE_ROOT.to_owned(),
        kind: ExportedTypeKind::Record { fields },
    })
}

/// Maps the key or value type `ty` of `slot` and checks it against the storage type surface.
///
/// `context` names the type in diagnostics.
fn slot_type_ref(
    slot: &StorageSlotSpec,
    ty: &Type,
    registry: &HashMap<String, ExportedTypeDef>,
    context: &str,
) -> Result<TypeRef, syn::Error> {
    let type_ref = map_schema_field_type(&ACCOUNT_STORAGE_SCHEMA, ty, registry, context)?;
    validate_schema_type_ref(&ACCOUNT_STORAGE_SCHEMA, &type_ref, ty.span(), context)?;
    let is_exported_type = type_ref.is_custom
        && type_ref.path.last().is_some_and(|rust_name| registry.contains_key(rust_name));
    if slot.has_type_override && is_exported_type {
        return Err(syn::Error::new(
            slot.field_ident.span(),
            "the schema type of a slot with an `#[export_type]` type is its WIT type; remove the \
             `type` argument",
        ));
    }
    Ok(type_ref)
}

/// Returns the `list<tuple<K, V>>` type of a map slot with the key type `key` and the value type
/// `value`.
fn map_type_ref(key: TypeRef, value: TypeRef) -> TypeRef {
    TypeRef {
        wit_name: format!("list<tuple<{}, {}>>", key.wit_name, value.wit_name),
        is_custom: false,
        // No written path: the identity checks visit the key and value types only.
        path: Vec::new(),
        leading_colon: false,
        dependencies: vec![key, value],
    }
}

/// Returns the type of a stored-procedure slot: the core `word` holding the procedure root.
fn procedure_root_type_ref() -> TypeRef {
    TypeRef {
        wit_name: "word".to_owned(),
        is_custom: false,
        path: vec!["miden".to_owned(), "Word".to_owned()],
        leading_colon: true,
        dependencies: Vec::new(),
    }
}

/// Rejects a referenced `#[export_type]` type whose WIT name is already taken in the
/// `account-storage` interface: by the `storage` root record or by an imported core type.
///
/// The diagnostic points at the storage struct, the only item this expansion can span.
fn reject_storage_type_name_collisions(
    item_struct: &ItemStruct,
    custom_types: &[ExportedTypeDef],
    core_imports: &BTreeSet<String>,
) -> Result<(), syn::Error> {
    for definition in custom_types {
        let taken_by = if definition.wit_name == ACCOUNT_STORAGE_ROOT {
            "the `storage` record the schema declares for the storage root"
        } else if core_imports.contains(&definition.wit_name) {
            "an SDK core type the schema imports"
        } else {
            continue;
        };
        return Err(syn::Error::new(
            item_struct.ident.span(),
            format!(
                "`#[export_type]` type `{}` referenced by `#[component_storage]` struct `{}` \
                 produces the WIT type name `{}`, which is already used by {taken_by}; rename the \
                 type",
                definition.rust_name, item_struct.ident, definition.wit_name
            ),
        ));
    }
    Ok(())
}

/// Returns the unversioned storage schema package name of a component package:
/// `<name>-storage-schema`.
///
/// Custom type FQNs embed this name, so the name carries no version and stays stable across
/// releases.
fn storage_schema_package_name(component_package: &str) -> String {
    let name = component_package.split_once('@').map_or(component_package, |(name, _)| name);
    format!("{name}-storage-schema")
}

#[cfg(test)]
mod tests {
    use midenc_expect_test::expect;
    use syn::parse_quote;
    use wit_bindgen_core::wit_parser::{Resolve, TypeDefKind};

    use super::*;
    use crate::{
        schema_render::macos_link_section,
        types::{
            exported_type_from_struct, lock_export_type_registry_for_tests,
            reset_export_type_registry_for_tests,
        },
    };

    /// Builds a value slot spec.
    fn value(name: &str, ty: Type, docs: &[&str]) -> StorageSlotSpec {
        slot(name, StorageSlotKind::Value { ty: Box::new(ty) }, docs)
    }

    /// Builds a map slot spec.
    fn map(name: &str, key: Type, value: Type, docs: &[&str]) -> StorageSlotSpec {
        slot(
            name,
            StorageSlotKind::Map {
                key: Box::new(key),
                value: Box::new(value),
            },
            docs,
        )
    }

    /// Builds a slot spec without a `type` argument.
    fn slot(name: &str, kind: StorageSlotKind, docs: &[&str]) -> StorageSlotSpec {
        StorageSlotSpec {
            field_ident: syn::parse_str(name).expect("test field name must parse"),
            docs: docs.iter().map(|doc| (*doc).to_owned()).collect(),
            has_type_override: false,
            kind,
        }
    }

    /// Maps test `#[export_type]` structs to registry definitions.
    fn registry(structs: &[ItemStruct]) -> Vec<ExportedTypeDef> {
        structs
            .iter()
            .map(|item| exported_type_from_struct(item).expect("test record must map"))
            .collect()
    }

    /// Renders the schema of `item_struct` with `slots` against `registry`.
    fn render(
        item_struct: &ItemStruct,
        slots: &[StorageSlotSpec],
        registry: &[ExportedTypeDef],
    ) -> Result<String, syn::Error> {
        let rendered = render_account_storage_schema_model(
            item_struct,
            slots,
            "miden:storage-example",
            registry,
        )?;
        validate_rendered_schema_for_tests(&rendered);
        Ok(rendered.source)
    }

    /// Checks that the rendered schema resolves and its root is the `storage` record.
    fn validate_rendered_schema_for_tests(rendered: &RenderedSchema) {
        crate::schema_render::validate_rendered_schema(&ACCOUNT_STORAGE_SCHEMA, rendered)
            .expect("the rendered schema must resolve");
        let mut resolve = Resolve::default();
        let package_id = resolve
            .push_str("account-storage-schema.wit", &rendered.source)
            .expect("the account storage schema must parse");
        let package = &resolve.packages[package_id];
        assert!(package.name.version.is_none(), "the schema package must be unversioned");
        let interface = &resolve.interfaces[package.interfaces["account-storage"]];
        let root = interface.types["storage"];
        assert!(matches!(resolve.types[root].kind, TypeDefKind::Record(_)));
    }

    /// Returns the schema package of a rendered schema, without the embedded core-types package.
    fn schema_package(source: &str) -> &str {
        let end = source.find("package miden:base@1.0.0 {").expect("core types are embedded");
        &source[..end]
    }

    /// Renders `slots` against `registry` and returns the error message it must fail with.
    fn render_error(slots: &[StorageSlotSpec], registry: &[ExportedTypeDef]) -> String {
        let item: ItemStruct = parse_quote!(
            struct MyAccount {}
        );
        render(&item, slots, registry)
            .expect_err("the schema must be rejected")
            .to_string()
    }

    #[test]
    fn derives_macos_account_storage_schema_link_section() {
        assert_eq!(
            macos_link_section(ACCOUNT_STORAGE_SCHEMA.link_section),
            "rodata,miden_account_st"
        );
    }

    #[test]
    fn renders_storage_example_shaped_schema() {
        let _registry_guard = lock_export_type_registry_for_tests();
        reset_export_type_registry_for_tests();
        let item: ItemStruct = parse_quote! {
            /// An example account demonstrating storage value and map usage.
            struct MyAccount {}
        };
        let slots = [
            value(
                "owner_public_key",
                parse_quote!(Word),
                &[" Public key authorized to update the stored asset quantities."],
            ),
            map(
                "asset_qty_map",
                parse_quote!(Word),
                parse_quote!(AssetAmount),
                &["asset quantity map"],
            ),
        ];
        let source = render(&item, &slots, &[]).expect("schema must render");

        expect![[r#"
            // This file is auto-generated by the `#[component_storage]` macro.
            // Do not edit this file manually.

            package miden:storage-example-storage-schema;

            use miden:base/core-types@1.0.0;

            interface account-storage {
                use core-types.{asset-amount, word};

                /// An example account demonstrating storage value and map usage.
                record storage {
                    /// Public key authorized to update the stored asset quantities.
                    %owner-public-key: word,
                    /// asset quantity map
                    %asset-qty-map: list<tuple<word, asset-amount>>,
                }
            }

            package miden:base@1.0.0 {
                interface core-types {
                    /// Represents an on-chain felt.
                    ///
                    /// Field modulus M = 2^64 - 2^32 + 1.
                    record felt {
                        /// The backing type is `f32` which will be treated as a felt by the compiler.
                	/// We're basically hijacking the Wasm `f32` type and treat as felt.
                        inner: f32,
                    }


                    /// A group of four field elements in the Miden base field.
                    record word {
                        a: felt,
                	b: felt,
                	c: felt,
                	d: felt,
                    }

                    /// A cryptographic digest representing a 256-bit hash value.
                    /// This is a wrapper around `word` which contains 4 field elements.
                    record digest {
                        inner: word
                    }

                    /// Unique identifier of an account.
                    ///
                    /// # Layout
                    ///
                    /// An `AccountId` consists of two field elements, where the first is called the prefix and the
                    /// second is called the suffix. It is laid out as follows:
                    ///
                    /// prefix: [hash (56 bits) | storage mode (2 bits) | type (2 bits) | version (4 bits)]
                    /// suffix: [zero bit | hash (55 bits) | 8 zero bits]
                    record account-id {
                    	prefix: felt,
                	suffix: felt
                    }

                    /// Recipient of the note, i.e., hash(hash(hash(serial_num, [0; 4]), note_script_hash), input_hash)
                    record recipient {
                        inner: word
                    }

                    record tag {
                        inner: u32
                    }

                    /// The identifier of an asset: the word that identifies it in an account vault. It encodes the
                    /// issuing faucet, the asset class and the composition rule; read them through the SDK's
                    /// `AssetId` getters rather than decoding the limbs.
                    record asset-id {
                        inner: word,
                    }

                    /// A fungible or a non-fungible asset, encoded as its asset id and a value word. For a
                    /// fungible asset the value holds the amount in its first element; for a non-fungible asset
                    /// it holds the asset data hash.
                    record asset {
                        id: asset-id,
                        value: word,
                    }

                    /// A validated fungible asset amount, at most 2^63 - 2^31.
                    record asset-amount {
                        inner: felt
                    }

                    /// Account nonce
                    record nonce {
                        inner: felt
                    }

                    /// A block height in the chain
                    record block-number {
                        inner: felt
                    }

                    /// Account hash
                    record account-hash {
                        inner: word
                    }

                    /// Block hash
                    record block-hash {
                        inner: word
                    }

                    /// Storage value
                    record storage-value {
                        inner: word
                    }

                    /// Account storage root
                    record storage-root {
                        inner: word
                    }

                    /// Account code root
                    record account-code-root {
                        inner: word
                    }

                    /// Commitment to the account vault
                    record vault-commitment {
                        inner: word
                    }

                    /// An index of the created note
                    record note-idx {
                        inner: u16
                    }

                    record note-type {
                        inner: u8
                    }

                    record note-execution-hint {
                        inner: felt
                    }

                }
            }
        "#]]
        .assert_eq(&source);
    }

    #[test]
    fn renders_custom_map_key_and_value_types_with_docs() {
        let _registry_guard = lock_export_type_registry_for_tests();
        reset_export_type_registry_for_tests();
        let registry = registry(&[
            parse_quote! {
                /// A map key made of two integers.
                struct Pair {
                    /// The left half.
                    left: u32,
                    right: u16,
                }
            },
            parse_quote! {
                /// A balance with a lock flag.
                struct Balance {
                    /// The amount held.
                    amount: Felt,
                    locked: bool,
                }
            },
        ]);
        let item: ItemStruct = parse_quote!(
            struct MyAccount {}
        );
        let slots = [map("balances", parse_quote!(Pair), parse_quote!(Balance), &["balances"])];
        let source = render(&item, &slots, &registry).expect("schema must render");

        expect![[r#"
            // This file is auto-generated by the `#[component_storage]` macro.
            // Do not edit this file manually.

            package miden:storage-example-storage-schema;

            use miden:base/core-types@1.0.0;

            interface account-storage {
                use core-types.{felt};

                /// A map key made of two integers.
                record pair {
                    /// The left half.
                    %left: u32,
                    %right: u16,
                }

                /// A balance with a lock flag.
                record balance {
                    /// The amount held.
                    %amount: felt,
                    %locked: bool,
                }

                record storage {
                    /// balances
                    %balances: list<tuple<pair, balance>>,
                }
            }

        "#]]
        .assert_eq(schema_package(&source));
    }

    #[test]
    fn renders_option_fields_of_custom_records() {
        let _registry_guard = lock_export_type_registry_for_tests();
        reset_export_type_registry_for_tests();
        let registry = registry(&[parse_quote! {
            struct Limit {
                max: Option<u32>,
            }
        }]);
        let item: ItemStruct = parse_quote!(
            struct MyAccount {}
        );
        let slots = [value("limit", parse_quote!(Limit), &[])];
        let source = render(&item, &slots, &registry).expect("schema must render");

        expect![[r#"
            // This file is auto-generated by the `#[component_storage]` macro.
            // Do not edit this file manually.

            package miden:storage-example-storage-schema;

            interface account-storage {
                record limit {
                    %max: option<u32>,
                }

                record storage {
                    %limit: limit,
                }
            }

        "#]]
        .assert_eq(schema_package(&source));
    }

    #[test]
    fn renders_stored_procedure_slots_as_words() {
        let _registry_guard = lock_export_type_registry_for_tests();
        reset_export_type_registry_for_tests();
        let item: ItemStruct = parse_quote!(
            struct MyAccount {}
        );
        let slots = [
            slot("authority", StorageSlotKind::StoredProcedure, &["authority procedure"]),
            value("count", parse_quote!(u16), &[]),
        ];
        let source = render(&item, &slots, &[]).expect("schema must render");

        expect![[r#"
            // This file is auto-generated by the `#[component_storage]` macro.
            // Do not edit this file manually.

            package miden:storage-example-storage-schema;

            use miden:base/core-types@1.0.0;

            interface account-storage {
                use core-types.{word};

                record storage {
                    /// authority procedure
                    %authority: word,
                    %count: u16,
                }
            }

        "#]]
        .assert_eq(schema_package(&source));
    }

    #[test]
    fn struct_without_slots_has_no_schema() {
        let item: ItemStruct = parse_quote!(
            struct MyAccount {}
        );
        let schema = expand_account_storage_schema(&item, None, &[]).expect("no slots expand");
        assert!(schema.is_none());
    }

    #[test]
    fn expansion_embeds_the_schema_in_its_link_section() {
        let _registry_guard = lock_export_type_registry_for_tests();
        reset_export_type_registry_for_tests();
        let item: ItemStruct = parse_quote!(
            struct MyAccount {}
        );
        let slots = [value("count", parse_quote!(Felt), &[])];
        let tokens = expand_account_storage_schema(&item, None, &slots)
            .expect("the schema expands")
            .expect("a struct with slots has a schema")
            .to_string();
        assert!(tokens.contains("\"rodata,miden_account_storage_schema\""), "{tokens}");
        assert!(tokens.contains("\"rodata,miden_account_st\""), "{tokens}");
        assert!(tokens.contains("__MIDEN_ACCOUNT_STORAGE_SCHEMA_BYTES"), "{tokens}");
        assert!(tokens.contains("miden:empty-storage-schema"), "{tokens}");
    }

    #[test]
    fn rejects_a_type_argument_on_a_slot_with_an_exported_type() {
        let _registry_guard = lock_export_type_registry_for_tests();
        reset_export_type_registry_for_tests();
        let registry = registry(&[parse_quote! {
            struct Pair {
                left: u32,
                right: u32,
            }
        }]);
        for mut spec in [
            value("pair", parse_quote!(Pair), &[]),
            map("pair", parse_quote!(Pair), parse_quote!(Word), &[]),
            map("pair", parse_quote!(Word), parse_quote!(Pair), &[]),
        ] {
            spec.has_type_override = true;
            let message = render_error(&[spec], &registry);
            assert_eq!(
                message,
                "the schema type of a slot with an `#[export_type]` type is its WIT type; remove \
                 the `type` argument"
            );
        }

        let mut spec = value("count", parse_quote!(Felt), &[]);
        spec.has_type_override = true;
        let item: ItemStruct = parse_quote!(
            struct MyAccount {}
        );
        render(&item, &[spec], &registry).expect("a core-typed slot keeps its type argument");
    }

    #[test]
    fn rejects_vec_in_slot_and_custom_types() {
        let _registry_guard = lock_export_type_registry_for_tests();
        reset_export_type_registry_for_tests();
        for ty in [parse_quote!(Vec<Felt>), parse_quote!(Option<Vec<Felt>>)] {
            let message = render_error(&[value("values", ty, &[])], &[]);
            assert_eq!(message, "`Vec` is not supported in account storage schemas yet");
        }
        let message =
            render_error(&[map("values", parse_quote!(Word), parse_quote!(Vec<Felt>), &[])], &[]);
        assert_eq!(message, "`Vec` is not supported in account storage schemas yet");

        // A custom type never reaches the schema with a `Vec`: `#[export_type]` rejects it.
        let batch: ItemStruct = parse_quote! {
            struct Batch {
                values: Vec<u32>,
            }
        };
        exported_type_from_struct(&batch).expect_err("`#[export_type]` rejects `Vec` fields");
    }

    #[test]
    fn rejects_unsupported_slot_types() {
        let _registry_guard = lock_export_type_registry_for_tests();
        reset_export_type_registry_for_tests();
        let message = render_error(&[value("count", parse_quote!(i32), &[])], &[]);
        assert!(message.contains("`#[component_storage]` slot `count` in struct `MyAccount`"));
        assert!(message.contains("unsupported WIT type `s32`"), "{message}");

        let message = render_error(
            &[map("balances", parse_quote!(Word), parse_quote!(Result<u32, u8>), &[])],
            &[],
        );
        assert!(message.contains("value type of slot `balances`"), "{message}");
    }

    #[test]
    fn rejects_custom_types_registered_after_the_storage_struct() {
        let _registry_guard = lock_export_type_registry_for_tests();
        reset_export_type_registry_for_tests();
        let message = render_error(&[value("owner", parse_quote!(Owner), &[])], &[]);
        assert_eq!(
            message,
            "custom type `Owner` in an account storage schema needs #[export_type] on its \
             definition before the #[component_storage] struct"
        );
    }

    #[test]
    fn rejects_custom_types_named_like_the_storage_root() {
        let _registry_guard = lock_export_type_registry_for_tests();
        reset_export_type_registry_for_tests();
        let registry = registry(&[parse_quote! {
            struct Storage {
                count: u32,
            }
        }]);
        let message = render_error(&[value("state", parse_quote!(Storage), &[])], &registry);
        assert!(
            message.contains(
                "`#[export_type]` type `Storage` referenced by `#[component_storage]` struct \
                 `MyAccount`"
            ),
            "{message}"
        );
        assert!(message.contains("the `storage` record"), "{message}");
        assert!(message.contains("rename the type"), "{message}");
    }

    #[test]
    fn rejects_field_names_that_do_not_round_trip_through_kebab_case() {
        let _registry_guard = lock_export_type_registry_for_tests();
        reset_export_type_registry_for_tests();
        for name in ["hookA", "type_"] {
            let message = render_error(&[value(name, parse_quote!(Word), &[])], &[]);
            assert!(message.contains(&format!("storage field `{name}`")), "{message}");
            assert!(message.contains("does not map back to the field name"), "{message}");
        }

        let item: ItemStruct = parse_quote!(
            struct MyAccount {}
        );
        let source = render(&item, &[value("r#type", parse_quote!(Word), &[])], &[])
            .expect("a raw identifier maps back to its field name");
        assert!(source.contains("%type: word,"), "{source}");
    }
}
