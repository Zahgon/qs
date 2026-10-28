// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2014, Nathan LaFreniere and other contributors (https://github.com/ljharb/qs/graphs/contributors)

//! `parse`: a query string (or an object of already split keys) into nested objects and arrays.

use std::rc::Rc;

use crate::formats::{Charset, Kind};
use crate::js::number::{number_to_string, parse_int, string_to_number, to_uint32};
use crate::js::string::index_of;
use crate::js::value::{is_object_prototype_key, DecoderFn};
use crate::js::{json, Array, Function, JsError, JsString, Object, Value};
use crate::utils::{self, array_limit_error, MergeOptions};

/// The delimiter that separates parameters: a string, or a regular expression of one character class, which
/// splits at every code unit of the class.
#[derive(Clone, Debug)]
pub enum Delimiter {
    /// A string delimiter; the empty string splits between code units.
    Str(JsString),
    /// The code units of a character class.
    Class(Vec<u16>),
}

/// What happens to a key given more than once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Duplicates {
    /// The values are combined into an array.
    Combine,
    /// The first value is kept.
    First,
    /// The last value is kept.
    Last,
}

/// The decoder of keys and values.
#[derive(Clone)]
pub enum Decoder {
    /// The library's default decoder, [`utils::decode`].
    Default,
    /// A decoder function from the options.
    Custom(Rc<DecoderFn>),
}

impl std::fmt::Debug for Decoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Decoder::Default => f.write_str("Decoder::Default"),
            Decoder::Custom(_) => f.write_str("Decoder::Custom"),
        }
    }
}

impl Decoder {
    fn decode(&self, text: &JsString, charset: Charset, kind: Kind) -> Result<Value, JsError> {
        match self {
            Decoder::Default => Ok(Value::String(utils::decode(text, charset))),
            Decoder::Custom(decoder) => decoder(text, &charset, kind),
        }
    }
}

/// The normalized options of [`parse()`].
#[derive(Clone, Debug)]
pub struct ParseOptions {
    /// `allowDots`: `a.b` is read as `a[b]`.
    pub allow_dots: bool,
    /// `allowEmptyArrays`: `a[]` with an empty value is an empty array.
    pub allow_empty_arrays: bool,
    /// `allowPrototypes`: keys that `Object.prototype` owns are kept.
    pub allow_prototypes: bool,
    /// `allowSparse`: arrays keep their holes.
    pub allow_sparse: bool,
    /// `arrayLimit`.
    pub array_limit: f64,
    /// `charset`.
    pub charset: Charset,
    /// `charsetSentinel`: a `utf8=` parameter selects the charset.
    pub charset_sentinel: bool,
    /// `comma`: comma-separated values are arrays.
    pub comma: bool,
    /// `decodeDotInKeys`: `%2E` in a key segment is a dot.
    pub decode_dot_in_keys: bool,
    /// `decoder`.
    pub decoder: Decoder,
    /// `delimiter`.
    pub delimiter: Delimiter,
    /// `depth`.
    pub depth: f64,
    /// `duplicates`.
    pub duplicates: Duplicates,
    /// `ignoreQueryPrefix`.
    pub ignore_query_prefix: bool,
    /// `interpretNumericEntities`.
    pub interpret_numeric_entities: bool,
    /// `parameterLimit`.
    pub parameter_limit: f64,
    /// `parseArrays`.
    pub parse_arrays: bool,
    /// `plainObjects`: objects have no prototype.
    pub plain_objects: bool,
    /// `strictDepth`: keys deeper than `depth` are an error.
    pub strict_depth: bool,
    /// `strictMerge`.
    pub strict_merge: bool,
    /// `strictNullHandling`: a key without `=` is `null`.
    pub strict_null_handling: bool,
    /// `throwOnLimitExceeded`.
    pub throw_on_limit_exceeded: bool,
}

impl Default for ParseOptions {
    fn default() -> Self {
        ParseOptions {
            allow_dots: false,
            allow_empty_arrays: false,
            allow_prototypes: false,
            allow_sparse: false,
            array_limit: 20.0,
            charset: Charset::Utf8,
            charset_sentinel: false,
            comma: false,
            decode_dot_in_keys: false,
            decoder: Decoder::Default,
            delimiter: Delimiter::Str(JsString::from("&")),
            depth: 5.0,
            duplicates: Duplicates::Combine,
            ignore_query_prefix: false,
            interpret_numeric_entities: false,
            parameter_limit: 1000.0,
            parse_arrays: true,
            plain_objects: false,
            strict_depth: false,
            strict_merge: true,
            strict_null_handling: false,
            throw_on_limit_exceeded: false,
        }
    }
}

