// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2014, Nathan LaFreniere and other contributors (https://github.com/ljharb/qs/graphs/contributors)

//! `stringify`: nested objects and arrays into a query string.

use std::cmp::Ordering;
use std::rc::Rc;

use crate::formats::{Charset, Format, Kind};
use crate::js::date;
use crate::js::number::number_to_string;
use crate::js::value::{join, DateFn, EncoderFn, FilterFn, SortFn};
use crate::js::{Array, Function, JsError, JsString, Value};
use crate::utils;

/// How array elements are keyed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrayFormat {
    /// `a[]=b&a[]=c`.
    Brackets,
    /// `a=b,c`.
    Comma,
    /// `a[0]=b&a[1]=c`, the default.
    Indices,
    /// `a=b&a=c`.
    Repeat,
}

impl ArrayFormat {
    /// The format named by an option value's property key, as `arrayPrefixGenerators[name]` finds it.
    fn from_name(name: &JsString) -> Option<ArrayFormat> {
        [
            ("brackets", ArrayFormat::Brackets),
            ("comma", ArrayFormat::Comma),
            ("indices", ArrayFormat::Indices),
            ("repeat", ArrayFormat::Repeat),
        ]
        .into_iter()
        .find(|(text, _)| name.eq_str(text))
        .map(|(_, format)| format)
    }

    /// The key of an array element: the generator functions of the library (`comma` has none and keeps the
    /// prefix).
    fn element_prefix(self, prefix: &JsString, key: &JsString) -> JsString {
        let mut out = prefix.clone();
        match self {
            ArrayFormat::Brackets => out.push_str("[]"),
            ArrayFormat::Indices => {
                out.push_str("[");
                out.push_js(key);
                out.push_str("]");
            }
            ArrayFormat::Comma | ArrayFormat::Repeat => {}
        }
        out
    }
}

/// The encoder of keys and values.
#[derive(Clone)]
pub enum Encoder {
    /// The library's default encoder, [`utils::encode`].
    Default,
    /// An encoder function from the options.
    Custom(Rc<EncoderFn>),
}

impl Encoder {
    fn encode(&self, value: &Value, charset: Charset, kind: Kind, format: &Value) -> Result<Value, JsError> {
        match self {
            Encoder::Default => Ok(utils::encode(value, charset, format)),
            Encoder::Custom(encoder) => encoder(value, &charset, kind, format),
        }
    }
}

/// The `filter` option.
#[derive(Clone)]
pub enum Filter {
    /// No filter.
    None,
    /// A function called with each prefix and value; its result replaces the value.
    Function(Rc<FilterFn>),
    /// The keys to visit, at every level, in this order.
    Keys(Vec<Value>),
}

/// The normalized options of [`stringify()`].
#[derive(Clone)]
pub struct StringifyOptions {
    /// `addQueryPrefix`.
    pub add_query_prefix: bool,
    /// `allowDots`: nested keys as `a.b`.
    pub allow_dots: bool,
    /// `allowEmptyArrays`: an empty array as `a[]`.
    pub allow_empty_arrays: bool,
    /// `arrayFormat`.
    pub array_format: ArrayFormat,
    /// `charset`.
    pub charset: Charset,
    /// `charsetSentinel`.
    pub charset_sentinel: bool,
    /// `commaRoundTrip`: a one-element array keeps `[]` with the comma format.
    pub comma_round_trip: bool,
    /// `delimiter`, as the string `join` converts it to.
    pub delimiter: JsString,
    /// `depth`.
    pub depth: f64,
    /// `encode`.
    pub encode: bool,
    /// `encodeDotInKeys`.
    pub encode_dot_in_keys: bool,
    /// `encoder`.
    pub encoder: Encoder,
    /// `encodeValuesOnly`.
    pub encode_values_only: bool,
    /// `filter`.
    pub filter: Filter,
    /// `format`, as given: the encoder receives it.
    pub format_value: Value,
    /// The formatter the format selects.
    pub format: Format,
    /// `serializeDate`; `None` for `Date.prototype.toISOString`.
    pub serialize_date: Option<Rc<DateFn>>,
    /// `skipNulls`.
    pub skip_nulls: bool,
    /// `sort`.
    pub sort: Option<Rc<SortFn>>,
    /// `strictNullHandling`.
    pub strict_null_handling: bool,
}

