use proc_macro2::TokenStream as TokenStream2;
use quote::quote;

/// Generates the shared runtime scaffolding required by no_std
pub(crate) fn runtime_boilerplate() -> TokenStream2 {
    quote! {
        #[doc = "Global allocator for Miden VM"]
        #[global_allocator]
        static __MIDEN_RUNTIME_ALLOCATOR: ::miden::BumpAlloc = ::miden::BumpAlloc::new();

        #[cfg(not(test))]
        #[doc = "Panic handler used when building for Miden VM"]
        #[panic_handler]
        #[allow(clippy::empty_loop)]
        fn __miden_runtime_panic_handler(_info: &::core::panic::PanicInfo) -> ! {
            loop {}
        }
    }
}
