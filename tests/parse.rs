// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2014, Nathan LaFreniere and other contributors (https://github.com/ljharb/qs/graphs/contributors)

//! `parse`, ported from the library's test/parse.js.

mod common;

use common::{error_text, to_json, value};
use qs::Value;

/// Parses a query string with options given as JSON; the result as JSON text, or `Name: message` of the error.
fn parse(query: &str, options: &str) -> String {
    match qs::parse(&Value::str(query), &value(options)) {
        Ok(result) => to_json(&result),
        Err(error) => error_text(&error),
    }
}

fn check(query: &str, options: &str, expected: &str) {
    assert_eq!(parse(query, options), expected, "parse({query:?}, {options})");
}

#[test]
fn parses_a_simple_string() {
    check("0=foo", "null", r#"{"0":"foo"}"#);
    check("foo=c++", "null", r#"{"foo":"c  "}"#);
    check("a[>=]=23", "null", r#"{"a":{">=":"23"}}"#);
    check("a[<=>]==23", "null", r#"{"a":{"<=>":"=23"}}"#);
    check("a[==]=23", "null", r#"{"a":{"==":"23"}}"#);
    check("foo", r#"{"strictNullHandling":true}"#, r#"{"foo":null}"#);
    check("foo", "null", r#"{"foo":""}"#);
    check("foo=", "null", r#"{"foo":""}"#);
    check("foo=bar", "null", r#"{"foo":"bar"}"#);
    check(" foo = bar = baz ", "null", r#"{" foo ":" bar = baz "}"#);
    check("foo=bar=baz", "null", r#"{"foo":"bar=baz"}"#);
    check("foo=bar&bar=baz", "null", r#"{"foo":"bar","bar":"baz"}"#);
    check("foo2=bar2&baz2=", "null", r#"{"foo2":"bar2","baz2":""}"#);
    check(
        "foo=bar&baz",
        r#"{"strictNullHandling":true}"#,
        r#"{"foo":"bar","baz":null}"#,
    );
    check("foo=bar&baz", "null", r#"{"foo":"bar","baz":""}"#);
    check(
        "cht=p3&chd=t:60,40&chs=250x100&chl=Hello|World",
        "null",
        r#"{"cht":"p3","chd":"t:60,40","chs":"250x100","chl":"Hello|World"}"#,
    );
}

#[test]
fn parses_nested_keys() {
    check("a[b]=c", "null", r#"{"a":{"b":"c"}}"#);
    check("a[b][c]=d", "null", r#"{"a":{"b":{"c":"d"}}}"#);
    check(
        "a[b][c][d][e][f][g][h]=i",
        "null",
        r#"{"a":{"b":{"c":{"d":{"e":{"f":{"[g][h]":"i"}}}}}}}"#,
    );
    check("a[b][c]=d", r#"{"depth":1}"#, r#"{"a":{"b":{"[c]":"d"}}}"#);
    check("a[b][c][d]=e", r#"{"depth":0}"#, r#"{"a[b][c][d]":"e"}"#);
    check("a[b][c]=d", r#"{"depth":false}"#, r#"{"a[b][c]":"d"}"#);
    check("[[x]]=y", "null", r#"{"[x]":"y"}"#);
}

#[test]
fn parses_arrays() {
    check("a=b&a=c", "null", r#"{"a":["b","c"]}"#);
    check("a[]=b&a[]=c", "null", r#"{"a":["b","c"]}"#);
    check("a[1]=c&a[0]=b&a[2]=d", "null", r#"{"a":["b","c","d"]}"#);
    check("a[1]=b&a[15]=c", "null", r#"{"a":["b","c"]}"#);
    check("a[20]=a", "null", r#"{"a":{"20":"a"}}"#);
    check("a[21]=a", "null", r#"{"a":{"21":"a"}}"#);
    check(
        "a[1]=b&a[15]=c",
        r#"{"allowSparse":true}"#,
        r#"{"a":[null,"b",null,null,null,null,null,null,null,null,null,null,null,null,null,"c"]}"#,
    );
    check("a[0]=b&a[b]=c", "null", r#"{"a":{"0":"b","b":"c"}}"#);
    check("a[]=b", r#"{"parseArrays":false}"#, r#"{"a":{"0":"b"}}"#);
    check("a[0]=b", r#"{"arrayLimit":0}"#, r#"{"a":{"0":"b"}}"#);
    check("a[-1]=b", "null", r#"{"a":{"-1":"b"}}"#);
    check(
        "foo[]&bar=baz",
        r#"{"allowEmptyArrays":true}"#,
        r#"{"foo":[],"bar":"baz"}"#,
    );
}

#[test]
fn applies_the_array_limit_to_repeated_keys() {
    check(
        "a=1&a=2&a=3",
        r#"{"arrayLimit":2}"#,
        r#"{"a":{"0":"1","1":"2","2":"3"}}"#,
    );
    check(
        "a[]=1&a[]=2&a[]=3",
        r#"{"arrayLimit":2,"throwOnLimitExceeded":true}"#,
        "RangeError: Array limit exceeded. Only 2 elements allowed in an array.",
    );
}

#[test]
fn merges_conflicting_shapes() {
    check("a[b]=c&a=d", "null", r#"{"a":[{"b":"c"},"d"]}"#);
    check("a[b]=c&a=d", r#"{"strictMerge":false}"#, r#"{"a":{"b":"c","d":true}}"#);
    check("a=b&a[]=c", "null", r#"{"a":["b","c"]}"#);
    check("foo=bar&foo=baz", r#"{"duplicates":"first"}"#, r#"{"foo":"bar"}"#);
    check("foo=bar&foo=baz", r#"{"duplicates":"last"}"#, r#"{"foo":"baz"}"#);
}

#[test]
fn reads_dots_and_commas() {
    check("a.b=c", r#"{"allowDots":true}"#, r#"{"a":{"b":"c"}}"#);
    check("a.b=c", "null", r#"{"a.b":"c"}"#);
    check(
        "name%252Eobj.first=John",
        r#"{"decodeDotInKeys":true}"#,
        r#"{"name.obj":{"first":"John"}}"#,
    );
    check("foo=1,2", r#"{"comma":true}"#, r#"{"foo":["1","2"]}"#);
    check("foo[]=1,2", r#"{"comma":true}"#, r#"{"foo":[["1","2"]]}"#);
    check("foo=", r#"{"comma":true}"#, r#"{"foo":""}"#);
}

#[test]
fn decodes_charsets() {
    check("%C2%A2=%E2%82%AC", "null", r#"{"¢":"€"}"#);
    check("%A2=%BD", r#"{"charset":"iso-8859-1"}"#, r#"{"¢":"½"}"#);
    check(
        "utf8=%26%2310003%3B&%C3%B8=%C3%B8",
        r#"{"charsetSentinel":true}"#,
        r#"{"Ã¸":"Ã¸"}"#,
    );
    check(
        "foo=%26%239786%3B",
        r#"{"charset":"iso-8859-1","interpretNumericEntities":true}"#,
        r#"{"foo":"☺"}"#,
    );
    check("a=%G1", "null", r#"{"a":"%G1"}"#);
}

#[test]
fn honours_delimiters_and_limits() {
    check("a=b;c=d", r#"{"delimiter":";"}"#, r#"{"a":"b","c":"d"}"#);
    check("a=b&c=d", r#"{"parameterLimit":1}"#, r#"{"a":"b"}"#);
    check(
        "a=b&c=d",
        r#"{"parameterLimit":1,"throwOnLimitExceeded":true}"#,
        "RangeError: Parameter limit exceeded. Only 1 parameter allowed.",
    );
    check("?foo=bar", r#"{"ignoreQueryPrefix":true}"#, r#"{"foo":"bar"}"#);
    check("?foo=bar", "null", r#"{"?foo":"bar"}"#);
}

#[test]
fn guards_prototype_keys() {
    check("a[hasOwnProperty]=b", "null", "{}");
    check(
        "a[hasOwnProperty]=b",
        r#"{"allowPrototypes":true}"#,
        r#"{"a":{"hasOwnProperty":"b"}}"#,
    );
    check("a[__proto__][b]=c", r#"{"allowPrototypes":true}"#, r#"{"a":{}}"#);
    check("toString=x", r#"{"plainObjects":true}"#, r#"{"toString":"x"}"#);
}

#[test]
fn checks_option_types() {
    check(
        "a=b",
        r#"{"charset":"foobar"}"#,
        "TypeError: The charset option must be either utf-8, iso-8859-1, or undefined",
    );
    check(
        "a=b",
        r#"{"duplicates":"x"}"#,
        "TypeError: The duplicates option must be either combine, first, or last",
    );
    check(
        "a=b",
        r#"{"decodeDotInKeys":"x"}"#,
        "TypeError: `decodeDotInKeys` option can only be `true` or `false`, when provided",
    );
}

#[test]
fn parses_object_inputs() {
    let input = value(r#"{"user[name]":{"pop[bob]":3},"user[email]":null}"#);
    let parsed = qs::parse(&input, &Value::Null).map(|v| to_json(&v));
    assert_eq!(
        parsed.ok().as_deref(),
        Some(r#"{"user":{"name":{"pop[bob]":3},"email":null}}"#)
    );
}