impl Default for StringifyOptions {
    fn default() -> Self {
        StringifyOptions {
            add_query_prefix: false,
            allow_dots: false,
            allow_empty_arrays: false,
            array_format: ArrayFormat::Indices,
            charset: Charset::Utf8,
            charset_sentinel: false,
            comma_round_trip: false,
            delimiter: JsString::from("&"),
            depth: f64::INFINITY,
            encode: true,
            encode_dot_in_keys: false,
            encoder: Encoder::Default,
            encode_values_only: false,
            filter: Filter::None,
            format_value: Value::str("RFC3986"),
            format: Format::Rfc3986,
            serialize_date: None,
            skip_nulls: false,
            sort: None,
            strict_null_handling: false,
        }
    }
}

/// Whether `name in value` holds for the property names the options are probed for: own properties of an
/// object; an `in` test on a primitive is a `TypeError`.
fn has_property(value: &Value, name: &str) -> Result<bool, JsError> {
    match value {
        Value::Object(object) => Ok(object.has_own_str(name)),
        Value::Array(array) => Ok(array.get_named(&JsString::from(name)).is_some()),
        Value::Date(_) | Value::Function(_) | Value::RegExp(_) => Ok(false),
        other => {
            let mut message = JsString::from(format!("Cannot use 'in' operator to search for '{name}' in "));
            message.push_js(&other.to_js_string());
            Err(JsError::type_error(message))
        }
    }
}

impl StringifyOptions {
    /// `normalizeStringifyOptions`: the options argument as the library reads it, with its type checks. Any value
    /// may be given; a falsy one means the defaults.
    pub fn normalize(opts: &Value) -> Result<StringifyOptions, JsError> {
        let defaults = StringifyOptions::default();
        if !opts.truthy() {
            return Ok(defaults);
        }
        let get = |name: &str| opts.get_property(&JsString::from(name));
        let boolean = |name: &str, default: bool| get(name).as_bool().unwrap_or(default);

        let allow_empty_arrays = get("allowEmptyArrays");
        if !allow_empty_arrays.is_undefined() && allow_empty_arrays.as_bool().is_none() {
            return Err(JsError::type_error(
                "`allowEmptyArrays` option can only be `true` or `false`, when provided",
            ));
        }
        let encode_dot_in_keys = get("encodeDotInKeys");
        if !encode_dot_in_keys.is_undefined() && encode_dot_in_keys.as_bool().is_none() {
            return Err(JsError::type_error(
                "`encodeDotInKeys` option can only be `true` or `false`, when provided",
            ));
        }
        let encoder = match get("encoder") {
            Value::Undefined | Value::Null => Encoder::Default,
            Value::Function(Function::Encoder(encoder)) => Encoder::Custom(encoder),
            _ => return Err(JsError::type_error("Encoder has to be a function.")),
        };
        let charset = match get("charset") {
            Value::Undefined => defaults.charset,
            value => value.as_string().and_then(Charset::from_name).ok_or_else(|| {
                JsError::type_error("The charset option must be either utf-8, iso-8859-1, or undefined")
            })?,
        };
        let format_value = get("format");
        let (format_value, format) = if format_value.is_undefined() {
            (defaults.format_value, defaults.format)
        } else {
            let format = Format::from_name(&format_value.to_js_string())
                .ok_or_else(|| JsError::type_error("Unknown format option provided."))?;
            (format_value, format)
        };
        let filter = match get("filter") {
            Value::Function(Function::Filter(filter)) => Filter::Function(filter),
            Value::Array(keys) => Filter::Keys(keys.slots().map(|slot| slot.cloned().unwrap_or_default()).collect()),
            _ => Filter::None,
        };
        let array_format = match ArrayFormat::from_name(&get("arrayFormat").to_js_string()) {
            Some(format) => format,
            None if has_property(opts, "indices")? => {
                if get("indices").truthy() {
                    ArrayFormat::Indices
                } else {
                    ArrayFormat::Repeat
                }
            }
            None => defaults.array_format,
        };
        let comma_round_trip = get("commaRoundTrip");
        if has_property(opts, "commaRoundTrip")? && comma_round_trip.as_bool().is_none() {
            return Err(JsError::type_error("`commaRoundTrip` must be a boolean, or absent"));
        }
        let allow_dots = match get("allowDots") {
            Value::Undefined => encode_dot_in_keys.is_true() || defaults.allow_dots,
            value => value.truthy(),
        };
        let delimiter = match get("delimiter") {
            Value::Undefined => defaults.delimiter,
            value => value.to_js_string(),
        };
        if opts.is_array() {
            // An array's `filter` is `Array.prototype.filter`, which the library then calls without a receiver.
            return Err(JsError::type_error(
                "Array.prototype.filter called on null or undefined",
            ));
        }
        Ok(StringifyOptions {
            add_query_prefix: boolean("addQueryPrefix", defaults.add_query_prefix),
            allow_dots,
            allow_empty_arrays: allow_empty_arrays.as_bool().unwrap_or(defaults.allow_empty_arrays),
            array_format,
            charset,
            charset_sentinel: boolean("charsetSentinel", defaults.charset_sentinel),
            comma_round_trip: comma_round_trip.truthy(),
            delimiter,
            depth: get("depth").as_number().unwrap_or(defaults.depth),
            encode: boolean("encode", defaults.encode),
            encode_dot_in_keys: encode_dot_in_keys.as_bool().unwrap_or(defaults.encode_dot_in_keys),
            encoder,
            encode_values_only: boolean("encodeValuesOnly", defaults.encode_values_only),
            filter,
            format_value,
            format,
            serialize_date: match get("serializeDate") {
                Value::Function(Function::SerializeDate(serialize)) => Some(serialize),
                _ => None,
            },
            skip_nulls: boolean("skipNulls", defaults.skip_nulls),
            sort: match get("sort") {
                Value::Function(Function::Sort(sort)) => Some(sort),
                _ => None,
            },
            strict_null_handling: boolean("strictNullHandling", defaults.strict_null_handling),
        })
    }
}

