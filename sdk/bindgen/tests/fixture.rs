//! Golden tests over in-test fixture packages: the generated bindings and stubs are compared with
//! the committed expectations in `expected/` (`UPDATE_EXPECT=1` rewrites them), must parse as
//! Rust, and the bindings must compile on the host.
//!
//! The fixture package has a nested module. `midenc_package_interface::testing::assemble_fixture`
//! assembles a single module, so the fixture is assembled here with the assembler itself.

use std::{collections::BTreeMap, process::Command, sync::Arc};

use miden_assembly_syntax::{
    ModuleParser,
    ast::{
        Path,
        types::{EnumType, StructType, Type, Variant},
    },
    debuginfo::{DefaultSourceManager, SourceLanguage, SourceManager, Uri},
};
use miden_sdk_bindgen::{Error, External, Options, Skipped, generate};
use midenc_expect_test::expect_file;
use midenc_package_interface::{
    ConstantItem, PackageInterface, SkipReason, TypeItem,
    testing::{FIXTURE_SOURCE, assemble_fixture},
};

/// The fixture's root module, `::fixture`.
///
/// The syntax the assembler (0.35) accepts, as settled here: a submodule is declared by its parent
/// (`pub mod nested`); an enum is `pub enum Kind : u8 { … }`, and its variants are exported as
/// constants of the enum's module too (`PRIVATE`, `PUBLIC`); a struct's representation is an
/// annotation after `struct` (`@align(16)`, `@packed`, `@transparent`); a hashed constant is
/// `word("…")` or `event("…")`, a word literal `[a, b, c, d]`. An integer constant is the
/// narrowest of `u8`, `u16`, `u32` and `felt` that holds it.
const ROOT_SOURCE: &str = r#"
pub mod nested

pub type Pair = struct { a: felt, b: felt }
pub type Tag = u16
pub type Key = word
pub type Cursor = ptr<felt, addrspace(felt)>
pub enum Kind : u8 { PRIVATE = 0, PUBLIC = 1 }
pub type Boxed = struct { count: u8, data: word }
pub type Words = struct { lo: word, hi: word }
pub type Outer = struct { inner: struct { x: felt, y: u32 }, tag: u8 }
pub type Pairs = [struct { x: felt }; 2]
pub type Aligned = struct @align(16) { a: felt }
pub type Spaced = struct @align(16) { a: felt, b: felt }
pub type Packed = struct @packed { a: u8, b: felt }
pub type Wrapper = struct @transparent { inner: felt }
pub type Stamp = struct { tag: u32, at: u64 }
pub type WordStamp = struct { w: word, at: u64 }

pub const LIMIT = 1024
pub const SLOT = word("fixture::slot")
pub const EVT = event("fixture::evt")
pub const BIG = 0x1fffffffffffffff
pub const SMALL = 7
pub const WIDE = 70000
pub const COUNTS = [1, 2, 3, 4]
pub const NAME = "fixture"

pub proc id(x: felt) -> felt
    nop
end

pub proc owner() -> struct { suffix: felt, prefix: felt }
    nop
end

pub proc set_owner(owner: struct { suffix: felt, prefix: felt })
    nop
end

pub proc make_pair(a: felt, b: felt) -> Pair
    nop
end

pub proc take_pair(p: Pair)
    drop drop
end

pub proc hash(w: word) -> word
    nop
end

pub proc tag_of(x: felt) -> Tag
    nop
end

pub proc write(p: ptr<u32, addrspace(felt)>, n: u32)
    drop drop
end

pub proc cursor() -> ptr<felt, addrspace(felt)>
    nop
end

pub proc bytes(p: ptr<u8, addrspace(byte)>) -> ptr<u8, addrspace(byte)>
    nop
end

pub proc kind_of(k: Kind) -> Kind
    nop
end

pub proc is_set(b: i1) -> i1
    nop
end

pub proc wide(x: u64) -> u64
    nop
end

pub proc boxed(b: Boxed) -> Boxed
    nop
end

pub proc swap(w: Words) -> Words
    nop
end

