//! Filtering for log records.
//!
//! You can use the [`Filter`] type in your own logger implementation to use the same
//! filter parsing and matching as `midenc_log`.
//!
//! ## Using `midenc_log::filter` in your own logger
//!
//! You can use `midenc_log`'s filtering functionality with your own logger.
//! Call [`Builder::parse`] to parse directives from a string when constructing
//! your logger. Call [`Filter::matches`] to check whether a record should be
//! logged based on the parsed filters when log records are received.
//!
//! ```
//! use midenc_log::filter::{Builder, FilteredLog};
//! use log::{Log, Metadata, Record};
//!
//! struct PrintLogger;
//!
//! impl Log for PrintLogger {
//!     fn enabled(&self, metadata: &Metadata) -> bool {
//!         true
//!     }
//!
//!     fn log(&self, record: &Record) {
//!         println!("{:?}", record);
//!     }
//!
//!     fn flush(&self) {}
//! }
//!
//! let mut builder = Builder::new();
//! // Parse a directives string from an environment variable
//! if let Ok(ref filter) = std::env::var("MY_LOG_LEVEL") {
//!     builder.parse(filter);
//! }
//!
//! let logger = FilteredLog::new(PrintLogger, builder.build());
//! ```

mod directive;
#[allow(clippy::module_inception)]
mod filter;
mod filtered_log;
mod kv_filter;
mod op;
mod parser;

use directive::{Directive, DirectiveKind, enabled};
pub use filter::{Builder, Filter};
pub use filtered_log::FilteredLog;
use kv_filter::KvFilter;
use op::FilterOp;
pub use parser::ParseError;
use parser::parse_spec;