/// Stringifies `value` with the options argument `opts`.
pub fn stringify(value: &Value, opts: &Value) -> Result<JsString, JsError> {
    let options = StringifyOptions::normalize(opts)?;
    stringify_with(value, &options)
}

/// Sorts with a comparator the way `Array.prototype.sort` does for these values: stably, by the sign of the
/// comparator's result.
fn sort_values(values: &mut [Value], sort: &SortFn) {
    values.sort_by(|a, b| {
        let order = sort(a, b);
        if order < 0.0 {
            Ordering::Less
        } else if order > 0.0 {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    });
}

/// `str.replace(/\./g, '%2E')`.
fn encode_dots(key: &JsString) -> JsString {
    key.replace_all(".", "%2E")
}

/// [`stringify()`] with options that are already normalized.
pub fn stringify_with(value: &Value, options: &StringifyOptions) -> Result<JsString, JsError> {
    let mut obj = value.clone();
    let mut filter_keys = None;
    match &options.filter {
        Filter::Function(filter) => obj = filter(&JsString::new(), &obj)?,
        Filter::Keys(keys) => filter_keys = Some(keys.clone()),
        Filter::None => {}
    }
    if !matches!(
        obj,
        Value::Object(_) | Value::Array(_) | Value::Date(_) | Value::RegExp(_)
    ) {
        return Ok(JsString::new());
    }
    let mut keys: Vec<Value> = match &filter_keys {
        Some(keys) => keys.clone(),
        None => own_keys(&obj).into_iter().map(Value::String).collect(),
    };
    if let Some(sort) = &options.sort {
        sort_values(&mut keys, sort.as_ref());
        if filter_keys.is_some() {
            // The sorted copy of the filter is also the key order of the nested levels.
            filter_keys = Some(keys.clone());
        }
    }
    let context = Context {
        options,
        filter_keys: filter_keys.as_deref(),
        comma_round_trip: options.array_format == ArrayFormat::Comma && options.comma_round_trip,
    };
    let encoder = options.encode.then_some(&options.encoder);
    let mut parts = Vec::new();
    for key in &keys {
        if key.is_nullish() {
            continue;
        }
        let item = obj.get_property(&key.to_js_string());
        if options.skip_nulls && item.is_null() {
            continue;
        }
        let key_text = key.to_js_string();
        let encoded_key = if options.encode_dot_in_keys {
            encode_dots(&key_text)
        } else {
            key_text
        };
        parts.extend(context.stringify(&item, &encoded_key, encoder, 0.0)?);
    }

    let joined = JsString::join_js(&parts, &options.delimiter);
    if joined.is_empty() {
        return Ok(joined);
    }
    let mut out = JsString::new();
    if options.add_query_prefix {
        out.push_str("?");
    }
    if options.charset_sentinel {
        out.push_str(match options.charset {
            // encodeURIComponent('&#10003;'), the "numeric entity" representation of a checkmark.
            Charset::Iso88591 => "utf8=%26%2310003%3B",
            // encodeURIComponent('✓').
            Charset::Utf8 => "utf8=%E2%9C%93",
        });
        out.push_js(&options.delimiter);
    }
    out.push_js(&joined);
    Ok(out)
}

/// `Object.keys(value)`.
fn own_keys(value: &Value) -> Vec<JsString> {
    match value {
        Value::Object(object) => object.keys(),
        Value::Array(array) => array.keys(),
        _ => Vec::new(),
    }
}

/// What every level of the recursion shares.
struct Context<'a> {
    options: &'a StringifyOptions,
    filter_keys: Option<&'a [Value]>,
    comma_round_trip: bool,
}

