use alloc::sync::Arc;
use core::fmt;

use miden_core::utils::DisplayHex;
use miden_processor::Digest;
use midenc_hir2::{constants::ConstantData, dialects::builtin};

use crate::{lower::NativePtr, masm};

pub struct MasmComponent {
    pub id: builtin::ComponentId,
    /// The symbol name of the component initializer function
    ///
    /// This function is responsible for initializing global variables and writing data segments
    /// into memory at program startup, and at cross-context call boundaries (in callee prologue).
    pub init: Option<masm::InvocationTarget>,
    /// The symbol name of the program entrypoint, if this component is executable.
    ///
    /// If unset, it indicates that the component is a library, even if it could be made executable.
    pub entrypoint: Option<masm::InvocationTarget>,
    /// The kernel library to link against
    pub kernel: Option<masm::KernelLibrary>,
    /// The rodata segments of this component keyed by the offset of the segment
    pub rodata: Vec<Rodata>,
    /// The address of the `__stack_pointer` global, if such a global has been defined
    pub stack_pointer: Option<u32>,
    /// The set of modules in this component
    pub modules: Vec<Box<masm::Module>>,
}

/// Represents a read-only data segment, combined with its content digest
#[derive(Clone, PartialEq, Eq)]
pub struct Rodata {
    /// The component to which this read-only data segment belongs
    pub component: builtin::ComponentId,
    /// The content digest computed for `data`
    pub digest: Digest,
    /// The address at which the data for this segment begins
    pub start: NativePtr,
    /// The raw binary data for this segment
    pub data: Arc<ConstantData>,
}
impl fmt::Debug for Rodata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rodata")
            .field("digest", &format_args!("{}", DisplayHex::new(&self.digest.as_bytes())))
            .field("start", &self.start)
            .field_with("data", |f| {
                f.debug_struct("ConstantData")
                    .field("len", &self.data.len())
                    .finish_non_exhaustive()
            })
            .finish()
    }
}
impl Rodata {
    pub fn size_in_bytes(&self) -> usize {
        self.data.len()
    }

    pub fn size_in_felts(&self) -> usize {
        self.data.len().next_multiple_of(4) / 4
    }

    pub fn size_in_words(&self) -> usize {
        self.size_in_felts().next_multiple_of(4) / 4
    }

    /// Attempt to convert this rodata object to its equivalent representation in felts
    ///
    /// The resulting felts will be in padded out to the nearest number of words, i.e. if the data
    /// only takes up 3 felts worth of bytes, then the resulting `Vec` will contain 4 felts, so that
    /// the total size is a valid number of words.
    pub fn to_elements(&self) -> Result<Vec<miden_processor::Felt>, String> {
        use miden_core::FieldElement;
        use miden_processor::Felt;

        let data = self.data.as_slice();
        let mut felts = Vec::with_capacity(data.len() / 4);
        let mut iter = data.iter().copied().array_chunks::<4>();
        felts.extend(iter.by_ref().map(|bytes| Felt::new(u32::from_le_bytes(bytes) as u64)));
        if let Some(remainder) = iter.into_remainder() {
            let mut chunk = [0u8; 4];
            for (i, byte) in remainder.into_iter().enumerate() {
                chunk[i] = byte;
            }
            felts.push(Felt::new(u32::from_le_bytes(chunk) as u64));
        }

        let padding = (self.size_in_words() * 4).abs_diff(felts.len());
        felts.resize(felts.len() + padding, Felt::ZERO);

        Ok(felts)
    }
}
