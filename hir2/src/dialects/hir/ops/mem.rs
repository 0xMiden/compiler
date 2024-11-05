use midenc_hir_macros::operation;

use crate::{dialects::hir::HirDialect, traits::*, *};

/// Store `value` on the heap at `addr`
#[operation(
    dialect = HirDialect,
    traits(HasSideEffects, MemoryWrite)
)]
pub struct Store {
    #[operand]
    addr: AnyPointer,
    #[operand]
    value: AnyType,
}

// TODO(pauls): StoreLocal

/// Load `result` from the heap at `addr`
///
/// The type of load is determined by the pointer operand type - cast the pointer to the type you
/// wish to load, so long as such a load is safe according to the semantics of your high-level
/// language.
#[operation(
    dialect = HirDialect,
    traits(HasSideEffects, MemoryRead),
    implements(InferTypeOpInterface)
)]
pub struct Load {
    #[operand]
    addr: AnyPointer,
    #[result]
    result: AnyType,
}

impl InferTypeOpInterface for Load {
    fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
        let span = self.span();
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
                let addr = self.addr();
                let addr_value = addr.value();
                let addr_ty = addr_value.ty();
                Err(context
                    .session
                    .diagnostics
                    .diagnostic(miden_assembly::diagnostics::Severity::Error)
                    .with_message("invalid operand for 'load'")
                    .with_primary_label(
                        span,
                        format!("invalid 'addr' operand, expected pointer, got '{addr_ty}'"),
                    )
                    .into_report())
            }
        }
    }
}

// TODO(pauls): LoadLocal

/// Declare a data segment in the shared memory of a [Component].
///
/// This operation type is only permitted in the body of a [Component] op, it is an error to use it
/// anywhere else. At best it will be ignored.
///
/// Data segments can have a size that is larger than the initializer data it describes; in such
/// cases, the remaining memory is either assumed to be arbitrary bytes, or if `zeroed` is set,
/// it is zeroed so that the padding bytes are all zero.
///
/// A data segment can be marked `readonly`, which indicates to the optimizer that it is allowed
/// to assume that no writes will ever occur in the boundaries of the segment, i.e. a value loaded
/// from within those bounds does not need to be reloaded after side-effecting operations, and
/// can in fact be rescheduled around them. Additionally, if a write is detected that would effect
/// memory in a readonly data segment boundary, an error will be raised.
///
/// NOTE: It is not guaranteed that the optimizer will make any assumptions with regard to data
/// segments. For the moment, even if `readonly` is set, the compiler assumes that segments are
/// mutable.
#[operation(
    dialect = HirDialect,
    traits(
        SingleRegion,
        SingleBlock,
        NoRegionArguments,
        IsolatedFromAbove,
    ),
)]
pub struct Segment {
    /// The offset from the start of linear memory where this segment starts
    #[attr]
    offset: u32,
    /// Whether or not this segment is intended to be read-only data
    #[attr]
    #[default]
    readonly: bool,
    /// Whether or not this segment starts as all zeros
    #[attr]
    #[default]
    zeroed: bool,
    /// The data to initialize this segment with, may not be larger than `size`
    #[region]
    initializer: RegionRef,
}

impl Segment {
    /// The size, in bytes, of this data segment.
    ///
    /// By default this will be the same size as `init`, unless explicitly given.
    pub fn size_in_bytes(&self) -> usize {
        todo!()
    }
}