impl ParseOptions {
    fn merge_options(&self) -> MergeOptions {
        MergeOptions {
            array_limit: self.array_limit,
            plain_objects: self.plain_objects,
            allow_prototypes: self.allow_prototypes,
            strict_merge: self.strict_merge,
            throw_on_limit_exceeded: self.throw_on_limit_exceeded,
        }
    }

    fn new_object(&self) -> Object {
        Object::with_proto(self.plain_objects)
    }

    /// `normalizeParseOptions`: the options argument as the library reads it, with its type checks. Any value may
    /// be given; a falsy one means the defaults.
    pub fn normalize(opts: &Value) -> Result<ParseOptions, JsError> {
        let defaults = ParseOptions::default();
        if !opts.truthy() {
            return Ok(defaults);
        }
        let get = |name: &str| opts.get_property(&JsString::from(name));
        let boolean = |name: &str, default: bool| get(name).as_bool().unwrap_or(default);
        let number = |name: &str, default: f64| get(name).as_number().unwrap_or(default);

        let allow_empty_arrays = get("allowEmptyArrays");
        if !allow_empty_arrays.is_undefined() && allow_empty_arrays.as_bool().is_none() {
            return Err(JsError::type_error(
                "`allowEmptyArrays` option can only be `true` or `false`, when provided",
            ));
        }
        let decode_dot_in_keys = get("decodeDotInKeys");
        if !decode_dot_in_keys.is_undefined() && decode_dot_in_keys.as_bool().is_none() {
            return Err(JsError::type_error(
                "`decodeDotInKeys` option can only be `true` or `false`, when provided",
            ));
        }
        let decoder = match get("decoder") {
            Value::Undefined | Value::Null => Decoder::Default,
            Value::Function(Function::Decoder(decoder)) => Decoder::Custom(decoder),
            _ => return Err(JsError::type_error("Decoder has to be a function.")),
        };
        let charset = match get("charset") {
            Value::Undefined => defaults.charset,
            value => value.as_string().and_then(Charset::from_name).ok_or_else(|| {
                JsError::type_error("The charset option must be either utf-8, iso-8859-1, or undefined")
            })?,
        };
        let throw_on_limit_exceeded = get("throwOnLimitExceeded");
        if !throw_on_limit_exceeded.is_undefined() && throw_on_limit_exceeded.as_bool().is_none() {
            return Err(JsError::type_error("`throwOnLimitExceeded` option must be a boolean"));
        }
        let duplicates = match get("duplicates") {
            Value::Undefined => Some(Duplicates::Combine),
            Value::String(name) if name.eq_str("combine") => Some(Duplicates::Combine),
            Value::String(name) if name.eq_str("first") => Some(Duplicates::First),
            Value::String(name) if name.eq_str("last") => Some(Duplicates::Last),
            _ => None,
        }
        .ok_or_else(|| JsError::type_error("The duplicates option must be either combine, first, or last"))?;

        let allow_dots = match get("allowDots") {
            Value::Undefined => decode_dot_in_keys.is_true() || defaults.allow_dots,
            value => value.truthy(),
        };
        let delimiter = match get("delimiter") {
            Value::String(text) => Delimiter::Str(text),
            Value::RegExp(class) => Delimiter::Class(class.units().to_vec()),
            _ => defaults.delimiter,
        };
        let depth = match get("depth") {
            Value::Number(n) => n,
            Value::Bool(false) => 0.0,
            _ => defaults.depth,
        };
        Ok(ParseOptions {
            allow_dots,
            allow_empty_arrays: allow_empty_arrays.as_bool().unwrap_or(defaults.allow_empty_arrays),
            allow_prototypes: boolean("allowPrototypes", defaults.allow_prototypes),
            allow_sparse: boolean("allowSparse", defaults.allow_sparse),
            array_limit: number("arrayLimit", defaults.array_limit),
            charset,
            charset_sentinel: boolean("charsetSentinel", defaults.charset_sentinel),
            comma: boolean("comma", defaults.comma),
            decode_dot_in_keys: decode_dot_in_keys.as_bool().unwrap_or(defaults.decode_dot_in_keys),
            decoder,
            delimiter,
            depth,
            duplicates,
            ignore_query_prefix: get("ignoreQueryPrefix").is_true(),
            interpret_numeric_entities: boolean("interpretNumericEntities", defaults.interpret_numeric_entities),
            parameter_limit: number("parameterLimit", defaults.parameter_limit),
            parse_arrays: !matches!(get("parseArrays"), Value::Bool(false)),
            plain_objects: boolean("plainObjects", defaults.plain_objects),
            strict_depth: boolean("strictDepth", defaults.strict_depth),
            strict_merge: boolean("strictMerge", defaults.strict_merge),
            strict_null_handling: boolean("strictNullHandling", defaults.strict_null_handling),
            throw_on_limit_exceeded: throw_on_limit_exceeded.as_bool().unwrap_or(false),
        })
    }
}

