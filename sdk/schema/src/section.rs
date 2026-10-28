//! Package custom-section access.

use miden_mast_package::{Package, SectionId};

use crate::{Error, Result};

/// Returns the only package section with `section_name`.
pub(crate) fn unique_package_section<'a>(
    package: &'a Package,
    section_id: SectionId,
    section_name: &str,
) -> Result<&'a [u8]> {
    let mut matches = package.sections.iter().filter(|section| section.id == section_id);
    let section = matches.next().ok_or_else(|| {
        Error::new(format!("package does not contain the `{section_name}` section"))
    })?;
    if matches.next().is_some() {
        return Err(Error::new(format!("package contains more than one `{section_name}` section")));
    }
    Ok(section.data.as_ref())
}
