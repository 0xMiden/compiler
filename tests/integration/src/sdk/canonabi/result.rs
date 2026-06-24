//! Integration tests for component-model CanonABI result values.

use super::run_canonabi_case;

/// Tests a component method that accepts and returns `result<felt, u64>`.
#[test]
fn result_with_felt_ok_and_u64_error_payloads() {
    let account_source = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{Felt, component, felt};

#[component]
struct CanonabiAccount;

#[component]
impl CanonabiAccount {
    /// Transforms a result with felt success and integer error payloads.
    pub fn roundtrip(&self, value: Result<Felt, u64>) -> Result<Felt, u64> {
        match value {
            Ok(value) => Ok(value + felt!(7)),
            Err(value) => Err(value + 9),
        }
    }
}
"#;
    let note_body = r#"let max = Felt::new(u64::MAX - u32::MAX as u64).unwrap();
let ok = roundtrip(Ok(max));
match ok {
    Ok(value) => assert_eq!(value, felt!(6)),
    Err(_) => assert_eq!(felt!(0), felt!(1)),
}

let err = roundtrip(Err(100));
match err {
    Ok(_) => assert_eq!(felt!(0), felt!(1)),
    Err(value) => {
        if value != 109 { assert_eq!(felt!(0), felt!(1)); }
    }
}"#;

    run_canonabi_case("result_felt_u64", account_source, note_body, |wit| {
        assert!(
            wit.contains("roundtrip: func(value: result<felt, u64>) -> result<felt, u64>;"),
            "generated WIT did not use result<felt, u64>:\n{wit}"
        );
    });
}

/// Tests that mixed felt and u32 result payloads preserve full field elements.
#[test]
fn result_with_large_felt_ok_and_u32_error_payloads() {
    let account_source = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{Felt, component};

#[component]
struct CanonabiAccount;

#[component]
impl CanonabiAccount {
    /// Returns the result unchanged across the canonical ABI boundary.
    pub fn roundtrip(&self, value: Result<Felt, u32>) -> Result<Felt, u32> {
        value
    }
}
"#;
    let note_body = r#"let max = Felt::new(u64::MAX - u32::MAX as u64).unwrap();
let ok = roundtrip(Ok(max));
match ok {
    Ok(value) => assert_eq!(value, max),
    Err(_) => assert_eq!(felt!(0), felt!(1)),
}

let err = roundtrip(Err(4_294_967_295));
match err {
    Ok(_) => assert_eq!(felt!(0), felt!(1)),
    Err(value) => {
        if value != 4_294_967_295 { assert_eq!(felt!(0), felt!(1)); }
    }
}"#;

    run_canonabi_case("result_large_felt_u32", account_source, note_body, |wit| {
        assert!(
            wit.contains("roundtrip: func(value: result<felt, u32>) -> result<felt, u32>;"),
            "generated WIT did not use result<felt, u32>:\n{wit}"
        );
    });
}

/// Tests that felt payloads joined with u64 payloads are not truncated to 32 bits.
#[test]
fn result_with_large_felt_ok_and_u64_error_payloads() {
    let account_source = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{Felt, component};

#[component]
struct CanonabiAccount;

#[component]
impl CanonabiAccount {
    /// Returns the result unchanged across the canonical ABI boundary.
    pub fn roundtrip(&self, value: Result<Felt, u64>) -> Result<Felt, u64> {
        value
    }
}
"#;
    let note_body = r#"let max = Felt::new(u64::MAX - u32::MAX as u64).unwrap();
let ok = roundtrip(Ok(max));
match ok {
    Ok(value) => assert_eq!(value, max),
    Err(_) => assert_eq!(felt!(0), felt!(1)),
}

let err = roundtrip(Err(4_294_967_301));
match err {
    Ok(_) => assert_eq!(felt!(0), felt!(1)),
    Err(value) => {
        if value != 4_294_967_301 { assert_eq!(felt!(0), felt!(1)); }
    }
}"#;

    run_canonabi_case("result_large_felt_u64", account_source, note_body, |wit| {
        assert!(
            wit.contains("roundtrip: func(value: result<felt, u64>) -> result<felt, u64>;"),
            "generated WIT did not use result<felt, u64>:\n{wit}"
        );
    });
}