pub proc many() -> (word, u8)
    nop
end

pub proc spaced() -> Spaced
    nop
end

pub proc stamp(s: Stamp) -> Stamp
    nop
end

pub proc word_stamp(s: WordStamp)
    dropw drop drop
end

pub proc delta(x: i8) -> i16
    nop
end

pub proc nothing() -> [felt; 0]
    nop
end

pub proc untyped
    nop
end

@account_procedure
pub proc role(x: felt) -> felt
    nop
end
"#;

/// The fixture's nested module, `::fixture::nested`, which refers to the root's types. A pointer
/// is in element space unless it says otherwise.
const NESTED_SOURCE: &str = r#"
use ::fixture

pub type Inner = struct {
    pair: fixture::Pair,
    kind: fixture::Kind,
    at: ptr<fixture::Pair, addrspace(felt)>,
    bytes: ptr<u8, addrspace(byte)>,
}

pub const DEPTH = 2

pub proc wrap(p: fixture::Pair, k: fixture::Kind) -> Inner
    nop
end

pub proc unwrap(inner: Inner) -> (fixture::Pair, fixture::Kind)
    nop
end

pub proc outer() -> fixture::Outer
    nop
end
"#;

/// Assemble a library package named `name` from `modules` (path, source), the first the root.
fn assemble(name: &str, modules: &[(&str, &str)]) -> PackageInterface {
    let source_manager: Arc<dyn SourceManager> = Arc::new(DefaultSourceManager::default());
    let mut parsed = modules
        .iter()
        .map(|(path, source)| {
            let uri = Uri::from(path.to_string().into_boxed_str());
            let file = source_manager.load(SourceLanguage::Masm, uri, source.to_string());
            ModuleParser::new(None)
                .parse(Some(Path::new(path)), file, source_manager.clone())
                .unwrap_or_else(|err| panic!("`{path}` must parse: {err}"))
        })
        .collect::<Vec<_>>();
    let root = parsed.remove(0);
    let package = miden_assembly::Assembler::new(source_manager)
        .assemble_library(name, root, parsed)
        .unwrap_or_else(|err| panic!("the fixture must assemble: {err}"));
    PackageInterface::from_package(&package)
}

fn fixture() -> PackageInterface {
    assemble("fixture", &[("::fixture", ROOT_SOURCE), ("::fixture::nested", NESTED_SOURCE)])
}

fn options(root: &str) -> Options {
    Options {
        root: root.into(),
        support: "crate::__support".into(),
        with: BTreeMap::new(),
    }
}

/// The bindings: the module tree, the types and constants, and a wrapper for every bindable
/// procedure. The procedure without a signature, the procedure with a zero-sized result and the
/// string constant are reported; the role procedure produces nothing.
#[test]
fn bindings_of_the_fixture_package() {
    let iface = fixture();
    let generated = generate(&iface, &[], &options("::fixture")).unwrap();
    expect_file!["./expected/fixture_procedures.rs"].assert_eq(&generated.bindings);
    syn::parse_file(&generated.bindings).expect("generated bindings must parse as Rust");
    assert_eq!(
        generated.skipped,
        [
            Skipped {
                path: "::fixture::untyped".into(),
                reason: SkipReason::Untyped.to_string(),
            },
            Skipped {
                path: "::fixture::NAME".into(),
                reason: "string constants have no Rust form".into(),
            },
            Skipped {
                path: "::fixture::nothing".into(),
                reason: "its results are zero-sized: there is no value for the wrapper to return"
                    .into(),
            },
        ]
    );
    assert!(!generated.bindings.contains("fn role"), "a role procedure is bound another way");
}

/// The stubs: one weak definition for every symbol the bindings declare, which is every bindable
/// procedure the bindings did not skip.
#[test]
fn stubs_of_the_fixture_package() {
    let iface = fixture();
    let generated = generate(&iface, &[], &options("::fixture")).unwrap();
    expect_file!["./expected/fixture_stubs.rs"].assert_eq(&generated.stubs);
    syn::parse_file(&generated.stubs).expect("generated stubs must parse as Rust");
    let declared = generated.bindings.matches("#[link_name = ").count();
    let defined = generated.stubs.matches("#[unsafe(export_name = ").count();
    let bound = iface.bindable().count() - 1;
    assert_eq!((declared, defined), (bound, bound));
    assert!(!generated.stubs.contains("fixture__nothing"));
}

