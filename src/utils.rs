// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2014, Nathan LaFreniere and other contributors (https://github.com/ljharb/qs/graphs/contributors)

//! Shared helpers: merging parsed keys into one result with the array limit, the overflow objects and the merge
//! conflicts the library tracks; compacting sparse arrays; and the default decoder and encoder.

use crate::formats::{Charset, Format};
use crate::js::number::{number_to_string, parse_int};
use crate::js::value::is_object_prototype_key;
use crate::js::{Array, JsError, JsString, Object, Value};

/// The options `merge` and `combine` consult.
#[derive(Clone, Copy, Debug)]
pub struct MergeOptions {
    /// `arrayLimit`.
    pub array_limit: f64,
    /// `plainObjects`: containers built here have no prototype.
    pub plain_objects: bool,
    /// `allowPrototypes`.
    pub allow_prototypes: bool,
    /// `strictMerge`: a primitive merged into an object makes a two-element conflict container.
    pub strict_merge: bool,
    /// `throwOnLimitExceeded`.
    pub throw_on_limit_exceeded: bool,
}

/// The `RangeError` for an array that would exceed `arrayLimit`.
pub fn array_limit_error(limit: f64) -> JsError {
    JsError::new(
        "RangeError",
        format!(
            "Array limit exceeded. Only {} element{} allowed in an array.",
            number_to_string(limit),
            if limit == 1.0 { "" } else { "s" }
        ),
    )
}

/// The property key of a number, as `object[number]` converts it.
pub fn number_key(number: f64) -> JsString {
    JsString::from(number_to_string(number))
}

/// `parseInt(key, 10)` when `String` of the result gives the key back, that is when the key is a canonical
/// integer.
fn canonical_int(key: &JsString) -> Option<f64> {
    let number = parse_int(key.units());
    (!number.is_nan() && number_key(number) == *key).then_some(number)
}

/// `arrayToObject`: the elements that are not holes and not `undefined`, keyed by index.
pub fn array_to_object(source: Array, plain_objects: bool) -> Object {
    let mut object = Object::with_proto(plain_objects);
    for (index, value) in source.into_entries() {
        if !value.is_undefined() {
            object.define(number_key(f64::from(index)), value);
        }
    }
    object
}

/// `[].concat(a, b)`: arrays are spread (keeping their holes), any other value becomes one element.
fn concat(parts: Vec<Value>) -> Array {
    let mut out = Array::new();
    for part in parts {
        match part {
            Value::Array(array) => {
                let base = out.len();
                let length = array.len();
                for (index, value) in array.into_entries() {
                    out.set_index(base + u64::from(index), value);
                }
                out.set_length(base + length);
            }
            other => out.push(other),
        }
    }
    out
}

/// An overflow object: the array's elements keyed by index, marked with its highest index.
fn overflow_object(array: Array, max_index: f64, plain_objects: bool) -> Value {
    let mut object = array_to_object(array, plain_objects);
    object.overflow = Some(max_index);
    Value::Object(object)
}

/// `limitedPair`: `[first, second]`, or an overflow object when the limit is below 2.
fn limited_pair(first: Value, second: Value, options: &MergeOptions) -> Result<Value, JsError> {
    let pair = Array::from_values(vec![first, second]);
    if 2.0 > options.array_limit {
        if options.throw_on_limit_exceeded {
            return Err(array_limit_error(options.array_limit));
        }
        return Ok(overflow_object(pair, 1.0, options.plain_objects));
    }
    Ok(Value::Array(pair))
}

/// `maxNumericIndex`: the highest canonical integer key of the object, at least `min`.
fn max_numeric_index(object: &Object, min: f64) -> f64 {
    object
        .keys()
        .iter()
        .filter_map(canonical_int)
        .fold(min, |max, number| if number > max { number } else { max })
}

/// Merges `source` into `target` the way the library assembles one parsed key into the result so far.
///
/// Recursion is bounded by [`crate::js::json::MAX_NESTING`]: request values nest no deeper, and `parse` refuses
/// keys with more segments, so the stack cannot be exhausted.
pub fn merge(target: Value, source: Value, options: &MergeOptions) -> Result<Value, JsError> {
    if !source.truthy() {
        return Ok(target);
    }
    if !source.is_object_like() {
        return merge_primitive(target, source, options);
    }
    if !target.truthy() || !target.is_object_like() {
        return merge_into_primitive(target, source, options);
    }
    match (target, source) {
        (Value::Array(target), Value::Array(source)) => merge_arrays(target, source, options),
        (Value::Array(target), source) => {
            let conflict = target.conflict;
            let length = target.len();
            let mut object = array_to_object(target, options.plain_objects);
            if conflict.is_some() {
                // The array may have grown past its last conflict, so continue after its last index.
                object.conflict = Some(length as f64 - 1.0);
            }
            merge_keys(object, source, options)
        }
        (Value::Object(target), source) => merge_keys(target, source, options),
        (target, _) => Ok(target),
    }
}