/// The parameter that says the form was submitted as ISO-8859-1: `encodeURIComponent('&#10003;')`.
const ISO_SENTINEL: &str = "utf8=%26%2310003%3B";
/// The parameter that says the form was submitted as UTF-8: `encodeURIComponent('✓')`.
const CHARSET_SENTINEL: &str = "utf8=%E2%9C%93";

/// Parses `input` with the options argument `opts`: a string is a query string, an object (or any other value)
/// supplies keys and values that are already split.
pub fn parse(input: &Value, opts: &Value) -> Result<Value, JsError> {
    let options = ParseOptions::normalize(opts)?;
    parse_with(input, &options)
}

/// [`parse()`] with options that are already normalized.
pub fn parse_with(input: &Value, options: &ParseOptions) -> Result<Value, JsError> {
    let is_string = matches!(input, Value::String(_));
    match input {
        Value::Undefined | Value::Null => return Ok(Value::Object(options.new_object())),
        Value::String(text) if text.is_empty() => return Ok(Value::Object(options.new_object())),
        _ => {}
    }
    let temp = match input {
        Value::String(text) => parse_values(text, options)?,
        other => copy_input(other),
    };
    let merge_options = options.merge_options();
    let mut result = Value::Object(options.new_object());
    for (key, value) in temp.into_entries() {
        let parsed = parse_keys(&key, value, options, is_string)?;
        result = utils::merge(result, parsed, &merge_options)?;
    }
    if options.allow_sparse {
        return Ok(result);
    }
    Ok(utils::compact(result))
}

/// `copyInput`: the own keys of an object (or array) input as one ordinary object; other values have none.
fn copy_input(input: &Value) -> Object {
    let mut result = Object::new();
    match input {
        Value::Object(object) => {
            for (key, value) in object.iter() {
                result.define(key.clone(), value.clone());
            }
        }
        Value::Array(array) => {
            for (key, value) in array.clone().into_keyed_entries() {
                result.define(key, value);
            }
        }
        _ => {}
    }
    result
}

/// `String.prototype.split(separator, limit)` for the delimiter: at most `limit` pieces.
fn split(text: &[u16], delimiter: &Delimiter, limit: u32) -> Vec<JsString> {
    let limit = limit as usize;
    let mut parts = Vec::new();
    if limit == 0 {
        return parts;
    }
    let matches_at = |i: usize| -> Option<usize> {
        match delimiter {
            Delimiter::Str(separator) => {
                let separator = separator.units();
                text[i..].starts_with(separator).then_some(separator.len())
            }
            Delimiter::Class(class) => class.contains(&text[i]).then_some(1),
        }
    };
    if let Delimiter::Str(separator) = delimiter {
        if separator.is_empty() {
            return text.iter().take(limit).map(|&unit| JsString::from_unit(unit)).collect();
        }
    }
    if text.is_empty() {
        return vec![JsString::new()];
    }
    let mut start = 0;
    let mut i = 0;
    while i < text.len() {
        match matches_at(i) {
            Some(width) => {
                parts.push(JsString::from_units(text[start..i].to_vec()));
                if parts.len() == limit {
                    return parts;
                }
                i += width;
                start = i;
            }
            None => i += 1,
        }
    }
    parts.push(JsString::from_units(text[start..].to_vec()));
    parts
}