impl Context<'_> {
    fn format(&self, value: &Value) -> JsString {
        self.options.format.apply(value.to_js_string())
    }

    fn encode(&self, encoder: &Encoder, value: &Value, kind: Kind) -> Result<Value, JsError> {
        encoder.encode(value, self.options.charset, kind, &self.options.format_value)
    }

    fn serialize_date(&self, time: f64) -> Result<Value, JsError> {
        match &self.options.serialize_date {
            Some(serialize) => serialize(time),
            None => date::to_iso_string(time).map(|text| Value::String(JsString::from(text))),
        }
    }

    /// The key of a value without its own `=`: the prefix, encoded unless only values are encoded.
    fn bare_key(&self, prefix: &JsString, encoder: Option<&Encoder>) -> Result<JsString, JsError> {
        let key = match encoder {
            Some(encoder) if !self.options.encode_values_only => {
                self.encode(encoder, &Value::String(prefix.clone()), Kind::Key)?
            }
            _ => Value::String(prefix.clone()),
        };
        Ok(self.format(&key))
    }

    /// The library's inner `stringify`: the `key=value` pairs of one value under `prefix`.
    ///
    /// The value nests no deeper than [`crate::js::json::MAX_NESTING`] (`JSON.parse` refuses deeper request values),
    /// which bounds the recursion.
    fn stringify(
        &self,
        object: &Value,
        prefix: &JsString,
        encoder: Option<&Encoder>,
        current_depth: f64,
    ) -> Result<Vec<JsString>, JsError> {
        let options = self.options;
        if current_depth > options.depth {
            return Err(JsError::new(
                "RangeError",
                format!(
                    "Input depth exceeded depth option of {}",
                    number_to_string(options.depth)
                ),
            ));
        }

        let mut obj = match &options.filter {
            Filter::Function(filter) => filter(prefix, object)?,
            _ => object.clone(),
        };
        let is_comma = options.array_format == ArrayFormat::Comma;
        if let Value::Date(time) = obj {
            obj = self.serialize_date(time)?;
        } else if let (true, Value::Array(array)) = (is_comma, &obj) {
            let mut mapped = Array::new();
            for slot in array.slots() {
                mapped.push(match slot {
                    Some(Value::Date(time)) => self.serialize_date(*time)?,
                    Some(other) => other.clone(),
                    None => Value::Undefined,
                });
            }
            obj = Value::Array(mapped);
        }

        if obj.is_null() {
            if options.strict_null_handling {
                return Ok(vec![self.bare_key(prefix, encoder)?]);
            }
            obj = Value::String(JsString::new());
        }

        if matches!(obj, Value::String(_) | Value::Number(_) | Value::Bool(_)) {
            let pair = match encoder {
                Some(encoder) => {
                    let key = if options.encode_values_only {
                        Value::String(prefix.clone())
                    } else {
                        self.encode(encoder, &Value::String(prefix.clone()), Kind::Key)?
                    };
                    let mut pair = self.format(&key);
                    pair.push_str("=");
                    pair.push_js(&self.format(&self.encode(encoder, &obj, Kind::Value)?));
                    pair
                }
                None => {
                    let mut pair = self.format(&Value::String(prefix.clone()));
                    pair.push_str("=");
                    pair.push_js(&self.format(&Value::String(obj.to_js_string())));
                    pair
                }
            };
            return Ok(vec![pair]);
        }

        if obj.is_undefined() {
            return Ok(Vec::new());
        }

        // Each key is the key value itself, or for the comma format the joined elements under the prefix.
        let keys: Vec<KeyEntry> = match (&obj, is_comma) {
            (Value::Array(array), true) => {
                let mut joined_source = array.clone();
                if options.encode_values_only {
                    if let Some(encoder) = encoder {
                        let mut mapped = Array::new();
                        for slot in array.slots() {
                            let item = slot.cloned().unwrap_or_default();
                            mapped.push(if item.is_nullish() {
                                item
                            } else {
                                self.encode(encoder, &item, Kind::Value)?
                            });
                        }
                        joined_source = mapped;
                    }
                }
                let value = if array.is_empty() {
                    Value::Undefined
                } else {
                    let joined = join(&joined_source, &JsString::from(","));
                    if joined.is_empty() {
                        Value::Null
                    } else {
                        Value::String(joined)
                    }
                };
                vec![KeyEntry::Joined(value)]
            }
            _ => match self.filter_keys {
                Some(keys) => keys.iter().cloned().map(KeyEntry::Key).collect(),
                None => {
                    let mut keys: Vec<Value> = own_keys(&obj).into_iter().map(Value::String).collect();
                    if let Some(sort) = &options.sort {
                        sort_values(&mut keys, sort.as_ref());
                    }
                    keys.into_iter().map(KeyEntry::Key).collect()
                }
            },
        };

        let is_array = obj.is_array();
        let array_length = obj.as_array().map_or(0, Array::len);
        let adjusted_prefix = if self.comma_round_trip && is_array && array_length == 1 {
            let mut adjusted = prefix.clone();
            adjusted.push_str("[]");
            adjusted
        } else {
            prefix.clone()
        };

        if options.allow_empty_arrays && is_array && array_length == 0 && own_keys(&obj).is_empty() {
            let mut key = self.bare_key(&adjusted_prefix, encoder)?;
            key.push_str("[]");
            return Ok(vec![key]);
        }

        let child_encoder = if is_comma && options.encode_values_only && is_array {
            None
        } else {
            encoder
        };
        let mut values = Vec::new();
        for entry in keys {
            let (key_text, value) = match entry {
                KeyEntry::Joined(value) => {
                    let key = JsString::from("[object Object]");
                    let value = if value.is_undefined() {
                        obj.get_property(&key)
                    } else {
                        value
                    };
                    (key, value)
                }
                KeyEntry::Key(key) => {
                    let value = match key.get_property(&JsString::from("value")) {
                        explicit if key.is_object_like() && !explicit.is_undefined() => explicit,
                        _ => obj.get_property(&key.to_js_string()),
                    };
                    (key.to_js_string(), value)
                }
            };
            if options.skip_nulls && value.is_null() {
                continue;
            }
            let encoded_key = if options.allow_dots && options.encode_dot_in_keys {
                encode_dots(&key_text)
            } else {
                key_text
            };
            let key_prefix = if is_array {
                options.array_format.element_prefix(&adjusted_prefix, &encoded_key)
            } else {
                let mut key_prefix = adjusted_prefix.clone();
                if options.allow_dots {
                    key_prefix.push_str(".");
                    key_prefix.push_js(&encoded_key);
                } else {
                    key_prefix.push_str("[");
                    key_prefix.push_js(&encoded_key);
                    key_prefix.push_str("]");
                }
                key_prefix
            };
            values.extend(self.stringify(&value, &key_prefix, child_encoder, current_depth + 1.0)?);
        }
        Ok(values)
    }
}