/// `merge` with a primitive source.
fn merge_primitive(target: Value, source: Value, options: &MergeOptions) -> Result<Value, JsError> {
    match target {
        Value::Array(mut array) => {
            let next_index = array.len();
            if array.count() as f64 >= options.array_limit {
                if options.throw_on_limit_exceeded {
                    return Err(array_limit_error(options.array_limit));
                }
                let combined = concat(vec![Value::Array(array), source]);
                return Ok(overflow_object(combined, next_index as f64, options.plain_objects));
            }
            array.push(source);
            Ok(Value::Array(array))
        }
        Value::Object(mut object) => {
            if let Some(max_index) = object.overflow {
                let new_index = max_index + 1.0;
                object.assign(number_key(new_index), source);
                object.overflow = Some(new_index);
            } else if options.strict_merge && object.conflict.is_some() {
                let next = object.conflict.unwrap_or(0.0) + 1.0;
                if options.throw_on_limit_exceeded && next >= options.array_limit {
                    return Err(array_limit_error(options.array_limit));
                }
                object.assign(number_key(next), source);
                object.conflict = Some(next);
            } else if options.strict_merge {
                return match limited_pair(Value::Object(object), source, options)? {
                    Value::Array(mut pair) => {
                        pair.conflict = Some(1.0);
                        Ok(Value::Array(pair))
                    }
                    other => Ok(other),
                };
            } else {
                let key = source.to_js_string();
                if options.plain_objects || options.allow_prototypes || !is_object_prototype_key(&key) {
                    object.assign(key, Value::Bool(true));
                }
            }
            Ok(Value::Object(object))
        }
        target if target.is_object_like() => Ok(target),
        target => limited_pair(target, source, options),
    }
}

/// `merge` of a container into a missing or primitive target: the target becomes the first element.
fn merge_into_primitive(target: Value, source: Value, options: &MergeOptions) -> Result<Value, JsError> {
    if let Value::Object(source) = &source {
        if let Some(max_index) = source.overflow {
            let mut result = Object::with_proto(options.plain_objects);
            result.assign(JsString::from("0"), target);
            for (key, value) in source.iter() {
                let old_key = parse_int(key.units());
                result.assign(number_key(old_key + 1.0), value.clone());
            }
            result.overflow = Some(max_index + 1.0);
            return Ok(Value::Object(result));
        }
    }
    let combined = concat(vec![Value::Array(Array::from_values(vec![target])), source]);
    let length = combined.len() as f64;
    if length > options.array_limit {
        if options.throw_on_limit_exceeded {
            return Err(array_limit_error(options.array_limit));
        }
        return Ok(overflow_object(combined, length - 1.0, options.plain_objects));
    }
    Ok(Value::Array(combined))
}

/// `merge` of two arrays: elements at the same index merge when both are objects, otherwise the source element is
/// appended; then the array limit applies.
fn merge_arrays(mut target: Array, source: Array, options: &MergeOptions) -> Result<Value, JsError> {
    for (index, item) in source.into_entries() {
        if target.has(index) {
            let existing = target.take(index);
            let both_objects = existing.truthy() && is_object_type(&existing) && item.truthy() && is_object_type(&item);
            if both_objects {
                let merged = merge(existing, item, options)?;
                target.set(index, merged);
            } else {
                target.set(index, existing);
                target.push(item);
            }
        } else {
            target.set(index, item);
        }
    }
    if target.count() as f64 > options.array_limit {
        if options.throw_on_limit_exceeded {
            return Err(array_limit_error(options.array_limit));
        }
        let max_index = target.len() as f64 - 1.0;
        return Ok(overflow_object(target, max_index, options.plain_objects));
    }
    Ok(Value::Array(target))
}

/// Whether `typeof value === 'object'` for a non-null value.
fn is_object_type(value: &Value) -> bool {
    matches!(value, Value::Array(_) | Value::Object(_) | Value::Date(_))
}

