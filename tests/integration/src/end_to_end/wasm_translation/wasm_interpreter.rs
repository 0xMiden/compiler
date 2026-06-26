use wasmi::{Engine, Error as WasmError, Instance, Linker, Store, WasmParams, WasmResults};

/// An interpreter for a Wasm module that exports an `entrypoint` function with signature
/// `(i32, i32) -> i32`.
///
/// The [`Store`] keeps the [`Engine`] alive for the lifetime of the instance, so the engine does
/// not need to be stored separately.
pub(super) struct WasmInterpreter {
    store: Store<()>,
    instance: Instance,
}

impl WasmInterpreter {
    pub(super) fn new(module: &[u8]) -> Self {
        let engine = Engine::default();
        let module =
            wasmi::Module::new(&engine, module).expect("failed to validate/compile wasm module");
        let mut store = Store::new(&engine, ());
        let linker = Linker::<()>::new(&engine);
        let instance = linker
            .instantiate_and_start(&mut store, &module)
            .expect("failed to instantiate wasm module");
        Self { store, instance }
    }

    /// Invoke the function with the given `params` and return its result.
    ///
    /// Returns an error if the call traps.
    pub(super) fn call_entrypoint<P, R>(&mut self, fn_name: &str, params: P) -> Result<R, WasmError>
    where
        P: WasmParams,
        R: WasmResults,
    {
        let func = self
            .instance
            .get_typed_func::<P, R>(&self.store, fn_name)
            .expect("module must export {fn_name} with a signature matching the requested types");
        func.call(&mut self.store, params)
    }
}
