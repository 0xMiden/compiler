//! Host-side checks for raw string formatting and the SDK debug derive.

use core::{fmt, ops::Deref};

use miden::debug::{MidenDebug, RawString};

#[test]
fn raw_string_preserves_contents() {
    assert_eq!(format!("{:?}", RawString("café\n\"input\"")), "café\n\"input\"");
}

#[test]
fn ordinary_fields_use_standard_debug() {
    #[derive(MidenDebug)]
    struct Input {
        value: u32,
    }

    assert_eq!(format!("{:?}", Input { value: 50 }), "Input { value: 50 }");
}

#[test]
fn unannotated_string_keeps_standard_debug() {
    #[derive(MidenDebug)]
    struct Input<'a> {
        label: &'a str,
    }

    assert_eq!(format!("{:?}", Input { label: "input" }), "Input { label: \"input\" }");
}

#[test]
fn annotated_borrowed_string_is_raw() {
    #[derive(MidenDebug)]
    struct Input<'a> {
        #[miden_debug(raw_str)]
        label: &'a str,
    }

    let input = Input {
        label: "café\ninput",
    };
    assert_eq!(format!("{input:?}"), "Input { label: café\ninput }");
}

#[test]
fn annotated_owned_string_is_borrowed() {
    #[derive(MidenDebug)]
    struct Input {
        #[miden_debug(raw_str)]
        label: String,
    }

    let input = Input {
        label: String::from("input"),
    };
    assert_eq!(format!("{input:?}"), "Input { label: input }");
    assert_eq!(input.label, "input");
}

#[test]
fn annotated_string_alias_is_raw() {
    type Label<'a> = &'a str;

    #[derive(MidenDebug)]
    struct Input<'a> {
        #[miden_debug(raw_str)]
        label: Label<'a>,
    }

    // Raw formatting omits the quotes normally added by Debug for strings.
    assert_eq!(format!("{:?}", Input { label: "input" }), "Input { label: input }");
}

#[test]
fn annotated_generic_deref_does_not_require_debug() {
    struct Label(String);

    impl Deref for Label {
        type Target = str;

        fn deref(&self) -> &str {
            &self.0
        }
    }

    #[derive(MidenDebug)]
    struct Input<T>
    where
        T: Deref<Target = str>,
    {
        #[miden_debug(raw_str)]
        label: T,
    }

    let input = Input {
        label: Label(String::from("input")),
    };
    assert_eq!(format!("{input:?}"), "Input { label: input }");
}

#[test]
fn generic_fields_receive_debug_bounds() {
    #[derive(MidenDebug)]
    struct Input<'a, T, const N: usize>
    where
        T: Copy,
    {
        values: &'a [T; N],
    }

    let input = Input { values: &[1u32, 2] };
    assert_eq!(format!("{input:?}"), "Input { values: [1, 2] }");
}

#[test]
fn unsized_field_uses_debug_impl() {
    struct Marker;

    impl fmt::Debug for Marker {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("marker debug")
        }
    }

    #[derive(MidenDebug)]
    struct Input<T: ?Sized> {
        value: T,
    }

    let input: &Input<dyn fmt::Debug> = &Input { value: Marker };
    assert_eq!(format!("{input:?}"), "Input { value: marker debug }");
}

#[test]
fn nested_struct_uses_its_debug_implementation() {
    #[derive(MidenDebug)]
    struct Limits {
        threshold: u32,
    }

    #[derive(MidenDebug)]
    struct Input {
        limits: Limits,
    }

    let input = Input {
        limits: Limits { threshold: 100 },
    };
    assert_eq!(format!("{input:?}"), "Input { limits: Limits { threshold: 100 } }");
}

#[test]
fn empty_named_struct_is_supported() {
    #[derive(MidenDebug)]
    struct Input {}

    assert_eq!(format!("{:?}", Input {}), "Input");
}

#[test]
fn raw_identifier_is_printed_without_prefix() {
    #[derive(MidenDebug)]
    struct Input {
        r#type: u32,
    }

    assert_eq!(format!("{:?}", Input { r#type: 50 }), "Input { type: 50 }");
}

#[test]
fn disabled_fields_are_not_formatted() {
    #[derive(MidenDebug)]
    struct Input {
        value: u32,
        #[cfg(any())]
        disabled: MissingType,
    }

    assert_eq!(format!("{:?}", Input { value: 50 }), "Input { value: 50 }");
}