/// The key-by-key part of `merge`: every own key of the source is merged into, or added to, the accumulator, which
/// keeps the source's overflow and conflict marks up to date.
fn merge_keys(mut acc: Object, source: Value, options: &MergeOptions) -> Result<Value, JsError> {
    let (source_overflow, entries) = match source {
        Value::Object(object) => (object.overflow, object.into_entries()),
        Value::Array(array) => (None, array.into_keyed_entries()),
        _ => (None, Vec::new()),
    };
    for (key, value) in entries {
        if acc.has_own(&key) {
            let existing = acc.take(&key);
            let merged = merge(existing, value, options)?;
            acc.define(key.clone(), merged);
        } else {
            acc.define(key.clone(), value);
        }
        if let (Some(source_max), None) = (source_overflow, acc.overflow) {
            acc.overflow = Some(max_numeric_index(&acc, source_max));
        }
        if let Some(max_index) = acc.overflow {
            if let Some(number) = canonical_int(&key) {
                if number >= 0.0 && number > max_index {
                    acc.overflow = Some(number);
                }
            }
        }
        if let Some(conflict) = acc.conflict {
            if let Some(number) = canonical_int(&key) {
                if number > conflict {
                    acc.conflict = Some(number);
                }
            }
        }
    }
    Ok(Value::Object(acc))
}

/// `combine(a, b)`: `[].concat(a, b)` within the array limit; past it an overflow object, and an overflow object
/// `a` takes the values of `b` at its next indices.
pub fn combine(a: Value, b: Value, options: &MergeOptions) -> Result<Value, JsError> {
    if let Value::Object(mut object) = a {
        if let Some(max_index) = object.overflow {
            if options.throw_on_limit_exceeded {
                return Err(array_limit_error(options.array_limit));
            }
            let values: Vec<Value> = match b {
                Value::Array(array) => array.slots().map(|slot| slot.cloned().unwrap_or_default()).collect(),
                other => vec![other],
            };
            let mut new_index = max_index;
            for value in values {
                new_index += 1.0;
                object.assign(number_key(new_index), value);
            }
            object.overflow = Some(new_index);
            return Ok(Value::Object(object));
        }
        return combine_concat(Value::Object(object), b, options);
    }
    combine_concat(a, b, options)
}

fn combine_concat(a: Value, b: Value, options: &MergeOptions) -> Result<Value, JsError> {
    let result = concat(vec![a, b]);
    let length = result.len() as f64;
    if length > options.array_limit {
        if options.throw_on_limit_exceeded {
            return Err(array_limit_error(options.array_limit));
        }
        return Ok(overflow_object(result, length - 1.0, options.plain_objects));
    }
    Ok(Value::Array(result))
}

/// `compact`: removes the holes and `undefined` elements of every array in the value, at any depth.
///
/// Recursion is bounded by [`crate::js::json::MAX_NESTING`]: request values nest no deeper, and `parse` refuses
/// keys with more segments, so the stack cannot be exhausted.
pub fn compact(value: Value) -> Value {
    match value {
        Value::Array(array) => {
            let conflict = array.conflict;
            let mut compacted = Array::new();
            for slot in array.into_slots() {
                match slot {
                    None | Some(Value::Undefined) => {}
                    Some(item) => compacted.push(compact(item)),
                }
            }
            compacted.conflict = conflict;
            Value::Array(compacted)
        }
        Value::Object(mut object) => {
            for key in object.keys() {
                let item = object.take(&key);
                object.define(key, compact(item));
            }
            Value::Object(object)
        }
        other => other,
    }
}

/// `maybeMap`: applies `f` to every element of an array (holes read as `undefined`) or to a single value.
pub fn maybe_map<F>(value: Value, mut f: F) -> Result<Value, JsError>
where
    F: FnMut(Value) -> Result<Value, JsError>,
{
    match value {
        Value::Array(array) => {
            let mut mapped = Array::new();
            for slot in array.into_slots() {
                mapped.push(f(slot.unwrap_or_default())?);
            }
            Ok(Value::Array(mapped))
        }
        other => f(other),
    }
}

/// The default decoder: `+` becomes a space, then percent escapes are decoded, as UTF-8 with
/// `decodeURIComponent` (the text is kept, pluses replaced, when it is not valid) or as ISO-8859-1 octets.
pub fn decode(text: &JsString, charset: Charset) -> JsString {
    let plus = u16::from(b'+');
    let without_plus: Vec<u16> = text
        .units()
        .iter()
        .map(|&unit| if unit == plus { u16::from(b' ') } else { unit })
        .collect();
    if !without_plus.contains(&u16::from(b'%')) {
        return JsString::from_units(without_plus);
    }
    match charset {
        Charset::Iso88591 => JsString::from_units(unescape_octets(&without_plus)),
        Charset::Utf8 => decode_uri_component(&without_plus)
            .map(JsString::from_units)
            .unwrap_or_else(|| JsString::from_units(without_plus)),
    }
}

