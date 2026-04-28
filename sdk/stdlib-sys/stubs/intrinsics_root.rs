#![no_std]

/// Produces a return value for an opaque linker stub.
trait StubRet {
    fn from_sinks(int_sink: u64, float_sink: f32) -> Self;
}

impl StubRet for () {
    fn from_sinks(_int_sink: u64, _float_sink: f32) -> Self {}
}

impl StubRet for f32 {
    fn from_sinks(int_sink: u64, _float_sink: f32) -> Self {
        Self::from_bits(int_sink as u32)
    }
}

impl StubRet for i32 {
    fn from_sinks(int_sink: u64, _float_sink: f32) -> Self {
        int_sink as u32 as i32
    }
}

impl StubRet for u64 {
    fn from_sinks(int_sink: u64, _float_sink: f32) -> Self {
        int_sink
    }
}

impl StubRet for usize {
    fn from_sinks(int_sink: u64, _float_sink: f32) -> Self {
        int_sink as usize
    }
}

/// Returns an opaque value from a linker stub.
#[track_caller]
#[inline(never)]
fn stub<T: StubRet>() -> T {
    core::hint::black_box(core::panic::Location::caller());
    let int_sink = unsafe { core::ptr::read_volatile(core::ptr::addr_of!(STUB_INT_ARG_SINK)) };
    let float_sink =
        unsafe { core::ptr::read_volatile(core::ptr::addr_of!(STUB_FLOAT_ARG_SINK.float)) };
    core::hint::black_box(T::from_sinks(int_sink, float_sink))
}

static mut STUB_INT_ARG_SINK: u64 = 0;

/// A typed sink used to keep floating-point stub dependencies visible in Wasm.
union StubF32Sink {
    int: u32,
    float: f32,
}

static mut STUB_FLOAT_ARG_SINK: StubF32Sink = StubF32Sink { int: 0 };

/// Records an integer-like argument in the opaque stub sinks.
fn write_int_sink(value: u64) {
    unsafe {
        core::ptr::write_volatile(core::ptr::addr_of_mut!(STUB_INT_ARG_SINK), value);
        core::ptr::write_volatile(
            core::ptr::addr_of_mut!(STUB_FLOAT_ARG_SINK.int),
            value as u32,
        );
    }
}

/// Records a floating-point argument as its raw bit pattern.
fn write_float_sink(value: f32) {
    unsafe {
        core::ptr::write_volatile(core::ptr::addr_of_mut!(STUB_INT_ARG_SINK), value.to_bits().into());
    }
}

/// Records a linker stub argument in an opaque sink.
trait StubArg {
    fn write_sink(self);
}

impl StubArg for f32 {
    fn write_sink(self) {
        write_float_sink(self);
    }
}

impl StubArg for i32 {
    fn write_sink(self) {
        write_int_sink(self as u32 as u64);
    }
}

impl StubArg for u32 {
    fn write_sink(self) {
        write_int_sink(self.into());
    }
}

impl StubArg for i64 {
    fn write_sink(self) {
        write_int_sink(self as u64);
    }
}

impl StubArg for u64 {
    fn write_sink(self) {
        write_int_sink(self);
    }
}

impl StubArg for usize {
    fn write_sink(self) {
        write_int_sink(self as u64);
    }
}

impl<T> StubArg for *const T {
    fn write_sink(self) {
        write_int_sink(self as usize as u64);
    }
}

impl<T> StubArg for *mut T {
    fn write_sink(self) {
        write_int_sink(self as usize as u64);
    }
}

/// Keeps a linker stub argument live across LTO.
#[inline(never)]
fn stub_arg<T: StubArg>(arg: T) {
    arg.write_sink();
}

macro_rules! define_stub {
    (
        $(#[$meta:meta])*
        pub extern "C" fn $name:ident($($arg:ident : $ty:ty),* $(,)?) $(-> $ret:ty)?;
    ) => {
        $(#[$meta])*
        #[inline(never)]
        pub extern "C" fn $name($($arg: $ty),*) $(-> $ret)? {
            $(crate::stub_arg($arg);)*
            crate::stub()
        }
    };
}

mod intrinsics;