/// `parseArrayValue`: with `comma`, a string containing a comma becomes the array of its pieces; the array limit
/// is checked against the pieces and against the array the key already has.
fn parse_array_value(value: Value, options: &ParseOptions, current_length: f64) -> Result<Value, JsError> {
    if let Value::String(text) = &value {
        let comma = u16::from(b',');
        if !text.is_empty() && options.comma && text.units().contains(&comma) {
            if options.throw_on_limit_exceeded {
                let commas = text.units().iter().filter(|&&u| u == comma).count();
                if commas as f64 >= options.array_limit {
                    return Err(array_limit_error(options.array_limit));
                }
            }
            let parts: Vec<Value> = text.split_unit(comma).into_iter().map(Value::String).collect();
            return Ok(Value::Array(Array::from_values(parts)));
        }
    }
    if options.throw_on_limit_exceeded && current_length >= options.array_limit && current_length > 0.0 {
        return Err(array_limit_error(options.array_limit));
    }
    Ok(value)
}

/// `parseValues`: splits the query string into keys and decoded values, combining repeated keys.
fn parse_values(text: &JsString, options: &ParseOptions) -> Result<Object, JsError> {
    let mut obj = Object::with_proto(true);
    let mut clean = text.units().to_vec();
    if options.ignore_query_prefix && clean.first() == Some(&u16::from(b'?')) {
        clean.remove(0);
    }
    let clean = replace_bracket_escapes(&clean);

    let limit = (options.parameter_limit != f64::INFINITY).then_some(options.parameter_limit);
    let split_limit = match limit {
        Some(limit) if options.throw_on_limit_exceeded => to_uint32(limit + 1.0),
        Some(limit) => to_uint32(limit),
        None => u32::MAX,
    };
    let parts = split(&clean, &options.delimiter, split_limit);
    if let Some(limit) = limit {
        if options.throw_on_limit_exceeded && parts.len() as f64 > limit {
            return Err(JsError::new(
                "RangeError",
                format!(
                    "Parameter limit exceeded. Only {} parameter{} allowed.",
                    number_to_string(limit),
                    if limit == 1.0 { "" } else { "s" }
                ),
            ));
        }
    }

    let mut skip_index = None;
    let mut charset = options.charset;
    if options.charset_sentinel {
        if let Some(i) = parts.iter().position(|part| part.starts_with_str("utf8=")) {
            if parts[i].eq_str(CHARSET_SENTINEL) {
                charset = Charset::Utf8;
            } else if parts[i].eq_str(ISO_SENTINEL) {
                charset = Charset::Iso88591;
            }
            skip_index = Some(i);
        }
    }

    let merge_options = options.merge_options();
    for (i, part) in parts.iter().enumerate() {
        if Some(i) == skip_index {
            continue;
        }
        let units = part.units();
        let pos = match find(units, "]=") {
            Some(bracket_equals) => Some(bracket_equals + 1),
            None => find(units, "="),
        };
        let is_empty_bracket_key = pos.is_some_and(|pos| pos > 1 && units[pos - 2..pos] == [0x5B, 0x5D]);

        let (key, mut value) = match pos {
            None => {
                let key = options.decoder.decode(part, charset, Kind::Key)?;
                let value = if options.strict_null_handling {
                    Value::Null
                } else {
                    Value::String(JsString::new())
                };
                (key, value)
            }
            Some(pos) => {
                let key_text = JsString::from_units(units[..pos].to_vec());
                let key = options.decoder.decode(&key_text, charset, Kind::Key)?;
                let mut value = Value::Undefined;
                if !key.is_null() {
                    let property = key.to_js_string();
                    let current_length = match obj.get(&property) {
                        Some(Value::Array(array))
                            if options.duplicates == Duplicates::Combine || is_empty_bracket_key =>
                        {
                            array.len() as f64
                        }
                        _ => 0.0,
                    };
                    let raw = Value::String(JsString::from_units(units[pos + 1..].to_vec()));
                    let split = parse_array_value(raw, options, current_length)?;
                    value = utils::maybe_map(split, |item| match item {
                        Value::String(text) => options.decoder.decode(&text, charset, Kind::Value),
                        other => options.decoder.decode(&other.to_js_string(), charset, Kind::Value),
                    })?;
                }
                (key, value)
            }
        };

        if value.truthy() && options.interpret_numeric_entities && charset == Charset::Iso88591 {
            value = utils::maybe_map(value, |item| {
                Ok(if item.truthy() {
                    Value::String(utils::interpret_numeric_entities(&item.to_js_string()))
                } else {
                    item
                })
            })?;
        }

        if is_empty_bracket_key && value.is_array() {
            value = Value::Array(Array::from_values(vec![value]));
        }

        if options.comma {
            if let Value::Array(array) = &value {
                if array.len() as f64 > options.array_limit {
                    value = utils::combine(Value::Array(Array::new()), value, &merge_options)?;
                }
            }
        }

        if key.is_null() {
            continue;
        }
        let property = key.to_js_string();
        let existing = obj.has_own(&property);
        if existing && (options.duplicates == Duplicates::Combine || is_empty_bracket_key) {
            let previous = obj.take(&property);
            let combined = utils::combine(previous, value, &merge_options)?;
            obj.assign(property, combined);
        } else if !existing || options.duplicates == Duplicates::Last {
            obj.assign(property, value);
        }
    }
    Ok(obj)
}

