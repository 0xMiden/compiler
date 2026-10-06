use gimli::{AttributeValue, Dwarf, Reader, UnitOffset, UnitRef, UnitSectionOffset};
use midenc_hir::{FxHashSet, interner::Symbol};

/// A function's declaration, independent of instruction locations and source availability.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FunctionDeclaration {
    /// `None` means absent; an explicitly unknown file is the empty symbol.
    pub file: Option<Symbol>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

impl FunctionDeclaration {
    pub fn resolve<R: Reader<Offset = usize>>(
        unit: UnitRef<'_, R>,
        offset: UnitOffset,
    ) -> gimli::Result<Self> {
        let mut declaration = Self::default();
        declaration.inherit(unit, offset, &mut FxHashSet::default())?;
        Ok(declaration)
    }

    fn inherit<R: Reader<Offset = usize>>(
        &mut self,
        unit: UnitRef<'_, R>,
        offset: UnitOffset,
        visited: &mut FxHashSet<(*const Dwarf<R>, UnitSectionOffset)>,
    ) -> gimli::Result<()> {
        // Bound both cycles and unusually deep reference chains in producer-controlled DWARF.
        // Unit-relative offsets alone are not identities: other units may reuse the same offset.
        if visited.len() >= 64
            || !visited.insert((unit.dwarf, offset.to_unit_section_offset(&unit.header)))
        {
            return Ok(());
        }

        let entry = unit.entry(offset)?;
        if self.file.is_none()
            && let Some(AttributeValue::FileIndex(index)) = entry.attr_value(gimli::DW_AT_decl_file)
        {
            self.file = Some(
                super::resolve_decl_file(unit.dwarf, unit.unit, index)
                    .unwrap_or(midenc_hir::interner::symbols::Empty),
            );
        }
        if self.line.is_none() {
            self.line = entry
                .attr_value(gimli::DW_AT_decl_line)
                .and_then(|value| value.udata_value())
                .and_then(|line| line.try_into().ok());
        }
        if self.column.is_none() {
            self.column = entry
                .attr_value(gimli::DW_AT_decl_column)
                .and_then(|value| value.udata_value())
                .and_then(|column| column.try_into().ok());
        }

        // Concrete attributes take precedence. Inherit only missing fields, interpreting each
        // file index in the compilation unit that owns the attribute.
        for reference in [gimli::DW_AT_abstract_origin, gimli::DW_AT_specification] {
            if self.file.is_some() && self.line.is_some() && self.column.is_some() {
                break;
            }
            match entry.attr_value(reference) {
                Some(AttributeValue::UnitRef(offset)) => self.inherit(unit, offset, visited)?,
                Some(AttributeValue::DebugInfoRef(offset)) => {
                    self.inherit_external(unit.dwarf, offset, visited)?;
                }
                Some(AttributeValue::DebugInfoRefSup(offset)) => {
                    if let Some(dwarf) = unit.dwarf.sup.as_deref() {
                        self.inherit_external(dwarf, offset, visited)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn inherit_external<R: Reader<Offset = usize>>(
        &mut self,
        dwarf: &Dwarf<R>,
        offset: gimli::DebugInfoOffset,
        visited: &mut FxHashSet<(*const Dwarf<R>, UnitSectionOffset)>,
    ) -> gimli::Result<()> {
        let mut units = dwarf.units();
        while let Some(header) = units.next()? {
            if let Some(offset) = offset.to_unit_offset(&header) {
                let unit = dwarf.unit(header)?;
                return self.inherit(unit.unit_ref(dwarf), offset, visited);
            }
        }
        Ok(())
    }
}
