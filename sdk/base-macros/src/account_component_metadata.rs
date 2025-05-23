use std::collections::BTreeSet;

use miden_objects::account::{
    component::FieldIdentifier, AccountComponentMetadata, AccountType, MapRepresentation,
    StorageEntry, StorageValueName, TemplateType, WordRepresentation,
};
use semver::Version;

pub struct AccountComponentMetadataBuilder {
    /// The human-readable name of the component.
    name: String,

    /// A brief description of what this component is and how it works.
    description: String,

    /// The version of the component using semantic versioning.
    /// This can be used to track and manage component upgrades.
    version: Version,

    /// A set of supported target account types for this component.
    supported_types: BTreeSet<AccountType>,

    /// A list of storage entries defining the component's storage layout and initialization
    /// values.
    storage: Vec<StorageEntry>,
}

impl AccountComponentMetadataBuilder {
    /// Adds a supported account type to this component metadata.
    pub fn add_supported_type(&mut self, account_type: AccountType) {
        self.supported_types.insert(account_type);
    }

    pub fn new(name: String, version: Version, description: String) -> Self {
        AccountComponentMetadataBuilder {
            name,
            description,
            version,
            supported_types: BTreeSet::new(),
            storage: Vec::new(),
        }
    }

    pub fn add_storage_entry(
        &mut self,
        name: &str,
        description: Option<String>,
        slot: u8,
        field_type: &syn::Type,
        field_type_attr: Option<String>,
    ) {
        let type_path = if let syn::Type::Path(type_path) = field_type {
            type_path
        } else {
            panic!("failed to get type path {:?}", field_type)
        };

        if let Some(segment) = type_path.path.segments.last() {
            let type_name = segment.ident.to_string();
            let storage_value_name =
                StorageValueName::new(name).expect("well formed storage value name");
            match type_name.as_str() {
                "StorageMap" => {
                    let mut map_repr = MapRepresentation::new(vec![], storage_value_name);
                    if let Some(description) = description {
                        map_repr = map_repr.with_description(description);
                    }
                    self.storage.push(StorageEntry::new_map(slot, map_repr));
                }
                "Value" => {
                    let r#type = if let Some(field_type) = field_type_attr {
                        TemplateType::new(&field_type)
                            .unwrap_or_else(|_| panic!("well formed attribute type {field_type}"))
                    } else {
                        TemplateType::native_word()
                    };
                    self.storage.push(StorageEntry::new_value(
                        slot,
                        WordRepresentation::Template {
                            r#type,
                            identifier: FieldIdentifier {
                                name: storage_value_name,
                                description,
                            },
                        },
                    ));
                }
                _ => panic!("unexpected field type: {}", type_name),
            }
        } else {
            panic!("failed to get last segment of the type path {:?}", type_path)
        }
    }

    pub fn build(self) -> AccountComponentMetadata {
        AccountComponentMetadata::new(
            self.name,
            self.description,
            self.version,
            self.supported_types,
            self.storage,
        )
        .expect("failed to build AccountComponentMetadata")
    }
}
