//! Module for Miden SDK macros
//!
//! ### How to use WIT generation.
//!
//! An account component is written in three parts:
//!
//! 1. A `#[component_storage]` struct declaring the component's `#[storage(...)]` fields.
//! 2. A `#[component]` `trait` declaring the component's API. The trait name yields the WIT
//!    interface name and its methods yield the exported functions.
//! 3. A `#[component]` `impl Trait for Storage` block providing the behavior.
//!
//! Add `#[export_type]` on every type that is used in an exported method signature.
//!
//! Example:
//! ```rust,ignore
//!
//! #[export_type]
//! pub struct StructA {
//!     pub foo: Word,
//!     pub asset: Asset,
//! }
//!
//! #[export_type]
//! pub struct StructB {
//!     pub bar: Felt,
//!     pub baz: Felt,
//! }
//!
//! #[component_storage]
//! struct MyAccountStorage;
//!
//! #[component]
//! trait MyAccount {
//!     fn foo(&self, a: StructA) -> StructB;
//! }
//!
//! #[component]
//! impl MyAccount for MyAccountStorage {
//!     fn foo(&self, a: StructA) -> StructB {
//!         ...
//!     }
//! }
//! ```
//!
//! Custom `#[export_type]` types referenced in component method signatures must be nameable from
//! the crate root (declared at, or `use`-imported into, the crate root).
//!

//! ### Escape hatch (disable WIT generation)
//!
//! in a small fraction of the cases where the WIT generation is not possible (think a type defined
//! only in an external WIT file) or not desirable the WIT generation can be disabled:
//!
//! To disable WIT interface generation:
//! - Don't use the `#[component]` trait/impl macros; keep `#[component_storage]` on the storage
//!   struct;
//!
//! To use manually crafted WIT interface:
//! - Put the WIT file in the `wit` folder;
//! - call `miden::generate!();` and `bindings::export!(MyAccountStorage);`
//! - implement `impl Guest for MyAccountStorage`;

use crate::script::ScriptConfig;

extern crate proc_macro;

mod account_component_metadata;
mod boilerplate;
mod component_macro;
mod dependency_ref;
mod export_type;
mod foreign_account;
mod fpi;
mod generate;
mod manifest_paths;
mod note;
mod script;
mod types;
mod util;
mod wit_builder;
mod wit_world;

/// Defines an account component's API and generates the WIT interface.
///
/// Apply `#[component]` to a `trait` (the API, and the source of the WIT interface — its name yields
/// the interface name) and to the matching `impl Trait for Storage` block (the behavior). Storage
/// lives on a separate `#[component_storage]` struct.
///
/// Both the trait and the implementation block must carry `#[component]`, and the storage struct
/// must carry `#[component_storage]`. A missing trait annotation surfaces as a missing-item error
/// naming `__MIDEN_COMPONENT_TRAIT_MARKER`, and a missing storage annotation as one naming
/// `__MIDEN_COMPONENT_STORAGE_MARKER` — hidden constants those expansions inject and the
/// implementation expansion checks for.
///
/// **NOTE:** Mark each type used in an exported method with the `#[export_type]` attribute macro.
///
/// # Sibling component calls
///
/// An account may be deployed with several components. To call the other ("sibling") components
/// of the same account, list them on the component trait as `package::Interface` references — the
/// Rust-style Miden package name (replace `-` with `_`) followed by the sibling's exported WIT
/// interface in UpperCamelCase. Each reference generates a `pub trait` named after the interface
/// whose default methods call the sibling through the Wasm component-model boundary (a
/// cross-context `call`, the same mechanism note scripts use to call the account). The generated
/// traits attach to `#[component_storage]` structs automatically, and may be declared as
/// supertraits of the component trait to make the dependency part of its API:
///
/// ```rust,ignore
/// use miden::{component, component_storage, native_account::NativeAccount, Asset};
///
/// #[component_storage]
/// struct MyComponentStorage;
///
/// // Generates `trait Pausable` and `trait CounterContract` with default methods that
/// // call the sibling components deployed on the same account.
/// #[component(pausable::Pausable, counter_contract::CounterContract)]
/// trait MyComponent: NativeAccount + Pausable + CounterContract {
///     fn receive_asset(&mut self, asset: Asset);
/// }
///
/// #[component] // the implementation block takes no arguments
/// impl MyComponent for MyComponentStorage {
///     fn receive_asset(&mut self, asset: Asset) {
///         assert!(!self.is_paused()); // sibling call into `pausable`
///         self.increment_count();     // sibling call into `counter-contract`
///         self.add_asset(asset);      // native account built-in
///     }
/// }
/// ```
///
/// Each referenced package must be declared as a dependency in `miden-project.toml`, and the
/// account must be deployed with the sibling components for the calls to resolve at runtime.
///
/// # Foreign Procedure Invocation (FPI)
///
/// Use `#[account(...)]` on an empty struct to generate typed account wrappers for account
/// dependencies. Each dependency is referenced as `package::Interface`: the package is the
/// Rust-style Miden package name (write the Miden package name as a Rust identifier by replacing
/// `-` with `_`) and the interface names the dependency's exported WIT interface in
/// UpperCamelCase.
///
/// ```rust,ignore
/// use miden::{account, component, component_storage, AccountId, Felt};
///
/// #[account(counter_contract::CounterContract)]
/// struct CounterContract;
///
/// #[component_storage]
/// struct CallerAccountStorage;
///
/// #[component]
/// trait CallerAccount {
///     fn read_counter(&self, counter_account_id: AccountId) -> Felt;
/// }
///
/// #[component]
/// impl CallerAccount for CallerAccountStorage {
///     fn read_counter(&self, counter_account_id: AccountId) -> Felt {
///         let counter = CounterContract::new(counter_account_id);
///         counter.get_count()
///     }
/// }
/// ```
///
/// The generated methods invoke the active account by default. Wrappers created with
/// `new(AccountId)` invoke a foreign account through the transaction kernel's
/// `execute_foreign_procedure` operation; the foreign account must be deployed with code matching
/// the dependency package used while compiling the caller.
///
/// To disable WIT interface generation:
/// - don't use the `#[component]` trait/impl macros; keep `#[component_storage]` on the storage
///   struct;
///
/// To use manually crafted WIT interface:
/// - put WIT interface file in the `wit` folder;
/// - call `miden::generate!();` and `bindings::export!(MyAccountStorage);`
/// - implement `impl Guest for MyAccountStorage`;
#[proc_macro_attribute]
pub fn component(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    component_macro::component(attr, item)
}

