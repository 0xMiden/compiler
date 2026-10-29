use alloc::format;

use midenc_hir::{
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
    dialects::builtin::attributes::LocalVariableAttr,
    effects::*,
    traits::*,
    *,
};

use crate::HirDialect;

/// Allocate a procedure local and return its byte-address-space pointer.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Alloca {
    #[attr]
    #[effects(MemoryEffect(MemoryEffect::Allocate))]
    local: LocalVariableAttr,
    #[result]
    #[effects(MemoryEffect(MemoryEffect::Allocate))]
    result: AnyPointer,
}

impl InferTypeOpInterface for Alloca {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = Type::from(PointerType::new_with_address_space(
            self.local().local_type(),
            AddressSpace::Byte,
        ));
        self.result_mut().set_type(ty);
        Ok(())
    }
}

/// Get the byte-address-space pointer to a procedure local without accessing its contents.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct LocalByteAddress {
    #[attr]
    local: LocalVariableAttr,
    #[result]
    result: AnyPointer,
}

impl InferTypeOpInterface for LocalByteAddress {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = Type::from(PointerType::new_with_address_space(
            self.local().local_type(),
            AddressSpace::Byte,
        ));
        self.result_mut().set_type(ty);
        Ok(())
    }
}

/// Get the element-address-space pointer to a procedure local.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct LocalAddress {
    #[attr]
    local: LocalVariableAttr,
    #[result]
    result: AnyPointer,
}

impl InferTypeOpInterface for LocalAddress {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        // Use the attribute-level type accessor: locals reconstructed from parsed HIR are not
        // bound to a function, and carry their type on the attribute instead
        let ty = Type::from(PointerType::new_with_address_space(
            self.local().local_type(),
            AddressSpace::Element,
        ));
        self.result_mut().set_type(ty);
        Ok(())
    }
}

/// Store `value` at `addr` in its declared address space.
///
/// For an element-addressed subword scalar, MASM updates the low bits of that element
/// and preserves the remaining bits. Incrementing an element address selects the next
/// element, not the next byte. This differs from the packed byte stride of `hir.mem_set`;
/// repeated typed stores at consecutive element addresses are not a packed fill.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(MemoryEffectOpInterface, OpPrinter)
)]
pub struct Store {
    #[operand]
    #[effects(MemoryEffect(MemoryEffect::Write))]
    addr: AnyPointer,
    #[operand]
    value: AnyType,
}

/// Store `value` on in procedure local memory
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(MemoryEffectOpInterface, OpPrinter)
)]
pub struct StoreLocal {
    #[attr]
    #[effects(MemoryEffect(MemoryEffect::Write))]
    local: LocalVariableAttr,
    #[operand]
    value: AnyType,
}

/// Load `result` from the heap at `addr`
///
/// The type of load is determined by the pointer operand type - cast the pointer to the type you
/// wish to load, so long as such a load is safe according to the semantics of your high-level
/// language.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Load {
    #[operand]
    #[effects(MemoryEffect(MemoryEffect::Read))]
    addr: AnyPointer,
    #[result]
    result: AnyType,
}

impl InferTypeOpInterface for Load {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let _span = self.span();
        let pointee = {
            let addr = self.addr();
            let addr_value = addr.value();
            addr_value.ty().pointee().cloned()
        };
        match pointee {
            Some(pointee) => {
                self.result_mut().set_type(pointee);
                Ok(())
            }
            None => {
                // let addr = self.addr();
                // let addr_value = addr.value();
                // let addr_ty = addr_value.ty();
                // Err(context
                //     .session
                //     .diagnostics
                //     .diagnostic(midenc_session::diagnostics::Severity::Error)
                //     .with_message("invalid operand for 'load'")
                //     .with_primary_label(
                //         span,
                //         format!("invalid 'addr' operand, expected pointer, got '{addr_ty}'"),
                //     )
                //     .into_report())
                Ok(())
            }
        }
    }
}

#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct LoadLocal {
    #[attr]
    #[effects(MemoryEffect(MemoryEffect::Read))]
    local: LocalVariableAttr,
    #[result]
    result: AnyType,
}

impl InferTypeOpInterface for LoadLocal {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        // Use the attribute-level type accessor: locals reconstructed from parsed HIR are not
        // bound to a function, and carry their type on the attribute instead
        let ty = self.local().local_type();
        self.result_mut().set_type(ty);

        Ok(())
    }
}

/// Load two VM words from memory and update the top-13 operand stack window.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Read, MemoryEffect::Write))]
pub struct MemStream {
    #[operands]
    stack: IntFelt,
    #[results]
    outputs: IntFelt,
}

impl InferTypeOpInterface for MemStream {
    fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
        const WINDOW_SIZE: usize = 13;

        if self.stack().len() != WINDOW_SIZE {
            return Err(Report::msg(format!(
                "invalid hir.mem_stream: expected {WINDOW_SIZE} operand(s), but got {}",
                self.stack().len()
            )));
        }

        if !self.op.results.is_empty() && self.op.results.len() != WINDOW_SIZE {
            return Err(Report::msg(format!(
                "invalid hir.mem_stream: expected {WINDOW_SIZE} result(s), but got {}",
                self.op.results.len()
            )));
        }

        if self.op.results.is_empty() {
            let span = self.span();
            let owner = self.as_operation_ref();
            for i in 0..WINDOW_SIZE {
                let value = context.make_result(span, Type::Felt, owner, i as u16);
                self.op.results.push(value);
            }
        } else {
            for result in self.op.results.iter_mut() {
                result.borrow_mut().set_type(Type::Felt);
            }
        }

        Ok(())
    }
}