/// One key of the inner loop: a key value, or the comma format's joined elements.
enum KeyEntry {
    Key(Value),
    Joined(Value),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::js::json;

    fn run(value: &str, options: &str) -> String {
        let value = json::parse_str(value).expect("test values are JSON");
        let options = json::parse_str(options).expect("test options are JSON");
        match stringify(&value, &options) {
            Ok(text) => text.to_string_lossy(),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn stringifies_nested_values() {
        assert_eq!(run(r#"{"a":{"b":"c"}}"#, "{}"), "a%5Bb%5D=c");
        assert_eq!(
            run(r#"{"a":["b","c"]}"#, r#"{"encodeValuesOnly":true}"#),
            "a[0]=b&a[1]=c"
        );
        assert_eq!(run(r#"{"a":["b","c"]}"#, r#"{"arrayFormat":"comma"}"#), "a=b%2Cc");
        assert_eq!(
            run(r#"{"a":["b"]}"#, r#"{"arrayFormat":"comma","commaRoundTrip":true}"#),
            "a%5B%5D=b"
        );
        assert_eq!(run(r#"{"b":1,"2":1e21}"#, "{}"), "2=1e%2B21&b=1");
    }

    #[test]
    fn handles_nulls_and_errors() {
        assert_eq!(run(r#"{"a":null,"b":""}"#, r#"{"strictNullHandling":true}"#), "a&b=");
        assert_eq!(run(r#"{"a":null,"b":1}"#, r#"{"skipNulls":true}"#), "b=1");
        assert_eq!(
            run(r#"{"a":{"b":1}}"#, r#"{"depth":0}"#),
            "RangeError: Input depth exceeded depth option of 0"
        );
        assert_eq!(
            run(r#"{"a":1}"#, "1"),
            "TypeError: Cannot use 'in' operator to search for 'indices' in 1"
        );
    }
}