/// Wires storage metadata for an account component's storage struct.
///
/// Apply this to the `struct` that declares the component's `#[storage(...)]` fields. It generates
/// the `Default` implementation and implements the account traits so the component's methods can
/// access storage and account operations. Use it together with a `#[component]` trait (the API) and
/// a `#[component]` trait implementation (the behavior).
///
/// ```rust,ignore
/// use miden::{StorageValue, Word, component, component_storage};
///
/// #[component_storage]
/// struct MyComponentStorage {
///     #[storage(description = "some field")]
///     foo: StorageValue<Word>,
/// }
///
/// #[component]
/// trait MyComponent {
///     fn get_foo(&self) -> Word;
/// }
///
/// #[component]
/// impl MyComponent for MyComponentStorage {
///     fn get_foo(&self) -> Word {
///         self.foo.get()
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn component_storage(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    component_macro::component_storage(attr, item)
}

/// Generates typed active and foreign account bindings for account dependencies on an empty
/// wrapper struct.
///
/// The attribute accepts `package::Interface` references. Write the Miden package name as a Rust
/// identifier by replacing `-` with `_`, followed by the dependency's exported WIT interface in
/// UpperCamelCase. For example, the `counter-contract` interface of a dependency named
/// `counter-contract` is requested with `#[account(counter_contract::CounterContract)]`.
#[proc_macro_attribute]
pub fn account(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    foreign_account::expand(attr, item)
}

/// Marks a component method as the authentication procedure entrypoint (`#[auth_script]`).
///
/// The method must be declared within a `trait` annotated with `#[component]`.
/// Authentication components must annotate exactly one method with `#[auth_script]`.
/// At most one method in a crate may be annotated with `#[auth_script]`.
#[proc_macro_attribute]
pub fn auth_script(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    component_macro::expand_auth_script(attr, item)
}

/// Generates an equvalent type in the WIT interface.
/// Required for every type mentioned in the public methods of an account component.
///
/// Intended to be used together with `#[component]` attribute macro.
#[proc_macro_attribute]
pub fn export_type(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    export_type::expand(attr, item)
}