fn hex_value(unit: u16) -> Option<u16> {
    char::from_u32(u32::from(unit))
        .and_then(|c| c.to_digit(16))
        .map(|d| d as u16)
}

/// `str.replace(/%[0-9a-f]{2}/gi, unescape)`: every `%XX` becomes the code unit XX.
fn unescape_octets(units: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(units.len());
    let mut i = 0;
    while i < units.len() {
        if units[i] == u16::from(b'%') && i + 2 < units.len() {
            if let (Some(high), Some(low)) = (hex_value(units[i + 1]), hex_value(units[i + 2])) {
                out.push(high << 4 | low);
                i += 3;
                continue;
            }
        }
        out.push(units[i]);
        i += 1;
    }
    out
}

/// `decodeURIComponent`; `None` where it throws a `URIError`.
fn decode_uri_component(units: &[u16]) -> Option<Vec<u16>> {
    let mut out = Vec::with_capacity(units.len());
    let mut i = 0;
    let octet = |at: usize| -> Option<u8> {
        if units.get(at) != Some(&u16::from(b'%')) {
            return None;
        }
        let high = hex_value(*units.get(at + 1)?)?;
        let low = hex_value(*units.get(at + 2)?)?;
        Some((high << 4 | low) as u8)
    };
    while i < units.len() {
        if units[i] != u16::from(b'%') {
            out.push(units[i]);
            i += 1;
            continue;
        }
        let first = octet(i)?;
        i += 3;
        if first < 0x80 {
            out.push(u16::from(first));
            continue;
        }
        let count = first.leading_ones() as usize;
        if !(2..=4).contains(&count) {
            return None;
        }
        let mut bytes = vec![first];
        for _ in 1..count {
            let next = octet(i)?;
            if next & 0xC0 != 0x80 {
                return None;
            }
            bytes.push(next);
            i += 3;
        }
        let text = std::str::from_utf8(&bytes).ok()?;
        out.extend(text.encode_utf16());
    }
    Some(out)
}

/// `escape(string)`: letters, digits and `@*_+-./` stay, other code units below 256 become `%XX` and the rest
/// `%uXXXX`.
fn escape(units: &[u16]) -> String {
    let mut out = String::with_capacity(units.len());
    for &unit in units {
        let literal = unit < 0x80 && {
            let c = unit as u8;
            c.is_ascii_alphanumeric() || b"@*_+-./".contains(&c)
        };
        if literal {
            out.push(char::from(unit as u8));
        } else if unit < 0x100 {
            out.push_str(&format!("%{unit:02X}"));
        } else {
            out.push_str(&format!("%u{unit:04X}"));
        }
    }
    out
}

/// The default encoder. An argument whose `length` is 0 is returned as it is; any other value is converted with
/// `String` and percent-encoded: as UTF-8 octets (an unpaired surrogate as U+FFFD), or for ISO-8859-1 with
/// `escape`, characters beyond it written as numeric entities. `format` is the option value as given: only the
/// exact format names change which characters stay literal.
pub fn encode(value: &Value, charset: Charset, format: &Value) -> Value {
    if value.get_property(&JsString::from("length")).as_number() == Some(0.0) {
        return value.clone();
    }
    let string = value.to_js_string();
    let format = format.as_string().and_then(Format::from_name);
    match charset {
        Charset::Iso88591 => Value::String(JsString::from(encode_iso(string.units()))),
        Charset::Utf8 => Value::String(JsString::from(encode_utf8(string.units(), format))),
    }
}

fn encode_iso(units: &[u16]) -> String {
    let escaped = escape(units);
    let mut out = String::with_capacity(escaped.len());
    let bytes = escaped.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && bytes.get(i + 1) == Some(&b'u') && i + 6 <= bytes.len() {
            if let Ok(code) = u32::from_str_radix(&escaped[i + 2..i + 6], 16) {
                out.push_str(&format!("%26%23{code}%3B"));
                i += 6;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push_str("%2B");
        } else {
            out.push(char::from(bytes[i]));
        }
        i += 1;
    }
    out
}

