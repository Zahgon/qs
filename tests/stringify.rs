// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2014, Nathan LaFreniere and other contributors (https://github.com/ljharb/qs/graphs/contributors)

//! `stringify`, ported from the library's test/stringify.js.

mod common;

use common::{error_text, to_json, value};
use qs::Value;

/// Stringifies a value given as JSON with options given as JSON; the query string, or `Name: message` of the
/// error.
fn stringify(input: &str, options: &str) -> String {
    match qs::stringify(&value(input), &value(options)) {
        Ok(text) => text.to_string_lossy(),
        Err(error) => error_text(&error),
    }
}

fn check(input: &str, options: &str, expected: &str) {
    assert_eq!(stringify(input, options), expected, "stringify({input}, {options})");
}

#[test]
fn stringifies_a_querystring_object() {
    check(r#"{"a":"b"}"#, "null", "a=b");
    check(r#"{"a":1}"#, "null", "a=1");
    check(r#"{"a":1,"b":2}"#, "null", "a=1&b=2");
    check(r#"{"a":"A_Z"}"#, "null", "a=A_Z");
    check(r#"{"a":"€"}"#, "null", "a=%E2%82%AC");
    check(r#"{"a":""}"#, "null", "a=%EE%80%80");
    check(r#"{"a":"א"}"#, "null", "a=%D7%90");
    check(r#"{"a":"𐐷"}"#, "null", "a=%F0%90%90%B7");
}

#[test]
fn stringifies_nested_objects() {
    check(r#"{"a":{"b":"c"}}"#, "null", "a%5Bb%5D=c");
    check(r#"{"a":{"b":{"c":{"d":"e"}}}}"#, "null", "a%5Bb%5D%5Bc%5D%5Bd%5D=e");
    check(r#"{"a":{"b":"c"}}"#, r#"{"allowDots":true}"#, "a.b=c");
    check(
        r#"{"name.obj":{"first":"John"}}"#,
        r#"{"encodeDotInKeys":true}"#,
        "name%252Eobj.first=John",
    );
}

#[test]
fn stringifies_arrays_in_every_format() {
    let value = r#"{"a":["b","c","d"]}"#;
    check(value, r#"{"encodeValuesOnly":true}"#, "a[0]=b&a[1]=c&a[2]=d");
    check(
        value,
        r#"{"arrayFormat":"indices","encodeValuesOnly":true}"#,
        "a[0]=b&a[1]=c&a[2]=d",
    );
    check(
        value,
        r#"{"arrayFormat":"brackets","encodeValuesOnly":true}"#,
        "a[]=b&a[]=c&a[]=d",
    );
    check(
        value,
        r#"{"arrayFormat":"repeat","encodeValuesOnly":true}"#,
        "a=b&a=c&a=d",
    );
    check(value, r#"{"arrayFormat":"comma","encodeValuesOnly":true}"#, "a=b,c,d");
    check(value, r#"{"indices":false}"#, "a=b&a=c&a=d");
    check(
        r#"{"a":["c"]}"#,
        r#"{"arrayFormat":"comma","commaRoundTrip":true}"#,
        "a%5B%5D=c",
    );
    check(r#"{"a":[],"b":"zz"}"#, r#"{"allowEmptyArrays":true}"#, "a[]&b=zz");
}

#[test]
fn handles_nulls() {
    check(r#"{"a":null}"#, "null", "a=");
    check(r#"{"a":null}"#, r#"{"strictNullHandling":true}"#, "a");
    check(r#"{"a":"b","c":null}"#, r#"{"skipNulls":true}"#, "a=b");
    check(r#"{"a":{}}"#, "null", "");
    check(r#"{"a":[]}"#, "null", "");
}

#[test]
fn applies_formats_and_charsets() {
    check(r#"{"a":"b c"}"#, "null", "a=b%20c");
    check(r#"{"a":"b c"}"#, r#"{"format":"RFC1738"}"#, "a=b+c");
    check(r#"{"a":"b c"}"#, r#"{"format":"RFC3986"}"#, "a=b%20c");
    check(r#"{"æ":"æ"}"#, r#"{"charset":"iso-8859-1"}"#, "%E6=%E6");
    check(r#"{"a":"☺"}"#, r#"{"charset":"iso-8859-1"}"#, "a=%26%239786%3B");
    check(r#"{"a":"æ"}"#, r#"{"charsetSentinel":true}"#, "utf8=%E2%9C%93&a=%C3%A6");
    check(r#"{"a":"b"}"#, r#"{"addQueryPrefix":true}"#, "?a=b");
    check(r#"{"a":"b","c":"d"}"#, r#"{"delimiter":";"}"#, "a=b;c=d");
    check(r#"{"a":"b"}"#, r#"{"encode":false}"#, "a=b");
}

#[test]
fn filters_and_sorts() {
    check(r#"{"a":"b","c":"d","e":{"f":"g"}}"#, r#"{"filter":["a","e"]}"#, "a=b");
    check(
        r#"{"a":{"b":[1,2,3,4],"c":"d"},"c":"f"}"#,
        r#"{"filter":["a","b",0,2],"encodeValuesOnly":true}"#,
        "a[b][0]=1&a[b][2]=3",
    );
    check(r#"{"b":"1","a":"2"}"#, r#"{"depth":0}"#, "b=1&a=2");
    check(
        r#"{"a":{"b":"c"}}"#,
        r#"{"depth":0}"#,
        "RangeError: Input depth exceeded depth option of 0",
    );
}

#[test]
fn checks_option_types() {
    check(
        r#"{"a":"b"}"#,
        r#"{"format":"UFO1234"}"#,
        "TypeError: Unknown format option provided.",
    );
    check(
        r#"{"a":"b"}"#,
        r#"{"encoder":"x"}"#,
        "TypeError: Encoder has to be a function.",
    );
    check(
        r#"{"a":"b"}"#,
        r#"{"commaRoundTrip":"x"}"#,
        "TypeError: `commaRoundTrip` must be a boolean, or absent",
    );
}

#[test]
fn round_trips_through_parse() {
    let original = value(r#"{"a":{"b":["c","d"],"e":"f g"},"h":"&="}"#);
    let text = qs::stringify(&original, &Value::Undefined).map(|t| t.to_string_lossy());
    assert_eq!(
        text.as_deref().ok(),
        Some("a%5Bb%5D%5B0%5D=c&a%5Bb%5D%5B1%5D=d&a%5Be%5D=f%20g&h=%26%3D")
    );
    let parsed = text.and_then(|t| qs::parse(&Value::str(&t), &Value::Undefined));
    assert_eq!(parsed.map(|v| to_json(&v)).ok(), Some(to_json(&original)));
}