/// A host stand-in for the support module: what the bindings need of `Felt`, `Word`,
/// `ElementPtr`, `WordAligned`, `FeltConstant` and `WordConstant`, with the Miden target's sizes
/// and alignments.
const HOST_SUPPORT: &str = r#"
pub mod __support {
    use core::marker::PhantomData;

    #[repr(transparent)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Felt(pub u32);

    #[repr(C, align(16))]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Word(pub [Felt; 4]);

    impl Word {
        pub const fn new(felts: [Felt; 4]) -> Self {
            Self(felts)
        }
    }

    impl core::ops::Index<usize> for Word {
        type Output = Felt;

        fn index(&self, index: usize) -> &Felt {
            &self.0[index]
        }
    }

    #[repr(transparent)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ElementPtr<T> {
        addr: u32,
        _marker: PhantomData<*mut T>,
    }

    impl<T> ElementPtr<T> {
        pub const fn new(addr: u32) -> Self {
            Self { addr, _marker: PhantomData }
        }

        pub const fn addr(self) -> u32 {
            self.addr
        }
    }

    #[repr(C, align(32))]
    pub struct WordAligned<T>(T);

    impl<T> WordAligned<T> {
        pub const fn new(value: T) -> Self {
            Self(value)
        }

        pub fn into_inner(self) -> T {
            self.0
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct FeltConstant(pub u64);

    impl FeltConstant {
        pub const fn new(canonical: u64) -> Self {
            Self(canonical)
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct WordConstant(pub [u64; 4]);

    impl WordConstant {
        pub const fn new(canonical: [u64; 4]) -> Self {
            Self(canonical)
        }
    }
}
"#;

/// What the host build asserts of the generated layouts. A 64-bit integer is 8-aligned on the
/// host as on wasm32, so `Stamp` has its HIR layout (`at` at 4, size 12) only packed to 4, and
/// `WordStamp` its HIR alignment (4; `at` at 16, size 24) only packed to 4, its `[felt; 4]` as
/// `[Felt; 4]`.
const HOST_LAYOUT_ASSERTIONS: &str = r#"
const _: () = assert!(::core::mem::size_of::<fixture::Stamp>() == 12);
const _: () = assert!(::core::mem::offset_of!(fixture::Stamp, at) == 4);
const _: () = assert!(::core::mem::align_of::<fixture::Stamp>() == 4);
const _: () = assert!(::core::mem::size_of::<fixture::WordStamp>() == 24);
const _: () = assert!(::core::mem::offset_of!(fixture::WordStamp, at) == 16);
const _: () = assert!(::core::mem::align_of::<fixture::WordStamp>() == 4);
"#;

/// The generated bindings compile on the host, against [`HOST_SUPPORT`] and nothing else, with
/// every warning an error: the host bodies, the types and the constants, and the layouts are
/// those [`HOST_LAYOUT_ASSERTIONS`] asserts. (The Miden target's bodies are compiled where the
/// SDK's own crates are built for it.)
#[test]
fn the_bindings_compile_on_the_host() {
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    if Command::new(&rustc).arg("--version").output().is_err() {
        println!("skipped: no `rustc` to compile the generated bindings with");
        return;
    }
    let generated = generate(&fixture(), &[], &options("::fixture")).unwrap();
    let dir =
        std::env::temp_dir().join(format!("miden-sdk-bindgen-host-check-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("bindings.rs"), &generated.bindings).unwrap();
    let root = format!(
        "{HOST_SUPPORT}\n#[allow(non_camel_case_types, non_snake_case, \
         clippy::too_many_arguments)]\npub mod fixture {{\n    \
         include!(\"bindings.rs\");\n}}\n{HOST_LAYOUT_ASSERTIONS}"
    );
    std::fs::write(dir.join("lib.rs"), root).unwrap();
    let output = Command::new(&rustc)
        .args([
            "--edition",
            "2024",
            "--crate-type",
            "lib",
            "--emit",
            "metadata",
            "-D",
            "warnings",
        ])
        .arg("--out-dir")
        .arg(&dir)
        .arg(dir.join("lib.rs"))
        .output()
        .unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(
        output.status.success(),
        "the generated bindings must compile on the host:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A skipped procedure is reported with the reason the interface gives; a role procedure is
/// not bindable either, but it is not reported: it is bound another way.
#[test]
fn skipped_procedures_are_reported() {
    let package = assemble_fixture("fixture", "::fixture", FIXTURE_SOURCE);
    let iface = PackageInterface::from_package(&package);
    let generated = generate(&iface, &[], &options("::fixture")).unwrap();
    assert_eq!(
        generated.skipped,
        [Skipped {
            path: "::fixture::untyped".into(),
            reason: "no typed signature in the package manifest".into(),
        }]
    );
}

/// A package whose root lies below the generated root's own parent: the modules above the root
/// (`::miden` above `::miden::protocol`) are left out of the tree.
#[test]
fn the_modules_above_the_root_are_left_out() {
    let iface = assemble(
        "p",
        &[("::miden::protocol::types", "pub const X = 1\npub proc f()\n    nop\nend\n")],
    );
    let generated = generate(&iface, &[], &options("::miden::protocol")).unwrap();
    let modules: Vec<&str> = generated
        .bindings
        .lines()
        .filter(|line| line.trim_start().starts_with("pub mod"))
        .collect();
    assert_eq!(modules, ["pub mod types {"]);
    assert!(generated.bindings.contains("    pub const X: u8 = 1;\n"));
}

/// Protocol and standards fixtures. The real standards package uses the protocol's `AccountId`
/// in its types without exporting it; an in-test package cannot import from another, and a type
/// export may only use a public type, so the standards fixture exports its own `AccountId`, which
/// the test then drops from its interface.
const PROTOCOL_SOURCE: &str = r#"
pub type AccountId = struct { suffix: felt, prefix: felt }

pub proc id() -> AccountId
    nop
end
"#;

const STANDARDS_SOURCE: &str = r#"
pub type AccountId = struct { suffix: felt, prefix: felt }
pub type OwnershipInfo = struct { owner: AccountId, nominated_owner: AccountId }

pub proc owner() -> AccountId
    nop
end
"#;

#[test]
fn a_type_of_another_package_is_declared_unless_with_names_it() {
    let protocol = assemble("miden-protocol", &[("::miden::protocol::types", PROTOCOL_SOURCE)]);
    let mut standards =
        assemble("miden-standards", &[("::miden::standards::access", STANDARDS_SOURCE)]);
    standards.types.retain(|item| item.path.last() != Some("AccountId"));

    // No `with` entry for the protocol: the module that uses `AccountId` declares it, once.
    let generated = generate(&standards, &[&protocol], &options("::miden::standards")).unwrap();
    let bindings = &generated.bindings;
    assert_eq!(bindings.matches("pub struct AccountId {").count(), 1, "{bindings}");
    assert!(bindings.contains("        pub owner: AccountId,\n"), "{bindings}");
    let declared = bindings.find("pub struct AccountId").unwrap();
    assert!(declared < bindings.find("pub struct OwnershipInfo").unwrap(), "declared first");

    // With it, the field refers to the protocol's bindings, and nothing is declared.
    let mut options = options("::miden::standards");
    options.with.insert(
        "miden-protocol".into(),
        External {
            root: "::miden::protocol".into(),
            rust_path: "crate::raw::protocol".into(),
        },
    );
    let generated = generate(&standards, &[&protocol], &options).unwrap();
    let bindings = &generated.bindings;
    assert!(!bindings.contains("pub struct AccountId"), "{bindings}");
    assert!(
        bindings.contains("pub owner: crate::raw::protocol::types::AccountId,"),
        "{bindings}"
    );
    syn::parse_file(bindings).expect("generated bindings must parse as Rust");
}

#[test]
fn an_export_outside_the_root_is_an_error() {
    let package = assemble_fixture("fixture", "::fixture", FIXTURE_SOURCE);
    let iface = PackageInterface::from_package(&package);
    assert_eq!(
        generate(&iface, &[], &options("::other")),
        Err(Error::OutsideRoot {
            path: "::fixture".into(),
            root: "::other".into(),
        })
    );
}

#[test]
fn two_types_with_one_rust_name_in_a_module_are_an_error() {
    let source = "pub type note_type = u8\npub type NoteType = u16\npub proc f()\n    nop\nend\n";
    let iface = assemble("p", &[("::p", source)]);
    assert_eq!(
        generate(&iface, &[], &options("::p")),
        Err(Error::ConflictingType {
            module: "::p".into(),
            name: "NoteType".into(),
        })
    );
}

/// A struct whose generated type has `Word` fields is 16-byte aligned on the Miden target, where
/// the HIR aligns it to 4; a struct that holds it at an offset that is not a multiple of 16 has
/// no Rust form with its layout (nor can it be packed: `Word` is `#[repr(align(16))]`), and
/// produces no code.
#[test]
fn a_struct_that_cannot_keep_a_word_struct_aligned_is_skipped() {
    let source = r#"
pub type Words = struct { w: word }
pub type Holder = struct { tag: u8, words: Words }

pub proc f()
    nop
end
"#;
    let iface = assemble("p", &[("::p", source)]);
    let generated = generate(&iface, &[], &options("::p")).unwrap();
    assert_eq!(
        generated.skipped,
        [Skipped {
            path: "::p::Holder".into(),
            reason: "no Rust struct has its layout (field offsets [0, 4], size 20, alignment 4)"
                .into(),
        }]
    );
    assert!(!generated.bindings.contains("Holder"), "{}", generated.bindings);
    assert!(generated.bindings.contains("pub struct Words {"), "{}", generated.bindings);
}

/// The assembler only writes enums whose variants are plain discriminants; an interface can hold
/// any, and an enum whose variants carry values has no Rust form here: it is reported, and the
/// rest of the package is generated.
#[test]
fn an_enum_with_values_is_skipped() {
    let mut iface = fixture();
    let variants = [
        Variant::new("Some".into(), Type::Felt, Some(1)),
        Variant::c_like("None".into(), Some(0)),
    ];
    let maybe = EnumType::new("Maybe".into(), Type::U8, variants).unwrap();
    iface.types.push(TypeItem {
        path: Path::new("::fixture::Maybe").into(),
        ty: maybe.into(),
    });
    iface.types.sort_by(|a, b| a.path.cmp(&b.path));
    let generated = generate(&iface, &[], &options("::fixture")).unwrap();
    let reason = "enum `Maybe` has variants that carry values; only an enum of plain \
                  discriminants has a Rust form";
    assert!(
        generated.skipped.contains(&Skipped {
            path: "::fixture::Maybe".into(),
            reason: reason.into(),
        }),
        "{:?}",
        generated.skipped
    );
    assert!(!generated.bindings.contains("enum Maybe"));
    assert!(generated.bindings.contains("pub fn make_pair("));
}

/// A wrapper that rebuilds a result field by field names a struct the bindings declare for a field
/// of the result's type export where it is declared, in the export's module: here a sibling of
/// the wrapper's.
#[test]
fn a_rebuilt_result_names_a_declared_struct_in_another_module() {
    let types = r#"
pub type Outer = struct { inner: struct { x: felt, y: u32 }, tag: u8 }

pub proc f()
    nop
end
"#;
    let api = "use ::p::types\n\npub proc get() -> types::Outer\n    nop\nend\n";
    let modules = [
        ("::p", "pub mod types\npub mod api\n"),
        ("::p::types", types),
        ("::p::api", api),
    ];
    let iface = assemble("p", &modules);
    let generated = generate(&iface, &[], &options("::p")).unwrap();
    assert_eq!(generated.skipped, []);
    let bindings = &generated.bindings;
    assert!(bindings.contains("pub fn get() -> super::types::Outer {"), "{bindings}");
    assert!(bindings.contains("inner: super::types::OuterInner {"), "{bindings}");
    assert_eq!(bindings.matches("pub struct OuterInner").count(), 1, "{bindings}");
    syn::parse_file(bindings).expect("generated bindings must parse as Rust");
}

/// A type export with no Rust form is reported and produces no code, and so does everything that
/// holds it or points at it; nothing is declared in its place.
#[test]
fn types_with_no_rust_form_are_skipped_with_what_uses_them() {
    let source = r#"
pub type Big = u256
pub type Wide = struct { x: u256 }
pub type Holder = struct { wide: Wide, n: felt }

pub proc load(p: ptr<struct { x: u256 }, addrspace(felt)>)
    drop
end

pub proc hold(p: ptr<Holder, addrspace(felt)>)
    drop
end

pub proc fine(x: felt) -> felt
    nop
end
"#;
    let iface = assemble("p", &[("::p", source)]);
    let generated = generate(&iface, &[], &options("::p")).unwrap();
    let u256 = "type `u256` has no Rust form in a binding";
    let wide = format!("field `x`: {u256}");
    let skipped: Vec<(&str, &str)> =
        generated.skipped.iter().map(|s| (s.path.as_str(), s.reason.as_str())).collect();
    assert_eq!(
        skipped,
        [
            ("::p::Big", u256),
            ("::p::Holder", &*format!("field `wide`: {wide}")),
            ("::p::Wide", &*wide),
            ("::p::hold", &*format!("struct `Holder` has no Rust form: field `wide`: {wide}")),
            ("::p::load", &*format!("an anonymous struct has no Rust form: {wide}")),
        ]
    );
    let bindings = &generated.bindings;
    assert!(!bindings.contains("pub struct") && !bindings.contains("pub type"), "{bindings}");
    assert!(bindings.contains("pub fn fine(arg0: Felt) -> Felt {"), "{bindings}");
    syn::parse_file(bindings).expect("generated bindings must parse as Rust");
}

/// A procedure whose Rust name another procedure or a constant of its module has is skipped, and
/// so is the other procedure; a constant keeps the name.
#[test]
fn procedures_of_one_rust_name_are_skipped() {
    let mut iface = fixture();
    let renamed = |from: &str, to: &str| {
        let mut item = iface.procedure(Path::new(from)).unwrap().clone();
        item.path = Path::new(to).into();
        item
    };
    let twin = renamed("::fixture::make_pair", "::fixture::\"make-pair\"");
    let shouting = renamed("::fixture::id", "::fixture::MAX_AMOUNT");
    iface.procedures.extend([twin, shouting]);
    iface.procedures.sort_by(|a, b| a.path.cmp(&b.path));
    let limit = iface.constants.iter().find(|c| c.path.last() == Some("LIMIT")).unwrap();
    let max_amount = ConstantItem {
        path: Path::new("::fixture::max_amount").into(),
        value: limit.value.clone(),
    };
    iface.constants.push(max_amount);
    iface.constants.sort_by(|a, b| a.path.cmp(&b.path));
    let generated = generate(&iface, &[], &options("::fixture")).unwrap();
    let twin = iface
        .procedures
        .iter()
        .find(|p| p.name() == "make-pair")
        .unwrap()
        .path
        .to_string();
    let reason =
        |path: &str| generated.skipped.iter().find(|s| s.path == path).map(|s| s.reason.clone());
    assert_eq!(
        reason("::fixture::make_pair"),
        Some(format!("its Rust name `make_pair` is also that of `{twin}`"))
    );
    assert_eq!(
        reason(&twin),
        Some("its Rust name `make_pair` is also that of `::fixture::make_pair`".to_string())
    );
    assert_eq!(
        reason("::fixture::MAX_AMOUNT"),
        Some("its Rust name `MAX_AMOUNT` is also that of `::fixture::max_amount`".to_string())
    );
    assert!(!generated.bindings.contains("fn make_pair"));
    assert!(generated.bindings.contains("pub const MAX_AMOUNT: u16 = 1024;"));
}

/// A procedure two of whose flattened parameters would have one name in the `extern "C"`
/// declaration (`arg0_a_b` for field `a_b` and for field `b` of field `a`) is skipped.
#[test]
fn flattened_parameters_of_one_name_are_skipped() {
    let source = r#"
pub proc clash(s: struct { a_b: felt, a: struct { b: felt } })
    drop drop
end

pub proc fine(x: felt) -> felt
    nop
end
"#;
    let iface = assemble("p", &[("::p", source)]);
    let generated = generate(&iface, &[], &options("::p")).unwrap();
    assert_eq!(
        generated.skipped,
        [Skipped {
            path: "::p::clash".into(),
            reason: "two of its flattened parameters would both be named `arg0_a_b`".into(),
        }]
    );
    assert!(!generated.stubs.contains("p__clash"));
}

/// Two modules, two constants or two stubs that would have one Rust name are an error, as are
/// two fields of a struct or two variants of an enum for that type, which is skipped.
#[test]
fn items_of_one_rust_name_are_an_error() {
    let constant = |path: &str| ConstantItem {
        path: Path::new(path).into(),
        value: fixture().constants[0].value.clone(),
    };
    let generate_with = |items: &[ConstantItem]| {
        let mut iface = fixture();
        iface.constants.extend(items.iter().cloned());
        iface.constants.sort_by(|a, b| a.path.cmp(&b.path));
        generate(&iface, &[], &options("::fixture"))
    };
    assert_eq!(
        generate_with(&[constant("::fixture::fooBar::X"), constant("::fixture::foo_bar::Y")]),
        Err(Error::ConflictingName {
            first: "::fixture::fooBar".into(),
            second: "::fixture::foo_bar".into(),
            name: "foo_bar".into(),
        })
    );
    assert_eq!(
        generate_with(&[constant("::fixture::maxAmount"), constant("::fixture::max_amount")]),
        Err(Error::ConflictingName {
            first: "::fixture::maxAmount".into(),
            second: "::fixture::max_amount".into(),
            name: "MAX_AMOUNT".into(),
        })
    );

    let mut iface = fixture();
    let id = iface.procedure(Path::new("::fixture::id")).unwrap().clone();
    for path in ["::fixture::a::b__c", "::fixture::a__b::c"] {
        let mut item = id.clone();
        item.path = Path::new(path).into();
        iface.procedures.push(item);
    }
    iface.procedures.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(
        generate(&iface, &[], &options("::fixture")),
        Err(Error::ConflictingName {
            first: "::fixture::a::b__c".into(),
            second: "::fixture::a__b::c".into(),
            name: "fixture__a__b__c".into(),
        })
    );

    let mut iface = fixture();
    let fields = [(Arc::from("a-b"), Type::Felt), (Arc::from("a_b"), Type::Felt)];
    let twins = StructType::named("Twins".into(), fields);
    let variants = [
        Variant::c_like("FOO_BAR".into(), Some(0)),
        Variant::c_like("FooBar".into(), Some(1)),
    ];
    let named = EnumType::new("Named".into(), Type::U8, variants).unwrap();
    iface.types.push(TypeItem {
        path: Path::new("::fixture::Twins").into(),
        ty: twins.into(),
    });
    iface.types.push(TypeItem {
        path: Path::new("::fixture::Named").into(),
        ty: named.into(),
    });
    iface.types.sort_by(|a, b| a.path.cmp(&b.path));
    let generated = generate(&iface, &[], &options("::fixture")).unwrap();
    let reason =
        |path: &str| generated.skipped.iter().find(|s| s.path == path).map(|s| s.reason.as_str());
    assert_eq!(
        reason("::fixture::Twins"),
        Some("fields `a-b` and `a_b` would both be named `a_b`")
    );
    assert_eq!(
        reason("::fixture::Named"),
        Some("variants `FOO_BAR` and `FooBar` would both be named `FooBar`")
    );
}
