// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2014, Nathan LaFreniere and other contributors (https://github.com/ljharb/qs/graphs/contributors)

//! The `qs` command: JSON-lines requests for `parse` and `stringify` on standard input, one JSON response per
//! line on standard output.

use std::cell::RefCell;
use std::rc::Rc;

use crate::formats::{Charset, Kind};
use crate::js::date::{time_clip, to_utc_string};
use crate::js::{json, Array, Function, JsError, JsString, Object, Value};
use crate::{parse, stringify, utils};

/// The text written to standard error when the command is given arguments.
pub const USAGE: &str = "usage: qs < requests.jsonl\n\
\n\
Reads one JSON request per line on standard input and writes one JSON response per line.\n\
Requests: {\"op\":\"parse\",\"input\":V,\"options\":O}, {\"op\":\"stringify\",\"value\":V,\"options\":O}\n";

/// An error of the front end itself.
fn request_error(message: &str) -> JsError {
    JsError::new("RequestError", message)
}

/// Answers every request line of `input`; returns the output and the exit status (1 when any request failed).
pub fn run(input: &[u8]) -> (Vec<u8>, u8) {
    let text: Vec<u16> = String::from_utf8_lossy(input).encode_utf16().collect();
    let mut output = String::new();
    let mut failed = false;
    for line in text.split(|&unit| unit == u16::from(b'\n')) {
        if line.is_empty() {
            continue;
        }
        let response = json::parse(line).and_then(handle).unwrap_or_else(|error| {
            failed = true;
            error_response(&error)
        });
        // A response is an object, which always serializes (`None` stands only for `undefined`).
        let serialized = json::stringify(&response).unwrap_or_default();
        output.push_str(&serialized.to_utf8());
        output.push('\n');
    }
    (output.into_bytes(), u8::from(failed))
}

fn error_response(error: &JsError) -> Value {
    let mut detail = Object::new();
    detail.define(JsString::from("name"), Value::str(&error.name));
    detail.define(JsString::from("message"), Value::String(error.message.clone()));
    let mut response = Object::new();
    response.define(JsString::from("error"), Value::Object(detail));
    Value::Object(response)
}

fn member(request: &Object, name: &str) -> Value {
    request.get_str(name).cloned().unwrap_or_default()
}

/// One request: `{"op":"parse",...}` or `{"op":"stringify",...}`.
fn handle(request: Value) -> Result<Value, JsError> {
    let Value::Object(request) = request else {
        return Err(request_error("a request must be a JSON object"));
    };
    let op = member(&request, "op");
    let mut response = Object::new();
    match op.as_string() {
        Some(name) if name.eq_str("parse") => {
            let options = build_options(Op::Parse, member(&request, "options"), None)?;
            let result = parse::parse(&member(&request, "input"), &options)?;
            response.define(JsString::from("result"), result);
        }
        Some(name) if name.eq_str("stringify") => {
            let calls = Rc::new(RefCell::new(Array::new()));
            let spec = member(&request, "options");
            let tracing = spec
                .as_object()
                .and_then(|spec| spec.get_str("filter"))
                .and_then(Value::as_string)
                .is_some_and(|filter| filter.eq_str("trace"));
            let options = build_options(Op::Stringify, spec, Some(Rc::clone(&calls)))?;
            let value = revive(member(&request, "value"))?;
            let result = stringify::stringify(&value, &options)?;
            response.define(JsString::from("result"), Value::String(result));
            if tracing {
                response.define(JsString::from("calls"), Value::Array(calls.take()));
            }
        }
        _ => {
            let mut message = JsString::from("unknown op: ");
            message.push_js(&json::stringify(&op).unwrap_or_else(|| JsString::from("undefined")));
            return Err(JsError::new("RequestError", message));
        }
    }
    Ok(Value::Object(response))
}