/// `str.indexOf(needle)` on code units.
fn find(units: &[u16], needle: &str) -> Option<usize> {
    let needle: Vec<u16> = needle.encode_utf16().collect();
    index_of(units, &needle, 0)
}

/// `%5B` and `%5D` (any case) become `[` and `]` before the query string is split.
fn replace_bracket_escapes(units: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(units.len());
    let mut i = 0;
    while i < units.len() {
        if units[i] == u16::from(b'%') && i + 2 < units.len() && units[i + 1] == u16::from(b'5') {
            let replacement = match units[i + 2] {
                0x42 | 0x62 => Some(u16::from(b'[')),
                0x44 | 0x64 => Some(u16::from(b']')),
                _ => None,
            };
            if let Some(bracket) = replacement {
                out.push(bracket);
                i += 3;
                continue;
            }
        }
        out.push(units[i]);
        i += 1;
    }
    out
}

/// Removes one pair of wrapping brackets: `[a]` is `a`.
fn strip_wrapping_brackets(key: &[u16]) -> &[u16] {
    if key.len() >= 2 && key[0] == u16::from(b'[') && key[key.len() - 1] == u16::from(b']') {
        &key[1..key.len() - 1]
    } else if key.len() == 1 && key[0] == u16::from(b'[') {
        // "[" both starts and ends with a bracket only when it is "[]"; a lone "[" is kept.
        key
    } else {
        key
    }
}

/// `topLevelDotsToBrackets`: `a.b` becomes `a[b]` for dots outside brackets that start a non-empty segment.
fn top_level_dots_to_brackets(key: &[u16]) -> Vec<u16> {
    let dot = u16::from(b'.');
    let open = u16::from(b'[');
    let close = u16::from(b']');
    let mut out = Vec::with_capacity(key.len() + 2);
    let mut depth = 0usize;
    let mut i = 0;
    while i < key.len() {
        let unit = key[i];
        if unit == open {
            depth += 1;
            out.push(open);
        } else if unit == close {
            depth = depth.saturating_sub(1);
            out.push(close);
        } else if unit == dot && depth == 0 && i + 1 < key.len() && key[i + 1] != dot && key[i + 1] != open {
            let start = i + 1;
            let mut j = start;
            while j < key.len() && key[j] != dot && key[j] != open {
                j += 1;
            }
            out.push(open);
            out.extend_from_slice(&key[start..j]);
            out.push(close);
            i = j;
            continue;
        } else {
            out.push(unit);
        }
        i += 1;
    }
    out
}

