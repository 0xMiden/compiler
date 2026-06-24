//! Integration tests for component-model CanonABI option values.

use super::run_canonabi_case;

/// Tests a component method that accepts and returns `option<felt>`.
#[test]
fn option_with_felt_payload() {
    let account_source = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{Felt, component, felt};

#[component]
struct CanonabiAccount;

#[component]
impl CanonabiAccount {
    /// Transforms an optional felt value.
    pub fn roundtrip(&self, value: Option<Felt>) -> Option<Felt> {
        match value {
            Some(value) => Some(value + felt!(7)),
            None => None,
        }
    }
}
"#;
    let note_body = r#"let none = roundtrip(None);
match none {
    None => (),
    Some(_) => assert_eq!(felt!(0), felt!(1)),
}

let max = Felt::new(u64::MAX - u32::MAX as u64).unwrap();
let some = roundtrip(Some(max));
match some {
    Some(value) => assert_eq!(value, felt!(6)),
    None => assert_eq!(felt!(0), felt!(1)),
}"#;

    run_canonabi_case("option_felt", account_source, note_body, |wit| {
        assert!(
            wit.contains("roundtrip: func(value: option<felt>) -> option<felt>;"),
            "generated WIT did not use option<felt>:\n{wit}"
        );
    });
}

/// Tests a component method that accepts and returns `option<word>`.
#[test]
fn option_with_word_payload() {
    let account_source = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{Word, component, felt};

#[component]
struct CanonabiAccount;

#[component]
impl CanonabiAccount {
    /// Transforms an optional word value.
    pub fn roundtrip(&self, value: Option<Word>) -> Option<Word> {
        match value {
            Some(word) => Some(Word::new([
                word.a + felt!(1),
                word.b + felt!(2),
                word.c + felt!(3),
                word.d + felt!(4),
            ])),
            None => None,
        }
    }
}
"#;
    let note_body = r#"let none = roundtrip(None);
match none {
    None => (),
    Some(_) => assert_eq!(felt!(0), felt!(1)),
}

let max = Felt::new(u64::MAX - u32::MAX as u64).unwrap();
let word = Word::new([max, max, max, max]);
let some = roundtrip(Some(word));
match some {
    Some(value) => {
        assert_eq!(value.a, felt!(0));
        assert_eq!(value.b, felt!(1));
        assert_eq!(value.c, felt!(2));
        assert_eq!(value.d, felt!(3));
    }
    None => assert_eq!(felt!(0), felt!(1)),
}"#;

    run_canonabi_case("option_word", account_source, note_body, |wit| {
        assert!(
            wit.contains("use core-types.{word};"),
            "generated WIT did not import word for option<word>:\n{wit}"
        );
        assert!(
            wit.contains("roundtrip: func(value: option<word>) -> option<word>;"),
            "generated WIT did not use option<word>:\n{wit}"
        );
    });
}