/// The value handed to `stringify`: `{"$date": number}` becomes a `Date`, `{"$undefined": ...}` becomes
/// `undefined` (each as the object's only member), at any depth. `JSON.parse` has already refused values nested
/// deeper than [`json::MAX_NESTING`], which bounds the recursion.
fn revive(value: Value) -> Result<Value, JsError> {
    match value {
        Value::Array(array) => {
            let mut revived = Array::new();
            for slot in array.into_slots() {
                revived.push(revive(slot.unwrap_or_default())?);
            }
            Ok(Value::Array(revived))
        }
        Value::Object(object) => {
            if object.len() == 1 {
                if let Some(time) = object.get_str("$date") {
                    let time = time
                        .as_number()
                        .ok_or_else(|| request_error("$date must be a number"))?;
                    return Ok(Value::Date(time_clip(time)));
                }
                if object.has_own_str("$undefined") {
                    return Ok(Value::Undefined);
                }
            }
            let mut revived = Object::new();
            for (key, item) in object.into_entries() {
                revived.define(key, revive(item)?);
            }
            Ok(Value::Object(revived))
        }
        other => Ok(other),
    }
}

/// The operation an options object is built for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Parse,
    Stringify,
}

/// The options argument: anything but a JSON object as it is; an object member by member, with the functions
/// chosen by name and the regular expression delimiter put in place.
fn build_options(op: Op, spec: Value, calls: Option<Rc<RefCell<Array>>>) -> Result<Value, JsError> {
    let Value::Object(spec) = spec else {
        return Ok(spec);
    };
    let mut options = Object::new();
    for (key, value) in spec.into_entries() {
        let value = match (op, key.to_string_lossy().as_str()) {
            (Op::Parse, "decoder") => named(value, "kinds", || Function::Decoder(Rc::new(kinds_decoder))),
            (Op::Parse, "delimiter") => delimiter_option(value)?,
            (Op::Stringify, "encoder") => named(value, "kinds", || Function::Encoder(Rc::new(kinds_encoder))),
            (Op::Stringify, "filter") => filter_option(value, calls.clone()),
            (Op::Stringify, "sort") => sort_option(value),
            (Op::Stringify, "serializeDate") => date_option(value),
            _ => value,
        };
        options.define(key, value);
    }
    Ok(Value::Object(options))
}

/// The function a string option value names, or the value as it is.
fn named(value: Value, name: &str, function: impl FnOnce() -> Function) -> Value {
    match &value {
        Value::String(text) if text.eq_str(name) => Value::Function(function()),
        _ => value,
    }
}

/// The decoder `"kinds"`: `KIND(DECODED)`, with the default decoder.
fn kinds_decoder(text: &JsString, charset: &Charset, kind: Kind) -> Result<Value, JsError> {
    let mut out = JsString::from(kind.as_str());
    out.push_str("(");
    out.push_js(&utils::decode(text, *charset));
    out.push_str(")");
    Ok(Value::String(out))
}

/// The encoder `"kinds"`: `KIND(ENCODED)`, with the default encoder.
fn kinds_encoder(value: &Value, charset: &Charset, kind: Kind, format: &Value) -> Result<Value, JsError> {
    let mut out = JsString::from(kind.as_str());
    out.push_str("(");
    out.push_js(&utils::encode(value, *charset, format).to_js_string());
    out.push_str(")");
    Ok(Value::String(out))
}

/// What the `"trace"` filter records about a value.
fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Array(_) => "array",
        Value::Date(_) => "date",
        other => other.type_of(),
    }
}

fn is_empty_string(value: &Value) -> bool {
    matches!(value, Value::String(text) if text.is_empty())
}

fn filter_option(value: Value, calls: Option<Rc<RefCell<Array>>>) -> Value {
    let Some(name) = value.as_string() else {
        return value;
    };
    let filter: Rc<crate::js::value::FilterFn> = if name.eq_str("trace") {
        let calls = calls.unwrap_or_default();
        Rc::new(move |prefix: &JsString, item: &Value| {
            let call = Array::from_values(vec![Value::String(prefix.clone()), Value::str(kind_of(item))]);
            calls.borrow_mut().push(Value::Array(call));
            Ok(item.clone())
        })
    } else if name.eq_str("dropEmpty") {
        Rc::new(|_: &JsString, item: &Value| {
            Ok(if is_empty_string(item) {
                Value::Undefined
            } else {
                item.clone()
            })
        })
    } else if name.eq_str("emptyToNull") {
        Rc::new(|_: &JsString, item: &Value| {
            Ok(if is_empty_string(item) {
                Value::Null
            } else {
                item.clone()
            })
        })
    } else {
        return value;
    };
    Value::Function(Function::Filter(filter))
}

