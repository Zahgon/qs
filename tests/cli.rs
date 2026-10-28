// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2014, Nathan LaFreniere and other contributors (https://github.com/ljharb/qs/graphs/contributors)

//! The `qs` command end to end: requests on standard input, responses on standard output, usage and exit status.

use std::io::Write;
use std::process::{Command, Stdio};

/// Runs the built `qs` command with the given arguments and standard input.
fn run(args: &[&str], input: &[u8]) -> (String, String, i32) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_qs"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the qs binary starts");
    child
        .stdin
        .take()
        .expect("standard input is piped")
        .write_all(input)
        .expect("standard input accepts the requests");
    let output = child.wait_with_output().expect("the qs binary finishes");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn answers_each_request_line() {
    let input = concat!(
        r#"{"op":"parse","input":"a[b]=c&a[d]=e"}"#,
        "\n\n",
        r#"{"op":"stringify","value":{"a":["b","c"]},"options":{"arrayFormat":"brackets"}}"#,
        "\n",
    );
    let (stdout, stderr, status) = run(&[], input.as_bytes());
    assert_eq!(
        stdout,
        "{\"result\":{\"a\":{\"b\":\"c\",\"d\":\"e\"}}}\n{\"result\":\"a%5B%5D=b&a%5B%5D=c\"}\n"
    );
    assert_eq!(stderr, "");
    assert_eq!(status, 0);
}

#[test]
fn reports_errors_and_fails() {
    let input = concat!(
        r#"{"op":"parse","input":"a=1"}"#,
        "\n{oops\n",
        r#"{"op":"stringify","value":{},"options":{"format":"x"}}"#,
        "\n",
    );
    let (stdout, _, status) = run(&[], input.as_bytes());
    assert_eq!(
        stdout,
        concat!(
            r#"{"result":{"a":"1"}}"#,
            "\n",
            r#"{"error":{"name":"SyntaxError","message":"#,
            r#""Expected property name or '}' in JSON at position 1 (line 1 column 2)"}}"#,
            "\n",
            r#"{"error":{"name":"TypeError","message":"Unknown format option provided."}}"#,
            "\n",
        )
    );
    assert_eq!(status, 1);
}

#[test]
fn rejects_arguments_with_usage() {
    let (stdout, stderr, status) = run(&["--help"], b"");
    assert_eq!(stdout, "");
    assert!(stderr.starts_with("usage: qs < requests.jsonl\n"));
    assert_eq!(status, 2);
}

#[test]
fn records_trace_calls() {
    let input = concat!(
        r#"{"op":"stringify","value":{"a":{"b":null}},"options":{"filter":"trace","strictNullHandling":true}}"#,
        "\n",
    );
    let (stdout, _, status) = run(&[], input.as_bytes());
    assert_eq!(
        stdout,
        "{\"result\":\"a%5Bb%5D\",\"calls\":[[\"\",\"object\"],[\"a\",\"object\"],[\"a[b]\",\"null\"]]}\n"
    );
    assert_eq!(status, 0);
}
