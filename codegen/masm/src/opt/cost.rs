//! Expanded stack-instruction costs used to compare otherwise valid physical constructions.
//!
//! MASM mnemonics are not uniformly priced: several indexed operations expand to multiple VM
//! operations. This narrow table follows the pinned assembler, excluding basic-block packing
//! noops and control-flow overhead. It ranks code; it never supplies verification evidence.
use crate::masm::{Immediate, Instruction, PushValue};

/// Number of VM operations in a supported stack instruction's assembler expansion.
///
/// Unknown instructions and unresolved immediates return `None`. Callers must not silently
/// treat those as cheap. The assembler conformance tests make dependency changes visible.
pub(crate) fn stack_instruction_cost(instruction: &Instruction) -> Option<usize> {
    use Instruction::*;
    Some(match instruction {
        Dup0 | Dup1 | Dup2 | Dup3 | Dup4 | Dup5 | Dup6 | Dup7 | Dup9 | Dup11 | Dup13 | Dup15
        | Swap1 | SwapW1 | SwapW2 | SwapW3 | SwapDw | MovUp2 | MovUp3 | MovUp4 | MovUp5
        | MovUp6 | MovUp7 | MovUp8 | MovDn2 | MovDn3 | MovDn4 | MovDn5 | MovDn6 | MovDn7
        | MovDn8 | Drop => 1,
        Swap2 | Swap3 | Swap4 | Swap5 | Swap6 | Swap7 | Swap8 | MovUpW2 | MovDnW2 => 2,
        Dup8 | Dup10 | Dup12 | Dup14 | MovUpW3 | MovDnW3 => 3,
        MovUp9 | MovUp10 | MovUp11 | MovUp12 | MovUp13 | MovUp14 | MovUp15 | MovDn9 | MovDn10
        | MovDn11 | MovDn12 | MovDn13 | MovDn14 | MovDn15 | DupW0 | DupW1 | DupW2 | DupW3
        | DropW | PadW => 4,
        Swap9 => 5,
        Swap10 | Swap11 | Swap12 | Swap13 | Swap14 | Swap15 => 6,
        Push(Immediate::Value(value)) => match value.inner() {
            PushValue::Int(value) => 1 + usize::from(value.as_int() == 1),
            PushValue::Word(_) => return None,
        },
        _ => return None,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use miden_assembly::{
        Assembler, DefaultSourceManager, Path,
        ast::{Module, ModuleKind},
    };
    use miden_core::operations::Operation;
    use midenc_hir::Report;

    /// Inspect a library root so program setup and inferred prologue costs cannot mask an
    /// instruction expansion. Ignore packing noops; none of the priced instructions emits one.
    pub(crate) fn assembled_cost(body: &str) -> Result<usize, Report> {
        assembled_cost_with_locals(body, 0)
    }

    /// Count the same expansions with a declared frame, including its fixed setup/teardown.
    /// Compare with an empty body at the same frame size to isolate a local access.
    pub(crate) fn assembled_cost_with_locals(body: &str, locals: u16) -> Result<usize, Report> {
        let manager = alloc::sync::Arc::new(DefaultSourceManager::default());
        let module = Module::parser(Some(ModuleKind::Library)).parse_str(
            Some(Path::new("::stack_cost")),
            alloc::format!("@locals({locals}) pub proc test {body} end"),
            manager.clone(),
        )?;
        let package = Assembler::new(manager).assemble_library(
            "stack_cost",
            module,
            core::iter::empty::<&str>(),
        )?;
        let export = package.manifest.exports().next().unwrap().as_procedure().unwrap();
        let block = package.mast_forest()[export.node.unwrap()].unwrap_basic_block();
        Ok(block.operations().filter(|op| !matches!(op, Operation::Noop)).count())
    }

    #[test]
    fn word_stack_costs_match_assembler_and_unknowns_remain_unpriced() -> Result<(), Report> {
        use super::{Instruction::*, stack_instruction_cost};
        for instruction in [
            SwapW1, SwapW2, SwapW3, SwapDw, MovUpW2, MovUpW3, MovDnW2, MovDnW3, DupW0, DupW1,
            DupW2, DupW3, DropW, PadW, Drop,
        ] {
            assert_eq!(
                stack_instruction_cost(&instruction),
                Some(assembled_cost(&alloc::format!("{instruction}"))?),
                "{instruction}"
            );
        }
        assert_eq!(stack_instruction_cost(&Add), None);
        assert_eq!(stack_instruction_cost(&MemLoad), None);
        Ok(())
    }
}
