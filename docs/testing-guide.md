# Testing Guide

This document describes guidelines to follow when creating, updating, or refactoring the compiler test suite. 

1. [Testing Methods](#testing-methods) describes what forms of testing are currently used in the compiler test suite, and where those tests live.
2. [Method Selection](#method-selection) describes the criteria to use in selecting specific testing methods for your change
3. [Best Practices](#best-practices) outlines practices we encourage all contributors to follow when developing tests for the compiler.

Tests are organized as follows:

* Unit tests are defined in the same crate as the code they test, ideally in a submodule annoated with `#[cfg(test)]`.
* All other tests are defined under the `tests` directory, in either the appropriate integration test crate, or in an appropriate lit test suite under `tests/lit`

## Testing Methods

The compiler contains five primary categories of tests:

* _Unit_ tests, which are focused on ensuring that specific APIs behave as expected. These are written in Rust, and live close to the source code they test. The purpose of these tests is to catch issues at a granular level, when they are easiest to diagnose and fix.
* _Functional_ tests, which is focused on checking specific functionality of the compiler meets its requirements. For example, testing that compilation with a specific feature enabled produces the expected output.
* _Integration_ tests, which are focused on testing the full compiler pipeline end-to-end, and often include execution of the compiled program to ensure that the compiled program not only executes, but produces the expected outputs.
* _Semantics_ tests, which are focused on testing whether the semantics of a specific frontend are upheld through the compiler pipeline, by comparing the results of executing a sample program on both a reference target and the Miden VM target.
* _Regression_ tests, which exercise tests that were originally reproducers of a bug or issue which has since been fixed. The purpose of these tests is two-fold: they reproduce an issue when it is first identified, and they fail if that issue resurfaces again in the future.

Tests may be written according to one or more of the following methodologies:

* _Assertion-based tests_. These are the simplest tests, where we write assertions in Rust (e.g. `assert_eq!` or `assert_matches!`) that check specific inputs produce specific outputs. These are a good choice for simple functions and APIs with a small number of interesting cases to test.
* _Property-based tests_. This is a variation on assertion-based testing, but taking a step towards formal methods. It relies on generating a large number of inputs, and asserting that the outputs adhere to some property defined by the test. For example, a function like `fn to_lower_case(input: &str) -> Cow<'_, str>` might want to guarantee that for any input where all of the input characters are already lowercase, the output is always a re-borrow of the input (i.e. a no-op). The set of possible inputs is essentially unbounded, but you still want to test a large number of possible inputs to ensure that various edge cases don't produce unexpected output. Property-based tests allow you to express this declaratively, and let the testing framework do the work of generating those inputs for you (and shrinking them to a minimal case when a property is violated). We use property-based tests in many places, particularly in arithmetic-sensitive code.
* _Expect tests_. These are simple, and intentionally brittle tests which assert that the textual form of some output exactly matches the expected text. These tests are primarily useful as a form of smoke testing - when they fail they tell us that something changed that is worth investigating further. In some cases, these failures are expected, and so we have facilities for automatically updating them, by running the test with `UPDATE_EXPECT=1` set in the environment. In cases where they fail unexpectedly, it very often is the result of a bug, hence their usefulness.
* _FileCheck/lit tests_. This is a particularly powerful and interesting form of output testing, which essentially boils down to executing some shell command, which must succeed _and_ the output must match one or more checks, which can range from literal strings, to complex inter-related patterns, including things like the ability to capture part of the output with one pattern and then use it in subsequent checks. This methodology is especially well-suited to testing compilers and their intermediate representations. This style of testing was popularized by LLVM and it's `FileCheck` and `lit` tools. In this project, we use `litcheck`, which is a re-implementation in Rust that combines both tools into one.

Lastly, tests which involve executing compiled programs are broken into two categories depending on whether they require the Miden protocol or not. Tests which are dependent on the protocol must go in the `tests/integration-network` crate, which is designed for that purpose.

## Method Selection

When determining how to test your changes, or add tests to cover previously-untested parts of the compiler, you should use the following heuristics:

* Changes which are dependent on execution of the compiled program should be defined in the appropriate integration test crate under `tests`, depending on whether execution requires the Miden protocol or not. You should generally avoid using lit tests for execution. Prefer property-based tests when there is a clear property that you want to demonstrate is upheld, and proving that property cannot be expressed solely by testing specific inputs.
* If you find yourself reaching for an expect test, you should first determine whether a lit test is more appropriate. A lit test is better when the part of the output you care about is a subset of it, and changes to unrelated parts of the output should not cause the test to fail. An expect test is better when you want to know when _anything_ in the output changes.
* Testing specific compiler passes/rewrites, or subsets of the pipeline, should almost always be written as a lit test which matches on the specific parts of the output that are relevant. Not only are such tests much more readable, but they are much easier to write and experiment with, since you do not need to tediously construct the IR by hand using builders. The tests are also much less fragile for that same reason. If you find that there is some specific blocker to writing a particular test as a lit test, you should consider filing an issue reporting it, or even fix the blocker as part of your change (so long as it is relatively trivial).
* Changes to Rust code of the compiler should almost always have accompanying unit tests, unless it is at such a high-level that a unit test essentially amounts to an integration test (i.e. requires a real input), in which case you should write it as an explicit integration test instead, in the appropriate test crate. Not only will it be easier, since you will benefit from being able to use our robust set of integration test helpers to more concisely express the test, but it avoids duplicating a lot of tedious test setup boilerplate that has already been defined in the integration test crates.

## Best Practices

* DO test using multiple methodologies, when doing so tests unique properties that any single methodology would fail to capture (e.g. a transformation pass might have unit tests for helper functions used by the pass internally, as well as lit tests that exercise the pass against specific IR shapes).
* DO add tests when changing existing code that lacks adequate coverage
* DO migrate old assertion-based tests which build IR manually to lit tests, if making changes to those tests. This makes those tests easier to maintain going forward.
* DON'T change existing test expectations or semantics without justifying those changes explicitly in your pull request.
* DON'T write tests directly in the SDK crates under `sdk/` - testing of these crates requires compilation to Miden Assembly, and thus require you to write an integration test that builds against the SDK and calls the functions you wish to test.
* DON'T duplicate test setup boilerplate that has already been defined in the repo. Try to use the existing test helpers where possible. If a specific helper would be useful in reducing boilerplate, try to define it in a central place if possible.