/// `String(a)` compared with `String(b)` by UTF-16 code units.
fn compare_strings(a: &Value, b: &Value) -> f64 {
    match a.to_js_string().cmp(&b.to_js_string()) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
}

fn sort_option(value: Value) -> Value {
    let Some(name) = value.as_string() else {
        return value;
    };
    let sort: Rc<crate::js::value::SortFn> = if name.eq_str("asc") {
        Rc::new(compare_strings)
    } else if name.eq_str("desc") {
        Rc::new(|a: &Value, b: &Value| compare_strings(b, a))
    } else if name.eq_str("length") {
        Rc::new(|a: &Value, b: &Value| a.to_js_string().len() as f64 - b.to_js_string().len() as f64)
    } else {
        return value;
    };
    Value::Function(Function::Sort(sort))
}

fn date_option(value: Value) -> Value {
    let Some(name) = value.as_string() else {
        return value;
    };
    let serialize: Rc<crate::js::value::DateFn> = if name.eq_str("epoch") {
        Rc::new(|time: f64| Ok(Value::Number(time)))
    } else if name.eq_str("utc") {
        Rc::new(|time: f64| Ok(Value::str(&to_utc_string(time))))
    } else {
        return value;
    };
    Value::Function(Function::SerializeDate(serialize))
}

/// `{"regexp": SOURCE}`: a character class of literal code units, none of `\`, `[`, `]`, `^` or `-`.
fn delimiter_option(value: Value) -> Result<Value, JsError> {
    let Some(source) = value.as_object().and_then(|object| object.get_str("regexp")) else {
        return Ok(value);
    };
    let units = source.as_string().map(JsString::units).unwrap_or_default();
    let forbidden = |unit: &u16| b"\\[]^-".iter().any(|&c| u16::from(c) == *unit);
    let valid = units.len() >= 3
        && units[0] == u16::from(b'[')
        && units[units.len() - 1] == u16::from(b']')
        && !units[1..units.len() - 1].iter().any(forbidden);
    if !valid {
        return Err(request_error("regexp must be a character class of literal characters"));
    }
    Ok(Value::RegExp(JsString::from_units(units[1..units.len() - 1].to_vec())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(lines: &str) -> (String, u8) {
        let (output, status) = run(lines.as_bytes());
        (String::from_utf8_lossy(&output).into_owned(), status)
    }

    #[test]
    fn answers_requests() {
        let (out, status) =
            answer("{\"op\":\"parse\",\"input\":\"a[b]=c\"}\n\n{\"op\":\"stringify\",\"value\":{\"a\":[1,2]}}\n");
        assert_eq!(
            out,
            "{\"result\":{\"a\":{\"b\":\"c\"}}}\n{\"result\":\"a%5B0%5D=1&a%5B1%5D=2\"}\n"
        );
        assert_eq!(status, 0);
    }

    #[test]
    fn reports_errors() {
        let (out, status) = answer("[]\n{\"op\":1}\n{\"op\":\"stringify\",\"value\":{\"d\":{\"$date\":\"x\"}}}\n");
        assert_eq!(
            out,
            "{\"error\":{\"name\":\"RequestError\",\"message\":\"a request must be a JSON object\"}}\n\
             {\"error\":{\"name\":\"RequestError\",\"message\":\"unknown op: 1\"}}\n\
             {\"error\":{\"name\":\"RequestError\",\"message\":\"$date must be a number\"}}\n"
        );
        assert_eq!(status, 1);
    }

    #[test]
    fn traces_filter_calls() {
        let (out, _) = answer("{\"op\":\"stringify\",\"value\":{\"a\":[\"x\"]},\"options\":{\"filter\":\"trace\"}}\n");
        assert_eq!(
            out,
            "{\"result\":\"a%5B0%5D=x\",\"calls\":[[\"\",\"object\"],[\"a\",\"array\"],[\"a[0]\",\"string\"]]}\n"
        );
    }
}
