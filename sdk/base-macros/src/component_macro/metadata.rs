use std::{fs, path::Path};

use proc_macro::Span;
use semver::Version;
use toml::Value;

/// Cargo metadata relevant for the `#[component]` macro expansion.
pub struct CargoMetadata {
    pub name: String,
    pub version: Version,
    pub description: String,
    pub supported_types: Vec<String>,
    pub component_package: Option<String>,
}

/// Reads component metadata (name/description/version/supported types) from the enclosing package
/// manifest.
pub fn get_package_metadata(call_site_span: Span) -> Result<CargoMetadata, syn::Error> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let current_dir = Path::new(&manifest_dir);

    let cargo_toml_path = current_dir.join("Cargo.toml");
    if !cargo_toml_path.is_file() {
        return Ok(CargoMetadata {
            name: String::new(),
            version: Version::new(0, 0, 1),
            description: String::new(),
            supported_types: vec![],
            component_package: None,
        });
    }

    let cargo_toml_content = fs::read_to_string(&cargo_toml_path).map_err(|e| {
        syn::Error::new(
            call_site_span.into(),
            format!("Failed to read {}: {}", cargo_toml_path.display(), e),
        )
    })?;
    let cargo_toml: Value = cargo_toml_content.parse::<Value>().map_err(|e| {
        syn::Error::new(
            call_site_span.into(),
            format!("Failed to parse {}: {}", cargo_toml_path.display(), e),
        )
    })?;

    let package_table = cargo_toml.get("package").ok_or_else(|| {
        syn::Error::new(
            call_site_span.into(),
            format!(
                "Cargo.toml ({}) does not contain a [package] table",
                cargo_toml_path.display()
            ),
        )
    })?;

    let name = package_table
        .get("name")
        .and_then(|n| n.as_str())
        .map(String::from)
        .ok_or_else(|| {
            syn::Error::new(
                call_site_span.into(),
                format!("Missing 'name' field in [package] table of {}", cargo_toml_path.display()),
            )
        })?;

    let version_str = package_table
        .get("version")
        .and_then(|v| v.as_str())
        .or_else(|| {
            let base = env!("CARGO_MANIFEST_DIR");
            if base.ends_with(cargo_toml_path.parent().unwrap().to_str().unwrap()) {
                Some("0.0.0")
            } else {
                None
            }
        })
        .ok_or_else(|| {
            syn::Error::new(
                call_site_span.into(),
                format!(
                    "Missing 'version' field in [package] table of {} (version.workspace = true \
                     is not yet supported for external crates)",
                    cargo_toml_path.display()
                ),
            )
        })?;

    let version = Version::parse(version_str).map_err(|e| {
        syn::Error::new(
            call_site_span.into(),
            format!(
                "Failed to parse version '{}' from {}: {}",
                version_str,
                cargo_toml_path.display(),
                e
            ),
        )
    })?;

    let description = package_table
        .get("description")
        .and_then(|d| d.as_str())
        .map(String::from)
        .unwrap_or_default();

    let supported_types = cargo_toml
        .get("package")
        .and_then(|pkg| pkg.get("metadata"))
        .and_then(|m| m.get("miden"))
        .and_then(|m| m.get("supported-types"))
        .and_then(|st| st.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    let component_package = cargo_toml
        .get("package")
        .and_then(|pkg| pkg.get("metadata"))
        .and_then(|meta| meta.get("component"))
        .and_then(|component| component.get("package"))
        .and_then(|pkg_val| pkg_val.as_str())
        .map(|pkg| pkg.to_string());

    Ok(CargoMetadata {
        name,
        version,
        description,
        supported_types,
        component_package,
    })
}