fn encode_utf8(units: &[u16], format: Option<Format>) -> String {
    let mut out = String::with_capacity(units.len());
    let mut i = 0;
    while i < units.len() {
        let c = units[i];
        let literal = c == 0x2D
            || c == 0x2E
            || c == 0x5F
            || (c == 0x7E && format != Some(Format::Whatwg))
            || (0x30..=0x39).contains(&c)
            || (0x41..=0x5A).contains(&c)
            || (0x61..=0x7A).contains(&c)
            || (format == Some(Format::Rfc1738) && (c == 0x28 || c == 0x29))
            || (format == Some(Format::Whatwg) && c == 0x2A);
        if literal {
            out.push(char::from(c as u8));
            i += 1;
            continue;
        }
        let code_point = match c {
            0xD800..=0xDBFF => match units.get(i + 1) {
                Some(&next @ 0xDC00..=0xDFFF) => {
                    i += 1;
                    0x10000 + ((u32::from(c) & 0x3FF) << 10 | (u32::from(next) & 0x3FF))
                }
                _ => 0xFFFD,
            },
            0xDC00..=0xDFFF => 0xFFFD,
            _ => u32::from(c),
        };
        let mut buffer = [0u8; 4];
        let encoded = char::from_u32(code_point)
            .unwrap_or('\u{FFFD}')
            .encode_utf8(&mut buffer);
        for byte in encoded.bytes() {
            out.push_str(&format!("%{byte:02X}"));
        }
        i += 1;
    }
    out
}

/// The `&#NNN;` numeric entities of ISO-8859-1 form submissions decoded: a code point above U+FFFF becomes a
/// surrogate pair, any other number the code unit `String.fromCharCode` makes of it.
pub fn interpret_numeric_entities(text: &JsString) -> JsString {
    let units = text.units();
    let mut out = Vec::with_capacity(units.len());
    let mut i = 0;
    while i < units.len() {
        if units[i] == u16::from(b'&') && units.get(i + 1) == Some(&u16::from(b'#')) {
            let digits = units[i + 2..]
                .iter()
                .take_while(|&&u| (0x30..=0x39).contains(&u))
                .count();
            if digits > 0 && units.get(i + 2 + digits) == Some(&u16::from(b';')) {
                let code = parse_int(&units[i + 2..i + 2 + digits]);
                if code > 65535.0 && code <= 1_114_111.0 {
                    let c = code as u32 - 0x10000;
                    out.push((0xD800 | (c >> 10)) as u16);
                    out.push((0xDC00 | (c & 0x3FF)) as u16);
                } else {
                    out.push(crate::js::number::to_uint16(code));
                }
                i += digits + 3;
                continue;
            }
        }
        out.push(units[i]);
        i += 1;
    }
    JsString::from_units(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> JsString {
        JsString::from(text)
    }

    #[test]
    fn decodes_like_the_library() {
        assert_eq!(decode(&s("a+b%20c"), Charset::Utf8), s("a b c"));
        assert_eq!(decode(&s("%E2%9C%93"), Charset::Utf8), s("\u{2713}"));
        assert_eq!(decode(&s("%ED%A0%80+x"), Charset::Utf8), s("%ED%A0%80 x"));
        assert_eq!(decode(&s("%E9%u00e9"), Charset::Iso88591), s("\u{e9}%u00e9"));
    }

    #[test]
    fn encodes_like_the_library() {
        let rfc3986 = Value::str("RFC3986");
        let encoded = encode(&Value::str("a b~(*)\u{e9}\u{1F600}"), Charset::Utf8, &rfc3986);
        assert_eq!(encoded.as_string(), Some(&s("a%20b~%28%2A%29%C3%A9%F0%9F%98%80")));
        let lone = Value::String(JsString::from_units(vec![0xD800, 0x41]));
        assert_eq!(
            encode(&lone, Charset::Utf8, &rfc3986).as_string(),
            Some(&s("%EF%BF%BDA"))
        );
        let iso = encode(&Value::str("\u{e9}+\u{263a}"), Charset::Iso88591, &rfc3986);
        assert_eq!(iso.as_string(), Some(&s("%E9%2B%26%239786%3B")));
    }

    #[test]
    fn interprets_entities() {
        assert_eq!(
            interpret_numeric_entities(&s("&#9786;&#128512;&#65;x")),
            s("\u{263a}\u{1F600}Ax")
        );
        assert_eq!(
            interpret_numeric_entities(&s("&#99999999999999999999;")),
            JsString::from_units(vec![0])
        );
    }
}