/// `splitKeyIntoSegments`: `a[b][c]` into `a`, `[b]`, `[c]`, at most `depth` bracket groups, the rest as one
/// segment; `None` when a segment names an `Object.prototype` property the options do not allow.
fn split_key_into_segments(original: &JsString, options: &ParseOptions) -> Result<Option<Vec<Vec<u16>>>, JsError> {
    let key = if options.allow_dots {
        top_level_dots_to_brackets(original.units())
    } else {
        original.units().to_vec()
    };
    let guarded = |segment: &[u16]| {
        !options.plain_objects
            && !options.allow_prototypes
            && is_object_prototype_key(&JsString::from_units(segment.to_vec()))
    };

    if options.depth <= 0.0 {
        if guarded(strip_wrapping_brackets(&key)) {
            return Ok(None);
        }
        return Ok(Some(vec![key]));
    }

    let open_unit = u16::from(b'[');
    let close_unit = u16::from(b']');
    let mut segments = Vec::new();
    let first = key.iter().position(|&u| u == open_unit);
    let parent = match first {
        Some(first) => &key[..first],
        None => &key[..],
    };
    if !parent.is_empty() {
        if guarded(parent) {
            return Ok(None);
        }
        segments.push(parent.to_vec());
    }

    let n = key.len();
    let mut open = first;
    let mut collected = 0.0;
    while let Some(start) = open {
        if collected >= options.depth {
            break;
        }
        let mut level = 1;
        let mut i = start + 1;
        let mut close = None;
        while i < n && close.is_none() {
            if key[i] == open_unit {
                level += 1;
            } else if key[i] == close_unit {
                level -= 1;
                if level == 0 {
                    close = Some(i);
                }
            }
            i += 1;
        }
        let Some(close) = close else {
            let mut segment = vec![open_unit];
            segment.extend_from_slice(&key[start..]);
            segment.push(close_unit);
            segments.push(segment);
            return Ok(Some(segments));
        };
        let segment = &key[start..=close];
        if guarded(&segment[1..segment.len() - 1]) {
            return Ok(None);
        }
        segments.push(segment.to_vec());
        collected += 1.0;
        open = key[close + 1..]
            .iter()
            .position(|&u| u == open_unit)
            .map(|offset| close + 1 + offset);
    }

    if let Some(start) = open {
        if options.strict_depth {
            return Err(JsError::new(
                "RangeError",
                format!(
                    "Input depth exceeded depth option of {} and strictDepth is true",
                    number_to_string(options.depth)
                ),
            ));
        }
        let mut segment = vec![open_unit];
        segment.extend_from_slice(&key[start..]);
        segment.push(close_unit);
        segments.push(segment);
    }
    Ok(Some(segments))
}

/// `parseKeys`: the nested value one key stands for, or `undefined` when the key is empty or not allowed.
fn parse_keys(key: &JsString, value: Value, options: &ParseOptions, values_parsed: bool) -> Result<Value, JsError> {
    if key.is_empty() {
        return Ok(Value::Undefined);
    }
    match split_key_into_segments(key, options)? {
        Some(chain) => parse_object(&chain, value, options, values_parsed),
        None => Ok(Value::Undefined),
    }
}

/// `String(parseInt(text, 10)) === text`: the integer a key segment denotes as an array index candidate.
fn segment_index(text: &[u16]) -> Option<f64> {
    let index = parse_int(text);
    (!index.is_nan() && JsString::from(number_to_string(index)).units() == text).then_some(index)
}

/// `parseObject`: builds the value for a key's segments from the innermost one out.
fn parse_object(
    chain: &[Vec<u16>],
    value: Value,
    options: &ParseOptions,
    values_parsed: bool,
) -> Result<Value, JsError> {
    if chain.len() > json::MAX_NESTING {
        // The original's merge and compact recurse once per segment and overflow the stack long before this.
        return Err(JsError::stack_overflow());
    }
    let merge_options = options.merge_options();
    let empty_brackets: &[u16] = &[0x5B, 0x5D];
    let mut leaf = if values_parsed {
        value
    } else {
        let mut current_length = 0.0;
        if chain.last().is_some_and(|last| last.as_slice() == empty_brackets) {
            let parent_key = JsString::from_units(chain[..chain.len() - 1].concat());
            if value.is_array() {
                let parent = value.get_property(&parent_key);
                if parent.truthy() {
                    current_length = to_number(&parent.get_property(&JsString::from("length")));
                }
            }
        }
        parse_array_value(value, options, current_length)?
    };

    for root in chain.iter().rev() {
        let obj = if root.as_slice() == empty_brackets && options.parse_arrays {
            if matches!(&leaf, Value::Object(object) if object.overflow.is_some()) {
                leaf
            } else if options.allow_empty_arrays
                && (matches!(&leaf, Value::String(s) if s.is_empty())
                    || (options.strict_null_handling && leaf.is_null()))
            {
                Value::Array(Array::new())
            } else {
                utils::combine(Value::Array(Array::new()), leaf, &merge_options)?
            }
        } else {
            let mut obj = options.new_object();
            let clean_root = strip_wrapping_brackets(root);
            let decoded_root = if options.decode_dot_in_keys {
                replace_encoded_dots(clean_root)
            } else {
                clean_root.to_vec()
            };
            let index = segment_index(&decoded_root)
                .filter(|&index| root.as_slice() != decoded_root.as_slice() && index >= 0.0 && options.parse_arrays);
            if !options.parse_arrays && decoded_root.is_empty() {
                obj.assign(JsString::from("0"), leaf);
                Value::Object(obj)
            } else if let Some(index) = index {
                if index < options.array_limit {
                    let mut array = Array::new();
                    match u32::try_from(index as u64) {
                        Ok(slot) if slot < u32::MAX => array.set(slot, leaf),
                        // Beyond the largest array index the key is an ordinary property of the array.
                        _ => array.set_named(utils::number_key(index), leaf),
                    }
                    Value::Array(array)
                } else if options.throw_on_limit_exceeded {
                    return Err(array_limit_error(options.array_limit));
                } else {
                    obj.assign(JsString::from(number_to_string(index)), leaf);
                    obj.overflow = Some(index);
                    Value::Object(obj)
                }
            } else {
                let decoded = JsString::from_units(decoded_root);
                if !decoded.eq_str("__proto__") {
                    obj.assign(decoded, leaf);
                }
                Value::Object(obj)
            }
        };
        leaf = obj;
    }
    Ok(leaf)
}