/// Tests a component method that accepts and returns `result<word, felt>`.
#[test]
fn result_with_word_ok_and_felt_error_payloads() {
    let account_source = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{Felt, Word, component, felt};

#[component]
struct CanonabiAccount;

#[component]
impl CanonabiAccount {
    /// Transforms a result with word success and felt error payloads.
    pub fn roundtrip(&self, value: Result<Word, Felt>) -> Result<Word, Felt> {
        match value {
            Ok(word) => Ok(Word::new([
                word.a + felt!(1),
                word.b + felt!(2),
                word.c + felt!(3),
                word.d + felt!(4),
            ])),
            Err(value) => Err(value + felt!(5)),
        }
    }
}
"#;
    let note_body = r#"let max = Felt::new(u64::MAX - u32::MAX as u64).unwrap();
let word = Word::new([max, max, max, max]);
let ok = roundtrip(Ok(word));
match ok {
    Ok(value) => {
        assert_eq!(value.a, felt!(0));
        assert_eq!(value.b, felt!(1));
        assert_eq!(value.c, felt!(2));
        assert_eq!(value.d, felt!(3));
    }
    Err(_) => assert_eq!(felt!(0), felt!(1)),
}

let err = roundtrip(Err(max));
match err {
    Ok(_) => assert_eq!(felt!(0), felt!(1)),
    Err(value) => assert_eq!(value, felt!(4)),
}"#;

    run_canonabi_case("result_word_felt", account_source, note_body, |wit| {
        assert!(
            wit.contains("use core-types.{felt, word};"),
            "generated WIT did not import word for result<word, felt>:\n{wit}"
        );
        assert!(
            wit.contains("roundtrip: func(value: result<word, felt>) -> result<word, felt>;"),
            "generated WIT did not use result<word, felt>:\n{wit}"
        );
    });
}

/// Tests a component method that accepts and returns a result of exported records.
#[test]
fn result_with_record_payloads() {
    let account_source = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{Felt, component, export_type, felt};

/// Success payload used inside a result.
#[derive(Clone, Copy, Debug)]
#[export_type]
pub struct ResultOkPayload {
    /// A 64-bit integer field.
    pub amount: u64,
    /// A field element.
    pub value: Felt,
    /// A boolean field.
    pub flag: bool,
}

/// Error payload used inside a result.
#[derive(Clone, Copy, Debug)]
#[export_type]
pub struct ResultErrPayload {
    /// A 32-bit integer field.
    pub code: u32,
    /// A 16-bit integer field.
    pub small: u16,
    /// An 8-bit integer field.
    pub tiny: u8,
}

#[component]
struct CanonabiAccount;

#[component]
impl CanonabiAccount {
    /// Transforms record payloads carried by a result.
    pub fn roundtrip(
        &self,
        payload: Result<ResultOkPayload, ResultErrPayload>,
    ) -> Result<ResultOkPayload, ResultErrPayload> {
        match payload {
            Ok(payload) => Ok(ResultOkPayload {
                amount: payload.amount + 11,
                value: payload.value + felt!(11),
                flag: !payload.flag,
            }),
            Err(payload) => Err(ResultErrPayload {
                code: payload.code + 13,
                small: payload.small + 17,
                tiny: payload.tiny + 19,
            }),
        }
    }
}
"#;
    let note_body = r#"let max = Felt::new(u64::MAX - u32::MAX as u64).unwrap();
let ok_payload = crate::ResultOkPayload {
    amount: 100,
    value: max,
    flag: false,
};
let ok = roundtrip(Ok(ok_payload));
match ok {
    Ok(value) => {
        if value.amount != 111 { assert_eq!(felt!(0), felt!(1)); }
        assert_eq!(value.value, felt!(10));
        if !value.flag { assert_eq!(felt!(0), felt!(1)); }
    }
    Err(_) => assert_eq!(felt!(0), felt!(1)),
}

let err_payload = crate::ResultErrPayload {
    code: 30,
    small: 40,
    tiny: 50,
};
let err = roundtrip(Err(err_payload));
match err {
    Ok(_) => assert_eq!(felt!(0), felt!(1)),
    Err(value) => {
        if value.code != 43 { assert_eq!(felt!(0), felt!(1)); }
        if value.small != 57 { assert_eq!(felt!(0), felt!(1)); }
        if value.tiny != 69 { assert_eq!(felt!(0), felt!(1)); }
    }
}"#;

    run_canonabi_case("result_record", account_source, note_body, |wit| {
        assert!(
            wit.contains("record result-ok-payload {"),
            "generated WIT did not define result-ok-payload record:\n{wit}"
        );
        assert!(
            wit.contains("record result-err-payload {"),
            "generated WIT did not define result-err-payload record:\n{wit}"
        );
        assert!(
            wit.contains(
                "roundtrip: func(payload: result<result-ok-payload, result-err-payload>) -> \
                 result<result-ok-payload, result-err-payload>;"
            ),
            "generated WIT did not use result<result-ok-payload, result-err-payload>:\n{wit}"
        );
    });
}

