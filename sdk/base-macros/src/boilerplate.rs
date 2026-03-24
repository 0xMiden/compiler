use proc_macro2::TokenStream as TokenStream2;
use quote::quote;

/// Generates the shared runtime scaffolding required by no_std
pub(crate) fn runtime_boilerplate() -> TokenStream2 {
    quote! {
        extern crate alloc as __miden_runtime_alloc_crate;

        #[doc = "Global allocator for Miden VM"]
        #[global_allocator]
        static __MIDEN_RUNTIME_ALLOCATOR: ::miden::BumpAlloc = ::miden::BumpAlloc::new();

        #[cfg(target_family = "wasm")]
        #[doc = "Canonical ABI realloc export required by generated component bindings when an indirect pointer is passed"]
        #[unsafe(export_name = "cabi_realloc")]
        unsafe extern "C" fn __miden_runtime_cabi_realloc(
            old_ptr: *mut u8,
            old_len: usize,
            align: usize,
            new_len: usize,
        ) -> *mut u8 {
            use __miden_runtime_alloc_crate::alloc::{
                Layout, alloc as allocate, handle_alloc_error, realloc,
            };

            let layout;
            let ptr = if old_len == 0 {
                if new_len == 0 {
                    return align as *mut u8;
                }

                layout = unsafe { Layout::from_size_align_unchecked(new_len, align) };
                unsafe { allocate(layout) }
            } else {
                debug_assert_ne!(new_len, 0, "non-zero old_len requires non-zero new_len!");
                layout = unsafe { Layout::from_size_align_unchecked(old_len, align) };
                unsafe { realloc(old_ptr, layout, new_len) }
            };

            if ptr.is_null() {
                if cfg!(debug_assertions) {
                    handle_alloc_error(layout);
                } else {
                    core::arch::wasm32::unreachable();
                }
            }

            ptr
        }

        #[cfg(not(test))]
        #[doc = "Panic handler used when building for Miden VM"]
        #[panic_handler]
        #[allow(clippy::empty_loop)]
        fn __miden_runtime_panic_handler(_info: &::core::panic::PanicInfo) -> ! {
            #[cfg(target_family = "wasm")]
            core::arch::wasm32::unreachable();

            #[cfg(not(target_family = "wasm"))]
            loop {}
        }

        #[cfg(not(test))]
        #[doc = "Allocation error handler used when building for Miden VM"]
        #[alloc_error_handler]
        #[allow(clippy::empty_loop)]
        fn __miden_runtime_alloc_error_handler(_layout: ::core::alloc::Layout) -> ! {
            #[cfg(target_family = "wasm")]
            core::arch::wasm32::unreachable();

            #[cfg(not(target_family = "wasm"))]
            loop {}
        }
    }
}
