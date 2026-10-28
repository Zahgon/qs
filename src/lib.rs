// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2014, Nathan LaFreniere and other contributors (https://github.com/ljharb/qs/graphs/contributors)

//! A querystring parser and stringifier with support for nesting and arrays, with a depth limit: a Rust port of
//! the `qs` JavaScript library.
//!
//! [`parse()`] turns a query string into nested objects and arrays and [`stringify()`] does the reverse. Values and
//! options are JavaScript values ([`js::Value`]): the library's behaviour, from its option checks to the order of
//! object keys and the printing of numbers, is that of the JavaScript original.

pub mod cli;
pub mod formats;
pub mod js;
pub mod parse;
pub mod stringify;
pub mod utils;

pub use formats::{Charset, Format};
pub use js::{JsError, JsString, Value};
pub use parse::{parse, ParseOptions};
pub use stringify::{stringify, StringifyOptions};