/// Marks a type/impl as a note script definition.
///
/// This attribute is intended to be used on:
/// - a note input type definition (`struct MyNote { ... }`)
/// - the associated inherent `impl` block that contains an entrypoint method annotated with
///   `#[note_script]`
///
/// # Foreign Procedure Invocation (FPI)
///
/// Use `#[account(...)]` on an empty struct to generate typed active and foreign account wrappers
/// for account dependencies. Each dependency is referenced as `package::Interface`: the
/// Rust-style Miden package name (replace `-` with `_`) followed by the dependency's exported WIT
/// interface in UpperCamelCase.
///
/// ```rust,ignore
/// use miden::*;
///
/// #[account(counter_contract::CounterContract)]
/// struct CounterContract;
///
/// #[note]
/// struct CounterCaller {
///     counter_account_id: AccountId,
/// }
///
/// #[note]
/// impl CounterCaller {
///     #[note_script]
///     pub fn run(self, _arg: Word) {
///         let counter = CounterContract::new(self.counter_account_id);
///         let count = counter.get_count();
///         assert_eq(count, felt!(1));
///     }
/// }
/// ```
///
/// The generated methods invoke the active account when the wrapper is passed to the note
/// entrypoint. Wrappers created with `new(AccountId)` invoke a foreign account through the
/// transaction kernel's `execute_foreign_procedure` operation; the foreign account must be
/// deployed with code matching the dependency package used while compiling the note.
///
/// # Example
///
/// The note's native (active) account is declared with `#[account(...)]`, listing the account
/// component packages whose methods should be available on it.
///
/// ```rust,ignore
/// use miden::*;
///
/// #[account(basic_wallet::BasicWallet)]
/// struct Wallet;
///
/// #[note]
/// struct MyNote {
///     recipient: AccountId,
/// }
///
/// #[note]
/// impl MyNote {
///     /// Exported note constructor: creates this note as an output note of the current
///     /// transaction and returns its index.
///     #[note_constructor]
///     pub fn create(recipient: AccountId, tag: Tag, note_type: NoteType, serial_num: Word) -> NoteIdx {
///         let inputs = MyNote { recipient };
///         let recipient = note::build_recipient(
///             serial_num,
///             note::get_entrypoint_root(),
///             inputs.to_felt_repr(),
///         );
///         output_note::create(tag, note_type, recipient)
///     }
///
///     #[note_script]
///     pub fn run(self, _arg: Word, account: &mut Wallet) {
///         assert_eq!(account.get_id(), self.recipient);
///     }
/// }
/// ```
///
/// # Note constructors
///
/// Methods annotated with `#[note_constructor]` are exported through the note's WIT interface
/// as note constructors. Other Miden packages — e.g. transaction scripts — can declare the note
/// package as a dependency and create the note by calling its constructor. Unannotated methods
/// stay plain Rust helpers and are not exported.
///
/// The note input struct also implements [`ToFeltRepr`](miden_field_repr::ToFeltRepr)
/// (mirroring the generated storage decoding), so constructors can serialize the note inputs
/// when computing the note recipient.
#[proc_macro_attribute]
pub fn note(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    note::expand_note(attr, item)
}

/// Marks a method as the note script entrypoint (`#[note_script]`).
///
/// The method must be contained within an inherent `impl` block annotated with `#[note]`.
/// At most one method in a crate may be annotated with `#[note_script]`.
/// The exported component procedure keeps the annotated method name (converted to WIT kebab-case).
///
/// # Supported entrypoint signature
///
/// - Receiver must be plain `self` (by value); `&self`, `&mut self`, `mut self`, and typed
///   receivers (e.g. `self: Box<Self>`) are not supported.
/// - The method must return `()`.
/// - Excluding `self`, the method must accept:
///   - exactly one `Word` argument, and
///   - optionally a single reference to an `#[account(...)]` type (`&MyAccount` or `&mut
///     MyAccount`, in either order).
/// - Generic methods and `async fn` are not supported.
#[proc_macro_attribute]
pub fn note_script(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    note::expand_note_script(attr, item)
}

/// Marks a method as an exported note constructor (`#[note_constructor]`).
///
/// The method must be contained within an inherent `impl` block annotated with `#[note]`. It is
/// exported through the note's WIT interface (named by the kebab-cased method name), so other
/// Miden packages — e.g. transaction scripts — can declare the note package as a dependency and
/// create the note by calling the constructor.
///
/// # Supported constructor signature
///
/// - The method must be `pub` and must not take `self`: constructors run before the note exists
///   (typically computing the note recipient via `note::get_entrypoint_root()` and calling
///   `output_note::create`).
/// - Parameter and return types are limited to SDK core types (e.g. `Felt`, `Word`, `AccountId`,
///   `Tag`, `NoteType`, `NoteIdx`) and primitives.
/// - Generic, `const`, `async`, `unsafe`, `extern`, and variadic methods are not supported.
#[proc_macro_attribute]
pub fn note_constructor(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    note::expand_note_constructor(attr, item)
}

/// Marks the function as a transaction script
#[proc_macro_attribute]
pub fn tx_script(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    script::expand(
        attr,
        item,
        ScriptConfig {
            export_interface: "miden:base/transaction-script@1.0.0",
            guest_trait_path: "self::bindings::exports::miden::base::transaction_script::Guest",
        },
    )
}