/// Tests a component method that accepts and returns a record with a result field.
#[test]
fn record_with_result_field() {
    let account_source = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{Felt, component, export_type, felt};

/// Payload whose record layout contains a result field.
#[derive(Clone, Copy, Debug)]
#[export_type]
pub struct ResultFieldPayload {
    /// A 64-bit integer field.
    pub amount: u64,
    /// A result field.
    pub outcome: Result<Felt, u32>,
    /// A boolean field.
    pub flag: bool,
}

#[component]
struct CanonabiAccount;

#[component]
impl CanonabiAccount {
    /// Transforms a record with a result field.
    pub fn roundtrip(&self, payload: ResultFieldPayload) -> ResultFieldPayload {
        ResultFieldPayload {
            amount: payload.amount + 3,
            outcome: match payload.outcome {
                Ok(value) => Ok(value + felt!(4)),
                Err(value) => Err(value + 5),
            },
            flag: !payload.flag,
        }
    }
}
"#;
    let note_body = r#"let max = Felt::new(u64::MAX - u32::MAX as u64).unwrap();
let ok_payload = crate::ResultFieldPayload {
    amount: 10,
    outcome: Ok(max),
    flag: false,
};
let ok = roundtrip(ok_payload);
if ok.amount != 13 { assert_eq!(felt!(0), felt!(1)); }
match ok.outcome {
    Ok(value) => assert_eq!(value, felt!(3)),
    Err(_) => assert_eq!(felt!(0), felt!(1)),
}
if !ok.flag { assert_eq!(felt!(0), felt!(1)); }

let err_payload = crate::ResultFieldPayload {
    amount: 30,
    outcome: Err(40),
    flag: true,
};
let err = roundtrip(err_payload);
if err.amount != 33 { assert_eq!(felt!(0), felt!(1)); }
match err.outcome {
    Ok(_) => assert_eq!(felt!(0), felt!(1)),
    Err(value) => {
        if value != 45 { assert_eq!(felt!(0), felt!(1)); }
    }
}
if err.flag { assert_eq!(felt!(0), felt!(1)); }"#;

    run_canonabi_case("record_result_field", account_source, note_body, |wit| {
        assert!(
            wit.contains("record result-field-payload {"),
            "generated WIT did not define result-field-payload record:\n{wit}"
        );
        assert!(
            wit.contains("outcome: result<felt, u32>,"),
            "generated WIT did not use result<felt, u32> for the record field:\n{wit}"
        );
        assert!(
            wit.contains("roundtrip: func(payload: result-field-payload) -> result-field-payload;"),
            "generated WIT did not use result-field-payload in roundtrip:\n{wit}"
        );
    });
}