/// Tests a component method that accepts and returns an option of an exported record.
#[test]
fn option_with_record_payload() {
    let account_source = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{Felt, component, export_type, felt};

/// Payload used inside an option.
#[derive(Clone, Copy, Debug)]
#[export_type]
pub struct OptionPayload {
    /// A 64-bit integer field.
    pub amount: u64,
    /// A field element.
    pub value: Felt,
    /// A 32-bit integer field.
    pub count: u32,
    /// A 16-bit integer field.
    pub small: u16,
    /// An 8-bit integer field.
    pub tiny: u8,
    /// A boolean field.
    pub flag: bool,
}

#[component]
struct CanonabiAccount;

#[component]
impl CanonabiAccount {
    /// Transforms an optional record payload.
    pub fn roundtrip(&self, payload: Option<OptionPayload>) -> Option<OptionPayload> {
        match payload {
            Some(payload) => Some(OptionPayload {
                amount: payload.amount + 11,
                value: payload.value + felt!(11),
                count: payload.count + 11,
                small: payload.small + 11,
                tiny: payload.tiny + 11,
                flag: !payload.flag,
            }),
            None => None,
        }
    }
}
"#;
    let note_body = r#"let none = roundtrip(None);
match none {
    None => (),
    Some(_) => assert_eq!(felt!(0), felt!(1)),
}

let max = Felt::new(u64::MAX - u32::MAX as u64).unwrap();
let payload = crate::OptionPayload {
    amount: 100,
    value: max,
    count: 30,
    small: 40,
    tiny: 50,
    flag: false,
};
let some = roundtrip(Some(payload));
match some {
    Some(value) => {
        if value.amount != 111 { assert_eq!(felt!(0), felt!(1)); }
        assert_eq!(value.value, felt!(10));
        if value.count != 41 { assert_eq!(felt!(0), felt!(1)); }
        if value.small != 51 { assert_eq!(felt!(0), felt!(1)); }
        if value.tiny != 61 { assert_eq!(felt!(0), felt!(1)); }
        if !value.flag { assert_eq!(felt!(0), felt!(1)); }
    }
    None => assert_eq!(felt!(0), felt!(1)),
}"#;

    run_canonabi_case("option_record", account_source, note_body, |wit| {
        assert!(
            wit.contains("record option-payload {"),
            "generated WIT did not define option-payload record:\n{wit}"
        );
        assert!(
            wit.contains(
                "roundtrip: func(payload: option<option-payload>) -> option<option-payload>;"
            ),
            "generated WIT did not use option<option-payload>:\n{wit}"
        );
    });
}

/// Tests a component method that accepts and returns a record with an option field.
#[test]
fn record_with_option_field() {
    let account_source = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{Felt, component, export_type, felt};

/// Payload whose record layout contains an option field.
#[derive(Clone, Copy, Debug)]
#[export_type]
pub struct OptionFieldPayload {
    /// A 64-bit integer field.
    pub amount: u64,
    /// An optional field element.
    pub maybe: Option<Felt>,
    /// A boolean field.
    pub flag: bool,
}

#[component]
struct CanonabiAccount;

#[component]
impl CanonabiAccount {
    /// Transforms a record with an optional field.
    pub fn roundtrip(&self, payload: OptionFieldPayload) -> OptionFieldPayload {
        OptionFieldPayload {
            amount: payload.amount + 3,
            maybe: match payload.maybe {
                Some(value) => Some(value + felt!(4)),
                None => None,
            },
            flag: !payload.flag,
        }
    }
}
"#;
    let note_body = r#"let max = Felt::new(u64::MAX - u32::MAX as u64).unwrap();
let some_payload = crate::OptionFieldPayload {
    amount: 10,
    maybe: Some(max),
    flag: false,
};
let some = roundtrip(some_payload);
if some.amount != 13 { assert_eq!(felt!(0), felt!(1)); }
match some.maybe {
    Some(value) => assert_eq!(value, felt!(3)),
    None => assert_eq!(felt!(0), felt!(1)),
}
if !some.flag { assert_eq!(felt!(0), felt!(1)); }

let none_payload = crate::OptionFieldPayload {
    amount: 30,
    maybe: None,
    flag: true,
};
let none = roundtrip(none_payload);
if none.amount != 33 { assert_eq!(felt!(0), felt!(1)); }
match none.maybe {
    None => (),
    Some(_) => assert_eq!(felt!(0), felt!(1)),
}
if none.flag { assert_eq!(felt!(0), felt!(1)); }"#;

    run_canonabi_case("record_option_field", account_source, note_body, |wit| {
        assert!(
            wit.contains("record option-field-payload {"),
            "generated WIT did not define option-field-payload record:\n{wit}"
        );
        assert!(
            wit.contains("maybe: option<felt>,"),
            "generated WIT did not use option<felt> for the record field:\n{wit}"
        );
        assert!(
            wit.contains("roundtrip: func(payload: option-field-payload) -> option-field-payload;"),
            "generated WIT did not use option-field-payload in roundtrip:\n{wit}"
        );
    });
}
