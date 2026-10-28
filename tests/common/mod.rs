// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2014, Nathan LaFreniere and other contributors (https://github.com/ljharb/qs/graphs/contributors)

//! Helpers shared by the integration tests: values and options are written as JSON, results compared as the JSON
//! text the command would print.

use qs::js::json;
use qs::{JsError, Value};

/// A JSON value from test text.
pub fn value(text: &str) -> Value {
    match json::parse_str(text) {
        Ok(value) => value,
        Err(error) => panic!("test JSON {text:?} does not parse: {error}"),
    }
}

/// `JSON.stringify` of a value.
pub fn to_json(value: &Value) -> String {
    json::stringify(value)
        .map(|text| text.to_string_lossy())
        .unwrap_or_default()
}

/// `name: message` of an error.
pub fn error_text(error: &JsError) -> String {
    error.to_string()
}