/// Generate bindings for an input WIT document.
///
/// The macro here will parse [WIT] as input and generate Rust bindings to work with the `world`
/// that's specified in the [WIT]. For a primer on WIT see [this documentation][WIT] and for a
/// primer on worlds see [here][worlds].
///
/// [WIT]: https://component-model.bytecodealliance.org/design/wit.html
/// [worlds]: https://component-model.bytecodealliance.org/design/worlds.html
///
/// For documentation on each option, see below.
///
/// ## Exploring generated bindings
///
/// Once bindings have been generated they can be explored via a number of means
/// to see what was generated:
///
/// * Using `cargo doc` should render all of the generated bindings in addition
///   to the original comments in the WIT format itself.
/// * If your IDE supports `rust-analyzer` code completion should be available
///   to explore and see types.
///
/// ## Namespacing
///
/// The generated bindings are put in `bindings` module.
/// In WIT, worlds can import and export `interface`s, functions, and types. Each
/// `interface` can either be "anonymous" and only named within the context of a
/// `world` or it can have a "package ID" associated with it. Names in Rust take
/// into account all the names associated with a WIT `interface`. For example
/// the package ID `foo:bar/baz` would create a `mod foo` which contains a `mod
/// bar` which contains a `mod baz`.
///
/// WIT imports and exports are additionally separated into their own
/// namespaces. Imports are generated at the level of the `generate!` macro
/// where exports are generated under an `exports` namespace.
///
/// ## Exports: The `export!` macro
///
/// Components are created by having exported WebAssembly functions with
/// specific names, and these functions are not created when `generate!` is
/// invoked. Instead these functions are created afterwards once you've defined
/// your own type an implemented the various `trait`s for it. The
/// `#[unsafe(no_mangle)]` functions that will become the component are created
/// with the generated `export!` macro.
///
/// Each call to `generate!` will itself generate a macro called `export!`.
/// The macro's first argument is the name of a type that implements the traits
/// generated:
///
/// ```rust,ignore
/// use miden::generate;
///
/// generate!({
///     inline: r#"
///         package my:test;
///
///         world my-world {
/// #           export hello: func();
///             // ...
///         }
///     "#,
/// });
///
/// struct MyComponent;
///
/// impl Guest for MyComponent {
/// #   fn hello() {}
///     // ...
/// }
///
/// export!(MyComponent);
/// #
/// # fn main() {}
/// ```
///
/// This argument is a Rust type which implements the `Guest` traits generated
/// by `generate!`. Note that all `Guest` traits must be implemented for the
/// type provided or an error will be generated.
///
/// ## Options to `generate!`
///
/// The full list of options that can be passed to the `generate!` macro are as
/// follows. Note that there are no required options, they all have default
/// values.
///
///
/// ```rust,ignore
/// use miden::generate;
/// # macro_rules! generate { ($($t:tt)*) => () }
///
/// generate!({
///     // Enables passing "inline WIT". If specified this is the default
///     // package that a world is selected from. Any dependencies that this
///     // inline WIT refers to must be defined in the `path` option above.
///     //
///     // By default this is not specified.
///     inline: "
///         world my-world {
///             import wasi:cli/imports;
///
///             export my-run: func()
///         }
///     ",
///
///     // When generating bindings for interfaces that are not defined in the
///     // same package as `world`, this option can be used to either generate
///     // those bindings or point to already generated bindings.
///     // For example, if your world refers to WASI types then the `wasi` crate
///     // already has generated bindings for all WASI types and structures. In this
///     // situation the key `with` here can be used to use those types
///     // elsewhere rather than regenerating types.
///     // If for example your world refers to some type and you want to use
///     // your own custom implementation of that type then you can specify
///     // that here as well. There is a requirement on the remapped (custom)
///     // type to have the same internal structure and identical to what would
///     // wit-bindgen generate (including alignment, etc.), since
///     // lifting/lowering uses its fields directly.
///     //
///     // If, however, your world refers to interfaces for which you don't have
///     // already generated bindings then you can use the special `generate` value
///     // to have those bindings generated.
///     //
///     // The `with` key here works for interfaces and individual types.
///     //
///     // When an interface or type is specified here no bindings will be
///     // generated at all. It's assumed bindings are fully generated
///     // somewhere else. This is an indicator that any further references to types
///     // defined in these interfaces should use the upstream paths specified
///     // here instead.
///     //
///     // Any unused keys in this map are considered an error.
///     with: {
///         "wasi:io/poll": wasi::io::poll,
///         "some:package/my-interface": generate,
///         "some:package/my-interface/my-type": my_crate::types::MyType,
///     },
/// });
/// ```
///
#[proc_macro]
pub fn generate(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    generate::expand(input)
}