/// `str.replace(/%2E/gi, '.')`.
fn replace_encoded_dots(units: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(units.len());
    let mut i = 0;
    while i < units.len() {
        if units[i] == u16::from(b'%')
            && units.get(i + 1) == Some(&u16::from(b'2'))
            && matches!(units.get(i + 2), Some(0x45 | 0x65))
        {
            out.push(u16::from(b'.'));
            i += 3;
            continue;
        }
        out.push(units[i]);
        i += 1;
    }
    out
}

/// `ToNumber` for the values a request can hold, as a relational comparison applies it.
fn to_number(value: &Value) -> f64 {
    match value {
        Value::Undefined => f64::NAN,
        Value::Null => 0.0,
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Number(n) => *n,
        Value::String(s) => string_to_number(s.units()),
        Value::Date(time) => *time,
        other => string_to_number(other.to_js_string().units()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(query: &str, options: &str) -> String {
        let options = json::parse_str(options).expect("test options are JSON");
        match parse(&Value::str(query), &options) {
            Ok(value) => json::stringify(&value)
                .map(|s| s.to_string_lossy())
                .expect("a parsed object serializes"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn parses_nested_keys() {
        assert_eq!(run("a[b][c]=d&e=f", "{}"), r#"{"a":{"b":{"c":"d"}},"e":"f"}"#);
        assert_eq!(run("a[]=b&a[]=c", "{}"), r#"{"a":["b","c"]}"#);
        assert_eq!(run("a[1]=c&a[0]=b", "{}"), r#"{"a":["b","c"]}"#);
        assert_eq!(run("a.b=c", r#"{"allowDots":true}"#), r#"{"a":{"b":"c"}}"#);
    }

    #[test]
    fn applies_limits() {
        assert_eq!(run("a[21]=b", "{}"), r#"{"a":{"21":"b"}}"#);
        assert_eq!(run("a=1&b=2&c=3", r#"{"parameterLimit":2}"#), r#"{"a":"1","b":"2"}"#);
        assert_eq!(
            run("a=1&b=2", r#"{"parameterLimit":1,"throwOnLimitExceeded":true}"#),
            "RangeError: Parameter limit exceeded. Only 1 parameter allowed."
        );
        assert_eq!(
            run("a[b][c][d]=e", r#"{"depth":1,"strictDepth":true}"#),
            "RangeError: Input depth exceeded depth option of 1 and strictDepth is true"
        );
    }

    #[test]
    fn guards_prototype_keys() {
        assert_eq!(run("toString=x&a[hasOwnProperty]=y", "{}"), "{}");
        assert_eq!(run("a[b]=1&a=c", "{}"), r#"{"a":[{"b":"1"},"c"]}"#);
        assert_eq!(
            run("a[b]=1&a=c", r#"{"strictMerge":false}"#),
            r#"{"a":{"b":"1","c":true}}"#
        );
    }
}
