use miden_debug::{ExecutionTrace, FromMidenRepr};

/// Read a value of type `T` from the program's linear memory at the given byte address.
///
/// The address must be aligned to 4 bytes (i.e. to a single Miden memory element).
pub fn read_rust_memory<T>(trace: &ExecutionTrace, byte_addr: u32) -> Option<T>
where
    T: FromMidenRepr,
{
    assert_eq!(byte_addr % 4, 0, "unaligned reads are not supported (byte_addr={byte_addr})");

    let element_addr = byte_addr / 4;
    let size = <T as FromMidenRepr>::size_in_felts();
    let mut felts = Vec::with_capacity(size);
    for i in 0..(size as u32) {
        felts.push(trace.read_memory_element(element_addr + i).unwrap_or_default());
    }
    Some(<T as FromMidenRepr>::from_felts(&felts))
}
