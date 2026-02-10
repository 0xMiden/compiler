//! A configurable logger that can be configured both directly and via environment variables.
//!
//! Designed for use with [`log`](log-crate-url).
//!
//! This crate is inspired by [`midenc_log`](https://crates.io/crates/midenc_log), but provides
//! additional features specific to the Miden compiler that make it more useful when diagnosing
//! issues in a pipeline that has an enormous quantity of verbose trace logs that are emitted for
//! a large number of different entities in the compilation graph. Typically, we're interested in
//! output related to a specific function, possibly refined to specific components of the compiler,
//! or even specific areas of those components.
//!
//! There are a few primary use cases we're interested in supporting with this crate:
//!
//! 1. Emit all logs at a given log level, e.g. `MIDENC_TRACE=trace cargo miden build`
//! 2. Same as 1, but for a specific component of the compiler, e.g. `MIDENC_TRACE=codegen=trace`
//! 3. Same as 2, but for a specific topic within that component, e.g.
//!    `MIDENC_TRACE=pass:cse=trace`
//! 4. Same as 2, but for any topic in that component which matches a regular expression, e.g.
//!    `MIDENC_TRACE=pass:.*=trace`
//! 5. Any of the above, but only for an operation which matches a given filter:
//!    a. Match any op that implements Symbol, e.g. `MIDENC_TRACE=trace MIDENC_TRACE_FILTER=symbol:*`
//!    b. Match a symbol whose name matches a regular expression, e.g. `MIDENC_TRACE=trace MIDENC_TRACE_FILTER=symbol:foo`
//!    c. Match a specific operation type, e.g. `MIDENC_TRACE=trace MIDENC_TRACE_FILTER=op:hir.function`
//! 6. Negated filters, e.g. emit all logs which match any positive filters and are not also matched
//!    by the negative filter, e.g.:
//!    a. Emit everything except logs for the "dataflow" component: `MIDENC_TRACE=trace,-dataflow`
//!    b. Emit everything except trace-level logs for the "dataflow" component: `MIDENC_TRACE=trace,-dataflow=trace`
//!    c. Emit logs for the "dataflow" component, except trace-level logs for the "solver" topic: `MIDENC_TRACE=dataflow=trace,-dataflow:solver=trace`
//!
//! By default, logs are written to `stderr`, but this is configurable.
//!
//! NOTE: The functionality here relies on `log` targets using a structured format that we can
//! parse and extract the relevant pieces from. Any `log` targets which are not in this format
//! will be treated as a single component and emitted according to the default logging rules,
//! while still supporting filtering those components. The format is as follows:
//!
//! ```ignore
//! scope := component (":" topic)*
//!
//! component := [A-Za-z0-9_-]+
//! topic := [^:,=]+
//! ```
//!
//! When there are nested topics, the last topic is considered to contain the relevant symbol for
//! that log message, and so will be filtered by `MIDENC_TRACE_FILTER`, not any topic patterns in
//! `MIDENC_TRACE`.
//!
//! ## Example
//!
//! ```
//! use log::{debug, error, log_enabled, info, Level};
//!
//! midenc_log::init();
//!
//! debug!("this is a debug {}", "message");
//! error!("this is printed by default");
//!
//! if log_enabled!(Level::Info) {
//!     let x = 3 * 4; // expensive computation
//!     info!("the answer was: {}", x);
//! }
//! ```
//!
//! Assumes the binary is `main`:
//!
//! ```console
//! $ RUST_LOG=error ./main
//! [2017-11-09T02:12:24Z ERROR main] this is printed by default
//! ```
//!
//! ```console
//! $ RUST_LOG=info ./main
//! [2017-11-09T02:12:24Z ERROR main] this is printed by default
//! [2017-11-09T02:12:24Z INFO main] the answer was: 12
//! ```
//!
//! ```console
//! $ RUST_LOG=debug ./main
//! [2017-11-09T02:12:24Z DEBUG main] this is a debug message
//! [2017-11-09T02:12:24Z ERROR main] this is printed by default
//! [2017-11-09T02:12:24Z INFO main] the answer was: 12
//! ```
//!
//! You can also set the log level on a per module basis:
//!
//! ```console
//! $ RUST_LOG=main=info ./main
//! [2017-11-09T02:12:24Z ERROR main] this is printed by default
//! [2017-11-09T02:12:24Z INFO main] the answer was: 12
//! ```
//!
//! And enable all logging:
//!
//! ```console
//! $ RUST_LOG=main ./main
//! [2017-11-09T02:12:24Z DEBUG main] this is a debug message
//! [2017-11-09T02:12:24Z ERROR main] this is printed by default
//! [2017-11-09T02:12:24Z INFO main] the answer was: 12
//! ```
//!
//! If the binary name contains hyphens, you will need to replace
//! them with underscores:
//!
//! ```console
//! $ RUST_LOG=my_app ./my-app
//! [2017-11-09T02:12:24Z DEBUG my_app] this is a debug message
//! [2017-11-09T02:12:24Z ERROR my_app] this is printed by default
//! [2017-11-09T02:12:24Z INFO my_app] the answer was: 12
//! ```
//!
//! This is because Rust modules and crates cannot contain hyphens
//! in their name, although `cargo` continues to accept them.
//!
//! See the documentation for the [`log` crate][log-crate-url] for more
//! information about its API.
//!
//! ## Enabling logging
//!
//! **By default all logging is disabled except for the `error` level**
//!
//! The **`RUST_LOG`** environment variable controls logging with the syntax:
//! ```console
//! RUST_LOG=[target][=][level][,...]
//! ```
//! Or in other words, its a comma-separated list of directives.
//! Directives can filter by **target**, by **level**, or both (using `=`).
//!
//! For example,
//! ```console
//! RUST_LOG=data=debug,hardware=debug
//! ```
//!
//! **target** is typically the path of the module the message
//! in question originated from, though it can be overridden.
//! The path is rooted in the name of the crate it was compiled for, so if
//! your program is in a file called, for example, `hello.rs`, the path would
//! simply be `hello`.
//!
//! Furthermore, the log can be filtered using prefix-search based on the
//! specified log target.
//!
//! For example, `RUST_LOG=example` would match the following targets:
//! - `example`
//! - `example::test`
//! - `example::test::module::submodule`
//! - `examples::and_more_examples`
//!
//! When providing the crate name or a module path, explicitly specifying the
//! log level is optional. If omitted, all logging for the item will be
//! enabled.
//!
//! **level** is the maximum [`log::Level`][level-enum] to be shown and includes:
//! - `error`
//! - `warn`
//! - `info`
//! - `debug`
//! - `trace`
//! - `off` (pseudo level to disable all logging for the target)
//!
//! Logging level names are case-insensitive; e.g.,
//! `debug`, `DEBUG`, and `dEbuG` all represent the same logging level. For
//! consistency, our convention is to use the lower case names. Where our docs
//! do use other forms, they do so in the context of specific examples, so you
//! won't be surprised if you see similar usage in the wild.
//!
//! Some examples of valid values of `RUST_LOG` are:
//!
//! - `RUST_LOG=hello` turns on all logging for the `hello` module
//! - `RUST_LOG=trace` turns on all logging for the application, regardless of its name
//! - `RUST_LOG=TRACE` turns on all logging for the application, regardless of its name (same as previous)
//! - `RUST_LOG=info` turns on all info logging
//! - `RUST_LOG=INFO` turns on all info logging (same as previous)
//! - `RUST_LOG=hello=debug` turns on debug logging for `hello`
//! - `RUST_LOG=hello=DEBUG` turns on debug logging for `hello` (same as previous)
//! - `RUST_LOG=hello,std::option` turns on `hello`, and std's option logging
//! - `RUST_LOG=error,hello=warn` turn on global error logging and also warn for `hello`
//! - `RUST_LOG=error,hello=off`  turn on global error logging, but turn off logging for `hello`
//! - `RUST_LOG=off` turns off all logging for the application
//! - `RUST_LOG=OFF` turns off all logging for the application (same as previous)
//!
//! ## Filtering results
//!
//! A `RUST_LOG` directive may include a regex filter. The syntax is to append `/`
//! followed by a regex. Each message is checked against the regex, and is only
//! logged if it matches. Note that the matching is done after formatting the
//! log string but before adding any logging meta-data. There is a single filter
//! for all modules.
//!
//! Some examples:
//!
//! * `hello/foo` turns on all logging for the 'hello' module where the log
//!   message includes 'foo'.
//! * `info/f.o` turns on all info logging where the log message includes 'foo',
//!   'f1o', 'fao', etc.
//! * `hello=debug/foo*foo` turns on debug logging for 'hello' where the log
//!   message includes 'foofoo' or 'fofoo' or 'fooooooofoo', etc.
//! * `error,hello=warn/[0-9]scopes` turn on global error logging and also
//!   warn for hello. In both cases the log message must include a single digit
//!   number followed by 'scopes'.
//!
//! ## Capturing logs in tests
//!
//! Records logged during `cargo test` will not be captured by the test harness by default.
//! The [`Builder::is_test`] method can be used in unit tests to ensure logs will be captured:
//!
//! ```test_harness
//! #[cfg(test)]
//! mod tests {
//!     use log::info;
//!
//!     fn init() {
//!         let _ = midenc_log::builder().is_test(true).try_init();
//!     }
//!
//!     #[test]
//!     fn it_works() {
//!         init();
//!
//!         info!("This record will be captured by `cargo test`");
//!
//!         assert_eq!(2, 1 + 1);
//!     }
//! }
//! ```
//!
//! Enabling test capturing comes at the expense of color and other style support
//! and may have performance implications.
//!
//! ## Colors
//!
//! Outputting of colors and other styles can be controlled by the `RUST_LOG_STYLE`
//! environment variable. It accepts the following [values][fmt::WriteStyle]:
//!
//! * `auto` (default) will attempt to print style characters, but don't force the issue.
//!   If the console isn't available on Windows, or if TERM=dumb, for example, then don't print colors.
//! * `always` will always print style characters even if they aren't supported by the terminal.
//!   This includes emitting ANSI colors on Windows if the console API is unavailable.
//! * `never` will never print style characters.
//!
//! Color may be applied in the logged message or a [custom formatter][fmt].
//!
//! <div class="warning">
//!
//! Logging of untrusted inputs can cause unexpected behavior as they may include ANSI escape codes which
//! will be forwarded to the users terminal as part of "Weaponizing ANSI Escape Sequences".
//!
//! Mitigations include:
//! - Setting `RUST_LOG_STYLE=never` to have all ANSI escape codes stripped
//! - In the application, calling [`Builder::write_style(Never)`][Builder::write_style] to have all ANSI escape codes stripped
//! - In the application, [stripping ANSI escape codes](https://docs.rs/anstream/latest/anstream/adapter/fn.strip_str.html)
//!   from user inputs
//!
//! Note: deactivating the build-time feature `color` is not a mitigation as that removes all ANSI escape code
//! stripping from `midenc_log`.
//!
//! </div>
//!
//! ## Tweaking the default format
//!
//! Parts of the default format can be excluded from the log output using the [`Builder`].
//! The following example excludes the timestamp from the log output:
//!
//! ```
//! midenc_log::builder()
//!     .format_timestamp(None)
//!     .init();
//! ```
//!
//! ### Stability of the default format
//!
//! The default format won't optimise for long-term stability, and explicitly makes no
//! guarantees about the stability of its output across major, minor or patch version
//! bumps during `0.x`.
//!
//! If you want to capture or interpret the output of `midenc_log` programmatically
//! then you should use a custom format.
//!
//! ### Using a custom format
//!
//! Custom formats can be provided as closures to the [`Builder`].
//! These closures take a [`Formatter`][crate::fmt::Formatter] and `log::Record` as arguments:
//!
//! ```
//! use std::io::Write;
//!
//! midenc_log::builder()
//!     .format(|buf, record| {
//!         writeln!(buf, "{}: {}", record.level(), record.args())
//!     })
//!     .init();
//! ```
//!
//! See the [`fmt`] module for more details about custom formats.
//!
//! ## Specifying defaults for environment variables
//!
//! `midenc_log` can read configuration from environment variables.
//! If these variables aren't present, the default value to use can be tweaked with the [`Env`] type.
//! The following example defaults to log `warn` and above if the `RUST_LOG` environment variable
//! isn't set:
//!
//! ```
//! use midenc_log::Env;
//!
//! midenc_log::Builder::from_env(Env::default().default_filter_or("warn")).init();
//! ```
//!
//! [level-enum]: https://docs.rs/log/latest/log/enum.Level.html
//! [log-crate-url]: https://docs.rs/log

#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(clippy::print_stderr)]
#![warn(clippy::print_stdout)]
#![allow(clippy::test_attr_in_doctest)]

pub mod filter;
mod logger;
mod writer;

pub mod fmt;

pub use self::{
    fmt::{Target, TimestampPrecision, WriteStyle},
    logger::*,
};

#[doc = include_str!("../README.md")]
#[cfg(doctest)]
pub struct ReadmeDoctests;
